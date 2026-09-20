//! The labeled fake Claude CLI speaks the stream shapes measured from 2.1.278
//! over the same stdio the adapter drives. No real Claude Code runs, so this
//! runs in CI on a machine with none installed.
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// Writing the wrapper and exec'ing it race across the tests in this binary: a
/// sibling test's fork inherits the still-open write descriptor and Linux then
/// refuses the exec with `ETXTBSY`. `Command::spawn` returns only once the child
/// has exec'd, so holding this across write and spawn closes the window.
static FAKE_EXECUTABLES: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn wrapper(dir: &Path) -> PathBuf {
    let path = dir.join("fake-claude");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nexec '{}' claude fake-cli \"$@\"\n",
            env!("CARGO_BIN_EXE_pio")
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

const STREAM_ARGS: &[&str] = &[
    "--print",
    "--input-format",
    "stream-json",
    "--output-format",
    "stream-json",
    "--verbose",
    "--replay-user-messages",
];

fn spawn(dir: &Path, scenario: &Value, extra: &[&str]) -> Child {
    let guard = FAKE_EXECUTABLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let exe = wrapper(dir);
    let child = Command::new(&exe)
        .args(STREAM_ARGS)
        .args(extra)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", dir)
        .env("PIO_CLAUDE_FAKE_SCENARIO", scenario.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    drop(guard);
    child
}

fn user_message(text: &str) -> Value {
    json!({"type":"user","message":{"role":"user","content":[{"type":"text","text":text}]}})
}

/// Drive one turn, answering any permission request with `decision`.
fn turn(dir: &Path, scenario: &Value, decision: Option<Value>) -> Vec<Value> {
    let mut child = spawn(dir, scenario, &["--permission-mode", "acceptEdits"]);
    let mut stdin = child.stdin.take().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    writeln!(stdin, "{}", user_message("do the fixture task")).unwrap();
    stdin.flush().unwrap();
    let mut messages = Vec::new();
    for line in stdout.lines() {
        let line = line.unwrap();
        if line.trim().is_empty() {
            continue;
        }
        let message: Value = serde_json::from_str(&line).unwrap();
        if message["type"] == "control_request"
            && let Some(decision) = &decision
        {
            writeln!(
                stdin,
                "{}",
                json!({"type":"control_response","response":{
                    "subtype":"success","request_id":&message["request_id"],
                    "response":decision}})
            )
            .unwrap();
            stdin.flush().unwrap();
        }
        messages.push(message);
    }
    drop(stdin);
    child.wait().unwrap();
    messages
}

fn kinds(messages: &[Value]) -> Vec<String> {
    messages
        .iter()
        .map(|m| match m["subtype"].as_str() {
            Some(subtype) => format!("{}/{subtype}", m["type"].as_str().unwrap_or("?")),
            None => m["type"].as_str().unwrap_or("?").to_owned(),
        })
        .collect()
}

fn markers(dir: &Path) -> Vec<Value> {
    let path = dir.join("markers/fake-claude-cli.jsonl");
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn a_turn_completes_and_the_replay_echoes_exactly_what_was_sent() {
    let dir = tempfile::tempdir().unwrap();
    let messages = turn(dir.path(), &json!({}), None);
    assert_eq!(
        kinds(&messages),
        ["system/init", "user", "assistant", "result/success"]
    );
    let replay = &messages[1];
    assert_eq!(replay["isReplay"], true);
    // The delivery proof is an exact echo, not a receipt the fake invented.
    assert_eq!(
        replay["message"],
        user_message("do the fixture task")["message"]
    );
    assert_eq!(messages[0]["permissionMode"], "acceptEdits");
    assert_eq!(messages[3]["is_error"], false);
}

/// Measured on the real harness: nothing at all arrives until stdin carries a
/// message. The fake must reproduce that, or a matrix case could prove a
/// pre-flight check the real transport does not allow.
#[test]
fn nothing_is_emitted_before_a_message_reaches_stdin() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = spawn(dir.path(), &json!({}), &[]);
    let stdout = child.stdout.take().unwrap();
    // A reader thread, because a buffered reader that has not read yet is
    // trivially empty: the assertion has to be about the pipe, not the buffer.
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if sender.send(line.unwrap()).is_err() {
                return;
            }
        }
    });
    assert_eq!(
        receiver.recv_timeout(std::time::Duration::from_millis(750)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout),
        "the fake emitted a message before anything was written to stdin"
    );
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "{}", user_message("go")).unwrap();
    stdin.flush().unwrap();
    let first: Value = serde_json::from_str(
        &receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("no init after the write"),
    )
    .unwrap();
    assert_eq!(first["subtype"], "init");
    drop(stdin);
    child.wait().unwrap();
}

#[test]
fn an_allow_is_single_use_and_echoes_the_input_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let scenario = json!({
        "permission_request":{"tool_name":"Bash","input":{"command":"ls -la"}},
        "markers":dir.path().join("markers")});
    let messages = turn(
        dir.path(),
        &scenario,
        Some(json!({"behavior":"allow","updatedInput":{"command":"ls -la"}})),
    );
    assert!(kinds(&messages).contains(&"control_request".to_owned()));
    let result = messages.last().unwrap();
    assert_eq!(result["permission_decision"], "allow");
    assert_eq!(result["permission_denials"], 0);
    let decision = &markers(dir.path())
        .into_iter()
        .find(|m| m["event"] == "permission_decision")
        .expect("no decision recorded");
    assert_eq!(decision["input_echoed_unchanged"], true);
    assert_eq!(decision["widening_fields_received"], json!([]));
}

#[test]
fn a_deny_is_counted_and_widens_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let scenario = json!({
        "permission_request":{"tool_name":"Bash","input":{"command":"ls /etc"}},
        "markers":dir.path().join("markers")});
    let messages = turn(
        dir.path(),
        &scenario,
        Some(json!({"behavior":"deny","message":"outside the fixture"})),
    );
    let result = messages.last().unwrap();
    assert_eq!(result["permission_decision"], "deny");
    assert_eq!(result["permission_denials"], 1);
    let decision = &markers(dir.path())
        .into_iter()
        .find(|m| m["event"] == "permission_decision")
        .expect("no decision recorded");
    assert_eq!(decision["widening_fields_received"], json!([]));
}

/// The fake records a widening attempt rather than refusing it, so the matrix
/// proves PIO never sends one instead of trusting that it does not. This test
/// is the proof that the detector works; no PIO code path produces it.
#[test]
fn the_fake_detects_a_widening_response_so_its_absence_is_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let scenario = json!({
        "permission_request":{"tool_name":"Bash","input":{"command":"ls"}},
        "markers":dir.path().join("markers")});
    turn(
        dir.path(),
        &scenario,
        Some(json!({"behavior":"allow","updatedInput":{"command":"ls"},
                    "updatedPermissions":[{"type":"addRules","destination":"userSettings"}]})),
    );
    let decision = &markers(dir.path())
        .into_iter()
        .find(|m| m["event"] == "permission_decision")
        .expect("no decision recorded");
    assert_eq!(
        decision["widening_fields_received"],
        json!(["updatedPermissions"])
    );
}

#[test]
fn tool_uses_are_reported_so_the_receipt_can_record_every_one() {
    let dir = tempfile::tempdir().unwrap();
    let scenario = json!({"tool_uses":[
        {"name":"Read","input":{"file_path":"a.txt"}},
        {"name":"Edit","input":{"file_path":"/etc/hosts"}}]});
    let messages = turn(dir.path(), &scenario, None);
    let fixture = dir.path().join("fixture");
    std::fs::create_dir_all(&fixture).unwrap();
    let record = pio_claude::tool_use_records(&messages, &fixture, &fixture);
    let uses = record["tool_uses"].as_array().unwrap();
    assert_eq!(uses.len(), 2);
    assert_eq!(uses[1]["placement"], "outside_fixture");
    assert_eq!(record["out_of_fixture_effect_observed"], true);
    assert_eq!(record["liability"], "unresolved");
}

/// The same fake answers the non-streaming surfaces, so one executable can be
/// qualified, drifted and driven.
#[test]
fn the_fake_answers_version_help_and_route_so_it_can_be_qualified() {
    let dir = tempfile::tempdir().unwrap();
    let guard = FAKE_EXECUTABLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let exe = wrapper(dir.path());
    let run = |args: &[&str], scenario: &str| -> (i32, String) {
        let out = Command::new(&exe)
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("PIO_CLAUDE_FAKE_SCENARIO", scenario)
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        )
    };
    assert!(
        run(&["--version"], "{}")
            .1
            .starts_with(pio_claude::PINNED_VERSION)
    );
    assert_eq!(
        run(&["--version"], r#"{"version":"9.9.9"}"#).1.trim(),
        "9.9.9 (Claude Code)"
    );
    // `auth --help` is a help, not a route query.
    assert!(run(&["auth", "--help"], "{}").1.contains("help for auth"));
    let (status, route) = run(&["auth", "status"], "{}");
    assert_eq!(status, 0);
    assert_eq!(
        serde_json::from_str::<Value>(&route).unwrap()["authMethod"],
        "claude.ai"
    );
    let (status, route) = run(&["auth", "status"], r#"{"route":null}"#);
    assert_eq!(status, 1, "a missing route must not exit 0");
    assert_eq!(
        serde_json::from_str::<Value>(&route).unwrap()["loggedIn"],
        false
    );
    // The help suffix moves the surface digest, which is how a drift case is
    // built without a second executable.
    assert_ne!(
        run(&["--help"], "{}").1,
        run(&["--help"], r#"{"help_suffix":" drifted"}"#).1
    );
    drop(guard);
}
