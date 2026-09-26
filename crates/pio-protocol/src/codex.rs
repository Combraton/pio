//! Codex app-server adapter on the Protocol provider (ADR 003). The durable
//! host owns native I/O; this module turns its fsync'd events into Protocol
//! facts and delivers committed controls to it.
use crate::provider::*;
use anyhow::{Context, Result};
use pio_host::harness::{append_control, events_path, read_jsonl};
use serde_json::{Value, json};

pub const CONTENT_EXTENSION: &str = "pio.combraton.dev/content";
/// An MCP server attached to **this** run's harness session, and no other's.
/// The lead tool for M4b: the owner starts a lead with it, and the runs the
/// lead starts get none. Owner authority only — a submit under any grant that
/// carries it is refused before admission (`grants.rs`), because a server spec
/// is a command the host's child will launch.
pub const LEAD_TOOL_EXTENSION: &str = "pio.combraton.dev/lead-tool";
/// The requests a run's host declined by itself, on `execution.exit.observed`.
pub const NATIVE_DECLINES: &str = "pio.combraton.dev/native-declines";
/// The threads a run's harness session reported on that the run did not
/// start (a sub-agent it spawned, or any other), with how each appeared and
/// its own usage, on `execution.exit.observed` (review of L3, round 3,
/// SPEND-2).
pub const OTHER_THREADS: &str = "pio.combraton.dev/other-threads";
/// The turns Codex started by itself on a run's own thread after the run's
/// turn had ended (a goal's continuation), each interrupted by the host, on
/// `execution.exit.observed` (review of L3, round 4, SPEND-9).
pub const CONTINUATIONS: &str = "pio.combraton.dev/continuations";
/// Why a run whose brief was delivered was refused and stopped (a Claude
/// stream drift, D11, or a permission-mode mismatch), with its usage
/// unknown, on `execution.exit.observed`.
pub const REFUSAL: &str = "pio.combraton.dev/refusal";
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
/// [`FEATURES`] without `execution.steering` (D6): the host does not
/// implement steering for an adapter whose [`Profile::steering_supported`]
/// is false, so it is never advertised there. A caller that still asks for
/// it in `core.negotiate` is refused `unknown_feature`/`unsupported_
/// required_feature`, and `execution.steer` itself is refused
/// `unsupported_required_feature` rather than a false "not running yet".
pub const FEATURES_WITHOUT_STEERING: &[&str] = &[
    "execution.controller",
    "execution.output",
    "execution.discovery",
    "execution.workspaces",
    "execution.usage",
    "execution.actions",
];

pub(crate) fn push(v: &mut Value, x: Value) {
    if !v.is_array() {
        *v = json!([]);
    }
    v.as_array_mut().unwrap().push(x);
}

/// Why a lead-tool spec for the Codex host is refused, if it is.
///
/// The same stdio shape and the same refusals as OpenCode's, plus one field
/// only this host can honour: `pre_allowed_tools`, the names of the lead
/// server's own tools whose approval mode the lead's thread config sets to
/// `approve`, so Codex does not ask before calling them (owner decision,
/// 2026-09-25, for L3's lead: its two tools, passed per launch, never written
/// to the owner's configuration). Every other request still comes to the
/// caller. OpenCode has no such setting, so its validator refuses the field.
///
/// And three settings of the server's own table on Codex, each sent as it
/// is (`pio_host::codex::LEAD_TOOL_SETTINGS`; rust-v0.157.0,
/// `config/src/mcp_types.rs:229-256`): `omit_tools_from`, the surfaces the
/// server's tools are kept off, only `code_mode` and `deferred` (omitting
/// `direct` would hide PIO's own tool from the model); `required`, a
/// boolean; and `startup_timeout_sec`, whole seconds from 1 to 120. L3's
/// lead sends `["code_mode","deferred"]`, `true` and `30`, so that
/// `gpt-5.6-terra`, which runs code-mode-only, sees its two tools in its own
/// tool list (review of L3's first live run, 2026-09-26).
pub(crate) fn codex_lead_tool_refusals(tool: &Value) -> Vec<Value> {
    let mut common = tool.clone();
    let allowed = common
        .as_object_mut()
        .and_then(|o| o.remove("pre_allowed_tools"));
    let settings: Vec<(&str, Value)> = ["omit_tools_from", "required", "startup_timeout_sec"]
        .into_iter()
        .filter_map(|name| {
            common
                .as_object_mut()
                .and_then(|o| o.remove(name))
                .map(|value| (name, value))
        })
        .collect();
    let mut refusals = pio_opencode::lead_tool_refusals(&common);
    for (name, value) in settings {
        let valid = match name {
            "omit_tools_from" => value.as_array().is_some_and(|surfaces| {
                let unique: std::collections::BTreeSet<_> =
                    surfaces.iter().filter_map(Value::as_str).collect();
                !surfaces.is_empty()
                    && unique.len() == surfaces.len()
                    && unique.iter().all(|s| ["code_mode", "deferred"].contains(s))
            }),
            "required" => value.is_boolean(),
            _ => value
                .as_u64()
                .is_some_and(|seconds| (1..=120).contains(&seconds)),
        };
        if !valid {
            refusals.push(json!({"reason":format!("lead_tool_{name}_not_accepted"),
                                 "detail":value}));
        }
    }
    if let Some(allowed) = allowed {
        let names = allowed.as_array().filter(|names| {
            !names.is_empty()
                && names.len() <= 8
                && names.iter().all(|n| {
                    n.as_str().is_some_and(|n| {
                        !n.is_empty()
                            && n.chars()
                                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                    })
                })
        });
        if names.is_none() {
            refusals.push(
                json!({"reason":"lead_tool_pre_allowed_tools_must_be_tool_names",
                                 "detail":allowed}),
            );
        }
    }
    refusals
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

/// Why a steer to a Claude Code run is refused, in the words a caller sees.
///
/// Decided for v0.1 (the orchestrator, 2026-09-26, G4): PIO does not steer a
/// Claude Code run mid-turn. The harness would accept a second message on
/// stdin, but whether it steers the running turn, queues it for the next or
/// drops it is not measured (ADR 004 §10), and the Claude host runs one turn,
/// so a queued message would never be run and must not be shown as
/// delivered. The steer is refused with this reason instead of the generic
/// one, which wrongly implied a running, acknowledged Claude turn would do.
pub const CLAUDE_STEER_REFUSAL: &str = "not_supported_mid_turn: PIO v0.1 does not steer a Claude Code run; \
     a message to a running run is neither delivered nor queued. Cancel the run and submit a new one.";

/// What differs between harnesses, in one place.
///
/// Both hosts emit the same normalized events; only these strings differ, so
/// the provider dispatches on one adapter value and reads the rest from here
/// rather than carrying a branch per harness. Codex's event names are the ones
/// its committed M2 receipts already use and are not renamed.
pub struct Profile {
    pub adapter: &'static str,
    pub fake_source: &'static str,
    pub real_source: &'static str,
    /// Evidence class for the delivery proof this harness gives, and the
    /// field on `turn_acknowledged` that must hold for the proof to stand.
    pub delivery_evidence: &'static str,
    pub ack_proof_field: &'static str,
    pub usage_measure: &'static str,
    /// Whether a `usage` report from this harness prices the whole turn, or
    /// only the model step it arrived with. Codex's app-server and Claude's
    /// `result` total the turn; OpenCode's `session/prompt` result does not
    /// (D4: measured on `L1b`, an 11,514-token last step against a turn the
    /// store's steps put at 96,495). PIO cannot see the rest of a multi-step
    /// turn's spend from the protocol, so a partial report is recorded as
    /// such rather than resolved.
    pub usage_covers_whole_turn: bool,
    /// Decisions PIO may forward. Anything else widens a permission.
    pub decisions: &'static [&'static str],
    /// How cancel is actually performed, in the words the receipt uses.
    pub cancel_description: &'static str,
    /// Event names that differ between the two hosts.
    pub session_event: &'static str,
    pub session_key: &'static str,
    pub guard_event: &'static str,
    pub guard_key: &'static str,
    pub exited_event: &'static str,
    /// `execution.discovery.list`'s own record of this harness: a stable
    /// installation id, the pinned version this build was qualified
    /// against, and the label a report gives the real harness versus a
    /// labeled fake standing in for it (D1: discovery must never report a
    /// different adapter's harness).
    pub installation_id: &'static str,
    pub pinned_version: &'static str,
    pub real_harness_name: &'static str,
    pub fake_harness_name: &'static str,
    /// Whether this host implements `execution.steer` at all. Only Codex
    /// does; advertising the feature elsewhere and refusing the call with a
    /// "not running yet" reason would be a false one (D6).
    pub steering_supported: bool,
}

pub const PROFILES: &[Profile] = &[
    Profile {
        adapter: "codex",
        fake_source: "pio-fake-app-server",
        real_source: "codex-app-server",
        delivery_evidence: "native_turn_acknowledged",
        ack_proof_field: "turn_id",
        usage_measure: "codex.tokens.total",
        usage_covers_whole_turn: true,
        decisions: &["accept", "decline", "cancel"],
        cancel_description: "turn/interrupt, in band",
        session_event: "thread_started",
        session_key: "thread",
        guard_event: "thread_settings_guard",
        guard_key: "thread_settings",
        exited_event: "app_server_exited",
        installation_id: "codex-selected",
        pinned_version: pio_codex::PINNED_VERSION,
        real_harness_name: "Codex CLI app-server",
        fake_harness_name: "PIO labeled fake Codex app-server (not Codex)",
        steering_supported: true,
    },
    Profile {
        adapter: "claude",
        fake_source: "pio-fake-claude-cli",
        real_source: "claude-code",
        // The replay echo: the exact message PIO sent, returned by the harness.
        // Evidence of receipt, but no identifier the harness returned, so no
        // proof class (D7; ADR 005 §7), the same as OpenCode's.
        delivery_evidence: "native_replay_echo",
        ack_proof_field: "replay_matches_sent",
        usage_measure: "claude.tokens.total",
        usage_covers_whole_turn: true,
        decisions: &["allow", "deny"],
        // Measured: the in-band interrupt is unverified against 2.1.278.
        cancel_description: "SIGINT, escalating to SIGKILL, not in band",
        session_event: "session_started",
        session_key: "session",
        guard_event: "settings_guard",
        guard_key: "permission_mode",
        exited_event: "harness_exited",
        installation_id: "claude-selected",
        pinned_version: pio_claude::PINNED_VERSION,
        real_harness_name: "Claude Code CLI",
        fake_harness_name: "PIO labeled fake Claude Code CLI (not Claude Code)",
        steering_supported: false,
    },
    Profile {
        adapter: "opencode",
        fake_source: "pio-fake-opencode-acp",
        real_source: "opencode-acp",
        // Measured: this harness acknowledges nothing. The first session
        // update shows it acting on the prompt, which is evidence of receipt
        // but not an identifier it returned, so there is no proof class.
        // ADR 005 §7.
        delivery_evidence: "native_session_update",
        ack_proof_field: "first_session_update",
        usage_measure: "opencode.tokens.total",
        usage_covers_whole_turn: false,
        decisions: &["allow", "deny"],
        cancel_description: "session/cancel, in band, escalating to SIGKILL",
        session_event: "session_started",
        session_key: "session",
        guard_event: "settings_guard",
        guard_key: "session_configuration",
        exited_event: "harness_exited",
        installation_id: "opencode-selected",
        pinned_version: pio_opencode::PINNED_VERSION,
        real_harness_name: "OpenCode ACP agent",
        fake_harness_name: "PIO labeled fake OpenCode ACP agent (not OpenCode)",
        steering_supported: false,
    },
];

pub fn profile(adapter: &str) -> Option<&'static Profile> {
    PROFILES.iter().find(|p| p.adapter == adapter)
}

/// The proof class an acknowledgment earns. Only an identifier the harness
/// returned is a `provider_ack_id` (EXECUTION §3.1, ADR 005 §7): Codex's turn
/// id. Claude Code's replay echo and OpenCode's first session update are
/// evidence of receipt, recorded by their evidence class, and carry none.
/// Claude's used to get one (D7).
pub fn delivery_proof(profile: &Profile, acknowledgment: &Value) -> Option<&'static str> {
    acknowledgment[profile.ack_proof_field]
        .is_string()
        .then_some("provider_ack_id")
}

impl Provider {
    pub(crate) fn adapter(&self) -> &str {
        self.host_config["adapter"].as_str().unwrap_or_default()
    }

    /// True for any qualified harness adapter, false for the fake host and the
    /// conformance service.
    pub(crate) fn native(&self) -> bool {
        profile(self.adapter()).is_some()
    }

    /// What differs for this harness. Only called on a native adapter.
    pub(crate) fn profile(&self) -> &'static Profile {
        profile(self.adapter()).expect("native adapter")
    }

    /// The label every record from this adapter carries. A labeled fake is
    /// never reported as the real harness.
    pub(crate) fn native_source(&self) -> &'static str {
        let profile = self.profile();
        if self.host_config["labeled_fake"] == true {
            profile.fake_source
        } else {
            profile.real_source
        }
    }

    pub(crate) fn codex_source(&self) -> &'static str {
        self.native_source()
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
        // The journal namespace is the adapter; for Codex this is `codex`,
        // so no persisted field name changes.
        let ns = self.adapter().to_owned();
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
        let lead_tool = p["extensions"].get(LEAD_TOOL_EXTENSION);
        if let Some(tool) = lead_tool {
            // The OpenCode host sends `mcpServers` on the run's own session,
            // and the Codex host puts the server in the run's own
            // `thread/start` config. Anywhere else the tool would be silently
            // dropped, and a lead that has no tool cannot do its job.
            if !matches!(ns.as_str(), "opencode" | "codex") {
                e["view"]["alternative"] =
                    "The lead tool is attached only through serve-opencode or serve-codex".into();
                return Some("capability_unavailable");
            }
            let refusals = if ns == "codex" {
                codex_lead_tool_refusals(tool)
            } else {
                pio_opencode::lead_tool_refusals(tool)
            };
            if !refusals.is_empty() {
                e["view"]["alternative"] = format!("Lead tool refused: {}", json!(refusals)).into();
                return Some("capability_unavailable");
            }
        }
        let host = &self.host_config;
        // The checks above are the same for every harness; only what the host
        // needs to launch differs.
        let mut spec = json!({
            "adapter":&ns,
            "labeled_fake":host["labeled_fake"] == true,
            "executable":host["executable"],
            "env":host["env"],
            "qualification":host["qualification_binding"],
            "cwd":repository,
            "brief":p["payload"]["brief"],
            // How long a surfaced permission request may wait for the caller
            // who asked for it. Theirs to set, not the host's.
            "action_answer_timeout_seconds":p["payload"]["timeouts"]["delivery"],
        });
        match ns.as_str() {
            "claude" => {
                spec["config_dir"] = host["config_dir"].clone();
                spec["home"] = host["home"].clone();
                spec["permission_mode"] = host["permission_mode"].clone();
                spec["model"] = host["model"].clone();
            }
            "opencode" => {
                spec["config_dir"] = host["config_dir"].clone();
                spec["home"] = host["home"].clone();
                // Every run passes an explicit model, under the dated
                // exception the admission already checked.
                spec["model"] = host["model"].clone();
                // The narrower session posture, when one was asked for. Only
                // narrowing values reach here: admission refuses the rest.
                spec["mode"] = host["mode"].clone();
                // The lead tool, for this run's session alone. Validated
                // above; journaled, which is why it may carry no secret.
                if let Some(tool) = lead_tool {
                    spec["lead_tool"] = tool.clone();
                }
            }
            _ => {
                spec["codex_home"] = host["codex_home"].clone();
                spec["thread"] = host["thread"].clone();
                // What Codex must answer for the thread's model and provider
                // before its first turn, when the operator names it.
                spec["expected_model_provider"] = host["expected_model_provider"].clone();
                // Codex's unmetered features off on every thread, under the
                // owner's recorded decision the service configuration was
                // admitted with (review of L3, rounds 3 and 4); absent
                // otherwise.
                if host["features_off_decision"].is_string() {
                    spec["features_off_decision"] = host["features_off_decision"].clone();
                }
                // The owner's plugins, apps and named MCP servers off on
                // every thread, under the owner's recorded decision of
                // 2026-09-26 (Q7); absent otherwise.
                if host["plugins_off_decision"].is_string() {
                    spec["plugins_off_decision"] = host["plugins_off_decision"].clone();
                    spec["mcp_servers_off"] = host["mcp_servers_off"].clone();
                }
                // The lead tool, for this run's thread alone, validated above.
                if let Some(tool) = lead_tool {
                    spec["lead_tool"] = tool.clone();
                }
                // Codex alone uses this, to label which projects it recorded
                // trust for. It is never a containment boundary: the boundary
                // is the workspace repository, which `cwd` already carries.
                spec["fixture_root"] = host["fixture_root"].clone();
            }
        }
        e[format!("{ns}_spec")] = spec;
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
        // The journal namespace is the adapter; for Codex this is `codex`,
        // so no persisted field name changes.
        let ns = self.adapter().to_owned();
        let id = text(&e["view"]["execution"]["id"]).to_owned();
        match method {
            "execution.steer" => {
                let under = self.under_grant(p);
                let n = num(&e["steer_count"]) + 1;
                e["steer_count"] = n.into();
                let steer_id = format!("{id}.steer-{n}");
                let delivery = format!("{id}.steering-{n}");
                let message = p["payload"]["message"].clone();
                let live = e[&ns]["turn_id"].is_string() && e["view"]["runtime"] != "exited";
                if self.content_available(&message).is_none() {
                    return Err(invalid(CONTENT_PATH));
                }
                // G4: never a Claude Code run, running or not.
                let refusal = if ns == "claude" {
                    Some(CLAUDE_STEER_REFUSAL)
                } else if !live {
                    Some("Steering needs an acknowledged, running native turn")
                } else {
                    None
                };
                if let Some(alternative) = refusal {
                    push(
                        &mut e["view"]["steering"],
                        json!({"steer_id":steer_id,"request":"not_supported","recorded_at":self.now,"alternative":alternative}),
                    );
                    let mut payload = json!({"steer_id":steer_id,"request":"not_supported"});
                    if let Some(under) = &under {
                        payload["pio.combraton.dev/under-grant"] = under.clone();
                    }
                    self.execution_event(e, "execution.steer.requested", payload, Some((p, op)));
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
                    &mut e[format!("{ns}_controls")],
                    json!({"id":delivery,"kind":"steer","digest":message["digest"],"appended":false}),
                );
                self.execution_event(
                    e,
                    "execution.steer.requested",
                    {
                        let mut payload = json!({"steer_id":steer_id,"request":"recorded"});
                        if let Some(under) = &under {
                            payload["pio.combraton.dev/under-grant"] = under.clone();
                        }
                        payload
                    },
                    Some((p, op)),
                );
                Ok(json!({"steer_id":steer_id,"request":"recorded","delivery_id":delivery}))
            }
            "execution.respond_action" => {
                let action_id = text(&p["payload"]["action_id"]).to_owned();
                // An answer past the deadline finds the lapse decided first.
                self.lapse_overdue(e);
                let pending = list(&e["view"]["actions"])
                    .iter()
                    .any(|a| a["action_id"] == action_id && a["state"] == "pending");
                if !pending {
                    // Still `not_found`: no action by that id is pending. Where
                    // one was and has been decided, say so and by whom, so an
                    // answer that lost to PIO's lapse is refused as already
                    // decided rather than told `answered` (CH-7).
                    let decided = &e[format!("{ns}_actions")][&action_id]["decided_by"];
                    let details = if decided.is_string() {
                        json!({"reason":"already_decided","decided_by":decided})
                    } else {
                        json!({})
                    };
                    return Err(err("not_found", details));
                }
                // And no answer is taken for a run whose host is no longer
                // running it: nothing would ever send it.
                let runtime = text(&e["view"]["runtime"]);
                if matches!(runtime, "exited" | "unknown") {
                    return Err(err(
                        "not_found",
                        json!({"reason":"run_not_running","runtime":runtime}),
                    ));
                }
                let response = p["payload"]["response"].clone();
                let decision = self
                    .content_available(&response)
                    .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                    .and_then(|v| v["decision"].as_str().map(str::to_owned));
                // The decision vocabulary is this harness's, from the table.
                // Anything outside it widens a permission and is refused here.
                let Some(decision) = decision.filter(|d| {
                    response["media_type"] == "application/json"
                        && self.profile().decisions.contains(&d.as_str())
                }) else {
                    return Err(invalid("/payload/response"));
                };
                let seq = e[format!("{ns}_actions")][&action_id]["seq"].clone();
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
                // One decision per action, and this is it: the caller's, on
                // the stream now. A lapse is decided only for an action still
                // pending, so none follows (CH-7).
                let record = &mut e[format!("{ns}_actions")][&action_id];
                record["decided_by"] = "caller".into();
                record["on_stream"] = true.into();
                if e["view"]["runtime_detail"]["action_id"] == action_id {
                    e["view"].as_object_mut().unwrap().remove("runtime_detail");
                    e["view"]["runtime"] = "active".into();
                }
                push(
                    &mut e[format!("{ns}_controls")],
                    json!({"id":effect,"kind":"respond_action","action_seq":seq,"decision":decision,"appended":false}),
                );
                self.execution_event(
                    e,
                    "execution.action.answered",
                    json!({"action_id":action_id,"response_effect":effect,
                           "pio.combraton.dev/decision":{
                               "decided_by":"caller","decision":decision}}),
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

    /// Who presented a grant with this command, when one was presented.
    ///
    /// **PIO's record, not the Protocol's.** `steering_entry` is closed, so
    /// nothing can go in the view, and the event record has no authorship
    /// field at all: on the stream today a steer from a lead and a steer
    /// from the owner are indistinguishable. This says who held the grant,
    /// which is the most PIO can say without inventing a Protocol field —
    /// and it says it in the payload, under a namespaced key, so a client
    /// that ignores it sees exactly what it saw before. The proposal that
    /// the Protocol carry authorship is filed separately; nothing here
    /// widens anything locally.
    pub(crate) fn under_grant(&self, p: &Value) -> Option<Value> {
        let id = p.get("grant")?.as_str()?;
        Some(json!({"grant":id,
                    "holder":self.grant(id).map(|g| g["holder"].clone()),
                    "recorded_by":"pio"}))
    }
    /// CH-7: PIO's lapse is decided here, in the one place every answer to
    /// a Codex approval is decided, so an action gets exactly one decision.
    ///
    /// Before this the host lapsed an overdue request itself, in the same
    /// pass that read answers, but after the sweep. A caller's answer the
    /// service had already accepted (and told the caller `answered`) could
    /// reach the host after it had declined, and the stream then carried two
    /// `execution.action.answered` for one action. Now an action still
    /// `pending` in this view past its deadline is marked decided by PIO and a
    /// decline is queued for the host like any answer. An answer that arrives
    /// after that is refused as already decided (`not_found`, reason
    /// `already_decided`), and a lapse is never decided for an action a
    /// caller has answered. PIO's decision reaches the stream when the host
    /// says it sent it (`request_denied_by_default`), so a lapse the host
    /// could no longer send (Codex settled the request, or the turn ended)
    /// is never shown as sent.
    ///
    /// The deadline is the one the host advertised, counted from when this
    /// view recorded the request, and only strictly after it: never before
    /// the advertised deadline, at most a second and a tick after it. Codex
    /// only; the Claude and OpenCode hosts still lapse by themselves.
    pub(crate) fn lapse_overdue(&mut self, e: &mut Value) {
        // Nothing is decided for a run whose host no longer runs it: no
        // lapse could be sent, and none is shown decided.
        if self.adapter() != "codex" || matches!(text(&e["view"]["runtime"]), "exited" | "unknown")
        {
            return;
        }
        let ns = self.adapter().to_owned();
        let records = format!("{ns}_actions");
        let due: Vec<(String, Value)> = list(&e["view"]["actions"])
            .iter()
            .filter(|a| a["state"] == "pending")
            .filter_map(|a| {
                let action_id = text(&a["action_id"]);
                let record = &e[&records][action_id];
                let seconds = record["deadline_seconds"].as_u64()?;
                let start = text(&a["requested_at"]);
                (!start.is_empty() && self.now > crate::execution::after(start, seconds))
                    .then(|| (action_id.to_owned(), record["seq"].clone()))
            })
            .collect();
        for (action_id, seq) in due {
            let control = format!("{action_id}.lapse");
            let record = &mut e[&records][&action_id];
            record["decided_by"] = "pio".into();
            record["lapse_control"] = control.clone().into();
            for action in e["view"]["actions"].as_array_mut().unwrap() {
                if action["action_id"] == action_id.as_str() {
                    action["state"] = "answered".into();
                    action["answered_at"] = self.now.clone().into();
                }
            }
            if e["view"]["runtime_detail"]["action_id"] == action_id.as_str() {
                e["view"].as_object_mut().unwrap().remove("runtime_detail");
                e["view"]["runtime"] = "active".into();
            }
            push(
                &mut e[format!("{ns}_controls")],
                json!({"id":control,"kind":"respond_action","action_seq":seq,
                       "decision":"decline","decided_by":"pio","appended":false}),
            );
        }
    }

    /// A request settled without any answer from PIO reaching the harness:
    /// Codex settled it itself (CH-8), or the turn ended with it open. The
    /// action gets its one decision here, with who settled it and why, and
    /// with no decision claimed sent. An answer a caller gave that never
    /// reached the harness is not reported again: its decision is already
    /// on the stream, so its response effect says it was not sent. A lapse
    /// PIO had decided and not yet sent is superseded, and says so.
    pub(crate) fn settle_unanswered(
        &mut self,
        e: &mut Value,
        action_id: &str,
        decided_by: &str,
        basis: &str,
        reason: &str,
    ) {
        let ns = self.adapter().to_owned();
        let records = format!("{ns}_actions");
        if !e[&records][action_id].is_object() {
            return;
        }
        if e[&records][action_id]["on_stream"] == true {
            let effect = list(&e["view"]["actions"])
                .iter()
                .find(|a| a["action_id"] == action_id)
                .and_then(|a| a["response_effect"].as_str().map(str::to_owned));
            if let Some(effect) = effect
                && self
                    .data
                    .effects
                    .get(&effect)
                    .is_some_and(|r| r["status"] == "pending")
            {
                self.codex_observe_effect(&effect, "failed", &format!("{basis}_before_sent"), true);
            }
            e[&records][action_id]["not_sent"] = basis.into();
            return;
        }
        let lapse_superseded = e[&records][action_id]["decided_by"] == "pio";
        let record = &mut e[&records][action_id];
        record["decided_by"] = decided_by.into();
        record["on_stream"] = true.into();
        let mut settled = false;
        if let Some(actions) = e["view"]["actions"].as_array_mut() {
            for action in actions {
                if action["action_id"] == action_id {
                    if action["state"] == "pending" {
                        action["answered_at"] = self.now.clone().into();
                    }
                    action["state"] = "answered".into();
                    settled = true;
                }
            }
        }
        if !settled {
            return;
        }
        if e["view"]["runtime_detail"]["action_id"] == action_id {
            e["view"].as_object_mut().unwrap().remove("runtime_detail");
            e["view"]["runtime"] = "active".into();
        }
        let mut decision = json!({"decided_by":decided_by,"decision":Value::Null,
                                  "basis":basis,"reason":reason,"sent":false});
        if lapse_superseded {
            decision["lapse_decided_not_sent"] = true.into();
        }
        self.execution_event(
            e,
            "execution.action.answered",
            json!({"action_id":action_id,"pio.combraton.dev/decision":decision}),
            None,
        );
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

    pub(crate) fn codex_observe_effect(
        &mut self,
        id: &str,
        status: &str,
        class: &str,
        close: bool,
    ) {
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

    /// `execution.discovery.list` for whichever native adapter this service
    /// is (D1): every field comes from this adapter's own [`Profile`] and
    /// its own journal namespace, never Codex's by default, so `serve-claude`
    /// and `serve-opencode` report their own harness instead of Codex's.
    pub(crate) fn native_discovery(&self) -> Value {
        // The journal namespace is the adapter; for Codex this is `codex`,
        // so no persisted field name changes.
        let ns = self.adapter().to_owned();
        let profile = self.profile();
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
            .filter(|e| e[&ns]["account_observed_at"].is_string())
            .max_by(|a, b| {
                text(&a[&ns]["account_observed_at"]).cmp(text(&b[&ns]["account_observed_at"]))
            });
        let (authentication, reachable, verified) = match observed {
            Some(e) => (
                if e[&ns]["authentication_type"].is_string() {
                    "authenticated"
                } else {
                    "unauthenticated"
                },
                "yes",
                Some(e[&ns]["account_observed_at"].clone()),
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
        let harness = if fake {
            profile.fake_harness_name
        } else {
            profile.real_harness_name
        };
        let mut installation = json!({"installation_id":profile.installation_id,"harness":harness,"detected":detected,"adapter_recognized":recognized,"version_supported":if qualified {"yes"} else if fake {"no"} else {"unknown"},"authentication":authentication,"reachable":reachable});
        if qualified {
            installation["version"] = profile.pinned_version.into();
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
    /// Launch the harness host for this execution and fold everything it
    /// has observed since the last pass into the view. Adapter-agnostic apart
    /// from the event codec it dispatches to.
    pub(crate) fn run_native(&mut self, e: &mut Value) -> Result<()> {
        // The journal namespace is the adapter; for Codex this is `codex`,
        // so no persisted field name changes.
        let ns = self.adapter().to_owned();
        let id = text(&e["view"]["execution"]["id"]).to_owned();
        let command = pio_core::digest(id.as_bytes());
        let command = command.trim_start_matches("sha256:").to_owned();
        if e["recovery_no_admit"] == true
            || matches!(
                text(&e["view"]["delivery"]),
                "failed_before_delivery" | "not_delivered"
            ) && e[format!("{ns}_events_offset")].is_null()
        {
            return Ok(());
        }
        self.dispatch_marker(e)?;
        let host = self.durable.as_ref().context("durable host")?;
        if host.inspect(&command).is_err() {
            e["host_submission"] =
                host.submit_harness(&ns, &command, e[format!("{ns}_spec")].clone())?;
        }
        let observed = host.inspect(&command)?;
        let invocation = text(&observed["invocation"]["invocation_id"]).to_owned();
        e[&ns]["invocation_id"] = invocation.clone().into();
        let root = host.root.clone();
        let phase = text(&observed["invocation"]["phase"]).to_owned();
        e[&ns]["phase"] = phase.clone().into();

        // A lapse is decided before controls are appended, so it goes to the
        // host in this pass (CH-7).
        self.lapse_overdue(e);
        // Controls are appended only after the command that created them was
        // committed; a repeated append after a crash is ignored by the host.
        let mut controls_changed = false;
        if let Some(controls) = e[format!("{ns}_controls")].as_array_mut() {
            for control in controls.iter_mut() {
                if control["appended"] != true {
                    let mut record = control.clone();
                    record.as_object_mut().unwrap().remove("appended");
                    append_control(&root, &ns, &invocation, &record)?;
                    control["appended"] = true.into();
                    controls_changed = true;
                }
            }
        }
        if let Some(cancel) = e["pending_cancel"].as_str().map(str::to_owned)
            && e[format!("{ns}_cancel_appended")] != true
        {
            append_control(
                &root,
                &ns,
                &invocation,
                &json!({"id":cancel,"kind":"interrupt"}),
            )?;
            e[format!("{ns}_cancel_appended")] = true.into();
            controls_changed = true;
        }
        let _ = controls_changed;
        // Owner correction 3: the execution deadline stops work with a real
        // turn/interrupt whose response and turn outcome are recorded. The host
        // is never killed for it.
        if list(&e["timeouts_passed"]).contains(&json!("execution_deadline"))
            && e["view"]["runtime"] != "exited"
            && e[&ns]["deadline_stop"].is_null()
        {
            let control = format!("{id}.deadline-stop");
            append_control(
                &root,
                &ns,
                &invocation,
                &json!({"id":control,"kind":"interrupt"}),
            )?;
            e[&ns]["deadline_stop"] =
                json!({"control_id":control,"requested_at":self.now,"request":"appended_for_host"});
        }

        let (events, offset) = read_jsonl(
            &events_path(&root, &ns, &invocation),
            num(&e[format!("{ns}_events_offset")]),
        )?;
        e[format!("{ns}_events_offset")] = offset.into();
        for event in events {
            self.native_event(e, &id, &event)?;
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
            e[&ns]["refusal"] = reason;
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

    /// Fold one host event into the view.
    ///
    /// Both hosts emit the same normalized events; what differs is in
    /// [`Profile`]. Kinds a harness never emits simply never match.
    pub(crate) fn native_event(&mut self, e: &mut Value, id: &str, event: &Value) -> Result<()> {
        // The journal namespace is the adapter; for Codex this is `codex`,
        // so no persisted field name changes.
        let ns = self.adapter().to_owned();
        let profile = self.profile();
        match text(&event["kind"]) {
            "account" => {
                e[&ns]["authentication_type"] = event["authentication_type"].clone();
                e[&ns]["account_observed_at"] = self.now.clone().into();
            }
            "spawned" => {
                e[&ns]["native"] = event["native"].clone();
            }
            "config_before" => {
                e[&ns]["config_before_sha256"] = event["snapshot"]["raw_sha256"].clone();
            }
            "config_after" => {
                e[&ns]["config_diff"] = event["diff"].clone();
            }
            k if k == profile.session_event => {
                e[&ns][profile.session_key] = json!({"thread_id":event["thread_id"],"configured_model":event["configured_model"],"requested_model":event["requested_model"],"model":event["model"],"model_provider":event["model_provider"],"sandbox":event["sandbox"],"approval_policy":event["approval_policy"]});
            }
            "turn_acknowledged"
                if matches!(
                    &event[profile.ack_proof_field],
                    Value::String(_) | Value::Bool(true)
                ) =>
            {
                e[&ns]["turn_id"] = event["turn_id"].clone();
                if matches!(text(&e["view"]["delivery"]), "pending" | "ambiguous") {
                    let reconcile = e["view"]["delivery"] == "ambiguous";
                    // Only a harness that returned an identifier gets a
                    // proof class. ADR 005 §7.
                    let proof = delivery_proof(profile, event);
                    self.delivery_observed(
                        e,
                        "acknowledged",
                        profile.delivery_evidence,
                        proof,
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
                e[&ns]["turn_error"] = event["error"].clone();
            }
            "action_requested" => {
                let action_id = format!("{id}.action-{}", num(&event["action_seq"]));
                // The deadline is kept with the action: on Codex the service,
                // not the host, decides the lapse (CH-7, `lapse_overdue`).
                e[format!("{ns}_actions")][&action_id] = json!({"seq":event["action_seq"],"method":event["method"],"approval_kind":event["approval_kind"],"request_id":event["request_id"],"deadline_seconds":event["answer_deadline_seconds"]});
                push(
                    &mut e["view"]["actions"],
                    json!({"action_id":action_id,"owner":profile.adapter,"state":"pending","requested_at":self.now}),
                );
                e["view"]["runtime"] = "requires_action".into();
                e["view"]["runtime_detail"] =
                    json!({"action_id":action_id,"owner":profile.adapter});
                // G2. `action_entry` is closed, so the deadline, the option
                // list the harness offered, what PIO will send if nobody
                // answers, and the classification cannot go in the view.
                // They ride here, in a payload that is already open, under a
                // namespaced key of the Protocol's own `extension_key` shape.
                // A client that reads only `runtime`, `action_id` and
                // `owner` behaves exactly as it does today.
                self.execution_event(
                    e,
                    "execution.runtime.changed",
                    json!({"runtime":"requires_action","action_id":action_id,"owner":profile.adapter,
                           "pio.combraton.dev/approval":{
                               "action_id":action_id,
                               "requested_at":self.now,
                               "method":event["method"],
                               "approval_kind":event["approval_kind"],
                               // Null where a harness has no such notion,
                               // and the nulls mean something: the Codex
                               // host offers no option list. Its deadline
                               // is the caller's delivery timeout, after
                               // which it declines once (owner decision,
                               // 2026-09-25); before that it had none.
                               "answer_deadline_seconds":event["answer_deadline_seconds"],
                               "options":event["options"],
                               "if_nobody_answers":event["if_nobody_answers"],
                               "classification":event["classification"],
                               // What is being approved, in the harness's
                               // own words, so whoever decides can see it:
                               // the command a Codex command approval
                               // names, and for an MCP tool-call approval
                               // the server, Codex's question and what it
                               // offered to remember (which PIO never
                               // sends). Null where the harness has none.
                               "command":event["command"],
                               "server":event["server"],
                               "message":event["message"],
                               "persist_offered":event["persist_offered"],
                               // Why the harness says it asks, and, for a
                               // Codex command, whether it asks for network
                               // access. The placement is `classification`.
                               "reason":event["reason"],
                               "network_approval":event["network_approval"],
                               "grant_root_requested":event["grant_root_requested"]}}),
                    None,
                );
            }
            k if k == profile.guard_event => {
                e[&ns][profile.guard_key] = event["guard"].clone();
            }
            "control_sent" | "control_response" | "control_rejected"
                if e[&ns]["deadline_stop"]["control_id"] == event["control_id"] =>
            {
                let stop = &mut e[&ns]["deadline_stop"];
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
            // The turn ended with a request nobody's answer had reached.
            "request_expired_with_turn" => {
                let action_id = format!("{id}.action-{}", num(&event["action_seq"]));
                self.settle_unanswered(
                    e,
                    &action_id,
                    "nobody",
                    "expired_with_turn",
                    "the turn ended before any answer reached the harness; nothing was sent",
                );
            }
            // Codex settled a request no answer from PIO had reached (CH-8).
            "request_resolved" if event["settled_by"] == "harness" => {
                let action_id = format!("{id}.action-{}", num(&event["action_seq"]));
                self.settle_unanswered(
                    e,
                    &action_id,
                    "harness",
                    "settled_by_harness",
                    "the harness settled the request itself; PIO sent no answer",
                );
            }
            "request_resolved" => {
                // The app-server resolved a request we answered.
                let answered: Vec<String> = list(&e["view"]["actions"])
                    .iter()
                    .filter(|a| a["state"] == "answered" && a["response_effect"].is_string())
                    .filter(|a| {
                        e[format!("{ns}_actions")][text(&a["action_id"])]["request_id"]
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
                // An answer already recorded as not sent, with the reason,
                // is not failed a second time.
                let control = text(&event["control_id"]).to_owned();
                if self
                    .data
                    .effects
                    .get(&control)
                    .is_none_or(|r| r["status"] != "failed")
                {
                    self.codex_observe_effect(&control, "failed", "host_rejected_control", true);
                }
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
                // The run's total is the sum over its threads, where the host
                // says so; a host that knows one thread reports that one.
                if let Some(total) = event["run_total"]
                    .as_u64()
                    .or_else(|| event["total"]["totalTokens"].as_u64())
                {
                    let invocation = e[&ns]["invocation_id"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("{id}.invocation-1"));
                    // D4: a harness whose `usage` report prices only the
                    // model step it arrived with (OpenCode's `session/prompt`
                    // result, never the running `usage_update` total) cannot
                    // be recorded as an observed, resolved total — a
                    // multi-step turn's real cost is higher and PIO cannot
                    // see it from the protocol. The figure is kept (it is
                    // real, and a floor), but named for what it covers and
                    // left owing.
                    let (basis, measure, liability) = if profile.usage_covers_whole_turn {
                        ("observed", profile.usage_measure.to_owned(), "resolved")
                    } else {
                        (
                            "estimated",
                            format!("{}.last_step_only", profile.usage_measure),
                            "unresolved",
                        )
                    };
                    let observation = json!({"invocation_id":invocation,"basis":basis,"measure":measure,"amount":total,"recorded_at":self.now});
                    e["view"]["usage"]["observations"] = json!([observation.clone()]);
                    // Resolved only when the report covers the whole turn
                    // (not OpenCode's last step, D4) and its host says it is
                    // final (not a Codex per-step report, D5).
                    e["view"]["usage"]["liability"] =
                        if liability == "resolved" && event["final"] != false {
                            "resolved"
                        } else {
                            "unresolved"
                        }
                        .into();
                    self.execution_event(e, "execution.usage.observed", observation, None);
                }
            }
            // D5: the run's own turn completed and nothing was cut short, so
            // the last report stands as the run's usage.
            "usage_final" => {
                e[&ns]["usage_final"] = event["run_total"].clone();
                if e["view"]["usage"]["observations"]
                    .as_array()
                    .is_some_and(|a| !a.is_empty())
                {
                    e["view"]["usage"]["liability"] = "resolved".into();
                }
            }
            // The end of a turn Codex started by itself after the run's own
            // (review of L3, round 4, SPEND-9): not the run's turn, whose
            // status stands; carried with the exit's continuations instead.
            "turn_completed" if event["continuation"] == true => {
                if let Some(list) = e[&ns]["continuations"].as_array_mut()
                    && let Some(started) = list
                        .iter_mut()
                        .rev()
                        .find(|c| c["turn_id"] == event["turn_id"])
                {
                    started["status"] = event["status"].clone();
                }
            }
            "turn_completed" => {
                let status = text(&event["status"]).to_owned();
                e[&ns]["turn_status"] = status.clone().into();
                if e[&ns]["deadline_stop"].is_object()
                    && e[&ns]["deadline_stop"]["outcome"].is_null()
                {
                    e[&ns]["deadline_stop"]["outcome"] = status.clone().into();
                    e[&ns]["deadline_stop"]["observed_at"] = self.now.clone().into();
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
            k if k == profile.exited_event => {
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
                // G3. The end-of-turn audit: where each tool use landed and
                // who decided it. `tool_uses` is a host record folded just
                // before this event, and nothing in the view can hold it —
                // `containment` carries counts, not rows. It rides here,
                // under a namespaced key, on the event that already marks
                // the end of the turn. A client that reads only `exit`
                // behaves exactly as it does today.
                //
                // `harness_status` travels beside the audit on purpose: it
                // is the harness's own record of each call, so a reader can
                // see the two disagree rather than being handed one of them.
                let mut payload = json!({"exit":exit});
                if e[&ns]["tool_uses"].is_object() {
                    payload["pio.combraton.dev/tool-uses"] = json!({"audit":e[&ns]["tool_uses"],
                               "harness_status":e[&ns]["tool_use_harness_status"]});
                }
                // Every request the host declined by itself, never put to a
                // caller: what was asked and PIO's reason. Nothing in the
                // view can hold it (`action_entry` is closed, and no caller
                // was asked), so it rides here under a namespaced key, as the
                // audit does. Always present, so "none" and "not carried"
                // differ (review of L3, CH-2/F1).
                payload[NATIVE_DECLINES] = match &e[&ns]["native_declines"] {
                    Value::Array(list) => Value::Array(list.clone()),
                    _ => json!([]),
                };
                // Why a delivered run was stopped before it could finish, and
                // that its usage is unknown for that reason. Present only for
                // a refused run.
                if e[&ns]["refusal"].is_object() {
                    payload[REFUSAL] = json!({"refusal":e[&ns]["refusal"],
                        "delivered_before_refusal":true,
                        "usage":"unknown",
                        "usage_reason":"the harness was stopped before `result`, the only message that reports usage"});
                }
                // Every thread the run did not start, the same way (Codex
                // only, the one host that tells threads apart): the
                // host's final list, or what it had recorded as each thread
                // appeared, and none only when none appeared.
                if ns == "codex" {
                    payload[OTHER_THREADS] =
                        match (&e[&ns]["other_threads"], &e[&ns]["other_threads_seen"]) {
                            (Value::Array(list), _) | (_, Value::Array(list)) => {
                                Value::Array(list.clone())
                            }
                            _ => json!([]),
                        };
                    // And every turn Codex started by itself on the run's own
                    // thread after its turn had ended: none, or each one.
                    payload[CONTINUATIONS] = match &e[&ns]["continuations"] {
                        Value::Array(list) => Value::Array(list.clone()),
                        _ => json!([]),
                    };
                }
                self.execution_event(e, "execution.exit.observed", payload, None);
            }
            // A thread the run did not start, as the host first heard of it,
            // and the host's list at the end with each thread's usage.
            "other_thread" => {
                let mut record = event.clone();
                if let Some(fields) = record.as_object_mut() {
                    fields.remove("kind");
                }
                push(&mut e[&ns]["other_threads_seen"], record);
            }
            "other_threads" => {
                e[&ns]["other_threads"] = event["threads"].clone();
            }
            // A turn Codex started by itself on the run's own thread after
            // the run's turn had ended, which the host interrupted.
            "continuation_started" => {
                let mut record = event.clone();
                if let Some(fields) = record.as_object_mut() {
                    fields.remove("kind");
                }
                push(&mut e[&ns]["continuations"], record);
            }
            // A request the host answered with an error by itself: never a
            // caller's decision, and never on the stream until now.
            "native_request_declined" => {
                let mut record = event.clone();
                if let Some(fields) = record.as_object_mut() {
                    fields.remove("kind");
                }
                push(&mut e[&ns]["native_declines"], record);
            }
            // An acknowledgment whose proof did not hold is not a delivery.
            "turn_acknowledged" => e[&ns]["acknowledgment_unproven"] = true.into(),

            // PIO's own two decisions: a request it declined before any
            // caller was asked, and a request whose deadline lapsed. Neither
            // has a command behind it, so both reach the stream as a
            // **provider-origin** `execution.action.answered` — the event a
            // caller already watches for "this request is settled".
            //
            // Before this, the decline lived only in the adapter namespace
            // and the lapse reached the stream nowhere at all: `actions[]`
            // showed a settled request `pending` for ever, and a caller could
            // not tell it from one still waiting. Screen 7 promises "anything
            // PIO decided, marked as PIO's decision"; neither reached it.
            //
            // `origin` stays `provider`, with no `operation_ref` and no
            // `command_id`. The event schema's if/then refuses the mix, and
            // there is no command here to name.
            "request_declined_by_pio" | "request_denied_by_default" => {
                let declined = text(&event["kind"]) == "request_declined_by_pio";
                if declined {
                    push(
                        &mut e[&ns]["declined_by_pio"],
                        event["classification"].clone(),
                    );
                }
                let action_id = format!("{id}.action-{}", num(&event["action_seq"]));
                if !e[format!("{ns}_actions")][&action_id].is_object() {
                    // A decline is never put to a caller, so no
                    // `action_requested` precedes it. The walk still needs an
                    // entry to show the decision against.
                    e[format!("{ns}_actions")][&action_id] =
                        json!({"seq":event["action_seq"],"request_id":Value::Null});
                }
                let mut settled = false;
                // A run whose only request PIO declined has no `actions`
                // array at all — nothing was ever put to a caller. The
                // caller-answered path can assume one; this cannot.
                if let Some(actions) = e["view"]["actions"].as_array_mut() {
                    for action in actions {
                        if action["action_id"] == action_id {
                            action["state"] = "answered".into();
                            action["answered_at"] = self.now.clone().into();
                            settled = true;
                        }
                    }
                }
                if !settled {
                    push(
                        &mut e["view"]["actions"],
                        json!({"action_id":action_id,"owner":profile.adapter,
                               "state":"answered","requested_at":self.now,
                               "answered_at":self.now}),
                    );
                }
                if e["view"]["runtime_detail"]["action_id"] == action_id {
                    e["view"].as_object_mut().unwrap().remove("runtime_detail");
                    e["view"]["runtime"] = "active".into();
                }
                let record = &mut e[format!("{ns}_actions")][&action_id];
                record["decided_by"] = "pio".into();
                record["on_stream"] = true.into();
                let mut decision = json!({"decided_by":"pio",
                "decision":event["decision"],
                "basis":if declined { "out_of_scope" } else { "deadline_lapsed" },
                "reason":if declined {
                    event["classification"]["reason"].clone()
                } else {
                    json!("no caller answered within the delivery timeout")
                }});
                if declined {
                    decision["classification"] = event["classification"].clone();
                } else {
                    decision["after_seconds"] = event["after_seconds"].clone();
                }
                // Recorded only where the harness has the notion at all.
                for field in ["option_id", "option_kind", "always_option_taken"] {
                    if !event[field].is_null() {
                        decision[field] = event[field].clone();
                    }
                }
                self.execution_event(
                    e,
                    "execution.action.answered",
                    json!({"action_id":action_id,
                           "pio.combraton.dev/decision":decision}),
                    None,
                );
            }

            // Containment is the harness's permission rules only, so a target
            // outside the fixture is an observed effect with unresolved
            // liability, not a refusal.
            "tool_uses" => {
                let record = &event["record"];
                e[&ns]["tool_uses"] = record.clone();
                e[&ns]["tool_use_harness_status"] = event["harness_status"].clone();
                e["view"]["containment"] = record["containment"].clone();
                // Three different things a caller must be able to tell apart:
                // what the harness refused on its own, what PIO declined, and
                // what a caller decided. A merged count would hide which.
                let refused = record["denied_by_harness_count"].as_u64().unwrap_or(0);
                if refused > 0 {
                    e["view"]["containment"]["denied_by_harness"] = refused.into();
                    e["view"]["containment"]["denied_by_harness_reason"] =
                        "the harness refused it under its own rules; PIO was not asked".into();
                }
                // The record's own verdict: an out-of-fixture or unclassifiable
                // use, or one whose outcome no `result` settled (D3).
                if record["liability"] == "unresolved" {
                    e["view"]["effects_liability"] = "unresolved".into();
                }
            }

            // A Claude run refused after its brief was delivered: a drifted
            // stream (D11) or a permission mode other than the one asked for.
            // The child was stopped before `result`, the only message that
            // reports usage, so what the turn spent is unknown, never none;
            // the reason rides on the exit.
            "stream_identity_refused" | "permission_mode_mismatch_refused" => {
                e[&ns]["refusal"] = event["refusal"].clone();
                e["view"]["usage"]["liability"] = "unresolved".into();
            }
            // A turn that ended without the message that reports usage leaves
            // usage unknown, never zero.
            "result_missing" => {
                e[&ns]["result_missing"] = true.into();
                e["view"]["usage"]["liability"] = "unresolved".into();
            }

            // A harness that ignored the interrupt was killed. Say so, and say
            // what it cost.
            "interrupt_escalated" => {
                e[&ns]["interrupt_escalation"] = json!({
                    "control_id":event["control_id"],
                    "from":event["from"],"to":event["to"],
                    "waited_ms":event["waited_ms"],
                    "cancel_description":profile.cancel_description,
                    "usage":"unknown"});
                e["view"]["usage"]["liability"] = "unresolved".into();
            }

            "host_error" => {
                e[&ns]["host_error"] = event["error"].clone();
                // A host that failed **before releasing the brief** is
                // finished: the brief never left PIO and the child is
                // stopped. Without this the execution sat in `preparing` for
                // ever, so a caller polling the runtime could not tell a
                // refusal from a slow start. The first live OpenCode run sat
                // there until it was stopped by hand, with
                // `delivery: failed_before_delivery` already recorded beside
                // it. `exit` stays `unavailable`: no exit code was observed
                // and none is claimed.
                if event["released"] == false
                    && !matches!(text(&e["view"]["runtime"]), "exited" | "unknown")
                {
                    e["view"]["runtime"] = "exited".into();
                    e["view"].as_object_mut().unwrap().remove("runtime_detail");
                    // Such a run has no `execution.exit.observed`, so what
                    // its host declined by itself before the failure rides
                    // here, under the same key, or no caller would ever see
                    // it (review of L3, round 4, R4-HC-2). Always present.
                    let declined = match &e[&ns]["native_declines"] {
                        Value::Array(list) => Value::Array(list.clone()),
                        _ => json!([]),
                    };
                    self.execution_event(
                        e,
                        "execution.runtime.changed",
                        json!({"runtime":"exited","reason":"refused_before_delivery",
                               NATIVE_DECLINES:declined}),
                        None,
                    );
                }
            }
            _ => {}
        }
        Ok(())
    }
}
