//! Journal-backed protocol read projections. The journal and outbox are the
//! authority; rows are individually replaceable, replayable derived state.
use crate::{Store, append};
use anyhow::{Result, ensure};
use rusqlite::{TransactionBehavior, params};
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

    /// The external service lifetime lock provides the single writer. The
    /// revision check additionally refuses stale caches, rather than overwriting
    /// a commit made through another connection. A commit that changes the
    /// projection is refused with [`CapacityExceeded`] before any row is staged
    /// when the resulting projection would exceed the ADR 002 bound.
    pub fn commit_protocol(&mut self, expected: u64, records: &Records) -> Result<u64> {
        self.commit_bounded(expected, records, Bound::Hard)
    }

    /// Commit that admits new execution work. It must also stay within the
    /// lower admission thresholds, leaving room for already-admitted work.
    pub fn commit_admission(&mut self, expected: u64, records: &Records) -> Result<u64> {
        self.commit_bounded(expected, records, Bound::Admission)
    }

    fn commit_bounded(&mut self, expected: u64, records: &Records, bound: Bound) -> Result<u64> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let revision: u64 = tx.query_row("SELECT revision FROM protocol_head", [], |r| r.get(0))?;
        ensure!(revision == expected, "protocol_writer_fenced");
        // Stored text is the compact serialization of its value, so an
        // unchanged record reuses its stored length.
        let mut old = BTreeMap::<String, (Value, usize)>::new();
        {
            let mut q = tx.prepare("SELECT key,value FROM protocol_projection")?;
            for row in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
                let (key, value) = row?;
                let length = value.len();
                old.insert(key, (serde_json::from_str(&value)?, length));
            }
        }
        let mut bytes = 0;
        let mut upserts = Vec::new();
        for (key, value) in records {
            match old.get(key) {
                Some((current, length)) if current == value => {
                    bytes += (key.len() + length) as u64;
                }
                _ => {
                    let text = serde_json::to_string(value)?;
                    bytes += record_bytes(key, &text);
                    upserts.push((key, value, text));
                }
            }
        }
        let deletes: Vec<&String> = old.keys().filter(|k| !records.contains_key(*k)).collect();
        if upserts.is_empty() && deletes.is_empty() {
            return Ok(revision);
        }
        within_bound(bytes, records.len() as u64, bound)?;
        let mut changes = Vec::new();
        for (key, value, text) in upserts {
            tx.execute("INSERT INTO protocol_projection VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,text])?;
            changes.push(json!({"key":key,"value":value}));
        }
        for key in deletes {
            tx.execute("DELETE FROM protocol_projection WHERE key=?1", [key])?;
            changes.push(json!({"key":key,"delete":true}));
        }
        let next = revision + 1;
        append(
            &tx,
            json!({"kind":"protocol.commit","source":"pio-core-conformance","revision":next,"changes":changes}),
        )?;
        tx.execute("UPDATE protocol_head SET revision=?1", [next])?;
        tx.commit()?;
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
        eprintln!("no-op commit {:?}", started.elapsed());
        records.insert("event/x".into(), json!({"a":1}));
        let started = std::time::Instant::now();
        store.commit_protocol(revision, &records).unwrap();
        eprintln!("one-record commit {:?}", started.elapsed());
        let started = std::time::Instant::now();
        store.protocol_records().unwrap();
        eprintln!("projection load {:?}", started.elapsed());
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
