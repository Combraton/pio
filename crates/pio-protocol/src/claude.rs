//! The Claude Code event codec.
//!
//! Everything around this — admission, launch, controls, effects, delivery and
//! the deadline stop — is the adapter-agnostic path in [`crate::codex`]. What
//! is here is the one genuinely per-harness step: what each event the Claude
//! host records means to the Protocol view. See ADR 004 and
//! `docs/work/m3/SERVICE-BINDING.md`.
use crate::codex::push;
use crate::provider::*;
use anyhow::Result;
use serde_json::{Value, json};

impl Provider {
    pub(crate) fn claude_event(&mut self, e: &mut Value, id: &str, event: &Value) -> Result<()> {
        let ns = self.adapter().to_owned();
        match text(&event["kind"]) {
            "spawned" => e[&ns]["native"] = event["native"].clone(),
            "config_before" => {
                e[&ns]["config_before"] = event["snapshot"]["settings"]["files"].clone()
            }
            "config_after" => e[&ns]["config_diff"] = event["diff"].clone(),
            "permission_mode_guard" => e[&ns]["permission_mode"] = event["guard"].clone(),

            // The in-band half of the mode check. Measured: `system/init` never
            // arrives before the brief, so this is corroboration after
            // delivery, and the view says so rather than implying a pre-flight
            // guarantee. Names only — never arguments, content or output.
            "session_init" => {
                e[&ns]["session"] = json!({
                    "session_id":event["session_id"],
                    "claude_code_version":event["claude_code_version"],
                    "requested_permission_mode":event["requested_permission_mode"],
                    "effective_permission_mode":event["effective_permission_mode"],
                    "effective_mode_matches_requested":event["effective_mode_matches_requested"],
                    "checked_after_delivery":event["checked_after_delivery"],
                    "api_key_source":event["api_key_source"],
                    "configured_model":event["configured_model"],
                    "requested_model":event["requested_model"],
                    "model":event["model"],
                    "capabilities":event["capabilities"],
                    "tools":event["tools"],
                    "mcp_servers":event["mcp_servers"],
                    "plugins":event["plugins"],
                    "slash_commands":event["slash_commands"],
                    "skills":event["skills"],
                    "agents":event["agents"],
                });
            }

            // The replay echo is the delivery proof: the exact message PIO
            // sent, returned by the harness. An echo that does not match is
            // not a delivery.
            "turn_acknowledged" if event["replay_matches_sent"] == true => {
                if matches!(text(&e["view"]["delivery"]), "pending" | "ambiguous") {
                    let reconcile = e["view"]["delivery"] == "ambiguous";
                    self.delivery_observed(
                        e,
                        "acknowledged",
                        "native_replay_echo",
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
            "turn_acknowledged" => {
                e[&ns]["replay_mismatch"] = true.into();
            }

            // PIO's own decline, recorded as such. It is not an action the
            // caller was asked about and never becomes one.
            "request_declined_by_pio" => {
                push(
                    &mut e[&ns]["declined_by_pio"],
                    event["classification"].clone(),
                );
            }

            "action_requested" => {
                let action_id = format!("{id}.action-{}", num(&event["action_seq"]));
                e[format!("{ns}_actions")][&action_id] = json!({
                    "seq":event["action_seq"],
                    "request_id":event["request_id"],
                    "classification":event["classification"]});
                push(
                    &mut e["view"]["actions"],
                    json!({"action_id":action_id,"owner":"claude","state":"pending","requested_at":self.now}),
                );
                e["view"]["runtime"] = "requires_action".into();
                e["view"]["runtime_detail"] = json!({"action_id":action_id,"owner":"claude"});
                self.execution_event(
                    e,
                    "execution.runtime.changed",
                    json!({"runtime":"requires_action","action_id":action_id,"owner":"claude"}),
                    None,
                );
            }

            // Containment is the harness's permission rules only, so a target
            // outside the fixture is an observed effect with unresolved
            // liability, not a refusal. ADR 004 §5.
            "tool_uses" => {
                let record = &event["record"];
                e[&ns]["tool_uses"] = record.clone();
                e["view"]["containment"] = record["containment"].clone();
                if record["out_of_fixture_effect_observed"] == true
                    || record["unclassifiable_target_count"].as_u64().unwrap_or(0) > 0
                {
                    e["view"]["effects_liability"] = "unresolved".into();
                }
            }

            "turn_completed" => {
                e[&ns]["turn_status"] = if event["is_error"] == true {
                    "failed"
                } else {
                    "completed"
                }
                .into();
                e[&ns]["terminal_reason"] = event["terminal_reason"].clone();
                e[&ns]["permission_denials"] = event["permission_denials"].clone();
                // Usage lands once, at the end of the turn. Whether a running
                // turn reports it incrementally is unmeasured, so nothing here
                // claims a mid-turn figure.
                if let Some(usage) = event["usage"].as_object() {
                    let total: u64 = ["input_tokens", "output_tokens"]
                        .iter()
                        .filter_map(|k| usage.get(*k).and_then(Value::as_u64))
                        .sum();
                    let invocation = e[&ns]["invocation_id"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("{id}.invocation-1"));
                    let observation = json!({"invocation_id":invocation,"basis":"observed",
                        "measure":"claude.tokens.total","amount":total,"recorded_at":self.now});
                    e["view"]["usage"]["observations"] = json!([observation.clone()]);
                    e["view"]["usage"]["liability"] = "resolved".into();
                    self.execution_event(e, "execution.usage.observed", observation, None);
                }
                if let Some(cancel) = e["pending_cancel"].as_str().map(str::to_owned)
                    && e["view"]["cancellation"].get("outcome").is_none()
                {
                    // A turn that finished after SIGINT may have finished
                    // anyway; only the harness's own reason can say.
                    let outcome = if text(&event["terminal_reason"]) == "interrupted" {
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

            // A signal may end the turn before `result`, which is the only
            // message that reports usage. Usage is then unknown, never zero.
            "child_exited_without_result" => {
                e[&ns]["result_missing"] = true.into();
                e["view"]["usage"]["liability"] = "unresolved".into();
            }

            "child_exited" => {
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

            "host_error" => e[&ns]["host_error"] = event["error"].clone(),
            _ => {}
        }
        Ok(())
    }
}
