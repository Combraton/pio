//! Journal-backed protocol read projections. The journal and outbox are the
//! authority; rows are individually replaceable, replayable derived state.
use crate::{Store, append};
use anyhow::{Result, ensure};
use rusqlite::{TransactionBehavior, params};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub type Records = BTreeMap<String, Value>;
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
    /// a commit made through another connection.
    pub fn commit_protocol(&mut self, expected: u64, records: &Records) -> Result<u64> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let revision: u64 = tx.query_row("SELECT revision FROM protocol_head", [], |r| r.get(0))?;
        ensure!(revision == expected, "protocol_writer_fenced");
        let mut old = Records::new();
        {
            let mut q = tx.prepare("SELECT key,value FROM protocol_projection")?;
            for row in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
                let (key, value) = row?;
                old.insert(key, serde_json::from_str(&value)?);
            }
        }
        let mut changes = Vec::new();
        for (key, value) in records {
            if old.get(key) != Some(value) {
                tx.execute("INSERT INTO protocol_projection VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,serde_json::to_string(value)?])?;
                changes.push(json!({"key":key,"value":value}));
            }
        }
        for key in old.keys().filter(|k| !records.contains_key(*k)) {
            tx.execute("DELETE FROM protocol_projection WHERE key=?1", [key])?;
            changes.push(json!({"key":key,"delete":true}));
        }
        if changes.is_empty() {
            return Ok(revision);
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
}
