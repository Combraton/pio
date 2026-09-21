//! Labeled fake OpenCode ACP server for offline tests. It speaks the Agent
//! Client Protocol shapes over stdio but runs no model, tool or command.
//! Everything it produces is labeled `pio-fake-opencode-acp`; it is never a
//! qualified OpenCode and never real-harness evidence.
//!
//! **Measured against 2.0.11 at zero tokens:** the `initialize` result, and
//! `session/new` returning `configOptions` with the model, effort and mode the
//! session will use — which is why the provider and model can be checked
//! before any prompt.
//!
//! **From the ACP specification, not measured:** the `session/request_permission`
//! request and its response. ADR 005 records them as unverified against the
//! real harness, and PIO forwards no decision whose single-use form it has
//! not measured. The option **ids** are this fake's own invention outright:
//! `opt_1`, `opt_2`, `opt_3`, chosen so that no id is its own kind and a host
//! that hard-codes one fails here rather than in front of the owner's
//! harness.
//!
//! Scenario (JSON in `PIO_OPENCODE_FAKE_SCENARIO`, all members optional):
//! `version`, `model` (what the session reports, so a silent downgrade can be
//! played), `permission_request` (whose own `omit_option_kinds` plays an agent
//! that does not offer one), `tool_calls`, `usage_total`, `message_chunks`,
//! `usage_on_updates` (a harness that reports usage as it goes rather than
//! once at the end), `model_on_creation`, `ignore_model_selection`,
//! `refuse_model_selection`, `mode_on_creation`, `mode`,
//! `ignore_mode_selection`, `session_error`, `delay_ms`, `ignore_cancel`,
//! `markers`.
//! A `tool_calls` entry takes `title`, `kind`, `input` and an optional
//! `status` for the state the call ends in.
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::PathBuf;

pub const SOURCE: &str = "pio-fake-opencode-acp";
pub const DEFAULT_MODEL: &str = "minimax-coding-plan/MiniMax-M2.7-highspeed";

/// What a **new** session starts on, before a client selects anything.
///
/// Measured from OpenCode 2.0.11: a new ACP session reports this, not the
/// owner's configured model and certainly not the one PIO asked for. This
/// fake used to echo the requested model back, which is exactly why nobody
/// noticed the host never selected one until the first live run refused.
pub const MODEL_ON_CREATION: &str = "opencode/deepseek-v4.1-flash";

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

/// The `usage_update` session update, in the shape measured from 2.0.11.
///
/// `used` is a running total, `size` the context window, `cost` a money
/// amount that is zero on a subscription plan. None of those names contains
/// `usage` or ends in `tokens`, which is the whole point of it being here.
fn usage_update(used: u64) -> Value {
    json!({"sessionUpdate":"usage_update",
           "cost":{"amount":0,"currency":"USD"},
           "size":204800,
           "used":used})
}

fn update(session: &str, update: Value) -> Result<()> {
    emit(&json!({"jsonrpc":"2.0","method":"session/update",
                 "params":{"sessionId":session,"update":update}}))
}

/// The session's own report of what it will use. Measured shape.
fn config_options(model: &str, mode: &str) -> Value {
    json!([
        {"id":"model","name":"Model","category":"model","type":"select",
         "currentValue":model,
         "options":[{"value":model,"name":model},
                    {"value":DEFAULT_MODEL,"name":DEFAULT_MODEL},
                    {"value":MODEL_ON_CREATION,"name":MODEL_ON_CREATION},
                    {"value":"opencode/nemotron-3.5-lightning-free",
                     "name":"opencode/nemotron-3.5-lightning-free"}]},
        {"id":"effort","name":"Effort","type":"select","currentValue":"default",
         "options":[{"value":"default","name":"default"}]},
        {"id":"mode","name":"Mode","type":"select","currentValue":mode,
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

    // A new session each time, as a real agent returns. A fixed id made two
    // runs look identical, and a receipt field that cannot vary cannot be
    // checked.
    let session: &str = Box::leak(
        format!(
            "ses_fake_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        )
        .into_boxed_str(),
    );
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    // Whether a client announced itself. ACP has no permission capability to
    // declare, so this is the handshake and nothing else.
    // What this session currently reports. A new session does not start on
    // the client's model; only a selection moves it.
    let mut current = scenario["model_on_creation"]
        .as_str()
        .unwrap_or(MODEL_ON_CREATION)
        .to_owned();
    // The session's posture. `build` is what a real session starts on.
    let mut mode = scenario["mode_on_creation"]
        .as_str()
        .unwrap_or("build")
        .to_owned();
    let mut handshook = false;
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
        if message["method"] == "initialize" {
            handshook = true;
            marker(&markers, json!({"event":"initialize_received"}))?;
        }
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

            // The session reports what it will actually use, before any
            // prompt — and it does **not** start on the model the client
            // wants. Measured from 2.0.11.
            "session/new" if !scenario["session_error"].is_null() => {
                // A harness refusing with a message of its own. Real harness
                // errors routinely name a file, which is why this exists.
                marker(&markers, json!({"event":"session_refused"}))?;
                emit(&json!({"jsonrpc":"2.0","id":&id,
                    "error":{"code":-32603,"message":&scenario["session_error"]}}))?;
            }

            "session/new" => {
                marker(
                    &markers,
                    json!({"event":"session_created","model":&current,
                           "model_on_creation":&current}),
                )?;
                reply(
                    &id,
                    json!({"sessionId":session,
                           "configOptions":config_options(&current, &mode)}),
                )?;
            }

            // Measured against 2.0.11: this is how a client selects a model,
            // and the answer carries the harness's own updated report.
            // `optionId` and `valueId` are rejected; the fields are
            // `configId` and `value`.
            "session/set_config_option" => {
                let config_id = message["params"]["configId"].as_str().unwrap_or_default();
                let value = message["params"]["value"].as_str().unwrap_or_default();
                if scenario["refuse_model_selection"] == true {
                    marker(
                        &markers,
                        json!({"event":"model_selection_refused","config_id":config_id}),
                    )?;
                    emit(&json!({"jsonrpc":"2.0","id":&id,
                        "error":{"code":-32602,"message":"Invalid params",
                                 "data":{"configId":{"_errors":["unknown option"]}}}}))?;
                    continue;
                }
                // A harness that answers without error and changes nothing.
                // PIO must not read the absence of an error as evidence the
                // selection took.
                let ignored = scenario["ignore_model_selection"] == true;
                // A harness that accepts the narrower posture and stays on
                // the one it had. PIO must read the report back, not the
                // absence of an error.
                if config_id == "mode" && scenario["ignore_mode_selection"] != true {
                    mode = scenario["mode"].as_str().unwrap_or(value).to_owned();
                }
                if config_id == "model" && !ignored {
                    // `model` in the scenario overrides, so a silent
                    // downgrade can still be played against a client that
                    // did everything right.
                    current = scenario["model"].as_str().unwrap_or(value).to_owned();
                }
                marker(
                    &markers,
                    json!({"event":"config_option_set","config_id":config_id,
                           "requested_value":value,"ignored":ignored,
                           "current_model":&current,"current_mode":&mode}),
                )?;
                reply(
                    &id,
                    json!({"configOptions":config_options(&current, &mode)}),
                )?;
            }

            "session/prompt" => {
                marker(
                    &markers,
                    json!({"event":"prompt_received",
                           "model":&current,
                           "prompt_blocks":message["params"]["prompt"].as_array().map(Vec::len)}),
                )?;
                if let Some(delay) = scenario["delay_ms"].as_u64() {
                    // A harness that works for a while starts streaming
                    // first. Sleeping before the first update instead made a
                    // cancel rehearsal wait out the whole delay and then
                    // cancel a turn that had already ended — the exact defect
                    // the Claude R5 attempts were spent on.
                    update(
                        session,
                        json!({"sessionUpdate":"agent_message_chunk",
                               "content":{"type":"text","text":"fake turn started"}}),
                    )?;
                    std::thread::sleep(std::time::Duration::from_millis(delay));
                }
                // An agent asks a client that is there to be asked, and
                // decides for itself otherwise. The Claude harness was
                // measured doing exactly that, and this fake used to ask
                // unconditionally — which is why the Claude attachment gap
                // survived four live runs before anything caught it.
                let decides_itself = !handshook || scenario["decide_by_rules"] == true;
                if !scenario["permission_request"].is_null() && decides_itself {
                    let state = if handshook {
                        "denied_by_harness_rules_shadowed_the_host"
                    } else {
                        "denied_by_harness_no_host_attached"
                    };
                    update(
                        session,
                        json!({"sessionUpdate":"tool_call","toolCallId":"call_permission",
                               "title":&scenario["permission_request"]["title"],
                               "kind":scenario["permission_request"]["kind"]
                                   .as_str().unwrap_or("execute"),
                               "rawInput":&scenario["permission_request"]["input"],
                               "status":"failed",
                               "content":[{"type":"content","content":{"type":"text",
                                   "text":"This command requires approval"}}]}),
                    )?;
                    marker(
                        &markers,
                        json!({"event":state,"attached":handshook,
                                            "tool_call_id":"call_permission"}),
                    )?;
                } else if !scenario["permission_request"].is_null() {
                    request_permission(
                        &scenario["permission_request"],
                        session,
                        &mut lines,
                        &markers,
                    )?;
                }
                if let Some(calls) = scenario["tool_calls"].as_array() {
                    for (index, call) in calls.iter().enumerate() {
                        // **Measured from 2.0.11.** A call is announced with
                        // an **empty** `rawInput` and no `locations`; the
                        // target arrives in a later `tool_call_update` keyed
                        // by the same id, and the final status after that.
                        // The fake used to put the input in the announcement,
                        // which is precisely why nothing caught the host
                        // reading only announcements.
                        let id = format!("call_{index}");
                        update(
                            session,
                            json!({"sessionUpdate":"tool_call","toolCallId":&id,
                                   "title":&call["title"],"kind":&call["kind"],
                                   "rawInput":{},"locations":[],"status":"pending"}),
                        )?;
                        let path = call["input"]["path"]
                            .as_str()
                            .or_else(|| call["input"]["file_path"].as_str());
                        update(
                            session,
                            json!({"sessionUpdate":"tool_call_update","toolCallId":&id,
                                   "title":&call["title"],"kind":&call["kind"],
                                   "rawInput":&call["input"],
                                   "locations":path.map(|p| json!([{"path":p}]))
                                       .unwrap_or_else(|| json!([])),
                                   "status":"in_progress"}),
                        )?;
                        update(
                            session,
                            json!({"sessionUpdate":"tool_call_update","toolCallId":&id,
                                   "status":call["status"].as_str().unwrap_or("completed")}),
                        )?;
                    }
                }
                let total = scenario["usage_total"].as_u64().unwrap_or(256);
                // Different work streams a different number of chunks, for the
                // same reason it costs a different number of tokens: a census
                // that is identical in every scenario measures nothing.
                let chunks = scenario["message_chunks"].as_u64().unwrap_or(1).max(1);
                for index in 0..chunks {
                    update(
                        session,
                        json!({"sessionUpdate":"agent_message_chunk",
                               "content":{"type":"text",
                                          "text":format!("fake turn chunk {index}")}}),
                    )?;
                    // A harness that reports as it goes rather than once at
                    // the end. R1 measured which one OpenCode is; this is the
                    // scenario that proves the census can see either.
                    if scenario["usage_on_updates"] == true {
                        update(session, usage_update(total * (index + 1) / chunks))?;
                    }
                }
                // **Measured from 2.0.11**, R1: exactly one of these arrives
                // on a short turn, and it names none of its fields `usage` or
                // `*tokens` — a client searching field names alone would miss
                // the one update kind that carries usage.
                if scenario["usage_on_updates"] != true {
                    update(session, usage_update(total))?;
                }
                marker(
                    &markers,
                    json!({"event":"turn_complete","usage_total":total,
                           "message_chunks":chunks,
                           "usage_on_updates":scenario["usage_on_updates"] == true}),
                )?;
                // **Measured from 2.0.11**, R1: the turn result carries
                // `usage` at the top level — not under `_meta` — and it counts
                // `thoughtTokens` beside input and output, with its own
                // `totalTokens`. The host read `_meta.usage` and a two-part
                // measure, so a live turn that cost 7,910 tokens was recorded
                // as unknown.
                let thought = total / 8;
                let input = (total - thought) * 3 / 4;
                reply(
                    &id,
                    json!({"stopReason":if cancelled { "cancelled" } else { "end_turn" },
                           "usage":{"inputTokens":input,
                                    "outputTokens":total - thought - input,
                                    "thoughtTokens":thought,
                                    "totalTokens":total},
                           "_meta":{"source":SOURCE}}),
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
/// The options the agent offers. **The ids are deliberately not the kinds.**
///
/// Ids are the agent's to invent, and a host that hard-codes `allow` or
/// `reject` has to fail here rather than in front of the owner's harness.
/// `omit_option_kinds` plays an agent that does not offer one of them.
fn permission_options(request: &Value) -> Value {
    let omit: Vec<&str> = request["omit_option_kinds"]
        .as_array()
        .map(|kinds| kinds.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    Value::Array(
        [
            ("opt_1", "Allow once", "allow_once"),
            ("opt_2", "Always allow", "allow_always"),
            ("opt_3", "Reject", "reject_once"),
        ]
        .iter()
        .filter(|(_, _, kind)| !omit.contains(kind))
        .map(|(id, name, kind)| json!({"optionId":id,"name":name,"kind":kind}))
        .collect(),
    )
}

fn request_permission(
    request: &Value,
    session: &str,
    lines: &mut impl Iterator<Item = std::io::Result<String>>,
    markers: &Option<PathBuf>,
) -> Result<()> {
    let id = json!(9001);
    let options = permission_options(request);
    // The tool call the request is about, announced the way a real agent
    // announces one. Without it there is no record for a decision to be
    // attributed to, which is how `decided_by` went untested here.
    // Announced the way 2.0.11 announces one: empty `rawInput`, no
    // `locations`, `status: pending`. What the call touches arrives next.
    emit(&json!({"jsonrpc":"2.0","method":"session/update",
        "params":{"sessionId":session,"update":{
            "sessionUpdate":"tool_call","toolCallId":"call_permission",
            "title":&request["title"],
            "kind":request["kind"].as_str().unwrap_or("execute"),
            "rawInput":{},"locations":[],"status":"pending"}}}))?;
    let target = request["input"]["path"]
        .as_str()
        .or_else(|| request["input"]["file_path"].as_str());
    emit(&json!({"jsonrpc":"2.0","method":"session/update",
        "params":{"sessionId":session,"update":{
            "sessionUpdate":"tool_call_update","toolCallId":"call_permission",
            "title":&request["title"],
            "kind":request["kind"].as_str().unwrap_or("execute"),
            "rawInput":&request["input"],
            "locations":target.map(|p| json!([{"path":p}]))
                .unwrap_or_else(|| json!([])),
            "status":"in_progress"}}}))?;
    emit(
        &json!({"jsonrpc":"2.0","id":id,"method":"session/request_permission",
        "params":{"sessionId":session,
            "toolCall":{"toolCallId":"call_permission","title":&request["title"],
                        "kind":request["kind"].as_str().unwrap_or("execute"),
                        "rawInput":&request["input"]},
            "options":&options}}),
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
        // Which option was taken is read back from the list this fake
        // offered, by id, so the kind is measured rather than guessed from a
        // name. A host that selects by kind and a fake that reports by kind
        // can disagree; a fake that assumes the id cannot even notice.
        let chosen = options
            .as_array()
            .and_then(|list| {
                list.iter()
                    .find(|option| option["optionId"] == outcome["optionId"])
            })
            .cloned();
        let chosen_kind = chosen
            .as_ref()
            .map(|option| option["kind"].clone())
            .unwrap_or(Value::Null);
        marker(
            markers,
            json!({"event":"permission_decision",
                   "outcome":&outcome["outcome"],
                   "option_id":&outcome["optionId"],
                   "option_kind":&chosen_kind,
                   // An "always" option is offered unless the scenario omits
                   // it; acting on one would widen a permission beyond this
                   // request.
                   "always_option_offered":options.as_array()
                       .is_some_and(|list| list.iter().any(|o| o["kind"] == "allow_always")),
                   "options_offered":&options,
                   "always_option_taken":chosen_kind.as_str()
                       .is_some_and(|kind| kind.ends_with("_always")),
                   "widening_fields_received":widening}),
        )?;
        return Ok(());
    }
    Ok(())
}
