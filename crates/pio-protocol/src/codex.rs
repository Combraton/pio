//! Codex app-server adapter on the Protocol provider (ADR 003). The durable
//! host owns native I/O; this module turns its fsync'd events into Protocol
//! facts and delivers committed controls to it.
use crate::provider::*;
use anyhow::{Context, Result};
use pio_host::codex::{ALLOWED_DECISIONS, append_control, events_path, read_jsonl};
use serde_json::{Value, json};

pub const CONTENT_EXTENSION: &str = "pio.combraton.dev/content";
const CONTENT_PATH: &str = "/extensions/pio.combraton.dev~1content";
pub const FEATURES: &[&str] = &[
    "execution.controller",
    "execution.output",
    "execution.discovery",
    "execution.workspaces",
    "execution.usage",
    "execution.actions",
    "execution.steering",
];

fn push(v: &mut Value, x: Value) {
    if !v.is_array() {
        *v = json!([]);
    }
    v.as_array_mut().unwrap().push(x);
}

/// Digest-referenced content a command carries, if any.
fn content_reference<'a>(method: &str, p: &'a Value) -> Option<&'a Value> {
    match method {
        "execution.submit" => Some(&p["payload"]["brief"]),
        "execution.steer" => Some(&p["payload"]["message"]),
        "execution.respond_action" => Some(&p["payload"]["response"]),
        _ => None,
    }
}

impl Provider {
    pub(crate) fn codex(&self) -> bool {
        self.host_config["adapter"] == "codex"
    }

    pub(crate) fn codex_source(&self) -> &'static str {
        if self.host_config["labeled_fake"] == true {
            pio_codex::fake::SOURCE
        } else {
            "codex-app-server"
        }
    }

    /// Verify and spool the optional content extension. The digest in the
    /// payload binds the bytes, so a verified extension is safe to store before
    /// the command commits; content-addressed bytes are not an effect.
    pub(crate) fn codex_content(&self, method: &str, p: &Value) -> Result<(), Error> {
        let Some(content) = p["extensions"].get(CONTENT_EXTENSION) else {
            return Ok(());
        };
        let Some(reference) = content_reference(method, p) else {
            return Err(invalid(CONTENT_PATH));
        };
        let bytes = match (content["text"].as_str(), content["base64"].as_str()) {
            (Some(text), None) => text.as_bytes().to_vec(),
            (None, Some(encoded)) => {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .map_err(|_| invalid(CONTENT_PATH))?
            }
            _ => return Err(invalid(CONTENT_PATH)),
        };
        if content["media_type"] != reference["media_type"]
            || pio_core::digest(&bytes) != text(&reference["digest"])
        {
            return Err(invalid(CONTENT_PATH));
        }
        pio_core::spool::Spool::open(&self.root)
            .and_then(|spool| spool.put(&bytes))
            .map_err(|_| err("unavailable", json!({})))?;
        Ok(())
    }

    fn content_available(&self, reference: &Value) -> Option<Vec<u8>> {
        pio_core::spool::Spool::open(&self.root)
            .ok()?
            .read(text(&reference["digest"]))
            .ok()
    }

    /// Workspace, content and launch-spec checks for a Codex submit. Returns
    /// a refusal reason or stores the spec on the execution.
    pub(crate) fn codex_admission(&self, e: &mut Value, p: &Value) -> Option<&'static str> {
        let Some(workspace) = p["payload"].get("workspace") else {
            e["view"]["alternative"] =
                "Request a workspace under the configured fixture root".into();
            return Some("capability_unavailable");
        };
        let fixture_root = std::path::Path::new(text(&self.host_config["fixture_root"]));
        let repository = std::fs::canonicalize(text(&workspace["repository"])).ok();
        let root = std::fs::canonicalize(fixture_root).ok();
        let Some(repository) = repository.filter(|r| {
            root.as_ref()
                .is_some_and(|root| r.starts_with(root) && r != root)
        }) else {
            e["view"]["alternative"] =
                "Use a throwaway fixture repository under the configured fixture root".into();
            return Some("capability_unavailable");
        };
        let head = std::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(["rev-parse", "HEAD"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
        if head.as_deref() != Some(text(&workspace["base"])) {
            e["view"]["alternative"] = "The workspace base must equal the repository HEAD".into();
            return Some("capability_unavailable");
        }
        if self.content_available(&p["payload"]["brief"]).is_none() {
            e["view"]["alternative"] =
                format!("Send the brief bytes in the {CONTENT_EXTENSION} extension").into();
            return Some("capability_unavailable");
        }
        let host = &self.host_config;
        e["codex_spec"] = json!({
            "adapter":"codex",
            "labeled_fake":host["labeled_fake"] == true,
            "executable":host["executable"],
            "env":host["env"],
            "codex_home":host["codex_home"],
            "fixture_root":host["fixture_root"],
            "thread":host["thread"],
            "qualification":host["qualification_binding"],
            "cwd":repository,
            "brief":p["payload"]["brief"],
        });
        None
    }

    /// Record a Codex steer, action response or cancel as an effect and a
    /// pending control; the tick appends controls only after the commit.
    pub(crate) fn codex_apply(
        &mut self,
        e: &mut Value,
        method: &str,
        p: &Value,
        op: &str,
        effects: &mut Vec<Value>,
    ) -> Reply {
        let id = text(&e["view"]["execution"]["id"]).to_owned();
        match method {
            "execution.steer" => {
                let n = num(&e["steer_count"]) + 1;
                e["steer_count"] = n.into();
                let steer_id = format!("{id}.steer-{n}");
                let delivery = format!("{id}.steering-{n}");
                let message = p["payload"]["message"].clone();
                let live = e["codex"]["turn_id"].is_string() && e["view"]["runtime"] != "exited";
                if self.content_available(&message).is_none() {
                    return Err(invalid(CONTENT_PATH));
                }
                if !live {
                    let alternative = "Steering needs an acknowledged, running native turn";
                    push(
                        &mut e["view"]["steering"],
                        json!({"steer_id":steer_id,"request":"not_supported","recorded_at":self.now,"alternative":alternative}),
                    );
                    self.execution_event(
                        e,
                        "execution.steer.requested",
                        json!({"steer_id":steer_id,"request":"not_supported"}),
                        Some((p, op)),
                    );
                    return Ok(
                        json!({"steer_id":steer_id,"request":"not_supported","alternative":alternative}),
                    );
                }
                self.codex_effect(
                    e,
                    &delivery,
                    "execution.steering_delivery",
                    "non_repeatable",
                    op,
                    &message["digest"],
                );
                effects.push(json!(delivery));
                push(
                    &mut e["view"]["steering"],
                    json!({"steer_id":steer_id,"request":"recorded","recorded_at":self.now,"delivery_id":delivery,"delivery":"pending","behavior":"not_observed"}),
                );
                push(
                    &mut e["codex_controls"],
                    json!({"id":delivery,"kind":"steer","digest":message["digest"],"appended":false}),
                );
                self.execution_event(
                    e,
                    "execution.steer.requested",
                    json!({"steer_id":steer_id,"request":"recorded"}),
                    Some((p, op)),
                );
                Ok(json!({"steer_id":steer_id,"request":"recorded","delivery_id":delivery}))
            }
            "execution.respond_action" => {
                let action_id = text(&p["payload"]["action_id"]).to_owned();
                let pending = list(&e["view"]["actions"])
                    .iter()
                    .any(|a| a["action_id"] == action_id && a["state"] == "pending");
                if !pending {
                    return Err(err("not_found", json!({})));
                }
                let response = p["payload"]["response"].clone();
                let decision = self
                    .content_available(&response)
                    .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                    .and_then(|v| v["decision"].as_str().map(str::to_owned));
                let Some(decision) = decision.filter(|d| {
                    response["media_type"] == "application/json"
                        && ALLOWED_DECISIONS.contains(&d.as_str())
                }) else {
                    return Err(invalid("/payload/response"));
                };
                let seq = e["codex_actions"][&action_id]["seq"].clone();
                let n = num(&e["response_count"]) + 1;
                e["response_count"] = n.into();
                let effect = format!("{id}.response-{n}");
                self.codex_effect(
                    e,
                    &effect,
                    "execution.action_response",
                    "non_repeatable",
                    op,
                    &response["digest"],
                );
                effects.push(json!(effect));
                for action in e["view"]["actions"].as_array_mut().unwrap() {
                    if action["action_id"] == action_id {
                        action["state"] = "answered".into();
                        action["answered_at"] = self.now.clone().into();
                        action["response_effect"] = effect.clone().into();
                    }
                }
                if e["view"]["runtime_detail"]["action_id"] == action_id {
                    e["view"].as_object_mut().unwrap().remove("runtime_detail");
                    e["view"]["runtime"] = "active".into();
                }
                push(
                    &mut e["codex_controls"],
                    json!({"id":effect,"kind":"respond_action","action_seq":seq,"decision":decision,"appended":false}),
                );
                self.execution_event(
                    e,
                    "execution.action.answered",
                    json!({"action_id":action_id,"response_effect":effect}),
                    Some((p, op)),
                );
                Ok(json!({"action_id":action_id,"state":"answered","response_effect":effect}))
            }
            _ => Err(err(
                "capability_unavailable",
                json!({"reason":"not implemented by the Codex adapter"}),
            )),
        }
    }

    fn codex_effect(
        &mut self,
        e: &mut Value,
        id: &str,
        kind: &str,
        class: &str,
        op: &str,
        digest: &Value,
    ) {
        let mut auth = json!({"principal":e["principal"]});
        if let Some(grant) = e["submit"].get("grant") {
            auth["grant"] = grant.clone();
        }
        let descriptor = json!({"id":id,"kind":kind,"target":e["view"]["execution"],"payload_digest":digest,"authorization":auth,"retry_class":class,"operation_ref":op});
        self.data.effects.insert(id.into(), json!({"effect":descriptor,"revision":1,"status":"pending","observations":[{"status":"pending","evidence":self.host_evidence("recorded_before_dispatch"),"recorded_at":self.now}],"attempts":[],"obligations":[{"id":format!("{id}.evidence"),"expects":"evidence","deadline":null,"state":"open"}]}));
        push(&mut e["view"]["effects"], json!(id));
    }

    fn codex_observe_effect(&mut self, id: &str, status: &str, class: &str, close: bool) {
        let evidence = self.host_evidence(class);
        let Some(r) = self.data.effects.get_mut(id) else {
            return;
        };
        r["revision"] = (num(&r["revision"]) + 1).into();
        r["status"] = status.into();
        push(
            &mut r["observations"],
            json!({"status":status,"evidence":evidence,"recorded_at":self.now}),
        );
        if close {
            for o in r["obligations"].as_array_mut().unwrap() {
                if matches!(text(&o["state"]), "open" | "overdue") {
                    o["state"] = "satisfied".into();
                }
            }
        }
    }

    pub(crate) fn codex_discovery(&self) -> Value {
        let executable = text(&self.host_config["executable"]);
        let detected = std::path::Path::new(executable).exists();
        let fake = self.host_config["labeled_fake"] == true;
        let qualified = self.host_config["qualification_binding"].is_object();
        // Only a launch observes authentication and reachability; discovery
        // never starts an app-server in the user's Codex home to probe.
        let observed = self
            .data
            .executions
            .values()
            .filter(|e| e["codex"]["account_observed_at"].is_string())
            .max_by(|a, b| {
                text(&a["codex"]["account_observed_at"])
                    .cmp(text(&b["codex"]["account_observed_at"]))
            });
        let (authentication, reachable, verified) = match observed {
            Some(e) => (
                if e["codex"]["authentication_type"].is_string() {
                    "authenticated"
                } else {
                    "unauthenticated"
                },
                "yes",
                Some(e["codex"]["account_observed_at"].clone()),
            ),
            None => ("unknown", "unknown", None),
        };
        let recognized = if fake {
            "no"
        } else if qualified {
            "yes"
        } else {
            "unknown"
        };
        let mut installation = json!({"installation_id":"codex-selected","harness":if fake {"PIO labeled fake Codex app-server (not Codex)"} else {"Codex CLI app-server"},"detected":detected,"adapter_recognized":recognized,"version_supported":if qualified {"yes"} else if fake {"no"} else {"unknown"},"authentication":authentication,"reachable":reachable});
        if qualified {
            installation["version"] = pio_codex::PINNED_VERSION.into();
        }
        if let Some(at) = verified {
            installation["last_verified"] = at;
        }
        installation["usable"] = (detected
            && recognized == "yes"
            && qualified
            && authentication == "authenticated"
            && reachable == "yes"
            && installation["last_verified"].is_string())
        .into();
        json!({"installations":[installation]})
    }

    /// One durable tick for a Codex execution: journal the dispatch marker,
    /// admit to the host, deliver committed controls, then translate host
    /// events and output into Protocol facts.
    pub(crate) fn run_codex(&mut self, e: &mut Value) -> Result<()> {
        let id = text(&e["view"]["execution"]["id"]).to_owned();
        let command = pio_core::digest(id.as_bytes());
        let command = command.trim_start_matches("sha256:").to_owned();
        if e["recovery_no_admit"] == true
            || matches!(
                text(&e["view"]["delivery"]),
                "failed_before_delivery" | "not_delivered"
            ) && e["codex_events_offset"].is_null()
        {
            return Ok(());
        }
        self.dispatch_marker(e)?;
        let host = self.durable.as_ref().context("durable host")?;
        if host.inspect(&command).is_err() {
            e["host_submission"] = host.submit_codex(&command, e["codex_spec"].clone())?;
        }
        let observed = host.inspect(&command)?;
        let invocation = text(&observed["invocation"]["invocation_id"]).to_owned();
        e["codex"]["invocation_id"] = invocation.clone().into();
        let root = host.root.clone();
        let phase = text(&observed["invocation"]["phase"]).to_owned();
        e["codex"]["phase"] = phase.clone().into();

        // Controls are appended only after the command that created them was
        // committed; a repeated append after a crash is ignored by the host.
        let mut controls_changed = false;
        if let Some(controls) = e["codex_controls"].as_array_mut() {
            for control in controls.iter_mut() {
                if control["appended"] != true {
                    let mut record = control.clone();
                    record.as_object_mut().unwrap().remove("appended");
                    append_control(&root, &invocation, &record)?;
                    control["appended"] = true.into();
                    controls_changed = true;
                }
            }
        }
        if let Some(cancel) = e["pending_cancel"].as_str().map(str::to_owned)
            && e["codex_cancel_appended"] != true
        {
            append_control(&root, &invocation, &json!({"id":cancel,"kind":"interrupt"}))?;
            e["codex_cancel_appended"] = true.into();
            controls_changed = true;
        }
        let _ = controls_changed;
        // Owner correction 3: the execution deadline stops work with a real
        // turn/interrupt whose response and turn outcome are recorded. The host
        // is never killed for it.
        if list(&e["timeouts_passed"]).contains(&json!("execution_deadline"))
            && e["view"]["runtime"] != "exited"
            && e["codex"]["deadline_stop"].is_null()
        {
            let control = format!("{id}.deadline-stop");
            append_control(
                &root,
                &invocation,
                &json!({"id":control,"kind":"interrupt"}),
            )?;
            e["codex"]["deadline_stop"] =
                json!({"control_id":control,"requested_at":self.now,"request":"appended_for_host"});
        }

        let (events, offset) = read_jsonl(
            &events_path(&root, &invocation),
            num(&e["codex_events_offset"]),
        )?;
        e["codex_events_offset"] = offset.into();
        for event in events {
            self.codex_event(e, &id, &event)?;
        }
        if phase == "known_not_released" && e["view"]["delivery"] == "pending" {
            let reason = observed["invocation"]["receipt"]["reason"].clone();
            self.delivery_observed(
                e,
                "failed_before_delivery",
                "native_turn_never_sent",
                None,
                true,
                false,
            );
            e["codex"]["refusal"] = reason;
        } else if observed["recovery"] == "uncertain_no_respawn" {
            // A lost host after release is never respawned or re-sent.
            if e["view"]["delivery"] == "pending" {
                self.delivery_observed(
                    e,
                    "ambiguous",
                    "host_lost_after_release",
                    None,
                    false,
                    false,
                );
            }
            if !matches!(text(&e["view"]["runtime"]), "exited" | "unknown") {
                e["view"]["runtime"] = "unknown".into();
                e["view"].as_object_mut().unwrap().remove("runtime_detail");
                self.execution_event(
                    e,
                    "execution.runtime.changed",
                    json!({"runtime":"unknown"}),
                    None,
                );
            }
        }
        self.drain_output(e, &invocation)?;
        Ok(())
    }

    fn codex_event(&mut self, e: &mut Value, id: &str, event: &Value) -> Result<()> {
        match text(&event["kind"]) {
            "account" => {
                e["codex"]["authentication_type"] = event["authentication_type"].clone();
                e["codex"]["account_observed_at"] = self.now.clone().into();
            }
            "spawned" => {
                e["codex"]["native"] = event["native"].clone();
            }
            "config_before" => {
                e["codex"]["config_before_sha256"] = event["snapshot"]["raw_sha256"].clone();
            }
            "config_after" => {
                e["codex"]["config_diff"] = event["diff"].clone();
            }
            "thread_started" => {
                e["codex"]["thread"] = json!({"thread_id":event["thread_id"],"configured_model":event["configured_model"],"requested_model":event["requested_model"],"model":event["model"],"model_provider":event["model_provider"],"sandbox":event["sandbox"],"approval_policy":event["approval_policy"]});
            }
            "turn_acknowledged" => {
                e["codex"]["turn_id"] = event["turn_id"].clone();
                if matches!(text(&e["view"]["delivery"]), "pending" | "ambiguous") {
                    let reconcile = e["view"]["delivery"] == "ambiguous";
                    self.delivery_observed(
                        e,
                        "acknowledged",
                        "native_turn_acknowledged",
                        Some("provider_ack_id"),
                        true,
                        reconcile,
                    );
                }
                if e["view"]["runtime"] != "requires_action" {
                    e["view"]["runtime"] = "active".into();
                    self.execution_event(
                        e,
                        "execution.runtime.changed",
                        json!({"runtime":"active"}),
                        None,
                    );
                }
            }
            "turn_start_failed" => {
                if e["view"]["delivery"] == "pending" {
                    self.delivery_observed(
                        e,
                        "not_delivered",
                        "native_turn_rejected",
                        None,
                        true,
                        false,
                    );
                }
                e["codex"]["turn_error"] = event["error"].clone();
            }
            "action_requested" => {
                let action_id = format!("{id}.action-{}", num(&event["action_seq"]));
                e["codex_actions"][&action_id] = json!({"seq":event["action_seq"],"method":event["method"],"approval_kind":event["approval_kind"],"request_id":event["request_id"]});
                push(
                    &mut e["view"]["actions"],
                    json!({"action_id":action_id,"owner":"codex","state":"pending","requested_at":self.now}),
                );
                e["view"]["runtime"] = "requires_action".into();
                e["view"]["runtime_detail"] = json!({"action_id":action_id,"owner":"codex"});
                self.execution_event(
                    e,
                    "execution.runtime.changed",
                    json!({"runtime":"requires_action","action_id":action_id,"owner":"codex"}),
                    None,
                );
            }
            "thread_settings_guard" => {
                e["codex"]["thread_settings"] = event["guard"].clone();
            }
            "control_sent" | "control_response" | "control_rejected"
                if e["codex"]["deadline_stop"]["control_id"] == event["control_id"] =>
            {
                let stop = &mut e["codex"]["deadline_stop"];
                match text(&event["kind"]) {
                    "control_sent" => stop["request"] = "turn_interrupt_sent".into(),
                    "control_response" => {
                        stop["request"] = if event["error"].is_null() {
                            "turn_interrupt_acknowledged"
                        } else {
                            "turn_interrupt_refused"
                        }
                        .into();
                        stop["response_error"] = event["error"].clone();
                    }
                    _ => {
                        stop["request"] = "rejected_by_host".into();
                        stop["reason"] = event["reason"].clone();
                    }
                }
            }
            "control_applied" => {
                let control = text(&event["control_id"]).to_owned();
                self.codex_observe_effect(&control, "pending", "native_response_written", false);
            }
            "request_resolved" => {
                // The app-server resolved a request we answered.
                let answered: Vec<String> = list(&e["view"]["actions"])
                    .iter()
                    .filter(|a| a["state"] == "answered" && a["response_effect"].is_string())
                    .filter(|a| {
                        e["codex_actions"][text(&a["action_id"])]["request_id"]
                            == event["request_id"]
                    })
                    .map(|a| text(&a["response_effect"]).to_owned())
                    .collect();
                for effect in answered {
                    self.codex_observe_effect(
                        &effect,
                        "succeeded",
                        "native_request_resolved",
                        true,
                    );
                }
            }
            "control_rejected" => {
                self.codex_observe_effect(
                    text(&event["control_id"]),
                    "failed",
                    "host_rejected_control",
                    true,
                );
            }
            "control_response" => {
                let control = text(&event["control_id"]).to_owned();
                if e["pending_cancel"] == control.as_str() {
                    // The interrupt response acknowledges the request only.
                    self.codex_observe_effect(
                        &control,
                        if event["error"].is_null() {
                            "succeeded"
                        } else {
                            "failed"
                        },
                        "native_interrupt_request_acknowledged",
                        event["error"].is_null(),
                    );
                } else if let Some(turn) = event["result"]["turnId"].as_str() {
                    let _ = turn;
                    self.codex_observe_effect(
                        &control,
                        "succeeded",
                        "native_steer_acknowledged",
                        true,
                    );
                    let evidence = self.host_evidence("native_steer_acknowledged");
                    let mut acknowledged = None;
                    if let Some(entries) = e["view"]["steering"].as_array_mut() {
                        for entry in entries.iter_mut() {
                            if entry["delivery_id"] == control.as_str() {
                                entry["delivery"] = "acknowledged".into();
                                entry["proof_class"] = "provider_ack_id".into();
                                entry["evidence"] = evidence.clone();
                                acknowledged = Some(entry["steer_id"].clone());
                            }
                        }
                    }
                    if let Some(steer_id) = acknowledged {
                        self.execution_event(e, "execution.steer.delivery.observed", json!({"steer_id":steer_id,"delivery_id":control,"delivery":"acknowledged","evidence":evidence}), None);
                    }
                } else {
                    self.codex_observe_effect(&control, "failed", "native_steer_refused", true);
                }
            }
            "usage" => {
                if let Some(total) = event["total"]["totalTokens"].as_u64() {
                    let invocation = e["codex"]["invocation_id"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("{id}.invocation-1"));
                    let observation = json!({"invocation_id":invocation,"basis":"observed","measure":"codex.tokens.total","amount":total,"recorded_at":self.now});
                    e["view"]["usage"]["observations"] = json!([observation.clone()]);
                    e["view"]["usage"]["liability"] = "resolved".into();
                    self.execution_event(e, "execution.usage.observed", observation, None);
                }
            }
            "turn_completed" => {
                let status = text(&event["status"]).to_owned();
                e["codex"]["turn_status"] = status.clone().into();
                if e["codex"]["deadline_stop"].is_object()
                    && e["codex"]["deadline_stop"]["outcome"].is_null()
                {
                    e["codex"]["deadline_stop"]["outcome"] = status.clone().into();
                    e["codex"]["deadline_stop"]["observed_at"] = self.now.clone().into();
                }
                if let Some(cancel) = e["pending_cancel"].as_str().map(str::to_owned)
                    && e["view"]["cancellation"].get("outcome").is_none()
                {
                    let outcome = if status == "interrupted" {
                        "cancelled"
                    } else {
                        "unknown"
                    };
                    e["view"]["cancellation"]["outcome"] = outcome.into();
                    self.codex_observe_effect(&cancel, "succeeded", "native_turn_completed", true);
                    self.execution_event(
                        e,
                        "execution.cancel.observed",
                        json!({"outcome":outcome}),
                        None,
                    );
                }
            }
            "app_server_exited" => {
                let exit = event["code"]
                    .as_i64()
                    .map(|code| json!({"code":code}))
                    .unwrap_or(json!("unavailable"));
                if e["view"]["usage"]["observations"]
                    .as_array()
                    .is_none_or(|a| a.is_empty())
                {
                    e["view"]["usage"]["liability"] = "unresolved".into();
                }
                e["view"]["exit"] = exit.clone();
                e["view"]["runtime"] = "exited".into();
                e["view"].as_object_mut().unwrap().remove("runtime_detail");
                self.execution_event(e, "execution.exit.observed", json!({"exit":exit}), None);
            }
            "host_error" => {
                e["codex"]["host_error"] = event["error"].clone();
            }
            _ => {}
        }
        Ok(())
    }
}
