//! The host lifecycle every harness adapter shares.
//!
//! Codex, Claude Code and OpenCode differ in the protocol they speak and in
//! nothing else that matters here: they all detach, pass the same launch
//! fences, snapshot the user's configuration, refuse a request broader than
//! the user's configured default, spawn a child under an explicit environment,
//! park, release exactly once before the first native write, read controls
//! from an append-only file and apply each at most once, stop the child on a
//! deadline, and record a receipt or a known-not-released failure.
//!
//! That sequence lives here, once. What a harness says on the wire stays in
//! its own module — this type never parses a harness message. The file names
//! keep the adapter prefix each adapter already used, so extracting this
//! changed no path the service reads.
use crate::{Lock, append_json, check_launch_guards, identity, secure_root};
use anyhow::{Context, Result, ensure};
use pio_core::{Invocation, Store};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long a child is given to exit on its own before it is killed.
pub const STOP_DEADLINE: Duration = Duration::from_secs(20);

pub fn events_path(root: &Path, adapter: &str, invocation: &str) -> PathBuf {
    root.join(format!("{adapter}-{invocation}.events.jsonl"))
}

pub fn controls_path(root: &Path, adapter: &str, invocation: &str) -> PathBuf {
    root.join(format!("{adapter}-{invocation}.controls.jsonl"))
}

/// Read JSON lines appended after `offset`; returns records and the new offset.
/// A trailing partial line is left for the next read.
pub fn read_jsonl(path: &Path, offset: u64) -> Result<(Vec<Value>, u64)> {
    let Ok(file) = std::fs::File::open(path) else {
        return Ok((vec![], offset));
    };
    let mut reader = BufReader::new(file);
    reader.seek(SeekFrom::Start(offset))?;
    let mut records = vec![];
    let mut position = offset;
    loop {
        let mut line = Vec::new();
        let n = reader.read_until(b'\n', &mut line)?;
        if n == 0 || line.last() != Some(&b'\n') {
            break;
        }
        records.push(serde_json::from_slice(&line)?);
        position += n as u64;
    }
    Ok((records, position))
}

/// Append a control for a host. Ids make repeated appends harmless.
pub fn append_control(root: &Path, adapter: &str, invocation: &str, control: &Value) -> Result<()> {
    ensure!(control["id"].is_string(), "control id required");
    append_json(&controls_path(root, adapter, invocation), control)
}

/// Held across the first native write. Dropping it lets controllers proceed.
/// The lock is the point: the field is never read, only owned until the drop.
pub struct ReleaseGate(#[allow(dead_code)] Lock);

/// One claimed invocation, from the launch fences to the receipt.
pub struct Lifecycle {
    pub root: PathBuf,
    pub adapter: &'static str,
    pub invocation: String,
    pub spec: Value,
    pub source: String,
    store: Store,
    state: Invocation,
    released: bool,
    controls_offset: u64,
    applied: BTreeSet<String>,
    _slot: Lock,
}

impl Lifecycle {
    /// Detach, pass every launch fence, and claim the invocation.
    ///
    /// The fences are the M1 ones and they are the reason a restarted daemon
    /// cannot double-launch: the invocation identity must match, no launch may
    /// already be recorded, and the host slot must be free.
    pub fn claim(
        root: &Path,
        command: &str,
        invocation_id: &str,
        adapter: &'static str,
        source: impl Fn(&Value) -> String,
    ) -> Result<Self> {
        ensure!(
            unsafe { libc::setsid() } != -1,
            "cannot detach {adapter} host session"
        );
        let root = secure_root(root)?;
        let mut store = Store::open(&root)?;
        let state = store.get(command)?.context("missing invocation")?;
        ensure!(
            state.invocation_id == invocation_id,
            "invocation_identity_fenced"
        );
        check_launch_guards(&root, &store)?;
        ensure!(
            state.phase == "intent",
            "host_phase_fence: launch attempt already recorded"
        );
        let spec = state.payload.clone();
        ensure!(spec["adapter"] == adapter, "not a {adapter} invocation");
        let slot = Lock::acquire(&root.join(format!("slot-{}.lock", state.host_slot)))
            .map_err(|_| anyhow::anyhow!("host_slot_fence: slot already owned"))?;
        let source = source(&spec);
        let state = store.transition(
            &state,
            "host_claimed",
            Some(identity(std::process::id())?),
            None,
            None,
        )?;
        Ok(Self {
            invocation: state.invocation_id.clone(),
            root,
            adapter,
            spec,
            source,
            store,
            state,
            released: false,
            controls_offset: 0,
            applied: BTreeSet::new(),
            _slot: slot,
        })
    }

    pub fn released(&self) -> bool {
        self.released
    }

    pub fn phase(&self) -> &str {
        &self.state.phase
    }

    /// Every event carries the host's label and time; the service decides
    /// meaning.
    pub fn event(&self, mut record: Value) -> Result<()> {
        record["source"] = self.source.clone().into();
        record["observed_at_unix_ms"] = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as u64)
            .into();
        // **The recording boundary.** Events reach receipts, which are
        // committed to a public repository. The owner's home path is not ours
        // to publish, and nor is what it discloses — the Claude receipts had
        // been carrying the full path of every installed plugin since R2.
        let record = pio_core::redact_home(&record, self.home());
        append_json(
            &events_path(&self.root, self.adapter, &self.invocation),
            &record,
        )
    }

    /// The home this invocation runs against, for redaction. Absent for the
    /// fake-process adapter, which has none.
    fn home(&self) -> &str {
        self.spec["home"].as_str().unwrap_or_default()
    }

    /// Record a guard's verdict and refuse if it did not allow the request.
    ///
    /// The event is written whether or not the guard allowed, because a
    /// refusal that leaves no evidence is not a refusal anyone can check.
    pub fn guard(&self, kind: &str, guard: &Value, refusal: &str) -> Result<()> {
        self.event(json!({"kind":kind,"guard":guard}))?;
        ensure!(guard["allowed"] == true, "{refusal}: {guard}");
        Ok(())
    }

    /// A child is running and bound to the qualification record.
    pub fn spawned(&self, identity: &Value, native: &Value) -> Result<()> {
        self.event(json!({"kind":"spawned","identity":identity,"native":native}))
    }

    /// The child exists and holds no brief yet.
    pub fn park(&mut self, child: pio_core::ProcessIdentity) -> Result<()> {
        self.state = self
            .store
            .transition(&self.state, "parked", None, Some(child), None)?;
        Ok(())
    }

    /// Record the release and take the controller gate.
    ///
    /// The returned guard must be held across the first native write, so a
    /// controller cannot observe a released invocation whose brief has not
    /// been sent. Released is recorded before the write, so a crash between
    /// them is reported as released rather than silently retried.
    pub fn release(&mut self) -> Result<ReleaseGate> {
        let gate = Lock::acquire_mode(&self.root.join("controller-gate.lock"), libc::LOCK_SH)?;
        self.state = self
            .store
            .transition(&self.state, "released", None, None, None)?;
        self.released = true;
        Ok(ReleaseGate(gate))
    }

    /// Controls appended since the last call, each returned at most once.
    pub fn controls(&mut self) -> Result<Vec<Value>> {
        let path = controls_path(&self.root, self.adapter, &self.invocation);
        let (records, offset) = read_jsonl(&path, self.controls_offset)?;
        self.controls_offset = offset;
        Ok(records
            .into_iter()
            .filter(|control| {
                control["id"]
                    .as_str()
                    .is_some_and(|id| !id.is_empty() && self.applied.insert(id.to_owned()))
            })
            .collect())
    }

    /// Wait for a child to exit on its own, then kill it. Returns its code.
    pub fn stop(&self, child: &mut std::process::Child) -> Result<Option<i32>> {
        let deadline = Instant::now() + STOP_DEADLINE;
        loop {
            if let Some(status) = child.try_wait()? {
                return Ok(status.code());
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                return Ok(child.wait()?.code());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    pub fn complete(&mut self, receipt: Value) -> Result<()> {
        // The same boundary: a receipt is published too.
        let receipt = pio_core::redact_home(&receipt, self.home());
        self.store
            .transition(&self.state, "completed", None, None, Some(receipt))?;
        Ok(())
    }

    /// Record a failure. An invocation that never released is recorded as
    /// known-not-released, so a caller is told the brief did not reach the
    /// harness rather than being left to guess.
    pub fn fail(&mut self, error: &anyhow::Error, cleanup: impl FnOnce()) {
        let _ = self.event(
            json!({"kind":"host_error","released":self.released,"error":format!("{error:#}")}),
        );
        cleanup();
        if !self.released && matches!(self.state.phase.as_str(), "host_claimed" | "parked") {
            let _ = self.store.transition(
                &self.state,
                "known_not_released",
                None,
                None,
                Some(json!({"source":self.source,"kind":"known_not_released",
                            "release_attempted":false,"reason":format!("{error:#}")})),
            );
        }
    }
}

/// A child speaking newline-delimited JSON on stdin and stdout.
///
/// Codex, Claude Code and OpenCode all speak this shape; only the messages
/// differ. `pio_codex::rpc::AppServer` keeps its own copy of the reader thread
/// because `pio-codex` cannot depend on `pio-host`, and it also owns JSON-RPC
/// id bookkeeping that is not shared. If a fourth harness arrives, the
/// transport belongs in a crate both can see.
pub struct StdioChild {
    pub child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    incoming: std::sync::mpsc::Receiver<Result<Value, String>>,
}

impl StdioChild {
    /// Spawn with exactly `env` and nothing inherited. Stderr goes to a file
    /// the caller owns, so a harness that writes there cannot block on a pipe.
    pub fn spawn(
        executable: &Path,
        args: &[String],
        env: &[(String, String)],
        cwd: &Path,
        stderr: std::fs::File,
    ) -> Result<Self> {
        let mut child = std::process::Command::new(executable)
            .args(args)
            .current_dir(cwd)
            .env_clear()
            .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::from(stderr))
            .spawn()
            .with_context(|| format!("spawn {}", executable.display()))?;
        let stdout = child.stdout.take().context("child stdout")?;
        let stdin = child.stdin.take();
        let (sender, incoming) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let parsed = match line {
                    Ok(line) if line.trim().is_empty() => continue,
                    Ok(line) => serde_json::from_str::<Value>(&line)
                        .map_err(|error| format!("invalid JSON: {error}")),
                    Err(error) => Err(format!("read error: {error}")),
                };
                if sender.send(parsed).is_err() {
                    return;
                }
            }
        });
        Ok(Self {
            child,
            stdin,
            incoming,
        })
    }

    pub fn send(&mut self, message: &Value) -> Result<()> {
        use std::io::Write;
        let stdin = self.stdin.as_mut().context("child stdin closed")?;
        let mut bytes = serde_json::to_vec(message)?;
        bytes.push(b'\n');
        stdin.write_all(&bytes)?;
        stdin.flush()?;
        Ok(())
    }

    /// The next message, or `None` if none arrived within `timeout`.
    /// An unparseable line is an error, never a silently dropped message.
    pub fn receive(&self, timeout: Duration) -> Result<Option<Value>> {
        match self.incoming.recv_timeout(timeout) {
            Ok(Ok(message)) => Ok(Some(message)),
            Ok(Err(error)) => anyhow::bail!("{error}"),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Ok(None),
        }
    }

    pub fn close_stdin(&mut self) {
        self.stdin.take();
    }
}
