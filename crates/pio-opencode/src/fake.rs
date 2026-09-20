//! Labeled fake OpenCode ACP server for offline tests. It speaks the Agent
//! Client Protocol shapes over stdio but runs no model, tool or command.
//! Everything it produces is labeled `pio-fake-opencode-acp`; it is never a
//! qualified OpenCode and never real-harness evidence.
//!
//! **Measured against 2.0.1 at zero tokens:** the `initialize` result, and
//! `session/new` returning `configOptions` with the model, effort and mode the
//! session will use — which is why the provider and model can be checked
//! before any prompt.
//!
//! **From the ACP specification, not measured:** the `session/request_permission`
//! request and its response. ADR 005 records them as unverified against 2.0.1,
//! and PIO forwards no decision whose single-use form it has not measured.
//!
//! Scenario (JSON in `PIO_OPENCODE_FAKE_SCENARIO`, all members optional):
//! `version`, `model` (what the session reports, so a silent downgrade can be
//! played), `permission_request`, `tool_calls`, `usage_total`, `delay_ms`,
//! `ignore_cancel`, `markers`.
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::PathBuf;

pub const SOURCE: &str = "pio-fake-opencode-acp";
pub const DEFAULT_MODEL: &str = "minimax-coding-plan/MiniMax-M2.7-highspeed";

/// Fields a permission response may never carry: anything that would make a
/// decision outlive the request it answered.
pub const WIDENING_FIELDS: &[&str] = &["updatedPermissions", "scope", "remember", "always"];

fn marker(dir: &Option<PathBuf>, record: Value) -> Result<()> {
    let Some(dir) = dir else { return Ok(()) };
    std::fs::create_dir_all(dir)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("fake-opencode-acp.jsonl"))?;
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

fn reply(id: &Value, result: Value) -> Result<()> {
    emit(&json!({"jsonrpc":"2.0","id":id,"result":result}))
}

fn update(session: &str, update: Value) -> Result<()> {
    emit(&json!({"jsonrpc":"2.0","method":"session/update",
                 "params":{"sessionId":session,"update":update}}))
}

/// The session's own report of what it will use. Measured shape.
fn config_options(model: &str) -> Value {
    json!([
        {"id":"model","name":"Model","category":"model","type":"select",
         "currentValue":model,
         "options":[{"value":model,"name":model},
                    {"value":"opencode/nemotron-3.5-lightning-free",
                     "name":"opencode/nemotron-3.5-lightning-free"}]},
        {"id":"effort","name":"Effort","type":"select","currentValue":"default",
         "options":[{"value":"default","name":"default"}]},
        {"id":"mode","name":"Mode","type":"select","currentValue":"build",
         "options":[{"value":"build","name":"build"},{"value":"plan","name":"plan"}]}
    ])
}

pub fn run() -> Result<()> {
    let scenario: Value = std::env::var("PIO_OPENCODE_FAKE_SCENARIO")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .context("PIO_OPENCODE_FAKE_SCENARIO is not JSON")?
        .unwrap_or_else(|| json!({}));
    let args: Vec<String> = std::env::args().skip(3).collect();
    let version = scenario["version"]
        .as_str()
        .unwrap_or(super::PINNED_VERSION)
        .to_owned();
    let markers = scenario["markers"].as_str().map(PathBuf::from);

    // PIO must never pass these. The fake records an attempt rather than
    // refusing, so their absence in a matrix run is evidence.
    let forbidden: Vec<&String> = args
        .iter()
        .filter(|a| super::FORBIDDEN_FLAGS.contains(&a.as_str()))
        .collect();
    if !forbidden.is_empty() {
        marker(
            &markers,
            json!({"event":"forbidden_flag","flags":forbidden}),
        )?;
    }

    if args.iter().any(|a| a == "--version") {
        println!("opencode v{version}");
        return Ok(());
    }
    if args.iter().any(|a| a == "--help") {
        let subcommand = args.first().filter(|a| !a.starts_with("--"));
        match subcommand {
            Some(name) => println!("{SOURCE} help for {name}"),
            None => println!("{SOURCE} top help"),
        }
        return Ok(());
    }

    let model = scenario["model"]
        .as_str()
        .unwrap_or(DEFAULT_MODEL)
        .to_owned();
    let session = "ses_fake";
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let mut cancelled = false;

    while let Some(line) = lines.next() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = message["id"].clone();
        match message["method"].as_str().unwrap_or_default() {
            "initialize" => reply(
                &id,
                json!({"protocolVersion":1,
                       "agentCapabilities":{"loadSession":true,
                           "promptCapabilities":{"embeddedContext":true,"image":true},
                           "sessionCapabilities":{"close":{},"delete":{},"fork":{},
                                                  "list":{},"resume":{}}},
                       "authMethods":[{"id":"opencode-login","name":"Login with opencode",
                                       "description":"Run `opencode auth login` in the terminal"}],
                       "agentInfo":{"name":"OpenCode","version":version},
                       "_meta":{"source":SOURCE}}),
            )?,

            // The session reports what it will actually use, before any prompt.
            "session/new" => {
                marker(&markers, json!({"event":"session_created","model":&model}))?;
                reply(
                    &id,
                    json!({"sessionId":session,"configOptions":config_options(&model)}),
                )?;
            }

            "session/prompt" => {
                marker(
                    &markers,
                    json!({"event":"prompt_received",
                           "model":&model,
                           "prompt_blocks":message["params"]["prompt"].as_array().map(Vec::len)}),
                )?;
                if let Some(delay) = scenario["delay_ms"].as_u64() {
                    std::thread::sleep(std::time::Duration::from_millis(delay));
                }
                if !scenario["permission_request"].is_null() {
                    request_permission(
                        &scenario["permission_request"],
                        session,
                        &mut lines,
                        &markers,
                    )?;
                }
                if let Some(calls) = scenario["tool_calls"].as_array() {
                    for (index, call) in calls.iter().enumerate() {
                        update(
                            session,
                            json!({"sessionUpdate":"tool_call","toolCallId":format!("call_{index}"),
                                   "title":&call["title"],"kind":&call["kind"],
                                   "rawInput":&call["input"],"status":"completed"}),
                        )?;
                    }
                }
                update(
                    session,
                    json!({"sessionUpdate":"agent_message_chunk",
                           "content":{"type":"text","text":"fake turn complete"}}),
                )?;
                let total = scenario["usage_total"].as_u64().unwrap_or(256);
                marker(
                    &markers,
                    json!({"event":"turn_complete","usage_total":total}),
                )?;
                reply(
                    &id,
                    json!({"stopReason":if cancelled { "cancelled" } else { "end_turn" },
                           "_meta":{"source":SOURCE,
                                    "usage":{"inputTokens":total / 2,"outputTokens":total / 2}}}),
                )?;
            }

            "session/cancel" => {
                // A harness that ignores cancel, so a host's bounded
                // escalation can be proven rather than assumed.
                if scenario["ignore_cancel"] == true {
                    marker(&markers, json!({"event":"ignoring_cancel"}))?;
                    continue;
                }
                cancelled = true;
                marker(&markers, json!({"event":"cancelled"}))?;
                if !id.is_null() {
                    reply(&id, json!({}))?;
                }
            }

            _ if !id.is_null() => emit(&json!({"jsonrpc":"2.0","id":id,
                "error":{"code":-32601,"message":"method not found"}}))?,
            _ => {}
        }
    }
    Ok(())
}

/// Ask the client for a decision, the way ACP specifies, and record what came
/// back — including any field that would make the decision outlive the request.
fn request_permission(
    request: &Value,
    session: &str,
    lines: &mut impl Iterator<Item = std::io::Result<String>>,
    markers: &Option<PathBuf>,
) -> Result<()> {
    let id = json!(9001);
    emit(
        &json!({"jsonrpc":"2.0","id":id,"method":"session/request_permission",
        "params":{"sessionId":session,
            "toolCall":{"toolCallId":"call_permission","title":&request["title"],
                        "kind":request["kind"].as_str().unwrap_or("execute"),
                        "rawInput":&request["input"]},
            "options":[{"optionId":"allow","name":"Allow","kind":"allow_once"},
                       {"optionId":"allow_always","name":"Always allow","kind":"allow_always"},
                       {"optionId":"reject","name":"Reject","kind":"reject_once"}]}}),
    )?;
    for line in lines {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if message["id"] != id {
            continue;
        }
        let outcome = &message["result"]["outcome"];
        let widening: Vec<&str> = WIDENING_FIELDS
            .iter()
            .filter(|field| !outcome[**field].is_null())
            .copied()
            .collect();
        marker(
            markers,
            json!({"event":"permission_decision",
                   "outcome":&outcome["outcome"],
                   "option_id":&outcome["optionId"],
                   // An "always" option was offered; acting on one would
                   // widen a permission beyond this request.
                   "always_option_offered":true,
                   "always_option_taken":outcome["optionId"] == "allow_always",
                   "widening_fields_received":widening}),
        )?;
        return Ok(());
    }
    Ok(())
}
