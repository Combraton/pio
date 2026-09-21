//! Labeled fake Claude Code CLI for offline tests. It speaks the stream-json
//! shapes measured from 2.1.278 over stdio, but runs no model, no tool and no
//! command. Everything it produces is labeled `pio-fake-claude-cli`; it is
//! never a qualified Claude Code and never real-harness evidence.
//!
//! It reproduces the measured behaviours the adapter depends on, including the
//! awkward one: **`system/init` is not emitted until a message arrives on
//! stdin**, so a matrix case cannot accidentally prove a pre-flight check that
//! the real harness does not allow.
//!
//! Scenario (JSON in `PIO_CLAUDE_FAKE_SCENARIO`, all members optional):
//! `version` (default the pinned version), `help_suffix` (moves the surface
//! digest, for drift cases), `route` (`"claude.ai"` default, `null` for none),
//! `permission_request` (`{"tool_name":…,"input":…}` asks for a decision),
//! `tool_uses` (blocks to report), `usage_total` (default 128), `delay_ms`,
//! `init` (members merged into `system/init`), `ignore_interrupt` (the fake
//! ignores SIGINT and hangs, so a host's escalation can be proven),
//! `deny_by_rules` (the harness refuses under its own rules even with a host
//! attached, which the pinned SDK warns is what shadowing does),
//! `abort_on_interrupt` (SIGINT during `delay_ms` ends the turn the way the
//! real harness was measured to: an aborted `result` with an empty usage
//! block),
//! `foreign_control_request` (a control request PIO must not answer on the
//! user's behalf, used to prove it still answers *something*),
//! `markers` (directory for independent records).
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::PathBuf;

pub const SOURCE: &str = "pio-fake-claude-cli";

/// Set by the SIGINT handler when the scenario asks the fake to abort the way
/// the real harness does. Only a flag is touched from the handler.
static INTERRUPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn note_interrupt(_signal: libc::c_int) {
    INTERRUPTED.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Measured on the live R5 cancel: after SIGINT the real harness emits a
/// `result` with `terminal_reason: aborted_streaming`, `is_error: true`, an
/// empty `iterations` and **every usage part zero**. Reproduced here because
/// that empty block is what PIO was passing on as an observation of zero.
fn aborted_result() -> Value {
    json!({
        "type":"result","subtype":"error_during_execution","uuid":"fake-abort-uuid",
        "session_id":"fake-session","is_error":true,"result_index":0,
        "terminal_reason":"aborted_streaming","stop_reason":Value::Null,
        "num_turns":1,"duration_ms":1,"duration_api_ms":1,"queued_turn_count":0,
        "permission_denials":0,"permission_decision":Value::Null,
        "total_cost_usd":0.0,"subagent_stats":{},
        "usage":{"input_tokens":0,"output_tokens":0,
                 "cache_creation_input_tokens":0,"cache_read_input_tokens":0,
                 "iterations":[]},
        "modelUsage":{},
        "source":SOURCE,
    })
}

/// Fields a permission response may never carry. The fake records an attempt
/// rather than refusing it, so the matrix proves PIO did not send one instead
/// of trusting that it did not. ADR 004 §9.
pub const WIDENING_FIELDS: &[&str] = &["updatedPermissions", "updatedPermission", "permissions"];

fn marker(dir: &Option<PathBuf>, record: Value) -> Result<()> {
    let Some(dir) = dir else { return Ok(()) };
    std::fs::create_dir_all(dir)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("fake-claude-cli.jsonl"))?;
    let mut bytes = serde_json::to_vec(&record)?;
    bytes.push(b'\n');
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

fn emit(message: &Value) -> Result<()> {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{}", serde_json::to_string(message)?)?;
    out.flush()?;
    Ok(())
}

fn flag_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

/// The `system/init` key set measured from the real harness, so a matrix run
/// exercises the same shape the adapter pins.
fn init_message(scenario: &Value, args: &[String]) -> Value {
    let mode = flag_value(args, "--permission-mode").unwrap_or("default");
    let mut init = json!({
        "type":"system","subtype":"init","uuid":"fake-init-uuid",
        "session_id":"fake-session","cwd":std::env::current_dir()
            .map(|p| p.display().to_string()).unwrap_or_default(),
        "claude_code_version":scenario["version"].as_str().unwrap_or(super::PINNED_VERSION),
        "model":"pio-fake-model","permissionMode":mode,
        // In-band half of the precedence rule: a login reports no API key
        // source, an API-key route names it. ADR 004 §3.
        "apiKeySource":match scenario["route"].as_str() {
            Some("apiKey") => "ANTHROPIC_API_KEY",
            _ => "none",
        },
        "capabilities":["interrupt_receipt_v1","interrupt_cancel_queued_v1","msg_lifecycle_v1"],
        "tools":["Bash","Read","Edit"],"mcp_servers":[],"plugins":[],
        "slash_commands":[],"skills":[],"agents":[],
        "memory_paths":[],"output_style":"default",
        "analytics_disabled":true,"product_feedback_disabled":true,
        "fast_mode_state":"disabled","fast_mode_disabled_reason":"fake",
        "messaging_socket_path":Value::Null,
        "source":SOURCE,
    });
    if let Some(overrides) = scenario["init"].as_object() {
        for (key, value) in overrides {
            init[key] = value.clone();
        }
    }
    init
}

/// The real harness keeps a session transcript under a slug of **the
/// session's working directory**, with a `memory` directory beside it, both
/// created during the turn. Measured on the R1 live run, after a receipt had
/// claimed nothing was written.
///
/// The fake reproduces that shape — an empty directory and a one-line file,
/// never content — so an offline case can catch a snapshot that slugs any
/// other path. The scenario names the configuration directory because an
/// as-configured run is not passed `CLAUDE_CONFIG_DIR`.
fn write_transcript(scenario: &Value) -> Result<()> {
    let Some(config_dir) = scenario["config_dir"].as_str() else {
        return Ok(());
    };
    let slug = std::env::current_dir()?
        .display()
        .to_string()
        .replace(['/', '.'], "-");
    let directory = PathBuf::from(config_dir).join("projects").join(slug);
    std::fs::create_dir_all(directory.join("memory"))?;
    std::fs::write(
        directory.join("fake-session.jsonl"),
        format!("{{\"source\":\"{SOURCE}\"}}\n"),
    )?;
    Ok(())
}

fn assistant(content: Value) -> Value {
    json!({"type":"assistant","message":{"role":"assistant","content":content},"source":SOURCE})
}

/// Ask for a decision and wait for it, the way the real control protocol does.
/// Returns the behavior the host chose, or `None` if the stream ended first.
fn request_permission(
    request: &Value,
    lines: &mut impl Iterator<Item = std::io::Result<String>>,
    markers: &Option<PathBuf>,
) -> Result<Option<String>> {
    let request_id = "req_1_fake";
    emit(&assistant(json!([{
        "type":"tool_use","name":request["tool_name"].as_str().unwrap_or("Bash"),
        "id":"toolu_fake_1","input":&request["input"]}])))?;
    emit(&json!({
        "type":"control_request","request_id":request_id,
        "request":{
            "subtype":"can_use_tool",
            "tool_name":request["tool_name"].as_str().unwrap_or("Bash"),
            "input":&request["input"],
            "tool_use_id":"toolu_fake_1",
            // The harness offers a rule update. A host that acts on one has
            // widened a permission; the marker below proves PIO did not.
            "permission_suggestions":[{
                "type":"addRules","destination":"userSettings","behavior":"allow",
                "rules":[{"tool_name":request["tool_name"].as_str().unwrap_or("Bash")}]}],
        },
        "source":SOURCE,
    }))?;
    for line in lines {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if message["type"] != "control_response" {
            continue;
        }
        let decision = &message["response"]["response"];
        let widening: Vec<&str> = WIDENING_FIELDS
            .iter()
            .filter(|field| !decision[**field].is_null())
            .copied()
            .collect();
        marker(
            markers,
            json!({"event":"permission_decision",
                   "behavior":&decision["behavior"],
                   "request_id":&message["response"]["request_id"],
                   "input_echoed_unchanged":decision["updatedInput"] == request["input"],
                   "widening_fields_received":widening}),
        )?;
        return Ok(decision["behavior"].as_str().map(str::to_owned));
    }
    Ok(None)
}

pub fn run() -> Result<()> {
    let scenario: Value = std::env::var("PIO_CLAUDE_FAKE_SCENARIO")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .context("PIO_CLAUDE_FAKE_SCENARIO is not JSON")?
        .unwrap_or_else(|| json!({}));
    // argv is `pio claude fake-cli <the arguments a real claude would get>`.
    let args: Vec<String> = std::env::args().skip(3).collect();
    let version = scenario["version"]
        .as_str()
        .unwrap_or(super::PINNED_VERSION)
        .to_owned();
    let markers = scenario["markers"].as_str().map(PathBuf::from);

    // The non-streaming surfaces, so one fake can be qualified, drifted and
    // driven without three executables.
    if args.iter().any(|a| a == "--version") {
        println!("{version} (Claude Code)");
        return Ok(());
    }
    if args.iter().any(|a| a == "--help") {
        let suffix = scenario["help_suffix"].as_str().unwrap_or("");
        let subcommand = args.first().filter(|a| !a.starts_with("--"));
        match subcommand {
            Some(name) => println!("{SOURCE} help for {name}{suffix}"),
            None => println!("{SOURCE} top help{suffix}"),
        }
        return Ok(());
    }

    if args.first().map(String::as_str) == Some("auth")
        && args.get(1).map(String::as_str) == Some("status")
    {
        let route = match scenario["route"].as_str() {
            None if scenario.get("route").is_some() => None,
            other => Some(other.unwrap_or("claude.ai")),
        };
        let status = match route {
            Some(method) => json!({"loggedIn":true,"authMethod":method,
                                   "apiProvider":"firstParty","subscriptionType":"max",
                                   "email":"fake@example.invalid","orgId":"org-fake",
                                   "orgName":"Fake Organization"}),
            None => json!({"loggedIn":false,"authMethod":"none","apiProvider":"firstParty"}),
        };
        print!("{}", serde_json::to_string(&status)?);
        std::process::exit(if route.is_some() { 0 } else { 1 });
    }
    // Stream mode. Measured: nothing is emitted until stdin carries a message,
    // so init cannot be used as a pre-flight check.
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    // Attachment, the way the real harness was measured to do it. A host that
    // names `--permission-prompt-tool stdio` **and** sends the handshake is
    // attached; anything less is not, and an unattached harness answers its
    // own model rather than asking anyone. R3b found PIO in that state.
    let prompt_tool = args
        .iter()
        .position(|a| a == "--permission-prompt-tool")
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
        == Some("stdio");
    let mut handshook = false;
    let sent: Value = loop {
        let Some(line) = lines.next() else {
            return Ok(());
        };
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let message: Value = serde_json::from_str(&line).context("stdin line is not JSON")?;
        if message["type"] == "control_request" && message["request"]["subtype"] == "initialize" {
            // A CLI that receives the handshake and never answers it. PIO
            // must refuse before the brief leaves, not discover mid-turn that
            // it was never the permission host — which is the shape of the
            // defect that cost four live Claude runs.
            if scenario["ignore_initialize"] == true {
                marker(&markers, json!({"event":"initialize_ignored"}))?;
                continue;
            }
            handshook = true;
            marker(&markers, json!({"event":"initialize_received"}))?;
            // The answer measured from 2.1.278, by its keys.
            emit(&json!({"type":"control_response","response":{
                "subtype":"success","request_id":message["request_id"],
                "response":{},
                "pending_permission_requests":[],
                "pending_user_dialog_requests":[]}}))?;
            continue;
        }
        break message;
    };
    let attached = prompt_tool && handshook;
    marker(
        &markers,
        json!({"event":"turn_received","attached":attached,
                            "permission_prompt_tool":prompt_tool,
                            "initialize_received":handshook}),
    )?;

    emit(&init_message(&scenario, &args))?;
    // The replay echo: the exact message that was sent. This is the delivery
    // acknowledgment the adapter treats as its proof class.
    let mut replay = sent.clone();
    replay["isReplay"] = json!(true);
    emit(&replay)?;

    // A harness that ignores the interrupt. Nothing in PIO produces this; it
    // exists so a host's bounded escalation can be proven rather than assumed.
    if scenario["ignore_interrupt"] == true {
        unsafe { libc::signal(libc::SIGINT, libc::SIG_IGN) };
        marker(&markers, json!({"event":"ignoring_interrupt"}))?;
        std::thread::sleep(std::time::Duration::from_secs(300));
        return Ok(());
    }
    if let Some(delay) = scenario["delay_ms"].as_u64() {
        if scenario["abort_on_interrupt"] == true {
            unsafe {
                libc::signal(
                    libc::SIGINT,
                    note_interrupt as *const () as libc::sighandler_t,
                )
            };
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(delay);
            while std::time::Instant::now() < deadline {
                if INTERRUPTED.load(std::sync::atomic::Ordering::SeqCst) {
                    marker(&markers, json!({"event":"aborted_on_interrupt"}))?;
                    emit(&aborted_result())?;
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(delay));
        }
    }

    // A request PIO will not answer on the user's behalf. It must still get a
    // response, or a real harness would wait forever.
    if let Some(subtype) = scenario["foreign_control_request"].as_str() {
        emit(
            &json!({"type":"control_request","request_id":"req_foreign_1",
            "request":{"subtype":subtype},"source":SOURCE}),
        )?;
        let mut answered = Value::Null;
        for line in lines.by_ref() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(message) = serde_json::from_str::<Value>(&line)
                && message["type"] == "control_response"
                && message["response"]["request_id"] == "req_foreign_1"
            {
                answered = message["response"].clone();
                break;
            }
        }
        marker(
            &markers,
            json!({"event":"foreign_control_request",
                   "subtype":subtype,
                   "answered":!answered.is_null(),
                   "response_subtype":&answered["subtype"],
                   "error":&answered["error"]}),
        )?;
    }

    let mut denials = 0;
    let mut decision = Value::Null;
    let mut denied_by_harness: Vec<Value> = Vec::new();
    // Measured on the real harness and warned about in the pinned SDK: an
    // attached host's callback is **shadowed** by the allow rules and the
    // permission mode, so a harness can still decide by itself while a host is
    // listening. `deny_by_rules` reproduces that; `attached` being false is
    // the R3b state, where nobody was listening at all.
    let harness_decides = !attached || scenario["deny_by_rules"] == true;
    if !scenario["permission_request"].is_null() && harness_decides {
        // Nobody is listening, so the harness decides for itself and tells its
        // own model. Measured text, from the R3b transcript.
        let request = &scenario["permission_request"];
        let tool_use_id = "toolu_fake_denied_1";
        emit(&assistant(json!([{
            "type":"tool_use","name":&request["tool_name"],
            "id":tool_use_id,"input":&request["input"]}])))?;
        emit(&json!({"type":"user","message":{"role":"user","content":[{
            "type":"tool_result","tool_use_id":tool_use_id,"is_error":true,
            "content":"This command requires approval"}]},"source":SOURCE}))?;
        denied_by_harness.push(json!({"tool_name":&request["tool_name"],
            "tool_use_id":tool_use_id,"tool_input":&request["input"]}));
        denials += 1;
        // Two different states, never merged: nobody was listening, or a host
        // was listening and the harness's own rules decided anyway.
        let state = if attached {
            "denied_by_harness_rules_shadowed_the_host"
        } else {
            "denied_by_harness_no_host_attached"
        };
        decision = json!(state);
        marker(
            &markers,
            json!({"event":state,"attached":attached,
                   "tool_name":&request["tool_name"]}),
        )?;
    } else if !scenario["permission_request"].is_null() {
        let request = &scenario["permission_request"];
        match request_permission(request, &mut lines, &markers)? {
            Some(behavior) => {
                decision = json!(behavior);
                if behavior == "deny" {
                    // The real harness names every refusal in `result`,
                    // whoever decided it. Listing only the ones it decided
                    // itself left the attribution path untested.
                    denied_by_harness.push(json!({"tool_name":&request["tool_name"],
                        "tool_use_id":"toolu_fake_1","tool_input":&request["input"]}));
                    denials += 1;
                }
            }
            None => decision = json!("no_response"),
        }
    }

    if let Some(uses) = scenario["tool_uses"].as_array() {
        let blocks: Vec<Value> = uses
            .iter()
            .enumerate()
            .map(|(index, use_)| {
                json!({"type":"tool_use","name":&use_["name"],
                       "id":format!("toolu_fake_{index}"),"input":&use_["input"]})
            })
            .collect();
        emit(&assistant(json!(blocks)))?;
    }
    emit(&assistant(json!([{
        "type":"text",
        "text":scenario["agent_text"].as_str().unwrap_or("fake turn complete")}])))?;

    write_transcript(&scenario)?;

    let total = scenario["usage_total"].as_u64().unwrap_or(128);
    emit(&json!({
        "type":"result","subtype":"success","uuid":"fake-result-uuid",
        "session_id":"fake-session","is_error":false,"result_index":0,
        "terminal_reason":"complete","stop_reason":"end_turn","num_turns":1,
        "duration_ms":1,"duration_api_ms":1,"queued_turn_count":0,
        "permission_denials":denied_by_harness,"permission_denial_count":denials,
        "permission_decision":decision,
        "total_cost_usd":0.0,"subagent_stats":{},
        "usage":{"input_tokens":total / 2,"output_tokens":total / 2,
                 "cache_creation_input_tokens":0,"cache_read_input_tokens":0},
        "modelUsage":{"pio-fake-model":{"inputTokens":total / 2,"outputTokens":total / 2}},
        "source":SOURCE,
    }))?;
    marker(
        &markers,
        json!({"event":"turn_complete","usage_total":total}),
    )?;
    Ok(())
}
