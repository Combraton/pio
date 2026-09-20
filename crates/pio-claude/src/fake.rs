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
//! `init` (members merged into `system/init`), `markers` (directory for
//! independent records).
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::PathBuf;

pub const SOURCE: &str = "pio-fake-claude-cli";

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
    let Some(first) = lines.next() else {
        return Ok(());
    };
    let sent: Value = serde_json::from_str(&first?).context("first stdin line is not JSON")?;
    marker(&markers, json!({"event":"turn_received"}))?;

    emit(&init_message(&scenario, &args))?;
    // The replay echo: the exact message that was sent. This is the delivery
    // acknowledgment the adapter treats as its proof class.
    let mut replay = sent.clone();
    replay["isReplay"] = json!(true);
    emit(&replay)?;

    if let Some(delay) = scenario["delay_ms"].as_u64() {
        std::thread::sleep(std::time::Duration::from_millis(delay));
    }

    let mut denials = 0;
    let mut decision = Value::Null;
    if !scenario["permission_request"].is_null() {
        match request_permission(&scenario["permission_request"], &mut lines, &markers)? {
            Some(behavior) => {
                decision = json!(behavior);
                if behavior == "deny" {
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

    let total = scenario["usage_total"].as_u64().unwrap_or(128);
    emit(&json!({
        "type":"result","subtype":"success","uuid":"fake-result-uuid",
        "session_id":"fake-session","is_error":false,"result_index":0,
        "terminal_reason":"complete","stop_reason":"end_turn","num_turns":1,
        "duration_ms":1,"duration_api_ms":1,"queued_turn_count":0,
        "permission_denials":denials,"permission_decision":decision,
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
