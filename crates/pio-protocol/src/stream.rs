//! Unix stream/1 binding. No native adapter is loaded by this conformance service.
use crate::{
    encoding,
    provider::{Provider, Session, err},
};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::{
    io::Read,
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    net::{UnixListener, UnixStream},
};

/// Owner decision of 2026-09-19: PIO may pass an explicit model for the M2
/// Codex fixture runs, because the configured model cannot complete a turn at
/// the pinned version and is expensive. It is test-only and dated on purpose;
/// the product still never selects a model. See ADR 003.
pub const MODEL_EXCEPTION: &str = "owner-2026-09-19-m2-fixture-runs";

/// The owner's dated decisions that PIO may turn Codex's unmetered,
/// default-on features off per launch: sub-agents, memories, goals,
/// standalone web search and image generation, by the keys
/// `pio_codex::features_off` lists, in every thread's `thread/start` config
/// and never in the owner's files (review of L3, round 3, SPEND-2; round 4,
/// SPEND-8, SPEND-9, and the web-search and image-generation gaps). One
/// override set, one decision list; a real Codex refuses any decision not
/// recorded here.
///
/// **Owner decision, 2026-09-26** (answering "Q6: ... how should they be
/// turned off?", chose "Per-launch override"): "L3's Codex threads are
/// launched with these five off, per launch, under a dated test-only
/// exception like the lead-tool pre-allowance. Your ~/.codex/config.toml is
/// not changed." It covers exactly the five: sub-agents (the three keys),
/// memories, goals, standalone web search and image generation, as
/// `pio_codex::features_off` lists them. L3's runner sends it for L3's
/// threads only.
pub const FEATURES_OFF_DECISIONS: &[&str] = &["owner-2026-09-26-l3-codex-unmetered-features-off"];
/// The same setting for a labeled fake only, so the rehearsal and the
/// matrix exercise what would go on the wire. Refused beside a real Codex.
pub const FEATURES_OFF_REHEARSAL: &str = "rehearsal-only-features-off";

pub fn serve(root: &Path, config: &Path, socket: &Path) -> Result<()> {
    serve_mode(root, config, socket, Mode::Conformance)
}
pub fn serve_fake(root: &Path, config: &Path, socket: &Path) -> Result<()> {
    serve_mode(root, config, socket, Mode::FakeProcess)
}
pub fn serve_codex(root: &Path, config: &Path, socket: &Path) -> Result<()> {
    serve_mode(root, config, socket, Mode::Codex)
}
pub fn serve_claude(root: &Path, config: &Path, socket: &Path) -> Result<()> {
    serve_mode(root, config, socket, Mode::Claude)
}
pub fn serve_opencode(root: &Path, config: &Path, socket: &Path) -> Result<()> {
    serve_mode(root, config, socket, Mode::OpenCode)
}
#[derive(PartialEq)]
enum Mode {
    Conformance,
    FakeProcess,
    Codex,
    Claude,
    OpenCode,
}
/// Validate a `pio-codex-service/1` configuration and qualify the selected
/// executable before any native work. A labeled fake app-server skips
/// qualification and is reported as not Codex.
fn codex_host_config(root: &Path, codex: &Value) -> Result<Value> {
    let object = codex.as_object().context("codex settings object")?;
    for name in object.keys() {
        anyhow::ensure!(
            [
                "executable",
                "env",
                "codex_home",
                "fixture_root",
                "thread",
                "labeled_fake",
                "test_only_model_exception",
                "expected_model_provider",
                "features_off_decision"
            ]
            .contains(&name.as_str()),
            "unsupported codex setting: {name}"
        );
    }
    for name in ["executable", "codex_home", "fixture_root"] {
        anyhow::ensure!(
            codex[name]
                .as_str()
                .is_some_and(|p| Path::new(p).is_absolute()),
            "codex.{name} must be an absolute path"
        );
    }
    let env = codex["env"].as_object().context("codex.env object")?;
    anyhow::ensure!(
        env.values().all(Value::is_string) && env.contains_key("PATH"),
        "codex.env needs string values including PATH"
    );
    if let Some(thread) = codex["thread"].as_object() {
        for (name, value) in thread {
            anyhow::ensure!(
                ["sandbox", "approvalPolicy", "model"].contains(&name.as_str()),
                "unsupported codex.thread setting: {name}"
            );
            // Never select full access or disable approvals on the user's behalf.
            anyhow::ensure!(
                name == "model" || !matches!(value.as_str(), Some("danger-full-access" | "never")),
                "codex.thread.{name} value is not permitted: {value}"
            );
        }
    }
    // PIO never selects a model. `codex.thread.model` exists only for the
    // owner's dated, test-only exception for M2 fixture runs, so it is refused
    // unless the configuration names that decision, and the exception is
    // refused on its own so it cannot sit unused in a shipped configuration.
    let model = codex["thread"]["model"].as_str();
    let exception = codex["test_only_model_exception"].as_str();
    anyhow::ensure!(
        model.is_none_or(|m| !m.is_empty()),
        "codex.thread.model must be a non-empty model name"
    );
    anyhow::ensure!(
        model.is_some() == exception.is_some_and(|e| e == MODEL_EXCEPTION),
        "codex.thread.model requires test_only_model_exception = \"{MODEL_EXCEPTION}\" and that exception requires a model"
    );
    // The provider Codex must report for the thread before its first turn.
    // A check, never a selection: PIO sends no `modelProvider`. It exists
    // only beside a requested model, so both are checked together.
    if let Some(provider) = codex.get("expected_model_provider") {
        anyhow::ensure!(
            provider.as_str().is_some_and(|p| !p.is_empty()) && model.is_some(),
            "codex.expected_model_provider must be a non-empty string beside codex.thread.model"
        );
    }
    // Codex's unmetered features off per launch, only under a recorded owner
    // decision, or the rehearsal's own token beside a labeled fake.
    if let Some(decision) = codex.get("features_off_decision") {
        let decision = decision.as_str().unwrap_or_default();
        let rehearsal = codex["labeled_fake"] == true && decision == FEATURES_OFF_REHEARSAL;
        anyhow::ensure!(
            rehearsal || FEATURES_OFF_DECISIONS.contains(&decision),
            "codex.features_off_decision is not a recorded owner decision: {decision:?}"
        );
    }
    let mut host = codex.clone();
    host["adapter"] = "codex".into();
    if codex["labeled_fake"] == true {
        host["qualification_binding"] = Value::Null;
        return Ok(host);
    }
    let expected: Value = serde_json::from_str(pio_codex::QUALIFIED_SCHEMA_IDENTITY)?;
    let record = pio_codex::qualify(
        Path::new(codex["executable"].as_str().unwrap()),
        &expected,
        env["PATH"].as_str().map(std::ffi::OsStr::new),
        &root.join("qualification"),
    )?;
    std::fs::write(
        root.join("qualification.json"),
        serde_json::to_vec_pretty(&record)?,
    )?;
    anyhow::ensure!(
        record["qualified"] == true,
        "codex_not_qualified: {}",
        record["refusals"]
    );
    let resolution = &record["resolution"];
    // Owner correction 4: the Node the service PATH actually resolves, observed
    // by running `/usr/bin/env node` with exactly that PATH, must be the Node
    // that was qualified.
    let node_resolution = if resolution["kind"] == "npm_node_wrapper" {
        let output = std::process::Command::new("/usr/bin/env")
            .env_clear()
            .envs(
                env.iter()
                    .map(|(k, v)| (k.as_str(), v.as_str().unwrap_or_default())),
            )
            .args(["node", "-e", "process.stdout.write(process.execPath)"])
            .output()?;
        anyhow::ensure!(
            output.status.success(),
            "service PATH does not resolve node"
        );
        let observed = std::fs::canonicalize(String::from_utf8_lossy(&output.stdout).as_ref())?;
        let qualified = std::fs::canonicalize(
            resolution["node"]["path"]
                .as_str()
                .context("qualified node path")?,
        )?;
        let sha = pio_codex::sha256_file(&observed)?;
        let matches =
            observed == qualified && Some(sha.as_str()) == resolution["node"]["sha256"].as_str();
        let record = json!({"applicable":true,"service_path_exec_path":observed,"qualified_node_path":qualified,"sha256":sha,"version":resolution["node"]["version"],"matches_qualification":matches});
        anyhow::ensure!(matches, "service_node_mismatch: {record}");
        record
    } else {
        json!({"applicable":false,"reason":"selected executable is native"})
    };
    let mut record = record;
    record["service_node_resolution"] = node_resolution;
    std::fs::write(
        root.join("qualification.json"),
        serde_json::to_vec_pretty(&record)?,
    )?;
    let resolution = &record["resolution"];
    host["qualification_binding"] = json!({
        "node_path":resolution["node"]["path"],
        "service_node_resolution":record["service_node_resolution"],
        "native_path":resolution["native"]["path"],
        "native_sha256":resolution["native"]["sha256"],
        "wrapper_sha256":resolution["wrapper"]["sha256"],
        "node_sha256":resolution["node"]["sha256"],
        "canonical_listing_sha256":record["schema"]["canonical_listing_sha256"],
    });
    Ok(host)
}
/// Validate a `pio-claude-service/1` configuration and make every decision
/// that must precede a spawn: the settings allowlist, the environment, the
/// dated model exception, the permission mode against the user's own settings,
/// qualification, and the credential route.
///
/// The decisions themselves live in `pio_claude::service_admission`, which the
/// offline matrix already exercises; this records the admission record beside
/// the store and refuses to start when it refuses.
fn claude_host_config(root: &Path, claude: &Value) -> Result<Value> {
    let record = pio_claude::service_admission(&root.join("qualification"), claude)?;
    std::fs::write(
        root.join("claude-admission.json"),
        serde_json::to_vec_pretty(&record)?,
    )?;
    anyhow::ensure!(
        record["admitted"] == true,
        "claude_not_admitted: {}",
        record["refusals"]
    );
    let mut host = claude.clone();
    host["adapter"] = "claude".into();
    host["configured_model"] = record["permission_mode"]["configured_model"].clone();
    host["qualification_binding"] = if claude["labeled_fake"] == true {
        Value::Null
    } else {
        json!({"binary_sha256":record["qualification"]["resolution"]["binary_sha256"],
               "version":record["qualification"]["version"],
               "surface_listing_sha256":record["qualification"]["surface"]["surface_listing_sha256"]})
    };
    Ok(host)
}

/// Validate a `pio-opencode-service/1` configuration and make every decision
/// that must precede a session, including the owner's exclusion of a provider
/// and the dated model exception.
fn opencode_host_config(root: &Path, opencode: &Value) -> Result<Value> {
    let record = pio_opencode::service_admission(&root.join("qualification"), opencode)?;
    std::fs::write(
        root.join("opencode-admission.json"),
        serde_json::to_vec_pretty(&record)?,
    )?;
    anyhow::ensure!(
        record["admitted"] == true,
        "opencode_not_admitted: {}",
        record["refusals"]
    );
    let mut host = opencode.clone();
    host["adapter"] = "opencode".into();
    host["qualification_binding"] = if opencode["labeled_fake"] == true {
        Value::Null
    } else {
        json!({"binary_sha256":record["qualification"]["resolution"]["binary_sha256"],
               "version":record["qualification"]["version"]})
    };
    Ok(host)
}

fn serve_mode(root: &Path, config: &Path, socket: &Path, mode: Mode) -> Result<()> {
    let durable = mode != Mode::Conformance;
    if durable {
        pio_host::secure_root(root)?;
    }
    pio_host::secure_root(socket.parent().context("socket parent")?)?;
    let _lock = StoreLock::acquire(root)?;
    let config: Value = serde_json::from_slice(&std::fs::read(config)?)?;
    let provider = if mode == Mode::OpenCode {
        anyhow::ensure!(
            config["format"] == "pio-opencode-service/1",
            "invalid opencode service config"
        );
        let protocol = config["protocol"].clone();
        anyhow::ensure!(
            protocol["executor"]["scripts"].is_null()
                && protocol["executor"]["default_script"].is_null(),
            "executor.script is conformance-only"
        );
        let host = opencode_host_config(root, &config["opencode"])?;
        Provider::with_host(root, protocol, Some(host))?
    } else if mode == Mode::Claude {
        anyhow::ensure!(
            config["format"] == "pio-claude-service/1",
            "invalid claude service config"
        );
        let protocol = config["protocol"].clone();
        anyhow::ensure!(
            protocol["executor"]["scripts"].is_null()
                && protocol["executor"]["default_script"].is_null(),
            "executor.script is conformance-only"
        );
        let host = claude_host_config(root, &config["claude"])?;
        Provider::with_host(root, protocol, Some(host))?
    } else if mode == Mode::Codex {
        anyhow::ensure!(
            config["format"] == "pio-codex-service/1",
            "invalid codex service config"
        );
        let protocol = config["protocol"].clone();
        anyhow::ensure!(
            protocol["executor"]["scripts"].is_null()
                && protocol["executor"]["default_script"].is_null(),
            "executor.script is conformance-only"
        );
        let host = codex_host_config(root, &config["codex"])?;
        Provider::with_host(root, protocol, Some(host))?
    } else if durable {
        anyhow::ensure!(
            config["format"] == "pio-fake-service/1",
            "invalid fake service config"
        );
        let host = config
            .get("fake_host")
            .context("fake_host required")?
            .clone();
        for name in host.as_object().context("fake_host object")?.keys() {
            anyhow::ensure!(
                ["duration_ms", "fault"].contains(&name.as_str()),
                "unsupported fake host setting: {name}"
            );
        }
        anyhow::ensure!(
            [
                "",
                "after_intent",
                "after_claim",
                "after_release",
                "after_receipt",
                "duplicate_launch",
                "replay_relaunch",
                "replay_without_launch_guard",
                "replay_without_host_phase",
                "before_release",
                "after_dispatch_marker",
                "reorder_dispatch_intent"
            ]
            .contains(&host["fault"].as_str().unwrap_or("")),
            "unsupported host launch fault"
        );
        pio_core::require_payload(&json!({"duration_ms":host["duration_ms"]}))?;
        let protocol = config["protocol"].clone();
        anyhow::ensure!(
            protocol["executor"]["scripts"].is_null()
                && protocol["executor"]["default_script"].is_null(),
            "executor.script is conformance-only"
        );
        Provider::with_host(root, protocol, Some(host))?
    } else {
        Provider::new(root, config)?
    };
    if durable && socket.exists() {
        use std::os::unix::fs::FileTypeExt;
        anyhow::ensure!(
            std::fs::symlink_metadata(socket)?.file_type().is_socket(),
            "refusing non-socket"
        );
        std::fs::remove_file(socket)?;
    }
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        let listener = UnixListener::bind(socket)?;
        std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600))?;
        let provider = Arc::new(Mutex::new(provider));
        let (done_tx, mut done_rx) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            if durable {
                loop {
                    std::thread::park();
                }
            }
            let mut input = std::io::stdin().lock();
            let mut buffer = [0u8; 256];
            while matches!(input.read(&mut buffer), Ok(n) if n > 0) {}
            let _ = done_tx.send(());
        });
        let mut tick = tokio::time::interval(Duration::from_millis(25));
        // A slow tick (whole-state commits near the ADR 002 bound) must not
        // run back-to-back and monopolize the provider lock; delay instead.
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = &mut done_rx => break,
                _ = tick.tick(), if durable => {
                    let mut p = provider.lock().unwrap();
                    let _ = p.clock(false);
                    if let Err(error) = p.execution_tick() && !crate::provider::is_capacity_refusal(&error) { eprintln!("PIO host observation: {error:#}"); }
                },
                accepted = listener.accept() => {
                    let (stream, _) = accepted?;
                    let stream = stream.into_std()?;
                    if !pio_host::same_user(&stream) { continue; }
                    let stream = UnixStream::from_std(stream)?;
                    let provider = provider.clone();
                    tokio::spawn(async move { let _ = connection(stream, provider).await; });
                }
            }
        }
        Ok::<_, anyhow::Error>(())
    })?;
    std::fs::remove_file(socket)?;
    Ok(())
}

async fn connection(mut stream: UnixStream, provider: Arc<Mutex<Provider>>) -> Result<()> {
    let mut output = crate::output::Output::new(&provider.lock().unwrap().config);
    let mut session = Session {
        receive: 1048576,
        ..Session::default()
    };
    let mut frame = Vec::new();
    loop {
        output.flush_ready(&mut stream)?;
        let limit = if session.selected.is_some() {
            provider.lock().unwrap().limits["max_frame_bytes"]
                .as_u64()
                .unwrap() as usize
        } else {
            1048576
        };
        let mut buffer = [0u8; 8192];
        let available = buffer
            .len()
            .min(limit.saturating_sub(frame.len()).saturating_add(1));
        let count = tokio::select! {
            result = stream.read(&mut buffer[..available]) => result?,
            _ = tokio::time::sleep(Duration::from_millis(25)) => {
                let frames = provider.lock().unwrap().notifications(&mut session);
                for notification in frames { output.send(&mut stream, notification, &session).await?; }
                continue;
            }
        };
        if count == 0 {
            output.signal("session.closed")?;
            return Ok(());
        }
        for byte in &buffer[..count] {
            if *byte != b'\n' {
                frame.push(*byte);
                if frame.len() > limit {
                    output
                        .send(
                            &mut stream,
                            err("frame_too_large", json!({})).frame(Value::Null),
                            &session,
                        )
                        .await?;
                    return Ok(());
                }
                continue;
            }
            if frame
                .iter()
                .all(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
            {
                frame.clear();
                continue;
            }
            let parsed = if std::str::from_utf8(&frame).is_err() {
                Err("invalid_utf8")
            } else {
                encoding::parse(&frame).map_err(|_| "parse_error")
            };
            frame.clear();
            let value = match parsed {
                Ok(value) => value,
                Err(code) => {
                    output
                        .send(
                            &mut stream,
                            err(code, json!({})).frame(Value::Null),
                            &session,
                        )
                        .await?;
                    return Ok(());
                }
            };
            if value.get("id").is_none() && value["jsonrpc"] == "2.0" && value["method"].is_string()
            {
                continue;
            }
            let id = value.get("id");
            let valid_id = id.is_some_and(|v| {
                v.as_str()
                    .is_some_and(|s| (1..=128).contains(&s.chars().count()))
                    || v.is_i64()
                    || v.is_u64()
            });
            let valid = valid_id
                && value["jsonrpc"] == "2.0"
                && value["method"].is_string()
                && value["params"].is_object()
                && value.as_object().is_some_and(|o| {
                    o.keys()
                        .all(|k| ["jsonrpc", "id", "method", "params"].contains(&k.as_str()))
                });
            session.request_id = value["id"].clone();
            let response = if !valid {
                err("invalid_request", json!({})).frame(if valid_id {
                    value["id"].clone()
                } else {
                    Value::Null
                })
            } else {
                match provider.lock().unwrap().handle(
                    &mut session,
                    value["method"].as_str().unwrap(),
                    &value["params"],
                ) {
                    Ok(result) => json!({"jsonrpc":"2.0", "id":value["id"], "result":result}),
                    Err(e) => e.frame(value["id"].clone()),
                }
            };
            output.send(&mut stream, response, &session).await?;
            let frames = provider.lock().unwrap().notifications(&mut session);
            for notification in frames {
                output.send(&mut stream, notification, &session).await?;
            }
        }
    }
}

struct StoreLock(std::fs::File);
impl StoreLock {
    fn acquire(root: &Path) -> Result<Self> {
        use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
        std::fs::create_dir_all(root)?;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(root.join("protocol.lock"))?;
        anyhow::ensure!(
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
            "protocol store already owned"
        );
        Ok(Self(file))
    }
}
impl Drop for StoreLock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    /// A labeled-fake configuration, which is validated without qualifying a
    /// real executable.
    fn codex_config(dir: &Path, thread: Value, exception: Value) -> Value {
        let mut codex = json!({
            "executable": dir.join("fake-codex"), "codex_home": dir.join("home"),
            "fixture_root": dir.join("fixtures"), "env": {"PATH": "/usr/bin:/bin"},
            "thread": thread, "labeled_fake": true,
        });
        if !exception.is_null() {
            codex["test_only_model_exception"] = exception;
        }
        codex
    }

    /// L3 (owner decision, 2026-09-25): the provider Codex must report is a
    /// check the operator names, never a selection, and only beside a model.
    #[test]
    fn an_expected_provider_is_only_accepted_beside_a_model() {
        let dir = tempfile::tempdir().unwrap();
        let plan = json!({"sandbox":"workspace-write","approvalPolicy":"on-request"});
        let with_model = json!({"sandbox":"workspace-write","approvalPolicy":"on-request","model":"gpt-5.6-terra"});
        let mut alone = codex_config(dir.path(), plan, Value::Null);
        alone["expected_model_provider"] = json!("openai");
        let error = codex_host_config(dir.path(), &alone).unwrap_err();
        assert!(
            format!("{error:#}").contains("beside codex.thread.model"),
            "{error:#}"
        );
        let mut empty = codex_config(dir.path(), with_model.clone(), json!(MODEL_EXCEPTION));
        empty["expected_model_provider"] = json!("");
        assert!(codex_host_config(dir.path(), &empty).is_err());
        let mut checked = codex_config(dir.path(), with_model, json!(MODEL_EXCEPTION));
        checked["expected_model_provider"] = json!("openai");
        let host = codex_host_config(dir.path(), &checked).unwrap();
        assert_eq!(host["expected_model_provider"], "openai");
        // Checked, never sent: the thread's own settings carry no provider.
        assert!(host["thread"].get("modelProvider").is_none());
    }

    #[test]
    fn a_model_is_only_accepted_under_the_dated_test_only_exception() {
        let dir = tempfile::tempdir().unwrap();
        let plan = json!({"sandbox":"workspace-write","approvalPolicy":"on-request"});
        let with_model = json!({"sandbox":"workspace-write","approvalPolicy":"on-request","model":"gpt-5.6-terra"});
        let refused = [
            // A model without the exception, which is what a shipped
            // configuration would look like if the option ever leaked.
            (with_model.clone(), Value::Null, "test_only_model_exception"),
            (
                with_model.clone(),
                json!("owner-2026-09-18-m2-fixture-runs"),
                "test_only_model_exception",
            ),
            // The exception without a model cannot sit unused.
            (
                plan.clone(),
                json!(MODEL_EXCEPTION),
                "test_only_model_exception",
            ),
            // An empty model name is not a model.
            (
                json!({"sandbox":"workspace-write","approvalPolicy":"on-request","model":""}),
                json!(MODEL_EXCEPTION),
                "non-empty model name",
            ),
        ];
        for (thread, exception, expected) in refused {
            let config = codex_config(dir.path(), thread, exception);
            let error = codex_host_config(dir.path(), &config).unwrap_err();
            assert!(
                format!("{error:#}").contains(expected),
                "{error:#} for {config}"
            );
        }
        let allowed = codex_host_config(
            dir.path(),
            &codex_config(dir.path(), with_model, json!(MODEL_EXCEPTION)),
        )
        .unwrap();
        assert_eq!(allowed["thread"]["model"], "gpt-5.6-terra");
        let plain =
            codex_host_config(dir.path(), &codex_config(dir.path(), plan, Value::Null)).unwrap();
        assert_eq!(plain["thread"]["model"], Value::Null);
    }
    /// Codex's unmetered features off per launch (review of L3, rounds 3 and
    /// 4): only a recorded owner decision, the one of 2026-09-26, or the
    /// rehearsal's own token beside a labeled fake.
    #[test]
    fn features_off_needs_a_recorded_decision() {
        let dir = tempfile::tempdir().unwrap();
        let plan = json!({"sandbox":"workspace-write","approvalPolicy":"on-request"});
        let mut fake = codex_config(dir.path(), plan.clone(), Value::Null);
        assert!(
            codex_host_config(dir.path(), &fake).unwrap()["features_off_decision"].is_null(),
            "absent unless asked for"
        );
        fake["features_off_decision"] = json!(FEATURES_OFF_REHEARSAL);
        assert_eq!(
            codex_host_config(dir.path(), &fake).unwrap()["features_off_decision"],
            FEATURES_OFF_REHEARSAL
        );
        for decision in [
            json!("owner-2026-09-26-l3-agents-off"),
            json!("rehearsal-only-agents-off"),
            json!(""),
            json!(true),
        ] {
            fake["features_off_decision"] = decision.clone();
            let error = codex_host_config(dir.path(), &fake).unwrap_err();
            assert!(
                format!("{error:#}").contains("not a recorded owner decision"),
                "{error:#} for {decision}"
            );
        }
        // The old, agents-only setting is gone: one override set, one list.
        let mut old = codex_config(dir.path(), plan.clone(), Value::Null);
        old["agents_off_decision"] = json!("rehearsal-only-agents-off");
        let error = codex_host_config(dir.path(), &old).unwrap_err();
        assert!(
            format!("{error:#}").contains("unsupported codex setting: agents_off_decision"),
            "{error:#}"
        );
        // Beside a real Codex, the rehearsal's token and any unrecorded
        // decision are refused before any qualification is attempted, and a
        // Codex given no decision at all gets none.
        let mut real = codex_config(dir.path(), plan, Value::Null);
        real["labeled_fake"] = json!(false);
        for decision in [
            json!(FEATURES_OFF_REHEARSAL),
            json!("owner-2026-09-26-l3-agents-off"),
            json!(""),
        ] {
            real["features_off_decision"] = decision.clone();
            let error = codex_host_config(dir.path(), &real).unwrap_err();
            assert!(
                format!("{error:#}").contains("not a recorded owner decision"),
                "{error:#} for {decision}"
            );
        }
        // The one recorded decision passes that check, and a real Codex is
        // then qualified as ever (this test's executable is not Codex, so
        // qualification is what refuses it).
        real["features_off_decision"] = json!(FEATURES_OFF_DECISIONS[0]);
        let error = codex_host_config(dir.path(), &real).unwrap_err();
        assert!(
            !format!("{error:#}").contains("not a recorded owner decision"),
            "{error:#}"
        );
        // And beside the labeled fake too.
        fake["features_off_decision"] = json!(FEATURES_OFF_DECISIONS[0]);
        assert_eq!(
            codex_host_config(dir.path(), &fake).unwrap()["features_off_decision"],
            FEATURES_OFF_DECISIONS[0]
        );
        assert_eq!(
            FEATURES_OFF_DECISIONS,
            &["owner-2026-09-26-l3-codex-unmetered-features-off"],
            "exactly the owner's decision of 2026-09-26"
        );
    }

    #[tokio::test]
    async fn non_json_whitespace_is_a_fatal_parse_error() {
        for byte in [0x0b, 0x0c] {
            let root = tempfile::tempdir().unwrap();
            let provider = Provider::new(
                root.path(),
                json!({"format":"combraton-conformance-config/1"}),
            )
            .unwrap();
            let (mut caller, server) = UnixStream::pair().unwrap();
            let task = tokio::spawn(connection(server, Arc::new(Mutex::new(provider))));
            caller.write_all(&[byte, b'\n']).await.unwrap();
            let mut reader = BufReader::new(caller);
            let mut line = String::new();
            tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut line))
                .await
                .unwrap()
                .unwrap();
            let frame: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(frame["error"]["data"]["code"], "parse_error");
            assert!(frame["id"].is_null());
            line.clear();
            assert_eq!(reader.read_line(&mut line).await.unwrap(), 0);
            task.await.unwrap().unwrap();
        }
    }
}
