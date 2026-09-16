//! Experimental M1 journal. The fake interface is not the public Protocol service.
use anyhow::{Result, bail, ensure};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;
use uuid::Uuid;

pub fn participant() -> Value {
    serde_json::from_str(include_str!("../../../conformance/participant.json"))
        .expect("checked-in participant descriptor")
}

pub fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub pid: u32,
    /// Kernel supplied start identity, including boot identity on Linux.
    pub start: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Invocation {
    pub source: String,
    pub command_id: String,
    pub invocation_id: String,
    pub host_slot: String,
    pub host_generation: u64,
    pub controller_generation: u64,
    pub store_id: String,
    pub payload: Value,
    pub digest: String,
    pub phase: String,
    pub host: Option<ProcessIdentity>,
    pub child: Option<ProcessIdentity>,
    pub receipt: Option<Value>,
}

pub mod projections;

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(root: &Path) -> Result<Self> {
        let conn = Connection::open(root.join("journal.sqlite3"))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let mode: String = conn.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
        if mode != "wal" {
            conn.pragma_update(None, "journal_mode", "WAL")?;
        }
        conn.execute_batch("PRAGMA synchronous=FULL; PRAGMA fullfsync=ON;")?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        ensure!(version <= 2, "unsupported store schema {version}");
        if version < 2 {
            conn.execute_batch("BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS meta (singleton INTEGER PRIMARY KEY CHECK(singleton=1), store_id TEXT NOT NULL, generation INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS invocations (command_id TEXT PRIMARY KEY, state TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS journal (sequence INTEGER PRIMARY KEY AUTOINCREMENT, record TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS outbox (sequence INTEGER PRIMARY KEY REFERENCES journal(sequence), record TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS protocol_projection (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS protocol_head (singleton INTEGER PRIMARY KEY CHECK(singleton=1),revision INTEGER NOT NULL);
            INSERT OR IGNORE INTO protocol_head VALUES(1,0);
            PRAGMA user_version=2; COMMIT;")?;
            conn.execute(
                "INSERT OR IGNORE INTO meta VALUES (1, ?1, 0)",
                [Uuid::new_v4().to_string()],
            )?;
        }
        Ok(Self { conn })
    }

    pub fn identity(&self) -> Result<(String, u64)> {
        Ok(self.conn.query_row(
            "SELECT store_id,generation FROM meta WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }

    /// Requires the separate daemon lifetime lock. The external generation witness
    /// must be compared before and durably replaced after this transaction.
    pub fn advance_controller(&mut self) -> Result<(String, u64)> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE meta SET generation=generation+1 WHERE singleton=1",
            [],
        )?;
        let identity: (String, u64) =
            tx.query_row("SELECT store_id,generation FROM meta", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        append(
            &tx,
            json!({"kind":"controller.started", "store_id":identity.0, "controller_generation":identity.1}),
        )?;
        tx.commit()?;
        Ok(identity)
    }

    pub fn get(&self, command_id: &str) -> Result<Option<Invocation>> {
        let text: Option<String> = self
            .conn
            .query_row(
                "SELECT state FROM invocations WHERE command_id=?1",
                [command_id],
                |r| r.get(0),
            )
            .optional()?;
        text.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }

    /// This transaction binds identity, admission, launch intent, journal and outbox.
    /// Only the Inserted result permits the one launch attempt. A replay never does.
    pub fn admit(
        &mut self,
        command: &str,
        payload: Value,
        generation: u64,
        fail_write: bool,
    ) -> Result<(Invocation, bool)> {
        ensure!(
            !command.is_empty()
                && command.len() <= 128
                && command
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
            "invalid fake command id"
        );
        let digest = digest(&serde_json::to_vec(&payload)?);
        if fail_write {
            self.conn.execute_batch("PRAGMA query_only=ON;")?;
        }
        let result = (|| {
            let tx = self
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let old: Option<String> = tx
                .query_row(
                    "SELECT state FROM invocations WHERE command_id=?1",
                    [command],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(old) = old {
                let state: Invocation = serde_json::from_str(&old)?;
                ensure!(
                    state.digest == digest && state.payload == payload,
                    "idempotency_conflict"
                );
                return Ok((state, false));
            }
            let (store_id, current): (String, u64) =
                tx.query_row("SELECT store_id,generation FROM meta", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?;
            ensure!(current == generation, "stale_controller_generation");
            let state = Invocation {
                source: "fake-host".into(),
                command_id: command.into(),
                invocation_id: Uuid::new_v4().to_string(),
                host_slot: Uuid::new_v4().to_string(),
                host_generation: 1,
                controller_generation: generation,
                store_id,
                payload,
                digest,
                phase: "intent".into(),
                host: None,
                child: None,
                receipt: None,
            };
            tx.execute(
                "INSERT INTO invocations VALUES (?1,?2)",
                params![command, serde_json::to_string(&state)?],
            )?;
            append(&tx, json!({"kind":"invocation.intent", "invocation":state}))?;
            tx.commit()?;
            Ok((state, true))
        })();
        if fail_write {
            self.conn.execute_batch("PRAGMA query_only=OFF;")?;
        }
        result
    }

    /// Host transitions compare store, slot, generation and expected phase atomically.
    /// An already-running host may publish observations after controller takeover.
    pub fn transition(
        &mut self,
        expected: &Invocation,
        next: &str,
        host: Option<ProcessIdentity>,
        child: Option<ProcessIdentity>,
        receipt: Option<Value>,
    ) -> Result<Invocation> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let text: String = tx.query_row(
            "SELECT state FROM invocations WHERE command_id=?1",
            [&expected.command_id],
            |r| r.get(0),
        )?;
        let mut state: Invocation = serde_json::from_str(&text)?;
        let (store_id, generation): (String, u64) =
            tx.query_row("SELECT store_id,generation FROM meta", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        ensure!(
            store_id == expected.store_id
                && state.invocation_id == expected.invocation_id
                && state.host_slot == expected.host_slot
                && state.host_generation == expected.host_generation,
            "host_identity_fenced"
        );
        ensure!(state.phase == expected.phase, "phase_conflict");
        if matches!(next, "host_claimed" | "released") {
            ensure!(
                generation == expected.controller_generation,
                "stale_controller_generation"
            );
        }
        ensure!(
            matches!(
                (state.phase.as_str(), next),
                ("intent", "host_claimed")
                    | ("host_claimed", "parked")
                    | ("parked", "released")
                    | ("released", "completed")
                    | ("parked", "known_not_released")
                    | ("host_claimed", "known_not_released")
                    | ("intent", "known_not_released")
            ),
            "invalid transition"
        );
        state.phase = next.into();
        if host.is_some() {
            state.host = host;
        }
        if child.is_some() {
            state.child = child;
        }
        if receipt.is_some() {
            state.receipt = receipt;
        }
        tx.execute(
            "UPDATE invocations SET state=?1 WHERE command_id=?2",
            params![serde_json::to_string(&state)?, state.command_id],
        )?;
        append(
            &tx,
            json!({"kind":format!("invocation.{next}"),"invocation":state}),
        )?;
        tx.commit()?;
        Ok(state)
    }

    pub fn journal(&self) -> Result<Vec<Value>> {
        let mut stmt = self
            .conn
            .prepare("SELECT record FROM journal ORDER BY sequence")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
}

fn append(tx: &rusqlite::Transaction<'_>, mut record: Value) -> Result<()> {
    if record.get("source").is_none() {
        record["source"] = json!("fake-host");
    }
    let text = serde_json::to_string(&record)?;
    tx.execute("INSERT INTO journal(record) VALUES (?1)", [&text])?;
    tx.execute(
        "INSERT INTO outbox VALUES (?1,?2)",
        params![tx.last_insert_rowid(), text],
    )?;
    Ok(())
}

pub fn require_payload(payload: &Value) -> Result<u64> {
    let Some(object) = payload.as_object() else {
        bail!("fake payload must be an object")
    };
    ensure!(
        object.keys().all(|k| k == "duration_ms"),
        "unknown fake payload member"
    );
    let duration = payload
        .get("duration_ms")
        .and_then(Value::as_u64)
        .unwrap_or(5000);
    ensure!(
        (1..=60000).contains(&duration),
        "fake duration must be 1..60000 ms"
    );
    if payload.get("duration_ms").is_some() {
        ensure!(
            payload["duration_ms"].as_u64().is_some(),
            "duration_ms must be an integer"
        );
    }
    Ok(duration)
}

#[cfg(test)]
mod tests;

pub mod spool;
