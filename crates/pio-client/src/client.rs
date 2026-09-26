//! The typed operations of Protocol core/1 and execution/1, over one
//! authenticated stream/1 session. Every answer is the service's own: a
//! result is returned as the service shaped it, and a refusal as
//! [`Failure::Refused`] carrying the error object verbatim.
use crate::encoding::canonical;
use crate::wire::{Connection, Credential, Failure, Refusal, Reply, digest};
use anyhow::Context;
use base64::Engine;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

/// The namespaced extension that carries a command's content bytes (a brief,
/// a steering message, an answer) beside its digest. PIO's, not the
/// Protocol's; outside the digested intent, as the service requires.
pub const CONTENT: &str = "pio.combraton.dev/content";
pub const EXECUTION: &str = "execution.execution";
pub const CONTROLLER: &str = "execution.controller";

/// Core features this client needs, and the ones it will use if offered.
const CORE_REQUIRED: [&str; 3] = ["core.events", "core.capabilities", "core.effects"];
/// `core.grants` must be negotiated to **present** a grant, not only to
/// issue one (board_fold.py established this).
const CORE_OPTIONAL: [&str; 1] = ["core.grants"];
/// Every execution feature is optional: a service that does not offer one
/// leaves it unselected, and an operation that needs it is then refused by
/// the service, in its own words.
const EXECUTION_OPTIONAL: [&str; 7] = [
    "execution.controller",
    "execution.output",
    "execution.discovery",
    "execution.workspaces",
    "execution.usage",
    "execution.actions",
    "execution.steering",
];

#[derive(Clone, Debug)]
pub struct Options {
    /// How long one request may wait for its answer.
    pub timeout: Duration,
    /// A grant to present on every request after negotiation.
    pub grant: Option<String>,
    pub caller: String,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(10),
            grant: None,
            caller: "pio-client".into(),
        }
    }
}

/// Where a read of the event stream starts.
#[derive(Clone, Debug, PartialEq)]
pub enum Position {
    Start,
    Now,
    Cursor(String),
}

impl Position {
    fn apply(&self, payload: &mut Value) {
        match self {
            Position::Start => payload["from"] = "start".into(),
            Position::Now => payload["from"] = "now".into(),
            Position::Cursor(cursor) => payload["cursor"] = cursor.clone().into(),
        }
    }
}

/// The two fences on every execution command: the subject's revision (a
/// precondition) and the controller's authority epoch. A stale one of
/// either is refused by the service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fence {
    pub revision: u64,
    pub epoch: u64,
}

/// One page of `execution.output.read`, with the bytes decoded.
#[derive(Clone, Debug)]
pub struct OutputChunk {
    pub data: Vec<u8>,
    pub offset: u64,
    pub next_offset: u64,
    pub end_offset: u64,
    pub coverage: String,
    pub lost_ranges: Value,
}

/// How `execution.reconcile` is asked.
#[derive(Clone, Debug)]
pub enum Reconcile {
    CommandId(String),
    DeliveryId(String),
}

/// A command envelope, digested the way the service checks it.
#[derive(Clone, Debug)]
pub struct Command {
    pub operation: String,
    pub subject: Value,
    pub preconditions: Vec<Value>,
    pub payload: Value,
    pub command_id: String,
    pub epoch: u64,
    pub dedupe_generation: u64,
    /// Content bytes carried beside their digest, as `(media_type, text)`.
    pub content: Option<(String, String)>,
}

impl Command {
    pub fn new(operation: &str, subject: Value, revision: u64, payload: Value) -> Self {
        Self {
            operation: operation.into(),
            preconditions: vec![json!({"subject": subject, "revision": revision})],
            subject,
            payload,
            command_id: format!("pio-client-{}", uuid::Uuid::new_v4()),
            epoch: 0,
            dedupe_generation: 1,
            content: None,
        }
    }

    /// The digest covers the intent: operation, subject, preconditions,
    /// requires, payload, and only those extensions `requires` names (none
    /// here), so the content extension rides outside it.
    pub fn envelope(&self) -> Value {
        let intent = json!({
            "operation": self.operation, "subject": self.subject,
            "preconditions": self.preconditions, "requires": [],
            "payload": self.payload, "extensions": {}});
        let mut envelope = intent.clone();
        envelope["message_id"] = uuid::Uuid::new_v4().to_string().into();
        envelope["command_id"] = self.command_id.clone().into();
        envelope["command_digest"] = digest(&canonical(&intent)).into();
        envelope["dedupe_generation"] = self.dedupe_generation.into();
        envelope["authority_epoch"] = self.epoch.into();
        if let Some((media_type, text)) = &self.content {
            envelope["extensions"] = json!({CONTENT: {"media_type": media_type, "text": text}});
        }
        envelope
    }
}

pub fn execution(id: &str) -> Value {
    json!({"kind": EXECUTION, "id": id})
}

/// One authenticated, negotiated session with the service.
pub struct Client {
    connection: Connection,
    principal: String,
    grant: Option<String>,
    /// What `core.describe` said when the session opened.
    pub described: Value,
    /// What `core.negotiate` selected.
    pub negotiated: Value,
}

impl Client {
    /// Connects, reads `core.describe`, authenticates with the local-API
    /// credential and negotiates. A refusal at any step is returned as the
    /// service gave it.
    pub fn connect(socket: &Path, credential: &Credential, options: &Options) -> Reply<Self> {
        let connection = Connection::open(socket, options.timeout)?;
        let mut client = Self {
            connection,
            principal: credential.principal().to_owned(),
            grant: None,
            described: Value::Null,
            negotiated: Value::Null,
        };
        client.described = client.query("core.describe", json!({}))?;
        client.query(
            "core.authenticate",
            json!({"credential": credential.secret()}),
        )?;
        client.negotiated = client.query(
            "core.negotiate",
            json!({
                "caller": {"name": options.caller, "version": env!("CARGO_PKG_VERSION")},
                "receive_limits": {"max_frame_bytes": crate::wire::MAX_FRAME_BYTES},
                "profiles": [
                    {"name": "core", "majors": [1], "required": true,
                     "required_features": CORE_REQUIRED, "optional_features": CORE_OPTIONAL},
                    {"name": "execution", "majors": [1], "required": true,
                     "required_features": [], "optional_features": EXECUTION_OPTIONAL}]}),
        )?;
        client.grant = options.grant.clone();
        Ok(client)
    }

    pub fn principal(&self) -> &str {
        &self.principal
    }

    /// The dedupe generation new commands are stamped with: the service's
    /// current one, from `core.describe`.
    fn generation(&self) -> u64 {
        self.described["dedupe_window"]["current"]
            .as_u64()
            .unwrap_or(1)
    }

    /// Sends one envelope as it is (plus the session's grant) and returns
    /// the result, or the refusal verbatim.
    pub fn call(&mut self, envelope: &Value) -> Reply {
        let operation = envelope["operation"]
            .as_str()
            .context("envelope has no operation")?
            .to_owned();
        let mut envelope = envelope.clone();
        if let Some(grant) = &self.grant
            && envelope.get("grant").is_none()
        {
            envelope["grant"] = grant.clone().into();
        }
        let frame = self.connection.request(&operation, &envelope)?;
        self.answer(&operation, frame)
    }

    /// Like [`Client::call`], but hands back the whole response frame, for a
    /// caller that records exactly what came off the wire.
    pub fn call_frame(&mut self, envelope: &Value) -> Reply {
        let operation = envelope["operation"]
            .as_str()
            .context("envelope has no operation")?
            .to_owned();
        Ok(self.connection.request(&operation, envelope)?)
    }

    fn answer(&self, operation: &str, frame: Value) -> Reply {
        if let Some(error) = frame.get("error") {
            return Err(Failure::Refused(Refusal {
                operation: operation.to_owned(),
                error: error.clone(),
            }));
        }
        frame
            .get("result")
            .cloned()
            .context("response frame has neither result nor error")
            .map_err(Failure::Transport)
    }

    pub fn query(&mut self, operation: &str, payload: Value) -> Reply {
        let mut envelope = json!({"operation": operation,
                                  "message_id": uuid::Uuid::new_v4().to_string(),
                                  "payload": payload});
        if matches!(
            operation,
            "core.describe" | "core.authenticate" | "core.negotiate"
        ) {
            let frame = self.connection.request(operation, &envelope)?;
            return self.answer(operation, frame);
        }
        if let Some(grant) = &self.grant {
            envelope["grant"] = grant.clone().into();
        }
        let frame = self.connection.request(operation, &envelope)?;
        self.answer(operation, frame)
    }

    pub fn command(&mut self, mut command: Command) -> Reply {
        command.dedupe_generation = self.generation();
        self.call(&command.envelope())
    }

    // --- core --------------------------------------------------------------

    pub fn describe(&mut self) -> Reply {
        self.query("core.describe", json!({}))
    }

    pub fn capabilities(&mut self) -> Reply {
        self.query("core.capabilities", json!({}))
    }

    /// One page of the event stream. `kinds` empty means every kind.
    pub fn events_read(&mut self, from: &Position, kinds: &[&str], limit: u32) -> Reply {
        let mut payload = json!({"limit": limit});
        from.apply(&mut payload);
        if !kinds.is_empty() {
            payload["kinds"] = json!(kinds);
        }
        self.query("core.events.read", payload)
    }

    /// Opens a subscription; its id is `result.subscription`.
    pub fn events_subscribe(&mut self, from: &Position, kinds: &[&str]) -> Reply {
        let mut payload = json!({});
        from.apply(&mut payload);
        if !kinds.is_empty() {
            payload["kinds"] = json!(kinds);
        }
        self.query("core.events.subscribe", payload)
    }

    pub fn events_unsubscribe(&mut self, subscription: &str) -> Reply {
        self.query(
            "core.events.unsubscribe",
            json!({"subscription": subscription}),
        )
    }

    /// The next `core.events.notify` params, waiting at most `wait`.
    pub fn notification(&mut self, wait: Duration) -> Reply<Option<Value>> {
        while let Some(frame) = self.connection.notification(wait)? {
            if frame["method"] == "core.events.notify" {
                return Ok(Some(frame["params"].clone()));
            }
        }
        Ok(None)
    }

    pub fn grant_issue(&mut self, grant: &str, terms: Value) -> Reply {
        let mut command = Command::new(
            "core.grant.issue",
            json!({"kind": "core.grant", "id": grant}),
            0,
            terms,
        );
        command.command_id = format!("grant-{grant}");
        self.command(command)
    }

    pub fn grant_get(&mut self, grant: &str) -> Reply {
        self.query("core.grant.get", json!({"grant": grant}))
    }

    pub fn grant_revoke(&mut self, grant: &str, revision: u64) -> Reply {
        self.command(Command::new(
            "core.grant.revoke",
            json!({"kind": "core.grant", "id": grant}),
            revision,
            json!({}),
        ))
    }

    // --- execution ---------------------------------------------------------

    pub fn discovery(&mut self) -> Reply {
        self.query("execution.discovery.list", json!({}))
    }

    pub fn inspect(&mut self, id: &str) -> Reply {
        self.query("execution.inspect", json!({"execution": id}))
    }

    pub fn output_read(&mut self, id: &str, offset: u64, max_bytes: u32) -> Reply<OutputChunk> {
        let result = self.query(
            "execution.output.read",
            json!({"execution": id, "offset": offset, "max_bytes": max_bytes}),
        )?;
        let data = base64::engine::general_purpose::STANDARD
            .decode(result["data_base64"].as_str().unwrap_or(""))
            .context("output data is not base64")?;
        let number = |key: &str| result[key].as_u64().unwrap_or(0);
        Ok(OutputChunk {
            data,
            offset: number("offset"),
            next_offset: number("next_offset"),
            end_offset: number("end_offset"),
            coverage: result["coverage"].as_str().unwrap_or("").to_owned(),
            lost_ranges: result["lost_ranges"].clone(),
        })
    }

    pub fn reconcile(&mut self, by: &Reconcile) -> Reply {
        let payload = match by {
            Reconcile::CommandId(id) => json!({"command_id": id}),
            Reconcile::DeliveryId(id) => json!({"delivery_id": id}),
        };
        self.query("execution.reconcile", payload)
    }

    /// Submits a prepared `execution.submit` envelope exactly as given: the
    /// caller owns its identity and digest (see the caller ledger in `pio
    /// client submit`).
    pub fn submit(&mut self, envelope: &Value) -> Reply {
        if envelope["operation"] != "execution.submit" {
            return Err(Failure::Transport(anyhow::anyhow!(
                "not an execution.submit envelope"
            )));
        }
        self.call(envelope)
    }

    pub fn cancel(&mut self, id: &str, fence: Fence, reason: Option<&str>) -> Reply {
        let payload = match reason {
            Some(reason) => json!({"reason": reason}),
            None => json!({}),
        };
        let mut command = Command::new("execution.cancel", execution(id), fence.revision, payload);
        command.epoch = fence.epoch;
        self.command(command)
    }

    pub fn steer(&mut self, id: &str, text: &str, fence: Fence) -> Reply {
        let mut command = Command::new(
            "execution.steer",
            execution(id),
            fence.revision,
            json!({"message": {"digest": digest(text.as_bytes()), "media_type": "text/plain"}}),
        );
        command.epoch = fence.epoch;
        command.content = Some(("text/plain".into(), text.into()));
        self.command(command)
    }

    /// Answers one pending action. `decision` is sent in the harness's own
    /// vocabulary (see [`decision_word`]); the service refuses anything
    /// outside it, an action that is not pending on **this** run, and a
    /// stale fence.
    pub fn respond_action(
        &mut self,
        id: &str,
        action_id: &str,
        decision: &str,
        fence: Fence,
    ) -> Reply {
        let body = serde_json::to_string(&json!({"decision": decision}))?;
        let mut command = Command::new(
            "execution.respond_action",
            execution(id),
            fence.revision,
            json!({"action_id": action_id,
                   "response": {"digest": digest(body.as_bytes()),
                                "media_type": "application/json"}}),
        );
        command.epoch = fence.epoch;
        command.content = Some(("application/json".into(), body));
        self.command(command)
    }

    /// Claims the execution controller, which advances its authority epoch.
    /// Every command fenced at an older epoch is refused afterwards.
    pub fn claim_controller(&mut self, host: &str, epoch: u64) -> Reply {
        let mut command = Command::new(
            "execution.controller.claim",
            json!({"kind": CONTROLLER, "id": host}),
            epoch,
            json!({}),
        );
        command.epoch = epoch;
        self.command(command)
    }

    /// Every controller this caller can see, and its current authority
    /// epoch (its subject revision), from the event stream alone. A
    /// controller never claimed has no events and is at epoch 0.
    pub fn controller_epochs(&mut self) -> Reply<BTreeMap<String, u64>> {
        let mut epochs = BTreeMap::new();
        let mut from = Position::Start;
        loop {
            let page = self.events_read(&from, &[CONTROLLER], 1000)?;
            let items = page["items"].as_array().cloned().unwrap_or_default();
            for item in &items {
                if let Some(event) = item.get("event") {
                    note(&mut epochs, &event["subject"], &event["revision"]);
                } else if let Some(gap) = item.get("gap") {
                    for entry in gap["snapshot"]["subjects"].as_array().into_iter().flatten() {
                        note(&mut epochs, &entry["subject"], &entry["revision"]);
                    }
                }
            }
            match page["next_cursor"].as_str() {
                Some(cursor) if !items.is_empty() => from = Position::Cursor(cursor.into()),
                _ => return Ok(epochs),
            }
        }
    }

    /// The fence for a command on one run, read now: the run's revision
    /// from `execution.inspect`, and the epoch of the controller that hosts
    /// it. Either can be overridden by a caller that holds its own.
    pub fn fence(&mut self, id: &str) -> Reply<(Fence, Value)> {
        let view = self.inspect(id)?;
        let host = view["host"]["id"].as_str().unwrap_or("").to_owned();
        let epoch = self.controller_epochs()?.get(&host).copied().unwrap_or(0);
        let revision = view["revision"].as_u64().unwrap_or(0);
        Ok((Fence { revision, epoch }, view))
    }

    /// Runs one fenced command on a run, with the fence read fresh.
    ///
    /// A working run's revision moves as its host reports, so a fence read
    /// a moment ago can be behind by the time the command lands. The
    /// service then refuses `precondition_failed` with `retry:
    /// after_reconcile`, and this reads the fence again and retries, at
    /// most `attempts` times in all. **What the caller pinned is never
    /// replaced**: a pinned revision or epoch that is stale is refused and
    /// that refusal is returned as it came. No other refusal is retried.
    pub fn fenced(
        &mut self,
        id: &str,
        pinned: Pinned,
        attempts: u32,
        mut command: impl FnMut(&mut Self, Fence, &Value) -> Reply,
    ) -> Reply {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let (mut fence, view) = self.fence(id)?;
            fence.revision = pinned.revision.unwrap_or(fence.revision);
            fence.epoch = pinned.epoch.unwrap_or(fence.epoch);
            match command(self, fence, &view) {
                Err(Failure::Refused(refusal))
                    if refusal.code() == "precondition_failed"
                        && refusal.error["data"]["retry"] == "after_reconcile"
                        && pinned.revision.is_none()
                        && attempt < attempts =>
                {
                    continue;
                }
                other => return other,
            }
        }
    }
}

/// A fence the caller holds and wants used as it is, stale or not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pinned {
    pub revision: Option<u64>,
    pub epoch: Option<u64>,
}

fn note(epochs: &mut BTreeMap<String, u64>, subject: &Value, revision: &Value) {
    if subject["kind"] != CONTROLLER {
        return;
    }
    if let (Some(id), Some(revision)) = (subject["id"].as_str(), revision.as_u64()) {
        let entry = epochs.entry(id.to_owned()).or_insert(0);
        *entry = (*entry).max(revision);
    }
}

/// The harness's word for a person's `allow` or `deny`. The action's owner
/// (`actions[].owner`) names the harness; Codex says `accept` and
/// `decline`, the other release harnesses `allow` and `deny`. Nothing else
/// is ever produced: "always" and "for this session" are not encodable.
pub fn decision_word(owner: &str, allow: bool) -> &'static str {
    match (owner, allow) {
        ("codex", true) => "accept",
        ("codex", false) => "decline",
        (_, true) => "allow",
        (_, false) => "deny",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_digests_its_intent_exactly_as_the_python_caller_does() {
        // scripts/public_api.py: command('execution.cancel',
        // {'kind':'execution.execution','id':'run-1'}, {}, command_id='x',
        // revision=3)['command_digest']
        let mut command = Command::new("execution.cancel", execution("run-1"), 3, json!({}));
        command.command_id = "x".into();
        let envelope = command.envelope();
        assert_eq!(
            envelope["command_digest"],
            "sha256:b796b42c0e7828c679d8e4d1f6b3da2ebcb6663a87b5b9cf1ae135b75aaee0db"
        );
        assert_eq!(envelope["authority_epoch"], 0);
        assert_eq!(envelope["preconditions"][0]["revision"], 3);
    }

    #[test]
    fn content_rides_outside_the_digest() {
        let mut command = Command::new("execution.steer", execution("run-1"), 1, json!({}));
        let bare = command.envelope()["command_digest"].clone();
        command.content = Some(("text/plain".into(), "hello".into()));
        let carried = command.envelope();
        assert_eq!(carried["command_digest"], bare);
        assert_eq!(carried["extensions"][CONTENT]["text"], "hello");
    }

    #[test]
    fn only_single_use_words_are_ever_produced() {
        assert_eq!(decision_word("codex", true), "accept");
        assert_eq!(decision_word("codex", false), "decline");
        assert_eq!(decision_word("opencode", true), "allow");
        assert_eq!(decision_word("claude", false), "deny");
    }
}
