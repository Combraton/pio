//! The labeled fake app-server speaks the pinned JSON-RPC shapes through the
//! same stdio client the durable host uses. No real Codex runs.
use pio_codex::rpc::AppServer;
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

fn fake_executable(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("fake-codex");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nexec '{}' codex fake-app-server \"$@\"\n",
            env!("CARGO_BIN_EXE_pio")
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Writing the wrapper and exec'ing it race across the tests in this binary: a
/// sibling test's fork inherits the still-open write descriptor and Linux then
/// refuses the exec with `ETXTBSY`. `Command::spawn` returns only once the child
/// has exec'd, so holding this across write and spawn closes the window.
static FAKE_EXECUTABLES: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn start(dir: &Path, scenario: Value) -> AppServer {
    let home = dir.join("codex-home");
    std::fs::create_dir_all(&home).unwrap();
    let env = vec![
        ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
        ("CODEX_HOME".to_owned(), home.display().to_string()),
        ("PIO_CODEX_FAKE_SCENARIO".to_owned(), scenario.to_string()),
    ];
    let stderr = std::fs::File::create(dir.join("stderr")).unwrap();
    let _guard = FAKE_EXECUTABLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    AppServer::spawn(&fake_executable(dir), &env, stderr).unwrap()
}

fn next(server: &AppServer, method: &str) -> Value {
    loop {
        let message = server
            .receive(Duration::from_secs(10))
            .unwrap()
            .expect("message before timeout");
        if message["method"] == method {
            return message;
        }
    }
}

#[test]
fn fake_app_server_handshake_thread_turn_approval_and_usage() {
    let dir = tempfile::tempdir().unwrap();
    let markers = dir.path().join("markers");
    std::fs::create_dir_all(&markers).unwrap();
    let repo = dir.path().join("fixture");
    std::fs::create_dir_all(&repo).unwrap();
    let mut server = start(
        dir.path(),
        json!({"approval":"command","delay_ms":50,"usage_total":10,"markers":markers}),
    );
    let id = server.request("thread/start", json!({})).unwrap();
    let refused = server
        .wait_response(id, Duration::from_secs(10), |_| Ok(()))
        .unwrap();
    assert_eq!(refused["error"]["message"], "Not initialized");
    let id = server
        .request(
            "initialize",
            json!({"clientInfo":{"name":"pio_test","version":"0"}}),
        )
        .unwrap();
    let init = server
        .wait_response(id, Duration::from_secs(10), |_| Ok(()))
        .unwrap();
    assert_eq!(init["result"]["userAgent"], pio_codex::fake::SOURCE);
    server.notify("initialized").unwrap();
    let id = server
        .request("account/read", json!({"refreshToken":false}))
        .unwrap();
    let account = server
        .wait_response(id, Duration::from_secs(10), |_| Ok(()))
        .unwrap();
    assert_eq!(account["result"]["account"]["type"], "apiKey");
    let id = server
        .request(
            "thread/start",
            json!({"cwd":repo,"sandbox":"workspace-write","approvalPolicy":"on-request"}),
        )
        .unwrap();
    let thread = server
        .wait_response(id, Duration::from_secs(10), |_| Ok(()))
        .unwrap();
    let thread_id = thread["result"]["thread"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(thread["result"]["sandbox"]["type"], "workspaceWrite");
    let config = std::fs::read_to_string(dir.path().join("codex-home/config.toml")).unwrap();
    assert!(config.contains("trust_level = \"trusted\""));
    let id = server
        .request(
            "turn/start",
            json!({"threadId":thread_id,"input":[{"type":"text","text":"fixture task"}]}),
        )
        .unwrap();
    let turn = server
        .wait_response(id, Duration::from_secs(10), |_| Ok(()))
        .unwrap();
    let turn_id = turn["result"]["turn"]["id"].as_str().unwrap().to_owned();
    let request = next(&server, "item/commandExecution/requestApproval");
    assert_eq!(request["params"]["turnId"], turn_id);
    // 0.155.1 added `kind` to command approvals; `command` is its default.
    assert_eq!(request["params"]["kind"], "command");
    server
        .respond(&request["id"], json!({"decision":"decline"}))
        .unwrap();
    assert_eq!(
        next(&server, "serverRequest/resolved")["params"]["requestId"],
        request["id"]
    );
    assert_eq!(
        next(&server, "item/completed")["params"]["item"]["status"],
        "declined"
    );
    assert_eq!(
        next(&server, "thread/tokenUsage/updated")["params"]["tokenUsage"]["total"]["totalTokens"],
        10
    );
    assert_eq!(
        next(&server, "turn/completed")["params"]["turn"]["status"],
        "completed"
    );
    server.close_stdin();
    assert!(server.child.wait().unwrap().success());
    let records = std::fs::read_to_string(markers.join("fake-app-server.jsonl")).unwrap();
    let kinds: Vec<Value> = records
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap()["kind"].clone())
        .collect();
    assert_eq!(
        kinds,
        [
            json!("spawned"),
            json!("turn_received"),
            json!("approval_answered"),
            json!("exiting")
        ]
    );
}

#[test]
fn fake_app_server_interrupt_steer_and_suppressed_ack() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = start(dir.path(), json!({"delay_ms":60000}));
    let id = server
        .request(
            "initialize",
            json!({"clientInfo":{"name":"t","version":"0"}}),
        )
        .unwrap();
    server
        .wait_response(id, Duration::from_secs(10), |_| Ok(()))
        .unwrap();
    server.notify("initialized").unwrap();
    let id = server.request("thread/start", json!({})).unwrap();
    let thread = server
        .wait_response(id, Duration::from_secs(10), |_| Ok(()))
        .unwrap();
    let thread_id = thread["result"]["thread"]["id"].clone();
    let id = server
        .request(
            "turn/start",
            json!({"threadId":thread_id,"input":[{"type":"text","text":"x"}]}),
        )
        .unwrap();
    let turn = server
        .wait_response(id, Duration::from_secs(10), |_| Ok(()))
        .unwrap();
    let turn_id = turn["result"]["turn"]["id"].clone();
    let id = server
        .request(
            "turn/steer",
            json!({"threadId":thread_id,"expectedTurnId":"wrong","input":[]}),
        )
        .unwrap();
    assert!(
        server
            .wait_response(id, Duration::from_secs(10), |_| Ok(()))
            .unwrap()["error"]
            .is_object()
    );
    let id = server
        .request(
            "turn/steer",
            json!({"threadId":thread_id,"expectedTurnId":turn_id,"input":[]}),
        )
        .unwrap();
    assert_eq!(
        server
            .wait_response(id, Duration::from_secs(10), |_| Ok(()))
            .unwrap()["result"]["turnId"],
        turn_id
    );
    let id = server
        .request(
            "turn/interrupt",
            json!({"threadId":thread_id,"turnId":turn_id}),
        )
        .unwrap();
    assert_eq!(
        server
            .wait_response(id, Duration::from_secs(10), |_| Ok(()))
            .unwrap()["result"],
        json!({})
    );
    assert_eq!(
        next(&server, "turn/completed")["params"]["turn"]["status"],
        "interrupted"
    );
    server.close_stdin();
    assert!(server.child.wait().unwrap().success());

    let dir = tempfile::tempdir().unwrap();
    let mut server = start(dir.path(), json!({"ack_turn":false,"delay_ms":60000}));
    let id = server
        .request(
            "initialize",
            json!({"clientInfo":{"name":"t","version":"0"}}),
        )
        .unwrap();
    server
        .wait_response(id, Duration::from_secs(10), |_| Ok(()))
        .unwrap();
    let id = server.request("thread/start", json!({})).unwrap();
    let thread = server
        .wait_response(id, Duration::from_secs(10), |_| Ok(()))
        .unwrap();
    let id = server
        .request("turn/start", json!({"threadId":thread["result"]["thread"]["id"],"input":[{"type":"text","text":"x"}]}))
        .unwrap();
    let suppressed = server.wait_response(id, Duration::from_millis(500), |message| {
        assert_ne!(message["method"], "turn/started");
        Ok(())
    });
    assert!(suppressed.is_err(), "no acknowledgment when suppressed");
    server.close_stdin();
    assert!(server.child.wait().unwrap().success());
}

/// File-change approvals have no `kind` at 0.155.1 or 0.157.0; only command approvals do.
#[test]
fn fake_app_server_file_change_approval_carries_no_kind() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = start(dir.path(), json!({"approval":"fileChange","delay_ms":100}));
    let id = server.request("initialize", json!({})).unwrap();
    server
        .wait_response(id, Duration::from_secs(10), |_| Ok(()))
        .unwrap();
    let id = server.request("thread/start", json!({})).unwrap();
    let thread = server
        .wait_response(id, Duration::from_secs(10), |_| Ok(()))
        .unwrap();
    let id = server
        .request("turn/start", json!({"threadId":thread["result"]["thread"]["id"],"input":[{"type":"text","text":"x"}]}))
        .unwrap();
    server
        .wait_response(id, Duration::from_secs(10), |_| Ok(()))
        .unwrap();
    let request = next(&server, "item/fileChange/requestApproval");
    assert_eq!(request["params"]["kind"], Value::Null);
    assert!(!request["params"].as_object().unwrap().contains_key("kind"));
    server
        .respond(&request["id"], json!({"decision":"decline"}))
        .unwrap();
    server.close_stdin();
    assert!(server.child.wait().unwrap().success());
}
