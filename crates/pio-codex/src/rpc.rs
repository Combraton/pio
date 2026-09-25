//! Minimal line-delimited JSON-RPC peer for `codex app-server` over stdio.
//! The pinned app-server omits the `"jsonrpc":"2.0"` member on the wire; this
//! codec is independent of PIO's Protocol framing.
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

/// Largest accepted app-server line. Longer lines end the connection.
pub const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

pub enum Incoming {
    Message(Value),
    /// The server closed stdout or sent an unreadable line.
    Closed(String),
}

pub struct AppServer {
    pub child: Child,
    stdin: Option<ChildStdin>,
    incoming: Receiver<Incoming>,
    next_id: u64,
}

impl AppServer {
    /// Spawn `executable app-server` with exactly `env`; no other variables are
    /// inherited. Stderr goes to `stderr`.
    pub fn spawn(
        executable: &Path,
        env: &[(String, String)],
        stderr: std::fs::File,
    ) -> Result<Self> {
        let mut command = Command::new(executable);
        command
            .arg("app-server")
            .env_clear()
            .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr));
        let mut child = command
            .spawn()
            .with_context(|| format!("spawn {} app-server", executable.display()))?;
        let stdout = child.stdout.take().context("app-server stdout")?;
        let stdin = child.stdin.take();
        let (sender, incoming) = channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = Vec::new();
                match std::io::Read::take(&mut reader, MAX_LINE_BYTES as u64 + 1)
                    .read_until(b'\n', &mut line)
                {
                    Ok(0) => {
                        let _ = sender.send(Incoming::Closed("stdout closed".into()));
                        return;
                    }
                    Ok(_) if line.len() > MAX_LINE_BYTES => {
                        let _ = sender.send(Incoming::Closed("line too long".into()));
                        return;
                    }
                    Ok(_) => match serde_json::from_slice::<Value>(&line) {
                        Ok(message) => {
                            if sender.send(Incoming::Message(message)).is_err() {
                                return;
                            }
                        }
                        Err(error) => {
                            let _ = sender.send(Incoming::Closed(format!("invalid JSON: {error}")));
                            return;
                        }
                    },
                    Err(error) => {
                        let _ = sender.send(Incoming::Closed(format!("read error: {error}")));
                        return;
                    }
                }
            }
        });
        Ok(Self {
            child,
            stdin,
            incoming,
            next_id: 0,
        })
    }

    pub fn send(&mut self, message: &Value) -> Result<()> {
        let stdin = self.stdin.as_mut().context("app-server stdin closed")?;
        let mut bytes = serde_json::to_vec(message)?;
        bytes.push(b'\n');
        stdin.write_all(&bytes)?;
        stdin.flush()?;
        Ok(())
    }

    /// Send a request and return its id without waiting.
    pub fn request(&mut self, method: &str, params: Value) -> Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"method":method,"id":id,"params":params}))?;
        Ok(id)
    }

    pub fn notify(&mut self, method: &str) -> Result<()> {
        self.send(&json!({"method":method}))
    }

    pub fn respond(&mut self, id: &Value, result: Value) -> Result<()> {
        self.send(&json!({"id":id,"result":result}))
    }

    pub fn receive(&self, timeout: Duration) -> Result<Option<Value>> {
        match self.incoming.recv_timeout(timeout) {
            Ok(Incoming::Message(message)) => Ok(Some(message)),
            Ok(Incoming::Closed(reason)) => bail!("app-server connection ended: {reason}"),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => bail!("app-server reader ended"),
        }
    }

    /// Wait for the response to `id`, handing every other message to `other`.
    pub fn wait_response(
        &self,
        id: u64,
        timeout: Duration,
        mut other: impl FnMut(Value) -> Result<()>,
    ) -> Result<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!("no app-server response to request {id}");
            }
            if let Some(message) = self.receive(remaining.min(Duration::from_millis(50)))? {
                if message.get("id") == Some(&json!(id))
                    && (message.get("result").is_some() || message.get("error").is_some())
                {
                    return Ok(message);
                }
                other(message)?;
            }
        }
    }

    /// Wait for the response to `id`, answering at once, with a JSON-RPC
    /// error, every server request that arrives meanwhile. `decline` builds
    /// the record of each (its `reason` is the error's message), and the
    /// records are returned with the response for the caller to keep. A
    /// request that arrived during a wait used to be handed to a closure that
    /// dropped it: never answered, never recorded (review of L3, round 2,
    /// V-2/HR-6).
    pub fn wait_response_declining(
        &mut self,
        id: u64,
        timeout: Duration,
        mut decline: impl FnMut(&str, &Value) -> Value,
    ) -> Result<(Value, Vec<Value>)> {
        let deadline = Instant::now() + timeout;
        let mut declined = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!("no app-server response to request {id}");
            }
            if let Some(message) = self.receive(remaining.min(Duration::from_millis(50)))? {
                if message.get("id") == Some(&json!(id))
                    && (message.get("result").is_some() || message.get("error").is_some())
                {
                    return Ok((message, declined));
                }
                if let (Some(request), Some(method)) =
                    (message.get("id").cloned(), message["method"].as_str())
                {
                    let mut record = decline(method, &message["params"]);
                    self.send(&json!({"id":request,"error":{"code":-32000,
                                      "message":record["reason"]}}))?;
                    record["request_id"] = request;
                    declined.push(record);
                }
            }
        }
    }

    /// Close stdin so the server can exit cleanly.
    pub fn close_stdin(&mut self) {
        self.stdin = None;
    }
}
