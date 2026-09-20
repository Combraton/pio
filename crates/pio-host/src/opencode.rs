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

    // The owner's rule, and the whole reason this adapter is better placed than
    // the Claude one: the session reports what it will use before any prompt,
    // so a refusal here happens with the brief still inside PIO.
    let guard = pio_opencode::session_configuration_guard(&created["result"], &requested_model);
    life.event(json!({"kind":"session_created",
        "session_id":created["result"]["sessionId"],
        "requested_model":requested_model,
        "reported_model":guard["reported"],
        "model_matches_requested":guard["allowed"],
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
    let mut pending_actions: std::collections::BTreeMap<u64, Value> =
        std::collections::BTreeMap::new();
    let mut action_seq = 0u64;
    // Who decided each tool call, by its ACP `toolCallId`, and which ones were
    // refused. Without these every refusal reads as an effect PIO observed.
    let mut decided = json!({});
    let mut refused: Vec<Value> = Vec::new();
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
                    let input = &message["params"]["toolCall"]["rawInput"];
                    let classification = pio_claude::classify_permission_request(
                        &json!({"request":{"input":input,
                            "tool_name":message["params"]["toolCall"]["kind"],
                            "tool_use_id":message["params"]["toolCall"]["toolCallId"]}}),
                        &cwd,
                        &cwd,
                    );
                    if classification["disposition"] == "decline" {
                        // Single-use reject. An always-allow option is offered
                        // on every request and is never taken.
                        child.send(&json!({"jsonrpc":"2.0","id":message["id"],
                            "result":{"outcome":{"outcome":"selected","optionId":"reject"}}}))?;
                        if let Some(id) = message["params"]["toolCall"]["toolCallId"].as_str() {
                            decided[id] = json!({"by":"pio","decision":"deny",
                                "reason":classification["reason"]});
                            refused.push(json!({"tool_use_id":id}));
                        }
                        life.event(json!({"kind":"request_declined_by_pio",
                            "action_seq":action_seq,"decision":"deny",
                            "decided_by":"pio",
                            "tool_use_id":message["params"]["toolCall"]["toolCallId"],
                            "classification":classification}))?;
                    } else {
                        pending_actions.insert(action_seq, message.clone());
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

        for control in life.controls()? {
            let id = control["id"].as_str().unwrap_or_default().to_owned();
            match control["kind"].as_str() {
                Some("respond_action") => {
                    let decision = control["decision"].as_str().unwrap_or_default();
                    let request = control["action_seq"]
                        .as_u64()
                        .and_then(|seq| pending_actions.remove(&seq));
                    match (request, decision) {
                        (Some(request), "allow" | "deny") => {
                            let option = if decision == "allow" {
                                "allow"
                            } else {
                                "reject"
                            };
                            child.send(&json!({"jsonrpc":"2.0","id":request["id"],
                                "result":{"outcome":{"outcome":"selected","optionId":option}}}))?;
                            if let Some(tool_id) =
                                request["params"]["toolCall"]["toolCallId"].as_str()
                            {
                                decided[tool_id] = json!({"by":"caller","decision":decision});
                                if decision == "deny" {
                                    refused.push(json!({"tool_use_id":tool_id}));
                                }
                            }
                            life.event(json!({"kind":"control_applied","control_id":id,
                                "action_seq":control["action_seq"],"decision":decision,
                                "decided_by":"caller",
                                "tool_use_id":request["params"]["toolCall"]["toolCallId"],
                                "option_id":option,"always_option_taken":false,
                                "widening_fields_sent":[]}))?;
                        }
                        (Some(request), _) => {
                            pending_actions
                                .insert(control["action_seq"].as_u64().unwrap_or(0), request);
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
