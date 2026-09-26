//! The caller ledger behind `pio client submit` and `pio client reconcile`:
//! bounded, physically separate from the service journal, and written
//! before any I/O. It owns request identities and correlations, never
//! copied service execution state.
//!
//! Moved here from `pio_protocol::client` for M4 T1, with one change: it
//! reaches the service through `pio_client`, the public client the rest of
//! `pio client` and the screen use, rather than a wire of its own. What it
//! records and prints is unchanged: the response frame exactly as it came.
use anyhow::{Context, Result, ensure};
use pio_client::{Client, Credential, Options};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

struct Ledger {
    db: Connection,
}
impl Ledger {
    fn open(root: &Path) -> Result<Self> {
        secure_root(root)?;
        ensure!(
            !root.join("journal.sqlite3").exists(),
            "caller store must be separate from service store"
        );
        let db = Connection::open(root.join("caller.sqlite3"))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "synchronous", "FULL")?;
        db.pragma_update(None, "fullfsync", true)?;
        db.busy_timeout(Duration::from_secs(2))?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS operations (id TEXT PRIMARY KEY, endpoint TEXT NOT NULL, principal TEXT NOT NULL, request TEXT NOT NULL, basis TEXT NOT NULL, status TEXT NOT NULL, observation TEXT)")?;
        Ok(Self { db })
    }
    fn prepare(
        &mut self,
        endpoint: &str,
        principal: &str,
        request: &Value,
        basis: &Value,
    ) -> Result<(String, bool)> {
        ensure!(
            request["operation"] == "execution.submit",
            "caller ledger supports execution.submit only"
        );
        ensure!(
            request["command_id"].is_string() && request["command_digest"].is_string(),
            "stable command identity required"
        );
        let id = pio_client::digest(&pio_client::canonical(&json!([
            endpoint,
            principal,
            request["command_id"]
        ])));
        let raw = serde_json::to_string(request)?;
        ensure!(
            raw.len() <= 1048576 && serde_json::to_vec(basis)?.len() <= 65536,
            "caller record exceeds bound"
        );
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let prior: Option<(String, String)> = tx
            .query_row(
                "SELECT request,basis FROM operations WHERE id=?",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((old, old_basis)) = prior {
            ensure!(
                serde_json::from_str::<Value>(&old)? == *request
                    && serde_json::from_str::<Value>(&old_basis)? == *basis,
                "caller identity conflict"
            );
            return Ok((id, false));
        }
        let count: u64 = tx.query_row("SELECT count(*) FROM operations", [], |r| r.get(0))?;
        ensure!(
            count < 1024,
            "caller operation bound reached; explicit archival required"
        );
        tx.execute(
            "INSERT INTO operations VALUES (?,?,?,?,?,'pending',NULL)",
            params![id, endpoint, principal, raw, serde_json::to_string(basis)?],
        )?;
        tx.commit()?; // The only path to initial submission is after this FULL commit.
        Ok((id, true))
    }
    fn observe(&self, id: &str, response: &Value) -> Result<()> {
        let status = if response.get("result").is_some() {
            "acknowledged"
        } else {
            "pending"
        };
        self.db.execute(
            "UPDATE operations SET observation=?,status=? WHERE id=?",
            params![serde_json::to_string(response)?, status, id],
        )?;
        Ok(())
    }
}
/// The caller's own directory: created 0700 if absent, and refused unless
/// it is a real directory this user owns that nobody else can read. The
/// same rule the service applies to its store, kept here so the command
/// line needs nothing from the service's crates.
fn secure_root(root: &Path) -> Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    if !root.exists() {
        std::fs::create_dir(root)?;
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
    }
    let meta = std::fs::symlink_metadata(root)?;
    ensure!(
        meta.is_dir()
            && !meta.file_type().is_symlink()
            && meta.uid() == unsafe { libc::geteuid() }
            && meta.mode() & 0o077 == 0,
        "unsafe caller store directory"
    );
    Ok(())
}

/// The strict encoding/1 parse, for a request file: the ledger must hold
/// exactly what the service will read, so it refuses what the service
/// refuses (a duplicate key, a float, an unsafe integer, a noncharacter).
fn parse(bytes: &[u8]) -> Result<Value> {
    Ok(pio_client::encoding::parse(bytes)?)
}

/// A session for the ledger. Authentication or negotiation refused is an
/// error here, as it was: nothing can be submitted or reconciled.
fn connect(socket: &Path, credential: &Credential) -> Result<Client> {
    Client::connect(
        socket,
        credential,
        &Options {
            timeout: Duration::from_secs(5),
            grant: None,
            caller: "pio-caller".into(),
        },
    )
    .map_err(|failure| anyhow::anyhow!("{failure}"))
}

/// On restart reconcile pending identities before any retry. An empty query
/// does not prove non-submission after history loss; leave it pending explicitly.
pub fn run(
    root: &Path,
    socket: &Path,
    credential_path: &Path,
    request_path: Option<&Path>,
    basis_path: Option<&Path>,
) -> Result<Value> {
    let credential = Credential::read(credential_path)?;
    let principal = credential.principal();
    let endpoint = socket.to_str().context("UTF-8 socket path")?;
    let mut ledger = Ledger::open(root)?;
    if let Some(path) = request_path {
        let request = parse(&std::fs::read(path)?)?;
        let basis = if let Some(path) = basis_path {
            parse(&std::fs::read(path)?)?
        } else {
            json!({})
        };
        let (id, inserted) = ledger.prepare(endpoint, principal, &request, &basis)?;
        if !inserted {
            let (status, prior): (String, Option<String>) = ledger.db.query_row(
                "SELECT status,observation FROM operations WHERE id=?",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            if status == "acknowledged" {
                return Ok(
                    json!({"caller_operation":id,"response":serde_json::from_str::<Value>(&prior.context("missing acknowledgment")?)?,"caller_replay":true}),
                );
            }
        }
        if inserted {
            let mut wire = connect(socket, &credential)?;
            let response = wire.call_frame(&request)?;
            ledger.observe(&id, &response)?;
            return Ok(json!({"caller_operation":id,"response":response}));
        }
    }
    let rows=ledger.db.prepare("SELECT id,request FROM operations WHERE status='pending' AND endpoint=? AND principal=? ORDER BY rowid")?
        .query_map(params![endpoint,principal],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
    let mut wire = connect(socket, &credential)?;
    let mut outcomes = vec![];
    for (id, raw) in rows {
        let request: Value = serde_json::from_str(&raw)?;
        let mut query = json!({"operation":"execution.reconcile","message_id":uuid::Uuid::new_v4().to_string(),"payload":{"command_id":request["command_id"]}});
        if let Some(grant) = request.get("grant") {
            query["grant"] = grant.clone();
        }
        let observed = wire.call_frame(&query)?;
        if observed["result"]["executions"]
            .as_array()
            .is_some_and(|a| a.contains(&request["subject"]))
        {
            // Fetch the original acknowledgment by exact idempotent replay; no
            // new command, digest, generation, subject or authority is invented.
            let response = wire.call_frame(&request)?;
            ledger.observe(&id, &response)?;
            outcomes
                .push(json!({"caller_operation":id,"reconciliation":observed,"response":response}));
        } else {
            outcomes.push(json!({"caller_operation":id,"status":"pending","reason":"reconciliation_did_not_establish_submission","reconciliation":observed}));
        }
    }
    Ok(json!({"operations":outcomes}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_caller_store_others_can_read_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(Ledger::open(root.path()).is_err());
        let fresh = root.path().join("fresh");
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        Ledger::open(&fresh).unwrap();
        let mode = std::fs::metadata(&fresh).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }

    #[test]
    fn a_request_file_is_parsed_as_strictly_as_the_service_parses_it() {
        assert!(parse(br#"{"operation":"execution.submit","command_id":"c"}"#).is_ok());
        assert!(parse(br#"{"command_id":"c","command_id":"d"}"#).is_err());
        assert!(parse(br#"{"timeouts":{"delivery":1.5}}"#).is_err());
    }

    #[test]
    fn request_identity_precedes_io_and_conflicts_cannot_replace_it() {
        let root = tempfile::tempdir().unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut ledger = Ledger::open(root.path()).unwrap();
        let request = json!({"operation":"execution.submit","command_id":"c","command_digest":"digest","payload":{"correlation":"chosen scope"}});
        let (id, inserted) = ledger
            .prepare(
                "/absent.sock",
                "owner",
                &request,
                &json!({"policy":"user selected"}),
            )
            .unwrap();
        assert!(inserted);
        drop(ledger);
        let mut ledger = Ledger::open(root.path()).unwrap();
        let raw: String = ledger
            .db
            .query_row("SELECT request FROM operations WHERE id=?", [&id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(serde_json::from_str::<Value>(&raw).unwrap(), request);
        let mut changed = request.clone();
        changed["command_digest"] = "changed".into();
        assert!(
            ledger
                .prepare("/absent.sock", "owner", &changed, &json!({}))
                .is_err()
        );
    }
}
