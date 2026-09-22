//! Labeled fake Codex app-server for offline tests. It speaks the pinned
//! app-server JSON-RPC shapes over stdio but runs no model, tools or commands.
//! Every result it produces is labeled `pio-fake-app-server`; it is never a
//! qualified Codex and never real-harness evidence.
//!
//! Scenario (JSON in `PIO_CODEX_FAKE_SCENARIO`, all members optional):
//! `account` (`"apiKey"` default, or `null` for no account), `ack_turn`
//! (default true; false suppresses the `turn/start` response and
//! `turn/started`), `approval` (`null`, `"command"` or `"fileChange"`),
//! `delay_ms` before completion (default 200), `usage_total` (default 42;
//! `null` sends no usage), `agent_text`, `steer` (default true),
//! `markers` (directory for independent spawn and turn markers).
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::{Duration, Instant};

pub const SOURCE: &str = "pio-fake-app-server";

fn marker(dir: &Option<PathBuf>, record: Value) -> Result<()> {
    let Some(dir) = dir else { return Ok(()) };
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("fake-app-server.jsonl"))?;
    let mut bytes = serde_json::to_vec(&record)?;
    bytes.push(b'\n');
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

fn write_trust(home: &Path, cwd: &str) -> Result<()> {
    let path = home.join("config.toml");
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    let table = format!("[projects.{}]", serde_json::to_string(cwd)?);
    if !text.contains(&table) {
        if !text.is_empty() && !text.ends_with("\n\n") {
            text.push('\n');
        }
        text.push_str(&format!("{table}\ntrust_level = \"trusted\"\n"));
        std::fs::write(path, text)?;
    }
    Ok(())
}

fn sandbox_projection(mode: &str) -> Value {
    match mode {
        "workspace-write" => {
            json!({"type":"workspaceWrite","writableRoots":[],"networkAccess":false,"excludeTmpdirEnvVar":false,"excludeSlashTmp":false})
        }
        "danger-full-access" => json!({"type":"dangerFullAccess"}),
        _ => json!({"type":"readOnly","networkAccess":false}),
    }
}

pub fn run() -> Result<()> {
    let scenario: Value = std::env::var("PIO_CODEX_FAKE_SCENARIO")
        .ok()
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .context("PIO_CODEX_FAKE_SCENARIO is not JSON")?
        .unwrap_or(json!({}));
    let markers = scenario["markers"].as_str().map(PathBuf::from);
    let home = PathBuf::from(
        std::env::var("CODEX_HOME")
            .unwrap_or_else(|_| format!("{}/.codex", std::env::var("HOME").unwrap_or_default())),
    );
    marker(
        &markers,
        json!({"source":SOURCE,"kind":"spawned","pid":std::process::id()}),
    )?;
    let (sender, lines) = channel::<Option<Value>>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            match line.ok().and_then(|l| serde_json::from_str(&l).ok()) {
                Some(value) => {
                    if sender.send(Some(value)).is_err() {
                        return;
                    }
                }
                None => break,
            }
        }
        let _ = sender.send(None);
    });
    let out = std::io::stdout();
    let send = |message: Value| -> Result<()> {
        let mut lock = out.lock();
        lock.write_all(&serde_json::to_vec(&message)?)?;
        lock.write_all(b"\n")?;
        lock.flush()?;
        Ok(())
    };
    let mut initialized = false;
    let mut thread: Option<String> = None;
    let mut sandbox = "read-only".to_owned();
    let mut cwd = String::new();
    // Active turn: id, completion deadline, pending approval request id.
    let mut active: Option<(String, Instant, Option<String>)> = None;
    let mut turns = 0u64;
    loop {
        let timeout = active
            .as_ref()
            .filter(|(_, _, pending)| pending.is_none())
            .map(|(_, deadline, _)| deadline.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_millis(250));
        match lines.recv_timeout(timeout) {
            Ok(None) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                let due =
                    matches!(&active, Some((_, deadline, None)) if Instant::now() >= *deadline);
                if due {
                    let (turn, _, _) = active.take().expect("due turn");
                    let turn = &turn;
                    let thread_id = thread.clone().unwrap_or_default();
                    if let Some(total) = scenario
                        .get("usage_total")
                        .cloned()
                        .unwrap_or(json!(42))
                        .as_u64()
                    {
                        let breakdown = json!({"cachedInputTokens":0,"inputTokens":total/2,"outputTokens":total-total/2,"reasoningOutputTokens":0,"totalTokens":total});
                        send(
                            json!({"method":"thread/tokenUsage/updated","params":{"threadId":thread_id,"turnId":turn,"tokenUsage":{"last":breakdown,"total":breakdown}}}),
                        )?;
                    }
                    send(
                        json!({"method":"turn/completed","params":{"threadId":thread_id,"turn":{"id":turn,"status":"completed","items":[],"error":null}}}),
                    )?;
                }
            }
            Ok(Some(message)) => {
                let id = message.get("id").cloned();
                let method = message["method"].as_str().unwrap_or("").to_owned();
                if method.is_empty() {
                    // A response to our server request.
                    if let (Some(id), Some((turn, deadline, pending))) =
                        (id.clone(), active.as_mut())
                        && pending.as_deref() == id.as_str()
                    {
                        // A client that refuses answers with an error, not a
                        // decision. Record which one happened.
                        let refusal = message.get("error").cloned();
                        let decision = message["result"]["decision"].as_str().unwrap_or("cancel");
                        let status = if refusal.is_none() && decision == "accept" {
                            "completed"
                        } else {
                            "declined"
                        };
                        let thread_id = thread.clone().unwrap_or_default();
                        send(
                            json!({"method":"serverRequest/resolved","params":{"threadId":thread_id,"requestId":id}}),
                        )?;
                        let item_type = match scenario["approval"].as_str() {
                            Some("fileChange") => "fileChange",
                            Some("permissions") => "permissions",
                            _ => "commandExecution",
                        };
                        send(
                            json!({"method":"item/completed","params":{"threadId":thread_id,"turnId":turn,"item":{"type":item_type,"id":"item-approval","command":"echo fixture","cwd":cwd,"status":status,"commandActions":[]}}}),
                        )?;
                        marker(
                            &markers,
                            match &refusal {
                                Some(error) => {
                                    json!({"source":SOURCE,"kind":"approval_refused","code":error["code"],"message":error["message"]})
                                }
                                None => {
                                    json!({"source":SOURCE,"kind":"approval_answered","decision":decision})
                                }
                            },
                        )?;
                        *pending = None;
                        *deadline = Instant::now() + Duration::from_millis(50);
                    }
                    continue;
                }
                if !initialized && method != "initialize" && method != "initialized" {
                    if let Some(id) = id {
                        send(json!({"id":id,"error":{"code":-32600,"message":"Not initialized"}}))?;
                    }
                    continue;
                }
                match method.as_str() {
                    "initialize" => {
                        initialized = true;
                        send(
                            json!({"id":id,"result":{"userAgent":SOURCE,"codexHome":home,"platformFamily":"unix","platformOs":std::env::consts::OS}}),
                        )?;
                    }
                    "initialized" => {}
                    "account/read" => {
                        let account = match scenario.get("account") {
                            Some(Value::Null) => Value::Null,
                            Some(Value::String(kind)) => json!({"type":kind}),
                            _ => json!({"type":"apiKey"}),
                        };
                        send(
                            json!({"id":id,"result":{"account":account,"requiresOpenaiAuth":true}}),
                        )?;
                    }
                    "thread/start" => {
                        let params = &message["params"];
                        sandbox = params["sandbox"].as_str().unwrap_or("read-only").to_owned();
                        cwd = params["cwd"].as_str().unwrap_or("").to_owned();
                        if !cwd.is_empty()
                            && matches!(sandbox.as_str(), "workspace-write" | "danger-full-access")
                        {
                            std::fs::create_dir_all(&home)?;
                            write_trust(&home, &cwd)?;
                        }
                        let thread_id = format!("fake-thread-{}", std::process::id());
                        thread = Some(thread_id.clone());
                        let thread_value =
                            json!({"id":thread_id,"modelProvider":"pio-fake","preview":""});
                        send(
                            json!({"id":id,"result":{"thread":thread_value,"model":"pio-fake-model","modelProvider":"pio-fake","cwd":cwd,"sandbox":sandbox_projection(&sandbox),"approvalPolicy":params["approvalPolicy"].as_str().unwrap_or("on-request"),
                        // Measured from the app-server's own schema: the
                        // enum is `user | auto_review | guardian_subagent`,
                        // and a thread nobody redirected answers `user`.
                        // The scenario can say otherwise so the host's
                        // refusal is something a case can reach.
                        "approvalsReviewer":scenario["approvals_reviewer"].as_str().unwrap_or("user")}}),
                        )?;
                        send(json!({"method":"thread/started","params":{"thread":thread_value}}))?;
                    }
                    "turn/start" => {
                        turns += 1;
                        let turn = format!("fake-turn-{turns}");
                        let text = message["params"]["input"][0]["text"].as_str().unwrap_or("");
                        marker(
                            &markers,
                            json!({"source":SOURCE,"kind":"turn_received","turn":turn,"input_sha256":crate::sha256_hex(text.as_bytes())}),
                        )?;
                        let thread_id = thread.clone().unwrap_or_default();
                        if scenario["ack_turn"] != false {
                            send(
                                json!({"id":id,"result":{"turn":{"id":turn,"status":"inProgress","items":[],"error":null}}}),
                            )?;
                            send(
                                json!({"method":"turn/started","params":{"threadId":thread_id,"turn":{"id":turn,"status":"inProgress","items":[],"error":null}}}),
                            )?;
                        }
                        let agent = scenario["agent_text"]
                            .as_str()
                            .unwrap_or("fake agent reply");
                        send(
                            json!({"method":"item/agentMessage/delta","params":{"threadId":thread_id,"turnId":turn,"itemId":"item-agent","delta":agent}}),
                        )?;
                        let delay = scenario["delay_ms"].as_u64().unwrap_or(200);
                        let pending = match scenario["approval"].as_str() {
                            Some(kind) => {
                                let request = format!("fake-request-{turns}");
                                let file_change = kind == "fileChange";
                                let permissions = kind == "permissions";
                                let method = match kind {
                                    "fileChange" => "item/fileChange/requestApproval",
                                    "permissions" => "item/permissions/requestApproval",
                                    _ => "item/commandExecution/requestApproval",
                                };
                                let mut params = json!({"threadId":thread_id,"turnId":turn,"itemId":"item-approval","command":"echo fixture","cwd":cwd,"reason":"labeled fake approval request"});
                                if permissions {
                                    // The real request asks for a profile, not a
                                    // decision.
                                    params = json!({"threadId":thread_id,"turnId":turn,"itemId":"item-approval","cwd":cwd,"startedAtMs":0,"reason":"labeled fake permission grant request","permissions":{"filesystem":{"write":[cwd]}}});
                                }
                                // Only command approvals carry `kind` at
                                // 0.155.1, and it is optional there too, so
                                // `absent` omits it and the client must read
                                // that as `command`.
                                let approval_kind =
                                    scenario["approval_kind"].as_str().unwrap_or("command");
                                if !file_change && !permissions && approval_kind != "absent" {
                                    params["kind"] = json!(approval_kind);
                                }
                                send(json!({"method":method,"id":request,"params":params}))?;
                                Some(request)
                            }
                            None => None,
                        };
                        active =
                            Some((turn, Instant::now() + Duration::from_millis(delay), pending));
                    }
                    "turn/interrupt" => {
                        send(json!({"id":id,"result":{}}))?;
                        if let Some((turn, _, _)) = active.take() {
                            send(
                                json!({"method":"turn/completed","params":{"threadId":thread.clone().unwrap_or_default(),"turn":{"id":turn,"status":"interrupted","items":[],"error":null}}}),
                            )?;
                        }
                    }
                    "turn/steer" => {
                        let expected = message["params"]["expectedTurnId"].as_str();
                        match (&active, scenario["steer"] != false) {
                            (Some((turn, _, _)), true) if Some(turn.as_str()) == expected => {
                                send(json!({"id":id,"result":{"turnId":turn}}))?;
                            }
                            _ => send(
                                json!({"id":id,"error":{"code":-32600,"message":"invalid request: no matching steerable turn"}}),
                            )?,
                        }
                    }
                    _ => {
                        if let Some(id) = id {
                            send(
                                json!({"id":id,"error":{"code":-32601,"message":format!("{SOURCE} does not implement {method}")}}),
                            )?;
                        }
                    }
                }
            }
        }
    }
    marker(
        &markers,
        json!({"source":SOURCE,"kind":"exiting","sandbox":sandbox}),
    )?;
    Ok(())
}
