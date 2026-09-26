//! stream/1 framing over the local Unix socket: one JSON-RPC 2.0 frame per
//! line, requests answered by id, unsolicited notifications kept rather than
//! dropped. Nothing here knows an operation; `client` does.
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

/// The service's own ceiling before negotiation (stream/1).
pub const MAX_FRAME_BYTES: usize = 1_048_576;
/// How long one blocking read waits before the caller's deadline is checked
/// again. Short, so a follower can notice a detach request promptly.
const TICK: Duration = Duration::from_millis(200);

/// The local-API credential, `ccred1.<principal>.<secret>`.
///
/// Never printed: `Debug` shows only the principal.
#[derive(Clone)]
pub struct Credential {
    raw: String,
    principal: String,
}

impl Credential {
    pub fn parse(text: &str) -> Result<Self> {
        let raw = text.trim().to_owned();
        let principal = raw
            .strip_prefix("ccred1.")
            .and_then(|rest| rest.rsplit_once('.'))
            .map(|(principal, _)| principal.to_owned())
            .filter(|principal| !principal.is_empty())
            .context("credential format: expected ccred1.<principal>.<secret>")?;
        Ok(Self { raw, principal })
    }

    /// Reads a credential file. The file holds the credential and nothing
    /// else; surrounding whitespace is ignored.
    pub fn read(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading credential file {}", path.display()))?;
        Self::parse(&text)
    }

    pub fn principal(&self) -> &str {
        &self.principal
    }

    pub(crate) fn secret(&self) -> &str {
        &self.raw
    }
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Credential({}, <redacted>)", self.principal)
    }
}

/// The service's refusal, **verbatim**: the JSON-RPC `error` object exactly
/// as it came off the wire. A caller shows this, never a paraphrase of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Refusal {
    pub operation: String,
    pub error: Value,
}

impl Refusal {
    /// `error.data.code`, the Protocol's machine-readable reason.
    pub fn code(&self) -> &str {
        self.error["data"]["code"]
            .as_str()
            .or_else(|| self.error["message"].as_str())
            .unwrap_or("unknown")
    }

    pub fn details(&self) -> &Value {
        &self.error["data"]["details"]
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} refused by the service: {}",
            self.operation,
            serde_json::to_string(&self.error).unwrap_or_default()
        )
    }
}

/// Every way a call can fail. A refusal is the service answering "no"; a
/// transport failure is not an answer at all, and the two are never merged.
#[derive(Debug)]
pub enum Failure {
    Refused(Refusal),
    Transport(anyhow::Error),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Refused(refusal) => refusal.fmt(f),
            Failure::Transport(error) => write!(f, "{error:#}"),
        }
    }
}

impl std::error::Error for Failure {}

impl From<anyhow::Error> for Failure {
    fn from(error: anyhow::Error) -> Self {
        Failure::Transport(error)
    }
}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        Failure::Transport(error.into())
    }
}

impl From<serde_json::Error> for Failure {
    fn from(error: serde_json::Error) -> Self {
        Failure::Transport(error.into())
    }
}

pub type Reply<T = Value> = std::result::Result<T, Failure>;

/// The canonical encoding the service digests a command intent with: object
/// keys sorted by UTF-16 code units, no whitespace (JCS for encoding/1's
/// value domain, which has no floats).
pub fn canonical(value: &Value) -> Vec<u8> {
    fn write(value: &Value, out: &mut String) {
        match value {
            Value::Object(map) => {
                out.push('{');
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort_by_cached_key(|k| k.encode_utf16().collect::<Vec<_>>());
                for (index, key) in keys.into_iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(key).unwrap_or_default());
                    out.push(':');
                    write(&map[key], out);
                }
                out.push('}');
            }
            Value::Array(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write(item, out);
                }
                out.push(']');
            }
            other => out.push_str(&serde_json::to_string(other).unwrap_or_default()),
        }
    }
    let mut out = String::new();
    write(value, &mut out);
    out.into_bytes()
}

/// `sha256:<hex>` of some bytes, the Protocol's digest form.
pub fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

/// Refuses a peer that is not this user. The service checks the same thing
/// from its side; a client that talks to a socket someone else bound would
/// hand them the credential.
fn same_user(stream: &UnixStream) -> bool {
    #[cfg(target_os = "linux")]
    {
        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
        let mut size = std::mem::size_of_val(&cred) as libc::socklen_t;
        let rc = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut cred as *mut libc::ucred).cast(),
                &mut size,
            )
        };
        rc == 0 && cred.uid == unsafe { libc::geteuid() }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let (mut uid, mut gid) = (0, 0);
        unsafe {
            libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) == 0 && uid == libc::geteuid()
        }
    }
}

/// One stream/1 connection.
pub struct Connection {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
    /// Bytes of a frame whose newline has not arrived yet. Kept across a
    /// read timeout, so a slow frame is never cut in half.
    partial: Vec<u8>,
    notifications: VecDeque<Value>,
    /// How long a request may wait for its answer.
    pub timeout: Duration,
}

impl Connection {
    pub fn open(socket: &Path, timeout: Duration) -> Result<Self> {
        let writer = UnixStream::connect(socket)
            .with_context(|| format!("connecting to {}", socket.display()))?;
        ensure!(
            same_user(&writer),
            "the service socket belongs to another user"
        );
        writer.set_read_timeout(Some(TICK))?;
        writer.set_write_timeout(Some(timeout))?;
        Ok(Self {
            reader: BufReader::new(writer.try_clone()?),
            writer,
            partial: Vec::new(),
            notifications: VecDeque::new(),
            timeout,
        })
    }

    fn send(&mut self, frame: &Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(frame)?;
        ensure!(bytes.len() < MAX_FRAME_BYTES, "request frame too large");
        bytes.push(b'\n');
        self.writer.write_all(&bytes)?;
        Ok(())
    }

    /// One frame, or `None` if nothing complete arrived before `deadline`.
    fn read_frame(&mut self, deadline: Instant) -> Result<Option<Value>> {
        loop {
            match self.reader.read_until(b'\n', &mut self.partial) {
                Ok(0) => bail!("the service closed the connection"),
                Ok(_) if self.partial.last() == Some(&b'\n') => {
                    let line = std::mem::take(&mut self.partial);
                    ensure!(
                        line.len() <= MAX_FRAME_BYTES + 1,
                        "response frame too large"
                    );
                    if line.iter().all(u8::is_ascii_whitespace) {
                        continue;
                    }
                    return Ok(Some(serde_json::from_slice(&line)?));
                }
                Ok(_) => bail!("the service closed the connection mid-frame"),
                Err(error)
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    ensure!(
                        self.partial.len() <= MAX_FRAME_BYTES,
                        "response frame too large"
                    );
                    if Instant::now() >= deadline {
                        return Ok(None);
                    }
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => {
                    if Instant::now() >= deadline {
                        return Ok(None);
                    }
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    /// Sends one request and waits for the frame that answers it. Anything
    /// else that arrives meanwhile is a notification, and is kept.
    pub fn request(&mut self, method: &str, params: &Value) -> Result<Value> {
        let id = uuid::Uuid::new_v4().to_string();
        self.send(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))?;
        let deadline = Instant::now() + self.timeout;
        loop {
            let Some(frame) = self.read_frame(deadline)? else {
                bail!("no answer to {method} within {:?}", self.timeout);
            };
            if frame.get("id").and_then(Value::as_str) == Some(id.as_str()) {
                return Ok(frame);
            }
            if frame.get("method").is_some() && frame.get("id").is_none() {
                self.notifications.push_back(frame);
            }
        }
    }

    /// The next unsolicited frame, waiting at most `wait` for one.
    pub fn notification(&mut self, wait: Duration) -> Result<Option<Value>> {
        if let Some(frame) = self.notifications.pop_front() {
            return Ok(Some(frame));
        }
        let deadline = Instant::now() + wait;
        while let Some(frame) = self.read_frame(deadline)? {
            if frame.get("method").is_some() && frame.get("id").is_none() {
                return Ok(Some(frame));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credential_names_its_principal_and_never_prints_its_secret() {
        let credential = Credential::parse(&format!("ccred1.owner.{}\n", "m".repeat(43))).unwrap();
        assert_eq!(credential.principal(), "owner");
        assert!(!format!("{credential:?}").contains("mmm"));
        assert!(Credential::parse("owner.secret").is_err());
        assert!(Credential::parse("ccred1..secret").is_err());
    }

    #[test]
    fn the_canonical_form_sorts_keys_and_drops_whitespace() {
        let value = json!({"b": [1, {"d": "x", "c": null}], "a": true});
        assert_eq!(
            String::from_utf8(canonical(&value)).unwrap(),
            r#"{"a":true,"b":[1,{"c":null,"d":"x"}]}"#
        );
        // The Python test caller's digest of the same value
        // (`scripts/public_api.py`: json.dumps, sort_keys, compact).
        assert_eq!(
            digest(&canonical(&value)),
            "sha256:10ea11db4288d39d67495f2bf8da28a7c86e60af567ca1ebba7c0be77fc4d100"
        );
    }

    #[test]
    fn a_refusal_keeps_the_error_object_verbatim() {
        let error = json!({"code": -32001, "message": "not_found",
                           "data": {"code": "not_found", "details": {"reason": "already_decided"}}});
        let refusal = Refusal {
            operation: "execution.respond_action".into(),
            error: error.clone(),
        };
        assert_eq!(refusal.code(), "not_found");
        assert_eq!(refusal.details()["reason"], "already_decided");
        assert!(
            refusal
                .to_string()
                .contains(&serde_json::to_string(&error).unwrap())
        );
    }
}
