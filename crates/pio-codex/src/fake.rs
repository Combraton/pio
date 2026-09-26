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
//! `subagent_asks`, and `subagent_restarts` sets the agent going again on its
//! thread, once, after its turn is interrupted; none is spawned when the
//! thread's config sets
//! `agents.enabled = false` and does not turn `features.multi_agent_v2` on.
//! `elicit_during` (a list of `initialize`, `account/read`, `thread/start`)
//! sends a url-mode elicitation before answering each wait it names;
//! `exit_after_early` then exits once the client has answered it.
//! `stream_retries` has the plain turn, or a led run whose prompt contains
//! `stream_retries_if`, report that many stream retries first, as Codex's
//! `error` notification with `willRetry: true` (`fake_turn::stream_retries`).
//! `continue_after_turn` (the plain turn) or `continue_after_turn_if` (a led
//! run's prompt) has the thread start a turn of its own once its turn has
//! ended, as a goal's continuation does (`fake_turn::continue_thread`:
//! `continuation_steps`, `continuation_step`, `continuation_step_ms`), unless
//! the thread's config turned goals off.
//! A thread whose model is `gpt-5.6-terra`, or whose scenario says
//! `tool_mode: "code_mode_only"`, is code-mode-only, as that model's catalog
//! entry says: its lead has an MCP server's tools in its own list only where
//! the server's `omit_tools_from` makes them `DirectModelOnly`, as Codex
//! 0.157.0 computes it (`fake_turn::exposure`); otherwise it says it cannot
//! access its tool, as L3's first live lead did, and calls nothing. Every
//! launched server's startup status (`mcpServer/startupStatus/updated`,
//! `starting` then `ready`) goes to the client, before the `thread/start`
//! answer for a server marked `required` and after it for any other. A
//! server the thread's config sends as `enabled = false` is never started.
//! After the answer the fake announces, and never launches, every other
//! server Codex would start on the thread: each `[mcp_servers.<name>]` of the
//! home's `config.toml` (by table header) the thread did not turn off, the
//! `plugin_servers` the scenario names unless `features.plugins` is off, and
//! `codex_apps` where the scenario has `apps_server`, unless apps are off (by
//! the last of `features.apps` and its alias `features.connectors`).
//! `touch_runtime_dbs` gives `memories_1.sqlite` and `goals_1.sqlite`, where
//! the home has them, a later modification time at every start, as Codex
//! opens its runtime databases; `memory_db_grows` appends to
//! `memories_1.sqlite` at the first turn.
//! `memory_pipeline` writes, at the first turn, what Codex's memory pipeline
//! would under the Codex home (`memories/`, `memories_1.sqlite`), unless the
//! thread's config turned memories off (`crate::features_off`).
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

/// The MCP servers a Codex home's `config.toml` names by table header,
/// `[mcp_servers.<name>]` or a subtable of one, and nothing else of the file.
fn config_servers(home: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(home.join("config.toml")).unwrap_or_default();
    let mut names: Vec<String> = Vec::new();
    for line in text.lines().map(str::trim) {
        let Some(inner) = line
            .strip_prefix('[')
            .filter(|rest| !rest.starts_with('['))
            .and_then(|rest| rest.split(']').next())
        else {
            continue;
        };
        let mut parts = inner.split('.').map(|p| p.trim().trim_matches('"'));
        if parts.next() == Some("mcp_servers")
            && let Some(name) = parts.next().filter(|n| !n.is_empty())
            && !names.iter().any(|n| n == name)
        {
            names.push(name.to_owned());
        }
    }
    names
}

/// One MCP server's startup on a thread, as Codex 0.157.0 tells the
/// client: `starting`, then `ready` (`McpServerStatusUpdatedNotification`,
/// `app-server-protocol/src/protocol/v2/mcp.rs:357-363`).
fn startup_status(thread: &str, name: &str) -> Result<()> {
    for status in ["starting", "ready"] {
        fake_turn::emit(&json!({"method":"mcpServer/startupStatus/updated",
            "params":{"threadId":thread,"name":name,"status":status,
                      "error":null,"failureReason":null}}))?;
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
///
/// Only the `thread/start` window is a shape Codex 0.157.0 sends: an MCP
/// server's elicitation comes from a thread, and carries that thread's id
/// (`app-server/src/bespoke_event_handling.rs:912-917`), which is `thread`
/// here. During `initialize` and `account/read` no thread exists yet, so a
/// request there is **defensive**, a shape Codex does not send, played so the
/// host is shown to decline it anyway; its `threadId` is null for want of a
/// thread (review of L3, round 4, R4-HC-5).
fn early_request(
    scenario: &Value,
    wait: &str,
    thread: Option<&str>,
    markers: &Option<PathBuf>,
) -> Result<Option<String>> {
    let named = scenario["elicit_during"]
        .as_array()
        .is_some_and(|waits| waits.iter().any(|w| w == wait));
    if !named {
        return Ok(None);
    }
    let id = format!("fake-early-{}", wait.replace('/', "-"));
    emit(&json!({"method":"mcpServer/elicitation/request","id":id,
                 "params":{"threadId":thread,"turnId":null,"serverName":"someone","mode":"url",
                           "elicitationId":format!("fake-{wait}"),
                           "url":"https://example.invalid/early",
                           "message":"Sign in before we start"}}))?;
    marker(
        markers,
        json!({"source":SOURCE,"kind":"early_request_sent","wait":wait,"thread_id":thread}),
    )?;
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
    // Whether this thread goes on by itself once its turn has ended (a
    // goal's continuation), and whether it has (review of L3, round 4,
    // SPEND-9): `continue_after_turn`, or `continue_after_turn_if` a led
    // run's prompt.
    let mut continues = scenario["continue_after_turn"] == true;
    let mut continued = false;
    loop {
        if let Some(turn) = scripted.as_ref().filter(|turn| turn.finished()).cloned() {
            scripted = None;
            if continues && !continued {
                continued = true;
                scripted =
                    fake_turn::continue_thread(&turn, &scenario, waiting.clone(), markers.clone())?;
            }
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
                    if continues && !continued {
                        continued = true;
                        let total = scenario
                            .get("usage_total")
                            .cloned()
                            .unwrap_or(json!(42))
                            .as_u64()
                            .unwrap_or(0);
                        let ended = fake_turn::Turn::continued(turn.clone(), thread_id, total);
                        scripted = fake_turn::continue_thread(
                            &ended,
                            &scenario,
                            waiting.clone(),
                            markers.clone(),
                        )?;
                    }
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
                        // D16: an accepted command approval's item completes
                        // by the command's own exit, not by the decision.
                        // Measured (docs/work/m2/codex-live/R6.json,
                        // "approval-allow"): the live accept still gave
                        // `failed` for the commandExecution item. The fake
                        // plays that observed outcome rather than a
                        // decision-shaped guess.
                        let status = if refusal.is_none() && decision == "accept" {
                            "failed"
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
                        if scenario["touch_runtime_dbs"] == true {
                            // What every Codex 0.157.0 app-server does at
                            // start, whatever features are on: it opens its
                            // runtime databases read-write and runs their
                            // migrations (`state/src/runtime.rs:123-200`,
                            // `state/src/sqlite.rs:251-310`). Played as a
                            // later modification time on those already
                            // there, and not a byte more.
                            for name in ["memories_1.sqlite", "goals_1.sqlite"] {
                                let path = home.join(name);
                                if path.is_file() {
                                    std::fs::OpenOptions::new()
                                        .append(true)
                                        .open(&path)?
                                        .set_modified(std::time::SystemTime::now())?;
                                }
                            }
                        }
                        if let Some(early) = early_request(&scenario, "initialize", None, &markers)?
                        {
                            leave_after = Some(early);
                            continue;
                        }
                        send(
                            json!({"id":id,"result":{"userAgent":SOURCE,"codexHome":home,"platformFamily":"unix","platformOs":std::env::consts::OS}}),
                        )?;
                    }
                    "initialized" => {}
                    "account/read" => {
                        if let Some(early) =
                            early_request(&scenario, "account/read", None, &markers)?
                        {
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
                        // And each server's own settings, as received.
                        let settings: serde_json::Map<String, Value> =
                            params["config"]["mcp_servers"]
                                .as_object()
                                .into_iter()
                                .flatten()
                                .map(|(name, spec)| {
                                    (
                                        name.clone(),
                                        json!({"omit_tools_from":spec["omit_tools_from"],
                                           "required":spec["required"],
                                           "startup_timeout_sec":spec["startup_timeout_sec"]}),
                                    )
                                })
                                .collect();
                        // And what it said of Codex's unmetered features, by
                        // the dotted keys a request override takes
                        // (`crate::features_off`): with `agents.enabled`
                        // false and `multi_agent_v2` not on, Codex offers no
                        // collaboration tools whatever the model's catalog
                        // says (`Config::multi_agent_version_override`,
                        // rust-v0.157.0), so this fake spawns none; with
                        // memories off (the alias `memory_tool` sorts last
                        // and decides where both are given) it runs no
                        // memory pipeline; with goals off it continues no
                        // turn by itself.
                        let config = &params["config"];
                        let key = |dotted: &str| {
                            let (table, name) = dotted.split_once('.').unwrap_or(("", dotted));
                            config[dotted].as_bool().or(config[table][name].as_bool())
                        };
                        let agents = key("agents.enabled");
                        let v2 = key("features.multi_agent_v2");
                        fake_turn::AGENTS_OFF.store(
                            agents == Some(false) && v2 != Some(true),
                            std::sync::atomic::Ordering::SeqCst,
                        );
                        let memories = key("features.memory_tool").or(key("features.memories"));
                        fake_turn::MEMORIES_OFF
                            .store(memories == Some(false), std::sync::atomic::Ordering::SeqCst);
                        fake_turn::GOALS_OFF.store(
                            key("features.goals") == Some(false),
                            std::sync::atomic::Ordering::SeqCst,
                        );
                        // Every key of the override set as received, and
                        // nothing else of the config.
                        let features_off: serde_json::Map<String, Value> = crate::features_off()
                            .into_iter()
                            .filter(|(_, dotted, _)| !config[*dotted].is_null())
                            .map(|(_, dotted, _)| (dotted.to_owned(), config[dotted].clone()))
                            .collect();
                        marker(
                            &markers,
                            json!({"source":SOURCE,"kind":"thread_config_received","servers":received,
                                   "settings":settings,
                                   "agents_enabled":agents,"multi_agent":config["features.multi_agent"],
                                   "multi_agent_v2":v2,"features_off":features_off}),
                        )?;
                        // What Codex answers, which is not always what was
                        // asked; the scenario can make it differ.
                        let model = scenario["model_reported"]
                            .as_str()
                            .or(params["model"].as_str())
                            .unwrap_or("pio-fake-model");
                        // `gpt-5.6-terra` runs code-mode-only
                        // (`models-manager/models.json:676`, rust-v0.157.0).
                        fake_turn::CODE_MODE_ONLY.store(
                            model == "gpt-5.6-terra" || scenario["tool_mode"] == "code_mode_only",
                            std::sync::atomic::Ordering::SeqCst,
                        );
                        // The servers this thread's own config names, launched
                        // and listed before the answer, as the M4b probe saw.
                        // Each one's startup status goes to the client, per
                        // thread (`app-server/src/bespoke_event_handling.rs:202-228`):
                        // before the answer for a server marked `required`,
                        // which Codex waits for at thread start
                        // (`core/src/session/mcp_runtime.rs:148`), after it
                        // for any other.
                        let mut after_answer = Vec::new();
                        let mut turned_off = Vec::new();
                        for (name, spec) in params["config"]["mcp_servers"]
                            .as_object()
                            .into_iter()
                            .flatten()
                        {
                            // A server turned off is never started
                            // (`codex-mcp/src/connection_manager.rs:288-291`).
                            if spec["enabled"] == false || spec["command"].is_null() {
                                marker(
                                    &markers,
                                    json!({"source":SOURCE,"kind":"mcp_server_off","name":name}),
                                )?;
                                turned_off.push(name.clone());
                                continue;
                            }
                            let server = McpServer::launch(name, spec)?;
                            marker(
                                &markers,
                                json!({"source":SOURCE,"kind":"mcp_server_launched","name":name,
                                       "exposure":server.exposure,
                                       "in_model_list":server.in_model_list}),
                            )?;
                            servers.lock().expect("servers lock").push(server);
                            if spec["required"] == true {
                                startup_status(&thread_id, name)?;
                            } else {
                                after_answer.push(name.clone());
                            }
                        }
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
                        if let Some(early) =
                            early_request(&scenario, "thread/start", Some(&thread_id), &markers)?
                        {
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
                        for name in &after_answer {
                            startup_status(&thread_id, name)?;
                        }
                        // Every other server Codex would start on this
                        // thread, announced and never launched: this fake
                        // runs no command of anyone's. Each server of the
                        // home's `config.toml`, found by its table header,
                        // unless the thread turned it off; the plugin
                        // servers the scenario names, unless the thread
                        // turned plugins off; and Codex's apps server where
                        // the scenario has one, unless apps are off (by the
                        // last of `apps` and its alias `connectors`).
                        let off = |dotted: &str| config[dotted] == false;
                        let apps_off = if config["features.connectors"].is_boolean() {
                            off("features.connectors")
                        } else {
                            off("features.apps")
                        };
                        let mut others: Vec<(String, &str)> = config_servers(&home)
                            .into_iter()
                            .filter(|name| {
                                !turned_off.contains(name) && !after_answer.contains(name)
                            })
                            .map(|name| (name, "config.toml"))
                            .collect();
                        if !off("features.plugins") {
                            others.extend(
                                scenario["plugin_servers"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .filter_map(Value::as_str)
                                    .map(|name| (name.to_owned(), "plugin")),
                            );
                        }
                        if scenario["apps_server"] == true && !apps_off {
                            others.push(("codex_apps".to_owned(), "apps"));
                        }
                        for (name, from) in &others {
                            marker(
                                &markers,
                                json!({"source":SOURCE,"kind":"mcp_server_announced","name":name,
                                       "from":from}),
                            )?;
                            startup_status(&thread_id, name)?;
                        }
                    }
                    "turn/start" => {
                        turns += 1;
                        if turns == 1 && scenario["memory_db_grows"] == true {
                            // Something writes to the memories database.
                            let path = home.join("memories_1.sqlite");
                            if path.is_file() {
                                std::fs::OpenOptions::new()
                                    .append(true)
                                    .open(&path)?
                                    .write_all(&[0u8; 4096])?;
                            }
                        }
                        let memories_off =
                            fake_turn::MEMORIES_OFF.load(std::sync::atomic::Ordering::SeqCst);
                        if turns == 1 && scenario["memory_pipeline"] == true && memories_off {
                            marker(
                                &markers,
                                json!({"source":SOURCE,"kind":"memory_pipeline_not_started"}),
                            )?;
                        }
                        if turns == 1 && scenario["memory_pipeline"] == true && !memories_off {
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
                                let dir = cwd.clone();
                                std::thread::spawn(move || {
                                    fake_turn::lead(playing, servers, script, queue, marks, dir)
                                });
                            } else {
                                continues = continues
                                    || scenario["continue_after_turn_if"]
                                        .as_str()
                                        .is_some_and(|needle| text.contains(needle));
                                let (dir, prompt) = (cwd.clone(), text.to_owned());
                                std::thread::spawn(move || {
                                    fake_turn::led(playing, script, queue, marks, dir, prompt)
                                });
                            }
                            continue;
                        }
                        fake_turn::stream_retries(&thread_id, &turn, &scenario)?;
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
                                    // 0.157.0's network presentation sends no
                                    // command, cwd or command actions
                                    // (`app-server/src/bespoke_event_handling.rs:
                                    // 741-745`; review of L3, round 4, R4-HC-5).
                                    if let Some(fields) = params.as_object_mut() {
                                        for field in ["command", "cwd", "commandActions"] {
                                            fields.remove(field);
                                        }
                                    }
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
                                    // `isBlocking` is required at 0.157.0 and Codex
                                    // sends true (review of L3, round 3, R3-HC-7); a
                                    // sentinel in every text a host could copy,
                                    // header and label included (R3-HC-5).
                                    params = json!({"threadId":thread_id,"turnId":turn,"itemId":"item-approval",
                                                    "isBlocking":true,
                                                    "questions":[{"id":"mcp_tool_call_approval_call-1","header":"Approve app tool call? SENTINEL-header",
                                                                  "question":"Allow the someone MCP server to run tool \"go\"? SENTINEL-question",
                                                                  "isOther":false,"isSecret":false,
                                                                  "options":[{"label":"Allow SENTINEL-label","description":"SENTINEL-option"}]}]});
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
                                        let mut url = json!({"threadId":thread_id,"turnId":turn,"serverName":"someone","mode":"url","elicitationId":"fake-elicitation","url":"https://example.invalid/login","message":"Sign in to continue"});
                                        // A server's own `_meta`, which Codex
                                        // forwards unchanged, claiming the
                                        // tool-call approval kind in url mode
                                        // (review of L3, round 4, R4-HC-4).
                                        if let Some(kind) =
                                            scenario["elicitation_meta_kind"].as_str()
                                        {
                                            url["_meta"] = json!({"codex_approval_kind":kind});
                                        }
                                        url
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
                                json!({"source":SOURCE,"kind":"sub_agent_interrupted","thread":sub.thread,
                                       "turn":sub.id}),
                            )?;
                            sub.interrupt()?;
                            // Set going again on the same thread, once
                            // (`subagent_restarts`).
                            if scenario["subagent_restarts"] == true && !sub.id.ends_with("-again")
                            {
                                fake_turn::restart_sub_agent(
                                    &sub,
                                    &scenario,
                                    waiting.clone(),
                                    markers.clone(),
                                )?;
                            }
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
