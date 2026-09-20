//! Durable host for Claude Code (ADR 004). The lifecycle around this is the
//! shared one; what is here is what Claude Code says on the wire.
//!
//! The awkward measured fact shapes the order below: **`system/init` does not
//! arrive until a message is written to stdin**. The effective permission mode
//! therefore cannot be checked before the brief is released. The mode is
//! controlled where it is actually decided — the argument vector, checked
//! before the spawn — and the `init` echo is corroboration after delivery. If
//! they differ the turn is aborted and the receipt says delivery preceded the
//! check, rather than claiming a pre-flight guarantee.
use crate::harness::{self, Lifecycle, StdioChild};
use crate::identity;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const ADAPTER: &str = "claude";

/// The arguments PIO drives. `--permission-prompts host` routes decisions to
/// PIO; neither dangerous-skip flag appears here or anywhere else.
pub const STREAM_ARGS: &[&str] = &[
    "--print",
    "--input-format",
    "stream-json",
    "--output-format",
    "stream-json",
    "--verbose",
    "--replay-user-messages",
    "--permission-prompts",
    "host",
];

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
        pio_claude::fake::SOURCE
    } else {
        "claude-code"
    }
}

fn user_message(text: &str) -> Value {
    json!({"type":"user","message":{"role":"user","content":[{"type":"text","text":text}]}})
}

pub fn claude_host(root: &Path, command: &str, invocation_id: &str) -> Result<()> {
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

fn run_turn(life: &mut Lifecycle, server: &mut Option<StdioChild>) -> Result<()> {
    let home = PathBuf::from(life.spec["home"].as_str().context("home")?);
    let config_dir = PathBuf::from(life.spec["config_dir"].as_str().context("config_dir")?);
    let fixture = PathBuf::from(life.spec["fixture_root"].as_str().context("fixture_root")?);
    let cwd = PathBuf::from(life.spec["cwd"].as_str().context("cwd")?);
    let requested = life.spec["permission_mode"]
        .as_str()
        .context("permission_mode")?
        .to_owned();

    let before = pio_claude::durable_snapshot(&home, &config_dir, &fixture)?;
    life.event(json!({"kind":"config_before","snapshot":before}))?;

    // Refused before the spawn: a mode that is not the user's configured
    // default, and a credential route that is not usable.
    let guard = pio_claude::permission_mode_guard(&before["settings"], &requested);
    life.guard("permission_mode_guard", &guard, "permission_mode_refused")?;

    let env: Vec<(String, String)> = life.spec["env"]
        .as_object()
        .context("env")?
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
        .collect();
    let executable = PathBuf::from(life.spec["executable"].as_str().context("executable")?);

    let mut args: Vec<String> = STREAM_ARGS.iter().map(|a| (*a).to_owned()).collect();
    args.push("--permission-mode".into());
    args.push(requested.clone());
    // PIO never selects a model outside the owner's dated, test-only exception,
    // which the service checked before this host was launched.
    if let Some(model) = life.spec["model"].as_str() {
        args.push("--model".into());
        args.push(model.to_owned());
    }

    let stderr = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(life.root.join(format!("claude-{}.stderr", life.invocation)))?;
    let child = server.insert(StdioChild::spawn(&executable, &args, &env, &cwd, stderr)?);
    let spawned = child.child.id();
    let child_identity = identity(spawned)?;
    let native = match life.spec["qualification"]["binary_sha256"].as_str() {
        Some(expected) => {
            let observed =
                pio_claude::sha256_hex(&std::fs::read(std::fs::canonicalize(&executable)?)?);
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

    let brief = pio_core::spool::Spool::open(&life.root)?.read(
        life.spec["brief"]["digest"]
            .as_str()
            .context("brief digest")?,
    )?;
    let text = String::from_utf8(brief).context("brief is not UTF-8 text")?;
    let sent = user_message(&text);

    // Nothing arrives from this harness until the brief is written, so the
    // release and that write happen together under the controller gate.
    let gate = life.release()?;
    child.send(&sent)?;
    life.event(json!({"kind":"turn_start_sent",
        "input_sha256":pio_claude::sha256_hex(text.as_bytes())}))?;
    drop(gate);

    let spool = pio_core::spool::Spool::open(&life.root)?;
    let refs = life
        .root
        .join(format!("output-{}.refs.jsonl", life.invocation));
    let mut output_offset = 0u64;
    let mut all_output = Vec::new();
    let mut result: Option<Value> = None;
    let mut acknowledged = false;
    let mut mode_matched: Option<bool> = None;
    let mut pending_actions: std::collections::BTreeMap<u64, Value> =
        std::collections::BTreeMap::new();
    let mut action_seq = 0u64;
    let mut messages: Vec<Value> = Vec::new();

    while result.is_none() {
        if let Some(message) = child.receive(Duration::from_millis(25))? {
            messages.push(message.clone());
            match message["type"].as_str().unwrap_or_default() {
                "system" if message["subtype"] == "init" => {
                    let effective = message["permissionMode"].as_str().unwrap_or_default();
                    mode_matched = Some(effective == requested);
                    life.event(json!({"kind":"session_init",
                        "session_id":message["session_id"],
                        "claude_code_version":message["claude_code_version"],
                        "requested_permission_mode":requested,
                        "effective_permission_mode":message["permissionMode"],
                        "effective_mode_matches_requested":mode_matched,
                        // Measured: init arrives only after the brief, so this
                        // check cannot precede delivery.
                        "checked_after_delivery":true,
                        "api_key_source":message["apiKeySource"],
                        "configured_model":life.spec["configured_model"],
                        "requested_model":life.spec["model"],
                        "model":message["model"],
                        "capabilities":message["capabilities"],
                        // Names only, never arguments, content or output.
                        "tools":message["tools"],
                        "mcp_servers":message["mcp_servers"],
                        "plugins":message["plugins"],
                        "slash_commands":message["slash_commands"].as_array().map(Vec::len),
                        "skills":message["skills"].as_array().map(Vec::len),
                        "agents":message["agents"].as_array().map(Vec::len),
                        "messaging_socket_path_present":!message["messaging_socket_path"].is_null()}))?;
                    ensure!(
                        mode_matched == Some(true),
                        "effective_permission_mode_mismatch: requested {requested}, effective {}",
                        message["permissionMode"]
                    );
                }
                // The replay echo is the delivery proof: the exact message PIO
                // sent, returned by the harness.
                "user" if message["isReplay"] == true => {
                    acknowledged = message["message"] == sent["message"];
                    life.event(json!({"kind":"turn_acknowledged",
                        "replay_matches_sent":acknowledged}))?;
                }
                "assistant" => {
                    let mut line = serde_json::to_vec(&json!({"type":"assistant",
                        "message":message["message"]}))?;
                    line.push(b'\n');
                    let digest = spool.put(&line)?;
                    crate::append_json(
                        &refs,
                        &json!({"digest":digest,"offset":output_offset,"length":line.len()}),
                    )?;
                    output_offset += line.len() as u64;
                    all_output.extend_from_slice(&line);
                }
                "control_request" if message["request"]["subtype"] == "can_use_tool" => {
                    action_seq += 1;
                    let classification =
                        pio_claude::classify_permission_request(&message, &fixture, &cwd);
                    if classification["disposition"] == "decline" {
                        // PIO's own decline, not a caller's decision.
                        let decision = pio_claude::permission_decision(
                            &message,
                            "deny",
                            classification["reason"]
                                .as_str()
                                .unwrap_or("refused by PIO"),
                        )?;
                        child.send(&decision["envelope"])?;
                        life.event(json!({"kind":"request_declined_by_pio",
                            "action_seq":action_seq,"classification":classification}))?;
                    } else {
                        pending_actions.insert(action_seq, message["request_id"].clone());
                        life.event(json!({"kind":"action_requested",
                            "action_seq":action_seq,"request_id":message["request_id"],
                            "classification":classification}))?;
                    }
                }
                "control_request" => {
                    // PIO answers nothing else on the user's behalf.
                    life.event(json!({"kind":"native_request_declined",
                        "subtype":message["request"]["subtype"],
                        "reason":"declined by PIO: no user is attached to answer this request"}))?;
                }
                "result" => {
                    life.event(json!({"kind":"turn_completed",
                        "is_error":message["is_error"],
                        "terminal_reason":message["terminal_reason"],
                        "stop_reason":message["stop_reason"],
                        "num_turns":message["num_turns"],
                        "permission_denials":message["permission_denials"],
                        "usage":message["usage"],
                        "model_usage":message["modelUsage"],
                        "total_cost_usd":message["total_cost_usd"]}))?;
                    result = Some(message);
                }
                _ => {}
            }
        } else if let Ok(Some(status)) = child.child.try_wait() {
            // The harness left without a result; say so rather than waiting.
            life.event(json!({"kind":"child_exited_without_result","code":status.code()}))?;
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
                    match request {
                        Some(request_id) => {
                            let original = messages
                                .iter()
                                .rev()
                                .find(|m| m["request_id"] == request_id)
                                .cloned()
                                .unwrap_or(Value::Null);
                            match pio_claude::permission_decision(
                                &original,
                                decision,
                                "declined by the caller",
                            ) {
                                Ok(encoded) => {
                                    child.send(&encoded["envelope"])?;
                                    life.event(json!({"kind":"control_applied",
                                        "control_id":id,"action_seq":control["action_seq"],
                                        "decision":decision,
                                        "suggestions_offered":encoded["suggestions_offered"],
                                        "suggestions_acted_on":0,
                                        "widening_fields_sent":[]}))?;
                                }
                                Err(error) => life.event(json!({"kind":"control_rejected",
                                    "control_id":id,"reason":format!("{error:#}")}))?,
                            }
                        }
                        None => life.event(json!({"kind":"control_rejected",
                            "control_id":id,"reason":"no pending action"}))?,
                    }
                }
                Some("interrupt") => {
                    // Measured: the in-band interrupt is unverified against
                    // 2.1.278, so this is SIGINT and is described as SIGINT.
                    // A signal may end the turn before `result`, which is the
                    // only place usage is reported, so usage may be unknown.
                    let pid = child.child.id() as i32;
                    let sent = unsafe { libc::kill(pid, libc::SIGINT) } == 0;
                    life.event(json!({"kind":"control_sent","control_id":id,
                        "method":"SIGINT","in_band":false,"signal_delivered":sent,
                        "usage_may_be_unknown":true}))?;
                }
                _ => life.event(json!({"kind":"control_rejected",
                    "control_id":id,"reason":"unknown control"}))?,
            }
        }
    }

    child.close_stdin();
    let exit = life.stop(&mut child.child)?;
    let after = pio_claude::durable_snapshot(&home, &config_dir, &fixture)?;
    let diff = pio_claude::durable_diff(&before, &after);
    let tool_uses = pio_claude::tool_use_records(&messages, &fixture, &cwd);
    life.event(json!({"kind":"child_exited","code":exit}))?;
    life.event(json!({"kind":"tool_uses","record":tool_uses}))?;
    life.event(json!({"kind":"config_after","snapshot":after,"diff":diff}))?;
    let receipt = json!({
        "source":source(&life.spec),
        "kind":"native_turn_completed",
        "turn_completed":result.is_some(),
        "delivery_acknowledged":acknowledged,
        "effective_mode_matches_requested":mode_matched,
        "child_exit":exit,
        "output_digest":pio_core::digest(&all_output),
        "output_bytes":output_offset,
        "containment":tool_uses["containment"],
        "liability":tool_uses["liability"],
        "completion_is_acceptance":false,
    });
    life.complete(receipt)?;
    Ok(())
}
