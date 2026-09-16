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

pub fn serve(root: &Path, config: &Path, socket: &Path) -> Result<()> {
    serve_mode(root, config, socket, Mode::Conformance)
}
pub fn serve_fake(root: &Path, config: &Path, socket: &Path) -> Result<()> {
    serve_mode(root, config, socket, Mode::FakeProcess)
}
pub fn serve_codex(root: &Path, config: &Path, socket: &Path) -> Result<()> {
    serve_mode(root, config, socket, Mode::Codex)
}
#[derive(PartialEq)]
enum Mode {
    Conformance,
    FakeProcess,
    Codex,
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
                "labeled_fake"
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
                ["sandbox", "approvalPolicy"].contains(&name.as_str()),
                "unsupported codex.thread setting: {name}"
            );
            // Never select full access or disable approvals on the user's behalf.
            anyhow::ensure!(
                !matches!(value.as_str(), Some("danger-full-access" | "never")),
                "codex.thread.{name} value is not permitted: {value}"
            );
        }
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
    host["qualification_binding"] = json!({
        "native_path":resolution["native"]["path"],
        "native_sha256":resolution["native"]["sha256"],
        "wrapper_sha256":resolution["wrapper"]["sha256"],
        "node_sha256":resolution["node"]["sha256"],
        "canonical_listing_sha256":record["schema"]["canonical_listing_sha256"],
    });
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
    let provider = if mode == Mode::Codex {
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
