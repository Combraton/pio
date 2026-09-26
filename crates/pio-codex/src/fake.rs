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
//!
//! For lead runs (M4, L3): `model_reported` and `model_provider` are what
//! `thread/start` answers (default: the model asked for, else
//! `pio-fake-model`, on `pio-fake`), so the host's check of Codex's own answer
//! has something to refuse. A thread whose `config.mcp_servers` names servers
//! launches them and lists their tools. `lead.calls` (with `until`, `repeat`,
//! `report_as`, and `lead.relay_offset`) plays a lead through its first
//! server, asking by `mcpServer/elicitation/request` wherever Codex would
//! (`fake_turn.rs`). `answer_line_counts` plays a led run instead: the one
//! command its prompt quotes, `led_delay_ms` long where the prompt contains
//! `led_delay_if`, then the line count (`led_offset`), asking a command
//! approval first where it contains `command_approval_if`. `usage_step` is
//! each model step's tokens (`lead_usage_step` for the lead's, where given); `led_heavy_step` replaces the first step's (the
//! one that runs the command) where the prompt contains `led_heavy_if`. A
//! model step takes `model_step_ms` (default 1,500), or `led_step_ms` in a led
//! run, before it says or calls anything. Usage is reported as Codex was measured reporting it: once a
//! step and its tool have finished, and, for the step an interrupt cuts
//! short, after the interrupt (M2 R1, R3, R5, R6); `usage_suppressed` reports
//! none at all. Shapes PIO declines by itself, for the runner's
//! mutants: `lead_asks_in_mode` has the lead ask before every tool call (or
//! only calls to `lead_asks_for`), in that elicitation mode, whatever the
//! approval mode says, and `lead_asks_by: "requestUserInput"` has it ask by
//! `item/tool/requestUserInput` instead; a led run whose
//! prompt contains `led_permissions_if` first asks a permission grant;
//! `elicit_during_thread_start` sends an url-mode elicitation before it answers
//! `thread/start`. A lead call whose `tool` is `!shell` plays a step that ends
//! in Codex's own shell tool instead of an MCP call. A led run whose prompt
//! contains `led_ignores_interrupt_if` acknowledges `turn/interrupt` and goes
//! on; `led_answer_ms` is how long a led run's answering step takes.
//! Where a led run's prompt contains `led_step_if`, every step is
//! `led_step` tokens; where it contains `led_commands_if`, it runs its
//! command `led_commands` times, a step each (the later ones
//! `led_repeat_delay_ms` long), before it answers.
//! `spawn_agent_if` (a led run's prompt) or `spawn_agent` (the plain turn)
//! spawns a sub-agent as Codex 0.157.0 does (`fake_turn::spawn_agent`):
//! `subagent_steps`, `subagent_step`, `subagent_step_ms` and
//! `subagent_asks`; none is spawned when the thread's config sets
//! `agents.enabled = false` and does not turn `features.multi_agent_v2` on.
//! `elicit_during` (a list of `initialize`, `account/read`, `thread/start`)
//! sends a url-mode elicitation before answering each wait it names;
//! `exit_after_early` then exits once the client has answered it.
//! `memory_pipeline` writes, at the first turn, what Codex's memory pipeline
//! would under the Codex home (`memories/`, `memories_1.sqlite`).
use crate::fake_turn::{self, McpServer, Turn, Waiting, emit};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const SOURCE: &str = "pio-fake-app-server";

pub(crate) fn marker(dir: &Option<PathBuf>, record: Value) -> Result<()> {
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

/// A request sent before the answer to `wait`, where `elicit_during` names
/// it (the waits a host has before its first turn, review of L3, round 3,
/// R3-HC-3 and C3-2). With `exit_after_early`, its id: the fake then leaves
/// `wait` unanswered and exits once the client has answered the request, so
/// the client's wait fails after it has declined.
fn early_request(scenario: &Value, wait: &str) -> Result<Option<String>> {
    let named = scenario["elicit_during"]
        .as_array()
        .is_some_and(|waits| waits.iter().any(|w| w == wait));
    if !named {
        return Ok(None);
    }
    let id = format!("fake-early-{}", wait.replace('/', "-"));
    emit(&json!({"method":"mcpServer/elicitation/request","id":id,
                 "params":{"threadId":null,"turnId":null,"serverName":"someone","mode":"url",
                           "elicitationId":format!("fake-{wait}"),
                           "url":"https://example.invalid/early",
                           "message":"Sign in before we start"}}))?;
    Ok((scenario["exit_after_early"] == true).then_some(id))
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
    let send = |message: Value| -> Result<()> { emit(&message) };
    // Lead runs: the thread's MCP servers, the turn a script is playing, and
    // the requests it is waiting on the client for.
    let servers: Arc<Mutex<Vec<McpServer>>> = Arc::new(Mutex::new(Vec::new()));
    let waiting: Waiting = Arc::new(Mutex::new(Default::default()));
    let mut scripted: Option<Arc<Turn>> = None;
    let mut initialized = false;
    let mut thread: Option<String> = None;
    let mut sandbox = "read-only".to_owned();
    let mut cwd = String::new();
    // Active turn: id, completion deadline, pending approval request id.
    let mut active: Option<(String, Instant, Option<String>)> = None;
    let mut turns = 0u64;
    // Every response the client sent, by request id: a request answered
    // twice is a defect the host's own record cannot show (review of L3,
    // CH-3).
    let mut responses: std::collections::HashMap<String, u32> = Default::default();
    // The early request whose answer ends the fake, where the scenario says.
    let mut leave_after: Option<String> = None;
    loop {
        if scripted.as_ref().is_some_and(|turn| turn.finished()) {
            scripted = None;
        }
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
                    if let Some(key) = id.as_ref().map(Value::to_string) {
                        let count = responses.entry(key).or_insert(0);
                        *count += 1;
                        marker(
                            &markers,
                            json!({"source":SOURCE,"kind":"response_received","id":id,
                                   "count":*count,"result":message.get("result"),
                                   "error":message.get("error")}),
                        )?;
                        if *count > 1 {
                            marker(
                                &markers,
                                json!({"source":SOURCE,"kind":"second_response","id":id}),
                            )?;
                        }
                    }
                    if leave_after.is_some()
                        && id.as_ref().and_then(Value::as_str) == leave_after.as_deref()
                    {
                        marker(
                            &markers,
                            json!({"source":SOURCE,"kind":"exiting_after_early"}),
                        )?;
                        return Ok(());
                    }
                    // A reply a scripted turn is waiting for.
                    let key = id.as_ref().and_then(Value::as_str).map(str::to_owned);
                    let waiter = key.and_then(|k| waiting.lock().expect("waiting lock").remove(&k));
                    if let Some(waiter) = waiter {
                        let _ = waiter.send(message.clone());
                        continue;
                    }
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
                        if let Some(early) = early_request(&scenario, "initialize")? {
                            leave_after = Some(early);
                            continue;
                        }
                        send(
                            json!({"id":id,"result":{"userAgent":SOURCE,"codexHome":home,"platformFamily":"unix","platformOs":std::env::consts::OS}}),
                        )?;
                    }
                    "initialized" => {}
                    "account/read" => {
                        if let Some(early) = early_request(&scenario, "account/read")? {
                            leave_after = Some(early);
                            continue;
                        }
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
                        // What this thread's config asked of each server's
                        // tools, as received: an independent witness of what
                        // PIO put on the wire (review of L3, CH-1). Never the
                        // command, arguments or environment.
                        let received: serde_json::Map<String, Value> = params["config"]
                            ["mcp_servers"]
                            .as_object()
                            .into_iter()
                            .flatten()
                            .map(|(name, spec)| {
                                (
                                    name.clone(),
                                    json!({"tools":spec["tools"],
                                           "default_tools_approval_mode":spec["default_tools_approval_mode"]}),
                                )
                            })
                            .collect();
                        // And what it said of agents, by the dotted keys a
                        // request override takes: with `agents.enabled`
                        // false and `multi_agent_v2` not on, Codex offers no
                        // collaboration tools whatever the model's catalog
                        // says (`Config::multi_agent_version_override`,
                        // rust-v0.157.0), so this fake spawns none.
                        let config = &params["config"];
                        let agents = config["agents.enabled"]
                            .as_bool()
                            .or(config["agents"]["enabled"].as_bool());
                        let v2 = config["features.multi_agent_v2"]
                            .as_bool()
                            .or(config["features"]["multi_agent_v2"].as_bool());
                        fake_turn::AGENTS_OFF.store(
                            agents == Some(false) && v2 != Some(true),
                            std::sync::atomic::Ordering::SeqCst,
                        );
                        marker(
                            &markers,
                            json!({"source":SOURCE,"kind":"thread_config_received","servers":received,
                                   "agents_enabled":agents,"multi_agent":config["features.multi_agent"],
                                   "multi_agent_v2":v2}),
                        )?;
                        // The servers this thread's own config names, launched
                        // and listed before the answer, as the M4b probe saw.
                        for (name, spec) in params["config"]["mcp_servers"]
                            .as_object()
                            .into_iter()
                            .flatten()
                        {
                            let server = McpServer::launch(name, spec)?;
                            marker(
                                &markers,
                                json!({"source":SOURCE,"kind":"mcp_server_launched","name":name}),
                            )?;
                            servers.lock().expect("servers lock").push(server);
                        }
                        // What Codex answers, which is not always what was
                        // asked; the scenario can make it differ.
                        let model = scenario["model_reported"]
                            .as_str()
                            .or(params["model"].as_str())
                            .unwrap_or("pio-fake-model");
                        let provider = scenario["model_provider"].as_str().unwrap_or("pio-fake");
                        let thread_value =
                            json!({"id":thread_id,"modelProvider":provider,"preview":""});
                        let mut result = json!({"thread":thread_value,"model":model,"modelProvider":provider,"cwd":cwd,"sandbox":sandbox_projection(&sandbox),"approvalPolicy":params["approvalPolicy"].as_str().unwrap_or("on-request"),
                        // Measured from the app-server's own schema: the
                        // enum is `user | auto_review | guardian_subagent`,
                        // and a thread nobody redirected answers `user`.
                        // The scenario can say otherwise so the host's
                        // refusal is something a case can reach.
                        "approvalsReviewer":scenario["approvals_reviewer"].as_str().unwrap_or("user")});
                        // `null` omits the field. The schema makes it
                        // required (0.155.1 and 0.157.0), so an answer without it is not from the
                        // qualified app-server, and silence is not `user`.
                        if let Some(Value::Null) = scenario.get("approvals_reviewer") {
                            result
                                .as_object_mut()
                                .context("thread/start result")?
                                .remove("approvalsReviewer");
                        }
                        if let Some(early) = early_request(&scenario, "thread/start")? {
                            leave_after = Some(early);
                            continue;
                        }
                        if scenario["elicit_during_thread_start"] == true {
                            // A server asking something before the thread is
                            // answered: Codex attaches the thread's listener
                            // first, and MCP elicitations need no turn
                            // (`turnId` is nullable at 0.157.0).
                            send(
                                json!({"method":"mcpServer/elicitation/request","id":"fake-early-1",
                                        "params":{"threadId":thread_id,"turnId":null,
                                                  "serverName":"someone","mode":"url",
                                                  "elicitationId":"fake-early",
                                                  "url":"https://example.invalid/early",
                                                  "message":"Sign in before we start"}}),
                            )?;
                        }
                        send(json!({"id":id,"result":result}))?;
                        send(json!({"method":"thread/started","params":{"thread":thread_value}}))?;
                    }
                    "turn/start" => {
                        turns += 1;
                        if turns == 1 && scenario["memory_pipeline"] == true {
                            // What Codex 0.157.0's memory pipeline writes, in the
                            // background, once a root thread's first turn starts
                            // with [features] memories on (read from source, not
                            // measured): Phase 2's files under `memories/`, one
                            // named for a session, and Phase 1's database.
                            let root = home.join("memories").join("rollout_summaries");
                            std::fs::create_dir_all(&root)?;
                            std::fs::write(
                                home.join("memories").join("raw_memories.md"),
                                "labeled fake raw memories\n",
                            )?;
                            std::fs::write(
                                root.join("2026-09-20T10-00-00-owner-private-topic.md"),
                                "labeled fake summary\n",
                            )?;
                            std::fs::write(home.join("memories_1.sqlite"), "labeled fake\n")?;
                        }
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
                        let lead = !servers.lock().expect("servers lock").is_empty()
                            && scenario["lead"]["calls"].is_array();
                        if lead || scenario["answer_line_counts"] == true {
                            let playing = Turn::new(turn.clone(), thread_id.clone());
                            playing.quiet.store(
                                scenario["usage_suppressed"] == true,
                                std::sync::atomic::Ordering::SeqCst,
                            );
                            scripted = Some(playing.clone());
                            let (script, queue, marks) =
                                (scenario.clone(), waiting.clone(), markers.clone());
                            if lead {
                                let servers = servers.clone();
                                std::thread::spawn(move || {
                                    fake_turn::lead(playing, servers, script, queue, marks)
                                });
                            } else {
                                let (dir, prompt) = (cwd.clone(), text.to_owned());
                                std::thread::spawn(move || {
                                    fake_turn::led(playing, script, queue, marks, dir, prompt)
                                });
                            }
                            continue;
                        }
                        let agent = scenario["agent_text"]
                            .as_str()
                            .unwrap_or("fake agent reply");
                        send(
                            json!({"method":"item/agentMessage/delta","params":{"threadId":thread_id,"turnId":turn,"itemId":"item-agent","delta":agent}}),
                        )?;
                        if scenario["spawn_agent"] == true {
                            fake_turn::spawn_agent(
                                &thread_id,
                                &turn,
                                &scenario,
                                waiting.clone(),
                                markers.clone(),
                            )?;
                        }
                        let delay = scenario["delay_ms"].as_u64().unwrap_or(200);
                        let pending = match scenario["approval"].as_str() {
                            Some(kind) => {
                                let request = format!("fake-request-{turns}");
                                let file_change = kind == "fileChange";
                                let permissions = kind == "permissions";
                                let method = match kind {
                                    "fileChange" => "item/fileChange/requestApproval",
                                    "permissions" => "item/permissions/requestApproval",
                                    "elicitation" => "mcpServer/elicitation/request",
                                    "user_input" => "item/tool/requestUserInput",
                                    _ => "item/commandExecution/requestApproval",
                                };
                                // Where the command would run: the thread's
                                // directory unless the scenario names another.
                                let approval_cwd =
                                    scenario["approval_cwd"].as_str().unwrap_or(&cwd);
                                // `startedAtMs` is required in both command and file-change
                                // approvals at 0.155.1 and 0.157.0 (review of L3, REPIN-7).
                                // Codex's reason: null in both command approvals
                                // M2 measured (R5, R6), a string only where a
                                // case names one (review of L3, round 2, V-9).
                                let reason = scenario["approval_reason"].clone();
                                let mut params = json!({"threadId":thread_id,"turnId":turn,"itemId":"item-approval","command":"echo fixture","cwd":approval_cwd,"reason":reason,"startedAtMs":fake_turn::now_ms()});
                                if permissions {
                                    // The real request asks for a profile, not a
                                    // decision.
                                    params = json!({"threadId":thread_id,"turnId":turn,"itemId":"item-approval","cwd":cwd,"startedAtMs":0,"reason":"labeled fake permission grant request",
                                                    // 0.157.0's RequestPermissionProfile: camelCase,
                                                    // both members present (review of L3, round 2, HR-5).
                                                    "permissions":{"fileSystem":{"write":[cwd]},"network":null}});
                                }
                                if file_change {
                                    // FileChangeRequestApprovalParams at 0.157.0:
                                    // no command and no cwd, and a grantRoot where
                                    // the scenario asks for writes under a root.
                                    params = json!({"threadId":thread_id,"turnId":turn,"itemId":"item-approval",
                                                    "reason":reason,
                                                    "startedAtMs":fake_turn::now_ms()});
                                    if let Some(root) = scenario["approval_grant_root"].as_str() {
                                        params["grantRoot"] = json!(root);
                                    }
                                }
                                // A network ask in 0.157.0's shape, and a command
                                // approval that names no cwd (it is optional):
                                // the two cases a relay must not answer (review
                                // of L3, round 2, HR-3).
                                if scenario["approval_network"] == true {
                                    params["networkApprovalContext"] =
                                        json!({"host":"example.invalid","protocol":"https"});
                                }
                                if scenario["approval_no_cwd"] == true
                                    && let Some(fields) = params.as_object_mut()
                                {
                                    fields.remove("cwd");
                                }
                                if kind == "user_input" {
                                    // Codex's other way to ask about an MCP tool
                                    // call (0.157.0): a question whose id begins
                                    // mcp_tool_call_approval, text PIO must never
                                    // record.
                                    params = json!({"threadId":thread_id,"turnId":turn,"itemId":"item-approval",
                                                    "questions":[{"id":"mcp_tool_call_approval_call-1","header":"Approve app tool call?",
                                                                  "question":"Allow the someone MCP server to run tool \"go\"? SENTINEL-question",
                                                                  "isOther":false,"isSecret":false,
                                                                  "options":[{"label":"Allow","description":"SENTINEL-option"}]}]});
                                }
                                if kind == "elicitation" {
                                    // An elicitation that is not an MCP
                                    // tool-call approval: a server asking for
                                    // a login, which PIO never supplies; or,
                                    // with `elicitation_mode: "form"`, a form
                                    // asking for data, in the same mode as a
                                    // tool-call approval but without its
                                    // `_meta.codex_approval_kind` (review of
                                    // L3, CH-5).
                                    params = if scenario["elicitation_mode"] == "form" {
                                        json!({"threadId":thread_id,"turnId":turn,"serverName":"someone","mode":"form","message":"Which region should I use?","requestedSchema":{"type":"object","properties":{"region":{"type":"string"}},"required":["region"]},
                                               // Where real Codex puts a call's arguments:
                                               // never to be recorded (review of L3, round 2, HR-4).
                                               "_meta":{"tool_params":{"brief":"SENTINEL-arg"},"tool_description":"SENTINEL-desc","tool_title":"Region picker"}})
                                    } else {
                                        json!({"threadId":thread_id,"turnId":turn,"serverName":"someone","mode":"url","elicitationId":"fake-elicitation","url":"https://example.invalid/login","message":"Sign in to continue"})
                                    };
                                }
                                // Only command approvals carry `kind` at
                                // 0.155.1 and 0.157.0, and it is optional there too, so
                                // `absent` omits it and the client must read
                                // that as `command`.
                                let approval_kind =
                                    scenario["approval_kind"].as_str().unwrap_or("command");
                                if !file_change
                                    && !permissions
                                    && kind != "elicitation"
                                    && kind != "user_input"
                                    && approval_kind != "absent"
                                {
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
                    // A sub-agent's turn, by its own thread and turn.
                    "turn/interrupt"
                        if fake_turn::SUB_TURNS
                            .lock()
                            .expect("sub turns lock")
                            .iter()
                            .any(|sub| {
                                message["params"]["turnId"] == sub.id.as_str()
                                    && message["params"]["threadId"] == sub.thread.as_str()
                            }) =>
                    {
                        send(json!({"id":id,"result":{}}))?;
                        let sub = fake_turn::SUB_TURNS
                            .lock()
                            .expect("sub turns lock")
                            .iter()
                            .find(|sub| message["params"]["turnId"] == sub.id.as_str())
                            .cloned();
                        if let Some(sub) = sub {
                            marker(
                                &markers,
                                json!({"source":SOURCE,"kind":"sub_agent_interrupted","thread":sub.thread}),
                            )?;
                            sub.interrupt()?;
                        }
                    }
                    "turn/interrupt" if scripted.is_some() => {
                        send(json!({"id":id,"result":{}}))?;
                        // Codex honours an interrupt (M2); the script stops
                        // at its next step, the step in flight is reported,
                        // and the turn ends now.
                        if scripted
                            .as_ref()
                            .is_some_and(|turn| turn.deaf.load(std::sync::atomic::Ordering::SeqCst))
                        {
                            // Acknowledged, and ignored: the turn goes on.
                            marker(
                                &markers,
                                json!({"source":SOURCE,"kind":"interrupt_ignored"}),
                            )?;
                        } else if let Some(turn) = scripted.take() {
                            turn.interrupt()?;
                        }
                    }
                    "turn/interrupt" => {
                        send(json!({"id":id,"result":{}}))?;
                        if let Some((turn, _, _)) = active.take() {
                            send(
                                json!({"method":"turn/completed","params":{"threadId":thread.clone().unwrap_or_default(),"turn":{"id":turn,"status":"interrupted","items":[],"error":null}}}),
                            )?;
                        }
                    }
                    "turn/steer" if scripted.is_some() => {
                        let expected = message["params"]["expectedTurnId"].as_str();
                        match (&scripted, scenario["steer"] != false) {
                            (Some(turn), true) if Some(turn.id.as_str()) == expected => {
                                send(json!({"id":id,"result":{"turnId":turn.id}}))?;
                                marker(
                                    &markers,
                                    json!({"source":SOURCE,"kind":"steer_received","turn":turn.id}),
                                )?;
                            }
                            _ => send(
                                json!({"id":id,"error":{"code":-32600,"message":"invalid request: no matching steerable turn"}}),
                            )?,
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
