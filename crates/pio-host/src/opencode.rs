//! Durable host for OpenCode over ACP (ADR 005). The lifecycle around this is
//! the shared one; what is here is what OpenCode says on the wire.
//!
//! The ordering is the point, and it is better than the Claude adapter's:
//! `session/new` reports the model the session will use **before any prompt**,
//! so the provider and model are checked, and refused, while the turn is still
//! unstarted and the brief has not left PIO.
use crate::harness::{self, Lifecycle, StdioChild};
use crate::identity;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const ADAPTER: &str = "opencode";

/// How long a turn is given to end after `session/cancel` before the child is
/// killed. Cancel here is in band, unlike the Claude adapter's signal.
pub const CANCEL_ESCALATION: Duration = Duration::from_secs(10);

pub fn events_path(root: &Path, invocation: &str) -> PathBuf {
    harness::events_path(root, ADAPTER, invocation)
}
pub fn controls_path(root: &Path, invocation: &str) -> PathBuf {
    harness::controls_path(root, ADAPTER, invocation)
}
pub fn append_control(root: &Path, invocation: &str, control: &Value) -> Result<()> {
    harness::append_control(root, ADAPTER, invocation, control)
}

/// Every place in a value where the harness reports token usage, by path.
///
/// Written as a search rather than a lookup. R1's job is to measure **where**
/// ACP puts usage and which update kinds carry it, and a lookup can only
/// confirm the place it already assumed — which is how the Claude adapter
/// summed two of four parts for five live runs before anything noticed.
fn usage_sites(value: &Value, prefix: &str, found: &mut Vec<(String, Value)>) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                if key.eq_ignore_ascii_case("usage") || key.to_ascii_lowercase().ends_with("tokens")
                {
                    // Recorded whole and not descended into, so one usage
                    // object counts once rather than once per counter.
                    found.push((path, inner.clone()));
                    continue;
                }
                usage_sites(inner, &path, found);
            }
        }
        Value::Array(items) => {
            for (index, inner) in items.iter().enumerate() {
                usage_sites(inner, &format!("{prefix}[{index}]"), found);
            }
        }
        _ => {}
    }
}

/// What to send back for one permission decision, and what to record about it.
///
/// **Option ids are the agent's to invent.** `allow` and `reject` were the
/// labeled fake's own invention, and a host that hard-codes them selects
/// nothing on a harness that names them anything else. ACP puts the meaning in
/// `kind`, so that is what PIO matches: `allow_once` for an allow and
/// `reject_once` for a deny or a default reject. An `*_always` kind is never
/// selected — every decision PIO encodes is single-use — and when the kind a
/// decision needs is not offered PIO selects nothing and answers `cancelled`,
/// which refuses the call rather than permitting it.
fn permission_answer(request: &Value, decision: &str) -> (Value, Value) {
    let wanted = if decision == "allow" {
        "allow_once"
    } else {
        "reject_once"
    };
    let offered = request["params"]["options"].clone();
    let chosen = offered
        .as_array()
        .and_then(|options| options.iter().find(|option| option["kind"] == wanted))
        .cloned();
    let outcome = match &chosen {
        Some(option) => json!({"outcome":"selected","optionId":&option["optionId"]}),
        None => json!({"outcome":"cancelled"}),
    };
    let record = json!({
        "option_kind_required":wanted,
        "option_kind_offered":chosen.is_some(),
        "option_id":chosen.as_ref().map(|option| option["optionId"].clone()),
        "option_kind":chosen.as_ref().map(|option| option["kind"].clone()),
        // Measured from the option actually selected, not asserted: a field
        // that cannot be false is not a check.
        "always_option_taken":chosen.as_ref()
            .and_then(|option| option["kind"].as_str())
            .is_some_and(|kind| kind.ends_with("_always")),
        "options_offered":offered,
        "outcome":&outcome,
    });
    (outcome, record)
}

pub fn source(spec: &Value) -> &'static str {
    if spec["labeled_fake"] == true {
        pio_opencode::fake::SOURCE
    } else {
        "opencode-acp"
    }
}

pub fn opencode_host(root: &Path, command: &str, invocation_id: &str) -> Result<()> {
    let mut life = Lifecycle::claim(root, command, invocation_id, ADAPTER, |spec| {
        source(spec).to_owned()
    })?;
    let mut child: Option<StdioChild> = None;
    let outcome = run_turn(&mut life, &mut child);
    if let Err(error) = &outcome {
        life.fail(error, || {
            if let Some(child) = child.as_mut() {
                let _ = child.child.kill();
                let _ = child.child.wait();
            }
        });
    }
    outcome
}

/// One JSON-RPC exchange on the ACP stdio transport.
struct Rpc {
    next_id: u64,
}

impl Rpc {
    fn request(&mut self, child: &mut StdioChild, method: &str, params: Value) -> Result<u64> {
        self.next_id += 1;
        child.send(&json!({"jsonrpc":"2.0","id":self.next_id,"method":method,"params":params}))?;
        Ok(self.next_id)
    }

    /// Wait for the response to `id`, letting the caller see everything else.
    fn wait(
        &mut self,
        child: &mut StdioChild,
        id: u64,
        seconds: u64,
        mut observe: impl FnMut(&Value) -> Result<()>,
    ) -> Result<Value> {
        let deadline = std::time::Instant::now() + Duration::from_secs(seconds);
        while std::time::Instant::now() < deadline {
            let Some(message) = child.receive(Duration::from_millis(25))? else {
                continue;
            };
            if message["id"].as_u64() == Some(id) {
                return Ok(message);
            }
            observe(&message)?;
        }
        anyhow::bail!("no ACP response to request {id} within {seconds}s")
    }
}

fn run_turn(life: &mut Lifecycle, server: &mut Option<StdioChild>) -> Result<()> {
    let home = PathBuf::from(life.spec["home"].as_str().context("home")?);
    // The boundary is this run's workspace repository, not the directory
    // fixtures are created in; see the note in the Claude host.
    let cwd = PathBuf::from(life.spec["cwd"].as_str().context("cwd")?);
    let requested_model = life.spec["model"].as_str().context("model")?.to_owned();

    // The owner's own service, recorded so a run can prove it did not move.
    let owner_before = pio_opencode::owner_service();
    life.event(json!({"kind":"config_before",
        "snapshot":{"owner_service":owner_before,
                    "home":pio_opencode::sha256_hex(home.display().to_string().as_bytes())}}))?;

    let env: Vec<(String, String)> = life.spec["env"]
        .as_object()
        .context("env")?
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
        .collect();
    for (name, _) in &env {
        ensure!(
            !pio_opencode::FORBIDDEN_FLAGS.contains(&name.as_str()),
            "forbidden_flag_in_environment: {name}"
        );
    }
    let executable = PathBuf::from(life.spec["executable"].as_str().context("executable")?);

    let stderr = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(
            life.root
                .join(format!("opencode-{}.stderr", life.invocation)),
        )?;
    // `opencode acp` starts its own private server as a child, so isolation is
    // structural: `--server` is never passed and the owner's service is never
    // reached. `--auto` is never passed either.
    let args = vec!["acp".to_owned()];
    let child = server.insert(StdioChild::spawn(&executable, &args, &env, &cwd, stderr)?);
    let spawned = child.child.id();
    let child_identity = identity(spawned)?;
    let native = match life.spec["qualification"]["binary_sha256"].as_str() {
        Some(expected) => {
            let observed =
                pio_opencode::sha256_hex(&std::fs::read(std::fs::canonicalize(&executable)?)?);
            ensure!(
                observed == expected,
                "executable_changed_since_qualification"
            );
            json!({"verified":true,"sha256":observed})
        }
        None => json!({"verified":false,"reason":"labeled fake has no qualification record"}),
    };
    life.spawned(&serde_json::to_value(&child_identity)?, &native)?;
    life.park(child_identity)?;

    let mut rpc = Rpc { next_id: 0 };
    let id = rpc.request(
        child,
        "initialize",
        json!({"protocolVersion":1,"clientCapabilities":{"fs":{}}}),
    )?;
    let initialize = rpc.wait(child, id, 60, |_| Ok(()))?;
    ensure!(
        initialize["error"].is_null(),
        "acp_initialize_refused: {}",
        initialize["error"]
    );
    life.event(json!({"kind":"session_started",
        "agent":initialize["result"]["agentInfo"],
        "auth_methods":initialize["result"]["authMethods"],
        "capabilities":initialize["result"]["agentCapabilities"]}))?;

    let id = rpc.request(
        child,
        "session/new",
        json!({"cwd":cwd.display().to_string(),"mcpServers":[]}),
    )?;
    let created = rpc.wait(child, id, 120, |_| Ok(()))?;
    ensure!(
        created["error"].is_null(),
        "acp_session_refused: {}",
        created["error"]
    );
    let session = created["result"]["sessionId"]
        .as_str()
        .context("session id")?
        .to_owned();

    // **A new session does not start on the model PIO asked for.** Measured
    // against 2.0.11: `session/new` reports the harness's own default —
    // `opencode/deepseek-v4.1-flash`, not even the owner's configured model —
    // and the only way to change it is to select it on this session. The
    // first live run refused here, correctly, because the host had only ever
    // *checked* the model and never *set* it; the labeled fake had hidden
    // that by echoing back whatever was requested.
    let selected = if requested_model.is_empty() {
        Value::Null
    } else {
        let id = rpc.request(
            child,
            "session/set_config_option",
            json!({"sessionId":session,"configId":"model","value":requested_model}),
        )?;
        let answer = rpc.wait(child, id, 120, |_| Ok(()))?;
        life.event(json!({"kind":"model_selected",
            "requested_model":requested_model,
            "method":"session/set_config_option",
            "config_id":"model",
            "error":answer["error"],
            // The harness answers with its own updated report, which is what
            // the guard below is run against. PIO does not take the absence
            // of an error as evidence that the selection took.
            "reported_after":answer["result"]["configOptions"]}))?;
        ensure!(
            answer["error"].is_null(),
            "model_selection_refused: {}",
            answer["error"]
        );
        answer["result"].clone()
    };

    // The owner's rule, and the whole reason this adapter is better placed than
    // the Claude one: the session reports what it will use before any prompt,
    // so a refusal here happens with the brief still inside PIO. It is run
    // against the report the harness gave **after** the selection, so a
    // selection that silently did not take is a refusal rather than a claim.
    let reported = if selected.is_null() {
        created["result"].clone()
    } else {
        selected
    };
    let guard = pio_opencode::session_configuration_guard(&reported, &requested_model);
    life.event(json!({"kind":"session_created",
        "session_id":created["result"]["sessionId"],
        "requested_model":requested_model,
        // What the session started on, before PIO selected anything. Recorded
        // because it is the harness's own default and nobody should have to
        // rediscover it from a refusal.
        "model_on_creation":pio_opencode::session_configuration_guard(
            &created["result"], &requested_model)["reported"],
        "reported_model":guard["reported"],
        "model_matches_requested":guard["allowed"],
        "model_selected_by_pio":!requested_model.is_empty(),
        // Measured on this transport: the session reports what it will use
        // **before** any prompt, so a refusal here happens with the brief
        // still inside PIO. The Claude adapter cannot do this.
        "checked_before_delivery":true}))?;
    life.guard("settings_guard", &guard, "session_configuration_refused")?;

    let brief = pio_core::spool::Spool::open(&life.root)?.read(
        life.spec["brief"]["digest"]
            .as_str()
            .context("brief digest")?,
    )?;
    let text = String::from_utf8(brief).context("brief is not UTF-8 text")?;

    let gate = life.release()?;
    let prompt = rpc.request(
        child,
        "session/prompt",
        json!({"sessionId":session,"prompt":[{"type":"text","text":text}]}),
    )?;
    life.event(json!({"kind":"turn_start_sent",
        "input_sha256":pio_opencode::sha256_hex(text.as_bytes())}))?;
    drop(gate);

    let spool = pio_core::spool::Spool::open(&life.root)?;
    let refs = life
        .root
        .join(format!("output-{}.refs.jsonl", life.invocation));
    let mut output_offset = 0u64;
    let mut all_output = Vec::new();
    let mut acknowledged = false;
    let mut tool_calls: Vec<Value> = Vec::new();
    // Each surfaced request, with the moment PIO will answer it itself if no
    // caller has.
    let mut pending_actions: std::collections::BTreeMap<u64, (Value, std::time::Instant)> =
        std::collections::BTreeMap::new();
    let mut action_seq = 0u64;
    // Measured rather than assumed: every session update kind seen, and every
    // place an update or the turn result reported usage. Offline the answer
    // can only be the fake's; on a live run it is OpenCode's, which is what
    // R1 exists to find out.
    let mut update_kinds: std::collections::BTreeMap<String, u64> =
        std::collections::BTreeMap::new();
    let mut usage_reports: Vec<Value> = Vec::new();
    let mut options_recorded = false;
    // Who decided each tool call, by its ACP `toolCallId`, and which ones were
    // refused. Without these every refusal reads as an effect PIO observed.
    let mut decided = json!({});
    let mut refused: Vec<Value> = Vec::new();
    // The caller's own delivery timeout is how long their decision may take.
    // A request nobody answers holds the harness open forever, so the default
    // is a **single-use reject**, recorded as PIO's.
    let answer_timeout = life.spec["action_answer_timeout_seconds"]
        .as_u64()
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(120));
    let mut cancel_deadline: Option<(std::time::Instant, String)> = None;
    let mut escalation: Option<Value> = None;
    let mut result: Option<Value> = None;

    while result.is_none() {
        if let Some(message) = child.receive(Duration::from_millis(25))? {
            if message["id"].as_u64() == Some(prompt) {
                let stop = &message["result"]["stopReason"];
                life.event(json!({"kind":"turn_completed",
                    "status":if message["error"].is_null() { "completed" } else { "failed" },
                    "stop_reason":stop,"error":message["error"]}))?;
                let mut sites = Vec::new();
                usage_sites(&message["result"], "", &mut sites);
                if !sites.is_empty() {
                    usage_reports.push(json!({"where":"session/prompt result",
                        "update_kind":Value::Null,
                        "paths":sites.iter().map(|(path, _)| path.clone()).collect::<Vec<_>>(),
                        "values":sites.iter().map(|(_, value)| value.clone()).collect::<Vec<_>>()}));
                }
                if let Some(usage) = message["result"]["_meta"]["usage"].as_object() {
                    let total: u64 = ["inputTokens", "outputTokens"]
                        .iter()
                        .filter_map(|k| usage.get(*k).and_then(Value::as_u64))
                        .sum();
                    life.event(json!({"kind":"usage","total":{"totalTokens":total},
                        "detail":message["result"]["_meta"]["usage"]}))?;
                }
                result = Some(message);
                continue;
            }
            match message["method"].as_str().unwrap_or_default() {
                "session/update" => {
                    // Measured: this harness acknowledges nothing. The first
                    // update shows it acting on the prompt, which is evidence
                    // of receipt but not an identifier it returned — so there
                    // is no proof class. ADR 005 §7.
                    if !acknowledged {
                        acknowledged = true;
                        life.event(json!({"kind":"turn_acknowledged",
                            "first_session_update":true,
                            "proof_class":Value::Null}))?;
                    }
                    let update = &message["params"]["update"];
                    let update_kind = update["sessionUpdate"]
                        .as_str()
                        .unwrap_or("(absent)")
                        .to_owned();
                    *update_kinds.entry(update_kind.clone()).or_insert(0) += 1;
                    let mut sites = Vec::new();
                    usage_sites(update, "", &mut sites);
                    if !sites.is_empty() {
                        usage_reports.push(json!({"where":"session/update",
                            "update_kind":update_kind,
                            "paths":sites.iter().map(|(path, _)| path.clone()).collect::<Vec<_>>(),
                            "values":sites.iter().map(|(_, value)| value.clone())
                                .collect::<Vec<_>>()}));
                    }
                    if update["sessionUpdate"] == "tool_call" {
                        tool_calls.push(update.clone());
                    }
                    let mut line = serde_json::to_vec(&json!({"type":"session_update",
                        "update":update}))?;
                    line.push(b'\n');
                    let digest = spool.put(&line)?;
                    crate::append_json(
                        &refs,
                        &json!({"digest":digest,"offset":output_offset,"length":line.len()}),
                    )?;
                    output_offset += line.len() as u64;
                    all_output.extend_from_slice(&line);
                }
                "session/request_permission" => {
                    action_seq += 1;
                    if !options_recorded {
                        options_recorded = true;
                        // The measurement ADR 005 promises: the option list as
                        // the agent actually offers it, ids and kinds apart.
                        let options = message["params"]["options"].as_array().cloned();
                        life.event(json!({"kind":"permission_options_observed",
                            "options":&message["params"]["options"],
                            "option_ids":options.as_ref().map(|list| list.iter()
                                .map(|option| option["optionId"].clone()).collect::<Vec<_>>()),
                            "option_kinds":options.as_ref().map(|list| list.iter()
                                .map(|option| option["kind"].clone()).collect::<Vec<_>>())}))?;
                    }
                    let input = &message["params"]["toolCall"]["rawInput"];
                    let classification = pio_claude::classify_permission_request(
                        &json!({"request":{"input":input,
                            "tool_name":message["params"]["toolCall"]["kind"],
                            "tool_use_id":message["params"]["toolCall"]["toolCallId"]}}),
                        &cwd,
                        &cwd,
                    );
                    if classification["disposition"] == "decline" {
                        // Single-use reject, selected by **kind**: the id is
                        // the agent's to invent. An always-allow option is
                        // offered on every request and is never taken.
                        let (outcome, answer) = permission_answer(&message, "deny");
                        child.send(&json!({"jsonrpc":"2.0","id":message["id"],
                            "result":{"outcome":outcome}}))?;
                        if let Some(id) = message["params"]["toolCall"]["toolCallId"].as_str() {
                            decided[id] = json!({"by":"pio","decision":"deny",
                                "reason":classification["reason"]});
                            refused.push(json!({"tool_use_id":id}));
                        }
                        life.event(json!({"kind":"request_declined_by_pio",
                            "action_seq":action_seq,"decision":"deny",
                            "decided_by":"pio",
                            "tool_use_id":message["params"]["toolCall"]["toolCallId"],
                            "option_id":&answer["option_id"],
                            "option_kind":&answer["option_kind"],
                            "always_option_taken":&answer["always_option_taken"],
                            "answer":answer,
                            "classification":classification}))?;
                    } else {
                        pending_actions.insert(
                            action_seq,
                            (message.clone(), std::time::Instant::now() + answer_timeout),
                        );
                        life.event(json!({"kind":"action_requested",
                            "action_seq":action_seq,"request_id":message["id"],
                            "classification":classification}))?;
                    }
                }
                _ if !message["id"].is_null() => {
                    // PIO answers nothing else on the user's behalf, but it
                    // answers, so the harness is never left waiting.
                    child.send(&json!({"jsonrpc":"2.0","id":message["id"],
                        "error":{"code":-32601,
                                 "message":"declined by PIO: no user is attached"}}))?;
                    life.event(json!({"kind":"native_request_declined",
                        "method":message["method"],"error_response_sent":true}))?;
                }
                _ => {}
            }
        } else if let Some((deadline, control)) = cancel_deadline.as_ref()
            && std::time::Instant::now() >= *deadline
        {
            let control = control.clone();
            let killed = child.child.kill().is_ok();
            let _ = child.child.wait();
            escalation = Some(json!({"control_id":control,"from":"session/cancel",
                "to":"SIGKILL","waited_ms":CANCEL_ESCALATION.as_millis() as u64,
                "killed":killed}));
            life.event(json!({"kind":"interrupt_escalated","control_id":control,
                "from":"session/cancel","to":"SIGKILL",
                "waited_ms":CANCEL_ESCALATION.as_millis() as u64,
                "killed":killed,"usage":"unknown"}))?;
            break;
        } else if let Ok(Some(status)) = child.child.try_wait() {
            life.event(json!({"kind":"result_missing","code":status.code()}))?;
            break;
        }

        // A request nobody answered. PIO decides nothing on the user's behalf
        // except this: the default is a **single-use reject**, recorded as
        // PIO's own, because leaving it unanswered leaves the harness waiting
        // and the execution open.
        let overdue: Vec<u64> = pending_actions
            .iter()
            .filter(|(_, (_, deadline))| std::time::Instant::now() >= *deadline)
            .map(|(seq, _)| *seq)
            .collect();
        for seq in overdue {
            let Some((request, _)) = pending_actions.remove(&seq) else {
                continue;
            };
            let (outcome, answer) = permission_answer(&request, "deny");
            child.send(&json!({"jsonrpc":"2.0","id":request["id"],
                "result":{"outcome":outcome}}))?;
            if let Some(tool_id) = request["params"]["toolCall"]["toolCallId"].as_str() {
                decided[tool_id] = json!({"by":"pio","decision":"deny",
                    "reason":"no caller answered within the delivery timeout"});
                refused.push(json!({"tool_use_id":tool_id}));
            }
            life.event(json!({"kind":"request_denied_by_default",
                "action_seq":seq,
                "decision":"deny",
                "decided_by":"pio",
                "tool_use_id":request["params"]["toolCall"]["toolCallId"],
                "after_seconds":answer_timeout.as_secs(),
                "option_id":&answer["option_id"],
                "option_kind":&answer["option_kind"],
                "always_option_taken":&answer["always_option_taken"],
                "answer":answer,
                "widening_fields_sent":[]}))?;
        }

        for control in life.controls()? {
            let id = control["id"].as_str().unwrap_or_default().to_owned();
            match control["kind"].as_str() {
                Some("respond_action") => {
                    let decision = control["decision"].as_str().unwrap_or_default();
                    let request = control["action_seq"]
                        .as_u64()
                        .and_then(|seq| pending_actions.remove(&seq))
                        .map(|(request, _)| request);
                    match (request, decision) {
                        (Some(request), "allow" | "deny") => {
                            let (outcome, answer) = permission_answer(&request, decision);
                            let encodable = answer["option_kind_offered"] == true;
                            child.send(&json!({"jsonrpc":"2.0","id":request["id"],
                                "result":{"outcome":outcome}}))?;
                            if let Some(tool_id) =
                                request["params"]["toolCall"]["toolCallId"].as_str()
                            {
                                if encodable {
                                    decided[tool_id] = json!({"by":"caller","decision":decision});
                                    if decision == "deny" {
                                        refused.push(json!({"tool_use_id":tool_id}));
                                    }
                                } else {
                                    // The caller's decision could not be
                                    // expressed without widening the
                                    // permission, so PIO selected nothing.
                                    // The refusal is PIO's, not the caller's.
                                    decided[tool_id] = json!({"by":"pio","decision":"deny",
                                        "reason":format!("the harness offered no {} option",
                                            answer["option_kind_required"].as_str()
                                                .unwrap_or_default())});
                                    refused.push(json!({"tool_use_id":tool_id}));
                                }
                            }
                            life.event(json!({"kind":"control_applied","control_id":id,
                                "action_seq":control["action_seq"],
                                "decision":if encodable { decision } else { "deny" },
                                "requested_decision":decision,
                                "applied":encodable,
                                "decided_by":if encodable { "caller" } else { "pio" },
                                "tool_use_id":request["params"]["toolCall"]["toolCallId"],
                                "option_id":&answer["option_id"],
                                "option_kind":&answer["option_kind"],
                                "always_option_taken":&answer["always_option_taken"],
                                "answer":&answer,
                                "widening_fields_sent":[]}))?;
                            if !encodable {
                                life.event(json!({"kind":"option_kind_not_offered",
                                    "control_id":id,
                                    "required_kind":&answer["option_kind_required"],
                                    "requested_decision":decision,
                                    "options_offered":&answer["options_offered"],
                                    "outcome_sent":"cancelled",
                                    "reason":"PIO answers by option kind and never selects an \
                                              always kind, so a decision whose kind is not \
                                              offered is refused rather than approximated"}))?;
                            }
                        }
                        (Some(request), _) => {
                            pending_actions.insert(
                                control["action_seq"].as_u64().unwrap_or(0),
                                (request, std::time::Instant::now() + answer_timeout),
                            );
                            life.event(json!({"kind":"control_rejected","control_id":id,
                                "reason":"decision not in the single-use allowlist"}))?;
                        }
                        (None, _) => life.event(json!({"kind":"control_rejected",
                            "control_id":id,"reason":"no pending action"}))?,
                    }
                }
                Some("interrupt") => {
                    // In band, unlike the Claude adapter's signal.
                    rpc.request(child, "session/cancel", json!({"sessionId":session}))?;
                    life.event(json!({"kind":"control_sent","control_id":id,
                        "method":"session/cancel","in_band":true,
                        "escalates_after_ms":CANCEL_ESCALATION.as_millis() as u64}))?;
                    cancel_deadline
                        .get_or_insert((std::time::Instant::now() + CANCEL_ESCALATION, id));
                }
                _ => life.event(json!({"kind":"control_rejected","control_id":id,
                    "reason":"unknown control"}))?,
            }
        }
    }

    child.close_stdin();
    let exit = life.stop(&mut child.child)?;
    let owner_after = pio_opencode::owner_service();
    let tool_use_messages: Vec<Value> = tool_calls
        .iter()
        .map(|call| {
            json!({"message":{"content":[{"type":"tool_use",
            "name":&call["kind"],"id":&call["toolCallId"],"input":&call["rawInput"]}]}})
        })
        .collect();
    // ACP sends no denial list of its own, so the refusals PIO knows about are
    // the ones it or the caller decided. A tool call refused that way never
    // ran, so it is an attempt rather than an effect — the defect R6 found in
    // the Claude host, which was still open here.
    let tool_uses = pio_claude::tool_use_records(
        &tool_use_messages,
        &Value::Array(refused.clone()),
        &decided,
        &cwd,
        &cwd,
    );
    // Ordered deliberately: the exit event is what turns the runtime to
    // `exited`, so everything a caller must see on a finished execution is
    // recorded first. A matrix run caught the other order.
    // R1's measurement: how often this harness reports usage, and which
    // session update kinds carry it. The stop rules are next-turn stops until
    // this says otherwise, and saying so is the whole point of the first run.
    let mut usage_bearing: std::collections::BTreeMap<String, u64> =
        std::collections::BTreeMap::new();
    for report in usage_reports
        .iter()
        .filter(|r| r["where"] == "session/update")
    {
        if let Some(kind) = report["update_kind"].as_str() {
            *usage_bearing.entry(kind.to_owned()).or_insert(0) += 1;
        }
    }
    life.event(json!({"kind":"usage_granularity",
        "session_update_kinds":update_kinds,
        "usage_bearing_update_kinds":usage_bearing,
        "report_count":usage_reports.len(),
        "reported_during_turn":!usage_bearing.is_empty(),
        "reported_at_turn_end":usage_reports.iter()
            .any(|report| report["where"] == "session/prompt result"),
        "reports":usage_reports,
        "measure":"input+output; ACP reports no cache counters"}))?;
    life.event(json!({"kind":"tool_uses","record":tool_uses}))?;
    life.event(json!({"kind":"config_after",
        "snapshot":{"owner_service":owner_after},
        "diff":{"owner_service_untouched":owner_before == owner_after}}))?;
    life.event(json!({"kind":"harness_exited","code":exit}))?;
    ensure!(
        owner_before == owner_after,
        "owner_service_moved: PIO must never touch the owner's OpenCode service"
    );
    let receipt = json!({
        "source":source(&life.spec),
        "kind":"native_turn_completed",
        "turn_completed":result.is_some(),
        "delivery_acknowledged":acknowledged,
        // This harness returns no acknowledgment identifier. ADR 005 §7.
        "delivery_proof_class":Value::Null,
        "requested_model":requested_model,
        "session_guard":guard,
        "child_exit":exit,
        "cancel_escalation":escalation,
        "output_digest":pio_core::digest(&all_output),
        "output_bytes":output_offset,
        "containment":tool_uses["containment"],
        "liability":tool_uses["liability"],
        "owner_service_untouched":owner_before == owner_after,
        "completion_is_acceptance":false,
    });
    life.complete(receipt)?;
    Ok(())
}
