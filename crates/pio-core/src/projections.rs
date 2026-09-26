//! Journal-backed protocol read projections. The journal and outbox are the
//! authority; rows are individually replaceable, replayable derived state.
use crate::{Store, append};
use anyhow::{Result, bail, ensure};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub type Records = BTreeMap<String, Value>;

/// ADR 002 hard bound on the retained Protocol projection, including events and
/// dedupe outcomes. Checked on the complete proposed projection before any row
/// is staged. It bounds retained state only, not journal, spool or disk growth.
pub const MAX_PROJECTED_STATE_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_PROJECTION_RECORDS: u64 = 32_768;

/// Bench values approved by the owner on 2026-09-16: 95 percent headroom for
/// new execution admission (31,129.6 records rounded up; 30.4 MiB rounded
/// down). Facts for already-admitted work may use the room above them, up to
/// the hard limits.
pub const MAX_ADMISSION_PROJECTION_RECORDS: u64 = 31_130;
pub const MAX_ADMISSION_STATE_BYTES: u64 = 31_876_710;

/// Explicit capacity refusal. Nothing is staged, journaled or sent to the outbox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapacityExceeded {
    pub limit: &'static str,
    pub maximum: u64,
    pub projected: u64,
}

impl std::fmt::Display for CapacityExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "projection_capacity_exceeded: {} would be {}, maximum {}",
            self.limit, self.projected, self.maximum
        )
    }
}

impl std::error::Error for CapacityExceeded {}

/// One record counts its UTF-8 key bytes plus its compact JSON value bytes.
/// Canonical encoding differs from the stored compact form only in member
/// order, so both have the same length.
fn record_bytes(key: &str, value_text: &str) -> u64 {
    (key.len() + value_text.len()) as u64
}

/// Canonical projected size in bytes and the record count.
pub fn projection_size(records: &Records) -> Result<(u64, u64)> {
    let mut bytes = 0;
    for (key, value) in records {
        bytes += record_bytes(key, &serde_json::to_string(value)?);
    }
    Ok((bytes, records.len() as u64))
}

/// Which limits a commit must satisfy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bound {
    Hard,
    Admission,
}

fn within_bound(bytes: u64, count: u64, bound: Bound) -> std::result::Result<(), CapacityExceeded> {
    if bound == Bound::Admission {
        if count > MAX_ADMISSION_PROJECTION_RECORDS {
            return Err(CapacityExceeded {
                limit: "admission_projection_records",
                maximum: MAX_ADMISSION_PROJECTION_RECORDS,
                projected: count,
            });
        }
        if bytes > MAX_ADMISSION_STATE_BYTES {
            return Err(CapacityExceeded {
                limit: "admission_projected_state_bytes",
                maximum: MAX_ADMISSION_STATE_BYTES,
                projected: bytes,
            });
        }
    }
    if count > MAX_PROJECTION_RECORDS {
        return Err(CapacityExceeded {
            limit: "projection_records",
            maximum: MAX_PROJECTION_RECORDS,
            projected: count,
        });
    }
    if bytes > MAX_PROJECTED_STATE_BYTES {
        return Err(CapacityExceeded {
            limit: "projected_state_bytes",
            maximum: MAX_PROJECTED_STATE_BYTES,
            projected: bytes,
        });
    }
    Ok(())
}

/// A changed-key commit: the new value of each record the caller changed, or
/// `None` to delete it. Keys are unique and ordered, as in the journal fact.
pub type Changes = BTreeMap<String, Option<Value>>;

/// Work done by one Protocol commit attempt. `examined` counts projection
/// records the commit read, parsed or compared, whether stored rows or the
/// caller's input; `staged` counts records written. The ADR 002 changed-key
/// amendment bounds `examined` by the changed keys, not the projection size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommitCost {
    pub examined: u64,
    pub staged: u64,
}

/// Retained projection totals at one committed revision, kept so a commit can
/// check the capacity bound from its own changes alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Totals {
    revision: u64,
    bytes: u64,
    records: u64,
}

/// What a commit proposes: the complete projection, or only changed keys.
enum Proposal<'a> {
    Whole(&'a Records),
    Changed(&'a Changes),
}

/// The whole-state diff: every stored row is read and compared. Its totals
/// come from the same rows, so no separate scan is needed.
fn whole_state_changes(
    tx: &rusqlite::Transaction<'_>,
    revision: u64,
    records: &Records,
    cost: &mut CommitCost,
) -> Result<(Changes, Totals)> {
    let mut old = BTreeMap::<String, Value>::new();
    let mut totals = Totals {
        revision,
        bytes: 0,
        records: 0,
    };
    {
        let mut q = tx.prepare("SELECT key,value FROM protocol_projection")?;
        for row in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (key, value) = row?;
            cost.examined += 1;
            totals.bytes += record_bytes(&key, &value);
            totals.records += 1;
            old.insert(key, serde_json::from_str(&value)?);
        }
    }
    let mut changes = Changes::new();
    for (key, value) in records {
        cost.examined += 1;
        if old.get(key) != Some(value) {
            changes.insert(key.clone(), Some(value.clone()));
        }
    }
    for key in old.keys().filter(|k| !records.contains_key(*k)) {
        changes.insert(key.clone(), None);
    }
    Ok((changes, totals))
}

/// Totals of the stored projection, for a store opened without them. This is
/// the one full scan left on the changed-key path: at the first commit of an
/// opened store (the service's startup commit), or after another connection
/// committed. It reads no values, only their lengths.
fn stored_totals(
    tx: &rusqlite::Transaction<'_>,
    revision: u64,
    cost: &mut CommitCost,
) -> Result<Totals> {
    let (records, bytes): (u64, u64) = tx.query_row(
        "SELECT count(*), coalesce(sum(length(CAST(key AS BLOB)) + length(CAST(value AS BLOB))), 0) FROM protocol_projection",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    cost.examined += records;
    Ok(Totals {
        revision,
        bytes,
        records,
    })
}

impl Store {
    pub fn protocol_records(&self) -> Result<(u64, Records)> {
        let revision = self
            .conn
            .query_row("SELECT revision FROM protocol_head", [], |r| r.get(0))?;
        let mut q = self
            .conn
            .prepare("SELECT key,value FROM protocol_projection ORDER BY key")?;
        let rows = q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut records = Records::new();
        for row in rows {
            let (key, value) = row?;
            records.insert(key, serde_json::from_str(&value)?);
        }
        Ok((revision, records))
    }

    /// Whole-state form: the complete proposed projection is diffed against
    /// every stored row. Kept for offline tooling and tests; the service
    /// commits through [`Store::commit_changes`]. The external service
    /// lifetime lock provides the single writer. The revision check
    /// additionally refuses stale caches, rather than overwriting a commit
    /// made through another connection. A commit that changes the projection
    /// is refused with [`CapacityExceeded`] before any row is staged when the
    /// resulting projection would exceed the ADR 002 bound.
    pub fn commit_protocol(&mut self, expected: u64, records: &Records) -> Result<u64> {
        self.commit_bounded(expected, Proposal::Whole(records), Bound::Hard)
    }

    /// Whole-state commit that admits new execution work. It must also stay
    /// within the lower admission thresholds, leaving room for admitted work.
    pub fn commit_admission(&mut self, expected: u64, records: &Records) -> Result<u64> {
        self.commit_bounded(expected, Proposal::Whole(records), Bound::Admission)
    }

    /// Changed-key commit (ADR 002 amendment, 2026-09-26). Only the named
    /// keys are read and compared; the capacity bound is checked from the
    /// cached totals of the committed revision plus these changes. The
    /// journal fact, fencing, refusal and no-op rules are those of the
    /// whole-state form: a named key whose value equals the stored one is not
    /// a change, and a commit with no change is not re-checked or journaled.
    pub fn commit_changes(&mut self, expected: u64, changes: &Changes) -> Result<u64> {
        self.commit_bounded(expected, Proposal::Changed(changes), Bound::Hard)
    }

    /// Changed-key commit that admits new execution work.
    pub fn commit_admission_changes(&mut self, expected: u64, changes: &Changes) -> Result<u64> {
        self.commit_bounded(expected, Proposal::Changed(changes), Bound::Admission)
    }

    /// Cost of the most recent commit attempt, including a no-op or refusal.
    pub fn last_commit_cost(&self) -> CommitCost {
        self.last_commit
    }

    /// The most expensive commit attempt since the previous call, by
    /// `examined`; resets it.
    pub fn take_peak_commit_cost(&mut self) -> CommitCost {
        std::mem::take(&mut self.peak_commit)
    }

    fn commit_bounded(
        &mut self,
        expected: u64,
        proposal: Proposal<'_>,
        bound: Bound,
    ) -> Result<u64> {
        let mut cost = CommitCost::default();
        let result = self.commit_measured(expected, proposal, bound, &mut cost);
        self.last_commit = cost;
        if cost.examined >= self.peak_commit.examined {
            self.peak_commit = cost;
        }
        result
    }

    fn commit_measured(
        &mut self,
        expected: u64,
        proposal: Proposal<'_>,
        bound: Bound,
        cost: &mut CommitCost,
    ) -> Result<u64> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let revision: u64 = tx.query_row("SELECT revision FROM protocol_head", [], |r| r.get(0))?;
        ensure!(revision == expected, "protocol_writer_fenced");
        if self.totals.is_some_and(|t| t.revision != revision) {
            self.totals = None;
        }
        let whole;
        let (changes, base) = match (proposal, self.totals) {
            (Proposal::Changed(changes), Some(totals)) => (changes, totals),
            (Proposal::Changed(changes), None) => (changes, stored_totals(&tx, revision, cost)?),
            (Proposal::Whole(records), _) => {
                let (changes, totals) = whole_state_changes(&tx, revision, records, cost)?;
                whole = changes;
                (&whole, totals)
            }
        };
        // Totals describe the committed revision whatever this commit does.
        self.totals = Some(base);
        // Stored text is the compact serialization of its value, so a size
        // delta needs only the changed records' stored lengths.
        let mut upserts = Vec::new();
        let mut deletes = Vec::new();
        let (mut added_bytes, mut removed_bytes, mut added, mut removed) = (0, 0, 0, 0);
        {
            let mut q = tx.prepare_cached("SELECT value FROM protocol_projection WHERE key=?1")?;
            for (key, value) in changes {
                cost.examined += 1;
                let stored: Option<String> = q.query_row([key], |r| r.get(0)).optional()?;
                match (stored, value) {
                    (Some(text), Some(value)) => {
                        if serde_json::from_str::<Value>(&text)? == *value {
                            continue;
                        }
                        let new = serde_json::to_string(value)?;
                        added_bytes += new.len() as u64;
                        removed_bytes += text.len() as u64;
                        upserts.push((key, value, new));
                    }
                    (None, Some(value)) => {
                        let new = serde_json::to_string(value)?;
                        added_bytes += record_bytes(key, &new);
                        added += 1;
                        upserts.push((key, value, new));
                    }
                    (Some(text), None) => {
                        removed_bytes += record_bytes(key, &text);
                        removed += 1;
                        deletes.push(key);
                    }
                    (None, None) => {}
                }
            }
        }
        if upserts.is_empty() && deletes.is_empty() {
            return Ok(revision);
        }
        // Removed records are among the stored ones, so a shortfall means the
        // cached totals no longer describe the rows.
        let (Some(bytes), Some(count)) = (
            (base.bytes + added_bytes).checked_sub(removed_bytes),
            (base.records + added).checked_sub(removed),
        ) else {
            bail!("protocol_projection_totals_inconsistent");
        };
        within_bound(bytes, count, bound)?;
        let mut facts = Vec::new();
        for (key, value, text) in upserts {
            tx.execute("INSERT INTO protocol_projection VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,text])?;
            facts.push(json!({"key":key,"value":value}));
        }
        for key in deletes {
            tx.execute("DELETE FROM protocol_projection WHERE key=?1", [key])?;
            facts.push(json!({"key":key,"delete":true}));
        }
        cost.staged = facts.len() as u64;
        let next = revision + 1;
        append(
            &tx,
            json!({"kind":"protocol.commit","source":"pio-core-conformance","revision":next,"changes":facts}),
        )?;
        tx.execute("UPDATE protocol_head SET revision=?1", [next])?;
        tx.commit()?;
        self.totals = Some(Totals {
            revision: next,
            bytes,
            records: count,
        });
        Ok(next)
    }

    /// Rebuild under the same exclusive writer lock. No journal facts or outbox
    /// entries are rewritten; retention deletions are replayed as facts too.
    pub fn rebuild_protocol(&mut self) -> Result<()> {
        let facts = self.journal()?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM protocol_projection", [])?;
        let mut head = 0;
        for fact in facts.iter().filter(|f| f["kind"] == "protocol.commit") {
            let revision = fact["revision"].as_u64().unwrap_or(0);
            ensure!(revision == head + 1, "protocol_journal_discontinuity");
            for change in fact["changes"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("invalid protocol fact"))?
            {
                let key = change["key"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("invalid record key"))?;
                if change["delete"] == true {
                    tx.execute("DELETE FROM protocol_projection WHERE key=?1", [key])?;
                } else {
                    tx.execute("INSERT INTO protocol_projection VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,serde_json::to_string(&change["value"])?])?;
                }
            }
            head = revision;
        }
        tx.execute("UPDATE protocol_head SET revision=?1", [head])?;
        tx.commit()?;
        self.totals = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn journal_rebuild_atomic_outbox_and_stale_writer() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::open(root.path()).unwrap();
        let mut records = Records::from([
            ("subject/x".into(), json!({"revision":1})),
            ("event/1".into(), json!({"kind":"changed"})),
        ]);
        assert_eq!(store.commit_protocol(0, &records).unwrap(), 1);
        let before = store.protocol_records().unwrap();
        store.conn.execute_batch("CREATE TRIGGER refuse_outbox BEFORE INSERT ON outbox BEGIN SELECT RAISE(ABORT,'disk fault'); END;").unwrap();
        records.insert("command/1".into(), json!({"result":"ok"}));
        assert!(store.commit_protocol(1, &records).is_err());
        assert_eq!(store.protocol_records().unwrap(), before);
        assert_eq!(store.journal().unwrap().len(), 1);
        store
            .conn
            .execute_batch("DROP TRIGGER refuse_outbox")
            .unwrap();
        assert_eq!(store.commit_protocol(1, &records).unwrap(), 2);
        assert!(
            store
                .commit_protocol(1, &Records::new())
                .unwrap_err()
                .to_string()
                .contains("protocol_writer_fenced")
        );
        records.remove("event/1");
        store.commit_protocol(2, &records).unwrap();
        let expected = store.protocol_records().unwrap();
        store
            .conn
            .execute_batch("DELETE FROM protocol_projection")
            .unwrap();
        store.rebuild_protocol().unwrap();
        assert_eq!(store.protocol_records().unwrap(), expected);
        let matching:u64=store.conn.query_row("SELECT count(*) FROM journal j JOIN outbox o ON j.sequence=o.sequence AND j.record=o.record",[],|r|r.get(0)).unwrap();
        assert_eq!(matching, 3);
        drop(store);
        assert_eq!(
            Store::open(root.path())
                .unwrap()
                .protocol_records()
                .unwrap(),
            expected
        );
    }

    fn unchanged(store: &Store) -> (u64, Records, Vec<Value>, u64) {
        let (revision, records) = store.protocol_records().unwrap();
        let outbox: u64 = store
            .conn
            .query_row("SELECT count(*) FROM outbox", [], |r| r.get(0))
            .unwrap();
        (revision, records, store.journal().unwrap(), outbox)
    }

    fn capacity(error: anyhow::Error) -> CapacityExceeded {
        error
            .downcast_ref::<CapacityExceeded>()
            .unwrap_or_else(|| panic!("expected capacity refusal, got {error:#}"))
            .clone()
    }

    #[test]
    fn projection_bytes_accept_exact_limit_and_refuse_one_more_before_staging() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::open(root.path()).unwrap();
        let key = "blob/0";
        // Record size is UTF-8 key bytes plus compact JSON value bytes; a JSON
        // string adds its two quotes.
        let fill = MAX_PROJECTED_STATE_BYTES as usize - key.len() - 2;
        let at_limit = Records::from([(key.to_owned(), json!("x".repeat(fill)))]);
        assert_eq!(
            projection_size(&at_limit).unwrap(),
            (MAX_PROJECTED_STATE_BYTES, 1)
        );
        let before = unchanged(&store);
        let over = Records::from([(key.to_owned(), json!("x".repeat(fill + 1)))]);
        // Any staged projection row or journal fact would raise a different error.
        store.conn.execute_batch("CREATE TRIGGER no_stage BEFORE INSERT ON protocol_projection BEGIN SELECT RAISE(ABORT,'staged'); END; CREATE TRIGGER no_fact BEFORE INSERT ON journal BEGIN SELECT RAISE(ABORT,'staged'); END;").unwrap();
        assert_eq!(
            capacity(store.commit_protocol(0, &over).unwrap_err()),
            CapacityExceeded {
                limit: "projected_state_bytes",
                maximum: MAX_PROJECTED_STATE_BYTES,
                projected: MAX_PROJECTED_STATE_BYTES + 1,
            }
        );
        assert_eq!(unchanged(&store), before, "refusal must stage nothing");
        store
            .conn
            .execute_batch("DROP TRIGGER no_stage; DROP TRIGGER no_fact;")
            .unwrap();
        assert_eq!(store.commit_protocol(0, &at_limit).unwrap(), 1);
        assert_eq!(store.protocol_records().unwrap().1, at_limit);
        // Growth by one byte through an unchanged plus a changed record is refused
        // with the same accounting; a shrinking commit is accepted.
        let before = unchanged(&store);
        let mut grown = at_limit.clone();
        grown.insert("e".into(), json!(1));
        let error = capacity(store.commit_protocol(1, &grown).unwrap_err());
        assert_eq!(error.projected, MAX_PROJECTED_STATE_BYTES + 2);
        assert_eq!(unchanged(&store), before);
        let shrunk = Records::from([(key.to_owned(), json!("x"))]);
        assert_eq!(store.commit_protocol(1, &shrunk).unwrap(), 2);
    }

    #[test]
    fn projection_records_accept_exact_limit_and_refuse_one_more_before_staging() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::open(root.path()).unwrap();
        let mut records: Records = (0..MAX_PROJECTION_RECORDS)
            .map(|n| (format!("event/{n:020}"), json!(n)))
            .collect();
        assert_eq!(projection_size(&records).unwrap().1, MAX_PROJECTION_RECORDS);
        records.insert("command/extra".into(), json!({"result":"bound"}));
        let before = unchanged(&store);
        assert_eq!(
            capacity(store.commit_protocol(0, &records).unwrap_err()),
            CapacityExceeded {
                limit: "projection_records",
                maximum: MAX_PROJECTION_RECORDS,
                projected: MAX_PROJECTION_RECORDS + 1,
            }
        );
        assert_eq!(unchanged(&store), before, "refusal must stage nothing");
        records.remove("command/extra");
        assert_eq!(store.commit_protocol(0, &records).unwrap(), 1);
        // Retention that removes a record makes room for exactly one new record.
        records.remove(&format!("event/{:020}", 0));
        records.insert("command/extra".into(), json!({"result":"bound"}));
        assert_eq!(store.commit_protocol(1, &records).unwrap(), 2);
        assert_eq!(
            store.protocol_records().unwrap().1.len() as u64,
            MAX_PROJECTION_RECORDS
        );
    }

    #[test]
    fn admission_headroom_refuses_only_new_admission_above_bench_thresholds() {
        // Owner bench values: 95 percent of each hard limit.
        assert_eq!(
            MAX_ADMISSION_PROJECTION_RECORDS,
            (MAX_PROJECTION_RECORDS * 95).div_ceil(100)
        );
        assert_eq!(
            MAX_ADMISSION_STATE_BYTES,
            MAX_PROJECTED_STATE_BYTES * 95 / 100
        );

        let root = tempfile::tempdir().unwrap();
        let mut store = Store::open(root.path()).unwrap();
        let mut records: Records = (0..MAX_ADMISSION_PROJECTION_RECORDS)
            .map(|n| (format!("event/{n:020}"), json!(n)))
            .collect();
        assert_eq!(store.commit_admission(0, &records).unwrap(), 1);
        records.insert("execution/new".into(), json!({}));
        let before = unchanged(&store);
        assert_eq!(
            capacity(store.commit_admission(1, &records).unwrap_err()),
            CapacityExceeded {
                limit: "admission_projection_records",
                maximum: MAX_ADMISSION_PROJECTION_RECORDS,
                projected: MAX_ADMISSION_PROJECTION_RECORDS + 1,
            }
        );
        assert_eq!(unchanged(&store), before);
        // Facts for already-admitted work may use the remaining room.
        assert_eq!(store.commit_protocol(1, &records).unwrap(), 2);

        let root = tempfile::tempdir().unwrap();
        let mut store = Store::open(root.path()).unwrap();
        let key = "blob/0";
        let fill = MAX_ADMISSION_STATE_BYTES as usize - key.len() - 2;
        let at = Records::from([(key.to_owned(), json!("x".repeat(fill)))]);
        assert_eq!(store.commit_admission(0, &at).unwrap(), 1);
        let over = Records::from([(key.to_owned(), json!("x".repeat(fill + 1)))]);
        let before = unchanged(&store);
        assert_eq!(
            capacity(store.commit_admission(1, &over).unwrap_err()),
            CapacityExceeded {
                limit: "admission_projected_state_bytes",
                maximum: MAX_ADMISSION_STATE_BYTES,
                projected: MAX_ADMISSION_STATE_BYTES + 1,
            }
        );
        assert_eq!(unchanged(&store), before);
        assert_eq!(store.commit_protocol(1, &over).unwrap(), 2);
    }

    /// Measurement, not a pass/fail property. Run with
    /// `cargo test -p pio-core [--release] -- --ignored --nocapture measure_commit_cost`.
    #[test]
    #[ignore]
    fn measure_commit_cost_at_record_bound() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::open(root.path()).unwrap();
        let mut records: Records = (0..MAX_PROJECTION_RECORDS - 1)
            .map(|n| {
                (
                    format!("subject/{{\"id\":\"{n}\",\"kind\":\"pio-test.filler\"}}"),
                    json!({"applied":0,"revision":1,"state":{},"subject":{"id":n.to_string(),"kind":"pio-test.filler"}}),
                )
            })
            .collect();
        let started = std::time::Instant::now();
        let revision = store.commit_protocol(0, &records).unwrap();
        eprintln!("initial commit {:?}", started.elapsed());
        let started = std::time::Instant::now();
        store.commit_protocol(revision, &records).unwrap();
        eprintln!("whole-state no-op commit {:?}", started.elapsed());
        records.insert("event/x".into(), json!({"a":1}));
        let started = std::time::Instant::now();
        let revision = store.commit_protocol(revision, &records).unwrap();
        eprintln!("whole-state one-record commit {:?}", started.elapsed());
        let started = std::time::Instant::now();
        store.protocol_records().unwrap();
        eprintln!("projection load {:?}", started.elapsed());
        // The changed-key path: a fresh handle scans totals once, then each
        // commit reads only the keys it names.
        drop(store);
        let mut store = Store::open(root.path()).unwrap();
        // The projection is at the record limit, so change an existing record.
        let meta = |n: u64| Changes::from([("event/x".to_owned(), Some(json!({"a": n})))]);
        let started = std::time::Instant::now();
        let revision = store.commit_changes(revision, &meta(1)).unwrap();
        eprintln!(
            "changed-key first commit (totals scan) {:?}",
            started.elapsed()
        );
        let started = std::time::Instant::now();
        store.commit_changes(revision, &meta(1)).unwrap();
        eprintln!("changed-key no-op commit {:?}", started.elapsed());
        let started = std::time::Instant::now();
        store.commit_changes(revision, &meta(2)).unwrap();
        eprintln!(
            "changed-key one-record commit {:?} {:?}",
            started.elapsed(),
            store.last_commit_cost()
        );
    }

    /// Execution-like records: an execution, its delivery effect, its subject
    /// and its dedupe outcome.
    fn executions(n: usize) -> Records {
        let mut records = Records::from([("meta".to_owned(), json!({"sequence":n}))]);
        for i in 0..n {
            let id = format!("x{i:05}");
            let view = json!({"execution":{"kind":"execution.execution","id":id},"revision":4,"runtime":"exited","delivery":"acknowledged","exit":{"code":0}});
            records.insert(
                format!("execution/{id}"),
                json!({"view":view,"order":i,"step":3}),
            );
            records.insert(
                format!("effect/{id}.delivery-1"),
                json!({"revision":2,"status":"acknowledged","attempts":[{"attempt":1}]}),
            );
            records.insert(
                format!("subject/{{\"id\":\"{id}\",\"kind\":\"execution.execution\"}}"),
                json!({"revision":4,"state":view,"applied":1}),
            );
            records.insert(
                format!("command/[\"owner\",\"{id}\"]"),
                json!({"digest":"d","generation":1,"result":{"replay":false}}),
            );
        }
        records
    }

    /// One command's change set: a new execution plus the rewritten meta. It
    /// names one key that does not change and one absent key it deletes, which
    /// the commit must examine but not journal.
    fn one_command(n: usize) -> Changes {
        Changes::from([
            ("meta".to_owned(), Some(json!({"sequence":n + 2}))),
            ("execution/new".to_owned(), Some(json!({"step":0}))),
            (
                "effect/new.delivery-1".to_owned(),
                Some(json!({"status":"pending"})),
            ),
            (
                "command/[\"owner\",\"new\"]".to_owned(),
                Some(json!({"generation":1})),
            ),
            (
                "command/[\"owner\",\"x00000\"]".to_owned(),
                Some(json!({"digest":"d","generation":1,"result":{"replay":false}})),
            ),
            ("event/gone".to_owned(), None),
        ])
    }

    #[test]
    fn changed_key_commit_cost_is_independent_of_projection_size() {
        let mut staged = vec![];
        for n in [10, 1000] {
            let root = tempfile::tempdir().unwrap();
            let mut store = Store::open(root.path()).unwrap();
            let seeded = executions(n);
            assert_eq!(seeded.len(), 4 * n + 1);
            let revision = store.commit_protocol(0, &seeded).unwrap();
            // Control: the whole-state form reads every stored row and compares
            // every proposed record, so its cost grows with the projection.
            let mut whole = seeded.clone();
            whole.insert("meta".into(), json!({"sequence":n + 1}));
            let revision = store.commit_protocol(revision, &whole).unwrap();
            assert!(store.last_commit_cost().examined >= 2 * seeded.len() as u64);
            // A fresh handle computes its totals once, at its first commit.
            drop(store);
            let mut store = Store::open(root.path()).unwrap();
            let revision = store
                .commit_changes(
                    revision,
                    &Changes::from([("meta".to_owned(), Some(json!({"sequence":n + 1})))]),
                )
                .unwrap();
            assert_eq!(
                store.last_commit_cost(),
                CommitCost {
                    examined: seeded.len() as u64 + 1,
                    staged: 0
                }
            );
            // Every later commit examines exactly the keys it names, and
            // stages only those that differ.
            store.take_peak_commit_cost();
            let changes = one_command(n);
            let revision = store.commit_changes(revision, &changes).unwrap();
            let cost = store.last_commit_cost();
            assert_eq!(
                cost,
                CommitCost {
                    examined: changes.len() as u64,
                    staged: 4
                },
                "commit_cost_scales_with_state: with {} records retained",
                seeded.len()
            );
            assert_eq!(store.take_peak_commit_cost(), cost);
            let fact = store.journal().unwrap().pop().unwrap();
            assert_eq!(fact["revision"], revision);
            staged.push(fact["changes"].clone());
        }
        // The same command journals the same facts at either size.
        assert_eq!(
            staged[0].to_string().replace("\"sequence\":12", "N"),
            staged[1].to_string().replace("\"sequence\":1002", "N")
        );
    }

    /// Drives one store through whole-state commits and another through
    /// changed-key commits of the same states. The journals, projections and
    /// capacity accounting must be identical.
    #[test]
    fn changed_key_commits_journal_exactly_what_whole_state_commits_journal() {
        let whole_root = tempfile::tempdir().unwrap();
        let changed_root = tempfile::tempdir().unwrap();
        let mut whole = Store::open(whole_root.path()).unwrap();
        let mut changed = Store::open(changed_root.path()).unwrap();
        let states = [
            executions(3),
            {
                let mut s = executions(3);
                s.insert("event/1".into(), json!({"text":"é😀\u{e000}","n":1}));
                s.remove("effect/x00001.delivery-1");
                s.insert("execution/x00002".into(), json!({"order":2,"step":4}));
                s
            },
            {
                let mut s = executions(2);
                s.insert("event/1".into(), json!({"text":"é😀\u{e000}","n":1}));
                s
            },
            executions(2),
            executions(2),
        ];
        let mut previous = Records::new();
        let (mut w, mut c) = (0, 0);
        for state in &states {
            w = whole.commit_protocol(w, state).unwrap();
            // A conservative change set: every key of both states, changed or not.
            let changes: Changes = previous
                .keys()
                .chain(state.keys())
                .map(|k| (k.clone(), state.get(k).cloned()))
                .collect();
            c = changed.commit_changes(c, &changes).unwrap();
            assert_eq!(w, c);
            assert_eq!(
                whole.protocol_records().unwrap(),
                changed.protocol_records().unwrap()
            );
            let totals = changed.totals.unwrap();
            assert_eq!(
                (totals.bytes, totals.records),
                projection_size(state).unwrap()
            );
            assert_eq!(whole.totals, changed.totals);
            previous = state.clone();
        }
        assert_eq!(w, 4, "the repeated state is a no-op in both forms");
        assert_eq!(whole.journal().unwrap(), changed.journal().unwrap());
        // A store opened fresh computes the same totals from its rows.
        drop(changed);
        let mut reopened = Store::open(changed_root.path()).unwrap();
        reopened.commit_changes(c, &Changes::new()).unwrap();
        assert_eq!(reopened.totals, whole.totals);
    }

    #[test]
    fn changed_key_commit_refuses_capacity_before_staging_and_ignores_stale_totals() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::open(root.path()).unwrap();
        let key = "a";
        let fill = MAX_PROJECTED_STATE_BYTES as usize - key.len() - 2;
        let blob = |n: usize| Changes::from([(key.to_owned(), Some(json!("x".repeat(n))))]);
        assert_eq!(store.commit_changes(0, &blob(fill - 1)).unwrap(), 1);
        // Another connection deletes the record. This handle's cached totals
        // describe revision 1 and must not be used at revision 2: with them,
        // the refusal below would report almost twice the limit.
        let mut other = Store::open(root.path()).unwrap();
        let delete = Changes::from([(key.to_owned(), None)]);
        assert_eq!(other.commit_changes(1, &delete).unwrap(), 2);
        let mut grow = blob(fill);
        grow.insert("e".to_owned(), Some(json!(1)));
        let before = unchanged(&store);
        store.conn.execute_batch("CREATE TRIGGER no_stage BEFORE INSERT ON protocol_projection BEGIN SELECT RAISE(ABORT,'staged'); END; CREATE TRIGGER no_fact BEFORE INSERT ON journal BEGIN SELECT RAISE(ABORT,'staged'); END;").unwrap();
        assert_eq!(
            capacity(store.commit_changes(2, &grow).unwrap_err()),
            CapacityExceeded {
                limit: "projected_state_bytes",
                maximum: MAX_PROJECTED_STATE_BYTES,
                projected: MAX_PROJECTED_STATE_BYTES + 2,
            }
        );
        assert_eq!(unchanged(&store), before, "refusal must stage nothing");
        store
            .conn
            .execute_batch("DROP TRIGGER no_stage; DROP TRIGGER no_fact;")
            .unwrap();
        // Exactly at the limit through a changed key: accepted, and a fresh
        // handle computes the same totals from the rows.
        assert_eq!(store.commit_changes(2, &blob(fill)).unwrap(), 3);
        let at_limit = (MAX_PROJECTED_STATE_BYTES, 1);
        let totals = store.totals.unwrap();
        assert_eq!((totals.bytes, totals.records), at_limit);
        let mut fresh = Store::open(root.path()).unwrap();
        assert_eq!(fresh.commit_changes(3, &Changes::new()).unwrap(), 3);
        assert_eq!(fresh.totals, store.totals);
        // The admission thresholds apply to the changed-key form too.
        let one = Changes::from([("e".to_owned(), Some(json!(1)))]);
        assert_eq!(
            capacity(store.commit_admission_changes(3, &one).unwrap_err()).limit,
            "admission_projected_state_bytes"
        );
    }

    #[test]
    fn unchanged_projection_is_not_refused_or_recommitted() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::open(root.path()).unwrap();
        let records = Records::from([("meta".to_owned(), json!({"generation":1}))]);
        assert_eq!(store.commit_protocol(0, &records).unwrap(), 1);
        let before = unchanged(&store);
        assert_eq!(store.commit_protocol(1, &records).unwrap(), 1);
        assert_eq!(unchanged(&store), before);
    }
}
