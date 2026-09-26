//! The feed: the one place the screen talks to the service, and only through
//! `pio_client`.
//!
//! It runs on its own thread, owns the session, and hands the screen a
//! [`Snapshot`] whenever something moved. Each cycle reads the event stream
//! from the board's cursor (`core.events.read`), inspects only the runs whose
//! revision moved (the T1 fold), reads the watched runs' transcripts from
//! where it left off (`execution.output.read`), and fills the end-of-turn
//! audit in place once a run has exited and all of its transcript is read.
//! Between cycles it waits on the subscription (`core.events.subscribe`), so
//! a change is drawn as soon as it is pushed; the read is what delivers, so
//! a push that races it loses nothing.
use crate::model::{Audit, Block, Snapshot, Transcript, word};
use pio_client::blocks::{self, Decoder};
use pio_client::board::Board;
use pio_client::client::EXECUTION;
use pio_client::walk::{self, Order};
use pio_client::{Client, Failure, Pinned, Position, Reply};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

/// What the screen asks of the feed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Want {
    /// Read these runs' transcripts: the previewed run and the open one.
    Watch(Vec<String>),
    /// Cancel one run, confirmed by the person. Fenced and sent once.
    Cancel(String),
    Stop,
}

/// Only these events are kept: the walk and the audit need nothing else,
/// and the board keeps its own positions.
const KEPT: [&str; 3] = [
    "execution.runtime.changed",
    "execution.action.answered",
    "execution.exit.observed",
];

/// One watched run's reader.
#[derive(Default)]
struct Reader {
    /// Chosen from the run's action owner, or from the first record's own
    /// shape; `None` for a record that is not a harness transcript (the
    /// labeled fake host prints `{"kind":"output","text":...}`).
    decoder: Option<Decoder>,
    decided: bool,
    who: String,
    transcript: Option<blocks::Transcript>,
    tail: Vec<u8>,
    plain: Vec<Block>,
    /// Tool use id -> (label, harness status), from the records themselves.
    labels: BTreeMap<String, (String, String)>,
    shown: Transcript,
}

pub struct Feed {
    client: Client,
    board: Board,
    events: Vec<Value>,
    exits: BTreeMap<String, Value>,
    first_seen: BTreeMap<String, i64>,
    gaps: usize,
    readers: BTreeMap<String, Reader>,
    watched: Vec<String>,
    subscription: Option<String>,
    snapshot: Snapshot,
}

/// The service's name and version, as `core.describe` gave them when the
/// session opened.
pub fn service(client: &Client) -> String {
    let described = &client.described;
    format!(
        "{} {}",
        word(&described["provider"]["name"]),
        word(&described["provider"]["version"])
    )
}

impl Feed {
    pub fn new(client: Client) -> Self {
        let snapshot = Snapshot {
            service: service(&client),
            principal: client.principal().to_owned(),
            walk_complete: true,
            ..Snapshot::default()
        };
        Self {
            client,
            board: Board::default(),
            events: vec![],
            exits: BTreeMap::new(),
            first_seen: BTreeMap::new(),
            gaps: 0,
            readers: BTreeMap::new(),
            watched: vec![],
            subscription: None,
            snapshot,
        }
    }

    /// The feed's thread. Returns when the screen says stop or goes away,
    /// or when the service stops answering (said on the screen first).
    pub fn run(mut self, wants: Receiver<Want>, out: Sender<Snapshot>) {
        self.snapshot.harnesses = self.harnesses();
        let mut first = true;
        loop {
            let mut asked = false;
            loop {
                match wants.try_recv() {
                    Ok(Want::Watch(ids)) => {
                        self.watched = ids;
                        asked = true;
                    }
                    Ok(Want::Cancel(id)) => {
                        self.snapshot.notice = Some(self.cancel(&id));
                        asked = true;
                    }
                    Ok(Want::Stop) | Err(TryRecvError::Disconnected) => {
                        self.leave();
                        return;
                    }
                    Err(TryRecvError::Empty) => break,
                }
            }
            match self.cycle() {
                Ok(changed) => {
                    if (changed || asked || first) && out.send(self.assemble()).is_err() {
                        self.leave();
                        return;
                    }
                }
                Err(error) => {
                    self.snapshot.live = false;
                    self.snapshot.error = Some(error.to_string());
                    let _ = out.send(self.assemble());
                    return;
                }
            }
            first = false;
            if self.subscription.is_none() {
                // Opened after the first read, from `now`: anything between
                // the two is read from the cursor on the next cycle anyway.
                if let Ok(opened) = self.client.events_subscribe(&Position::Now, &[EXECUTION]) {
                    self.subscription = opened["subscription"].as_str().map(str::to_owned);
                    self.snapshot.live = self.subscription.is_some();
                    let _ = out.send(self.assemble());
                }
            }
            // Woken by a push, or after a short wait regardless: the
            // transcripts are not on the event stream.
            if let Err(error) = self.client.notification(Duration::from_millis(250)) {
                self.snapshot.live = false;
                self.snapshot.error = Some(error.to_string());
                let _ = out.send(self.assemble());
                return;
            }
        }
    }

    fn leave(&mut self) {
        if let Some(subscription) = self.subscription.take() {
            let _ = self.client.events_unsubscribe(&subscription);
        }
    }

    fn harnesses(&mut self) -> Vec<String> {
        let Ok(found) = self.client.discovery() else {
            return vec![];
        };
        found["installations"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|i| {
                let mark = if i["version_supported"] == "yes" {
                    "\u{2713}"
                } else {
                    "\u{25c7}"
                };
                match i["version"].as_str() {
                    Some(version) => format!("{} {version} {mark}", word(&i["installation_id"])),
                    None => format!("{} {mark}", word(&i["installation_id"])),
                }
            })
            .collect()
    }

    /// One cycle. Returns whether anything the screen draws moved.
    fn cycle(&mut self) -> Reply<bool> {
        let mut changed = false;
        // Every cycle reads the stream from the board's cursor: new runs and
        // every change to a known one arrive here, pushed or not.
        changed |= self.read_events()?;
        for id in self.watched.clone() {
            changed |= self.read_output(&id)?;
        }
        Ok(changed)
    }

    /// The stream from the board's cursor to its end; the runs whose
    /// revision moved are inspected, and nothing else.
    fn read_events(&mut self) -> Reply<bool> {
        let mut changed = false;
        loop {
            let from = match &self.board.cursor {
                Some(cursor) => Position::Cursor(cursor.clone()),
                None => Position::Start,
            };
            let page = self.client.events_read(&from, &[EXECUTION], 1000)?;
            let moved = self.board.absorb(&page);
            let items = page["items"].as_array().cloned().unwrap_or_default();
            for item in &items {
                self.remember(item);
            }
            if !moved.is_empty() {
                self.board.draw(&mut self.client, &moved)?;
                changed = true;
            }
            if items.is_empty() {
                return Ok(changed);
            }
        }
    }

    fn remember(&mut self, item: &Value) {
        if item.get("gap").is_some() {
            self.gaps += 1;
            return;
        }
        let Some(event) = item.get("event") else {
            return;
        };
        let Some(id) = event["subject"]["id"].as_str() else {
            return;
        };
        if let Some(at) = event["recorded_at"]
            .as_str()
            .and_then(walk::epoch_seconds)
        {
            self.first_seen.entry(id.to_owned()).or_insert(at);
        }
        let kind = event["type"].as_str().unwrap_or("");
        if !KEPT.contains(&kind) {
            return;
        }
        if kind == "execution.exit.observed" {
            self.exits.insert(id.to_owned(), event.clone());
        }
        self.events.push(event.clone());
    }

    /// A watched run's transcript, from where the last read ended.
    fn read_output(&mut self, id: &str) -> Reply<bool> {
        let view = self.board.drawn.get(id).cloned();
        let reader = self.readers.entry(id.to_owned()).or_default();
        if reader.shown.audit != Audit::NotYet || reader.shown.problem.is_some() {
            return Ok(false);
        }
        let mut changed = false;
        for _ in 0..8 {
            let chunk = match self.client.output_read(id, reader.shown.offset, 65536) {
                Ok(chunk) => chunk,
                Err(Failure::Refused(refusal)) => {
                    reader.shown.problem = Some(format!(
                        "the transcript is not readable here: {}",
                        refusal.code()
                    ));
                    return Ok(true);
                }
                Err(error) => return Err(error),
            };
            if chunk.coverage != reader.shown.coverage || chunk.end_offset != reader.shown.end {
                changed = true;
            }
            reader.shown.coverage = chunk.coverage.clone();
            reader.shown.end = chunk.end_offset;
            if chunk.data.is_empty() {
                break;
            }
            reader.shown.offset = chunk.next_offset;
            reader.ingest(&chunk.data, view.as_ref());
            changed = true;
            if chunk.next_offset >= chunk.end_offset {
                break;
            }
        }
        // The audit, once: the run has exited, everything spooled is read,
        // and the event that carries it is in hand. A harness that produces
        // none leaves every placement `not yet classified`, and says so.
        let exited = view.as_ref().is_some_and(|v| v["runtime"] == "exited");
        if exited
            && reader.shown.offset >= reader.shown.end
            && let Some(event) = self.exits.get(id)
        {
            reader.flush();
            reader.shown.audit = match reader.transcript.as_mut() {
                Some(transcript) => match transcript.fill_from_exit(event) {
                    Ok(true) => Audit::Filled,
                    Ok(false) => Audit::Absent,
                    Err(error) => {
                        reader.shown.problem = Some(error.to_string());
                        Audit::NotYet
                    }
                },
                None if event["payload"].get(blocks::AUDIT).is_some() => Audit::Filled,
                None => Audit::Absent,
            };
            reader.shown.containment =
                event["payload"][blocks::AUDIT]["audit"]["containment"].clone();
            changed = true;
        }
        if changed {
            reader.shown.blocks = reader.blocks();
        }
        Ok(changed)
    }

    /// One cancel, fenced at the run's revision and its controller's epoch,
    /// read fresh. What the service said is what the screen says.
    fn cancel(&mut self, id: &str) -> String {
        let reply = self
            .client
            .fenced(id, Pinned::default(), 3, |client, fence, _| {
                client.cancel(id, fence, Some("pio tui"))
            });
        match reply {
            Ok(result) => format!(
                "cancel {} for {id}; its outcome shows on the run when the host reports it",
                word(&result["outcome"]["receipt"]["state"])
            ),
            Err(Failure::Refused(refusal)) => format!(
                "cancel for {id} refused by the service: {}",
                serde_json::to_string(&refusal.error).unwrap_or_default()
            ),
            Err(Failure::Transport(error)) => {
                format!("cancel for {id}: the outcome is not known ({error}); watch the run")
            }
        }
    }

    fn assemble(&mut self) -> Snapshot {
        let mut waiting = walk::walk(&self.events, Order::Deadline, None);
        // After a retention gap the walk may be missing a request whose
        // event is gone; the views still list it pending (T1's rule).
        let complete = self.gaps == 0;
        if !complete {
            let lost = walk::lost_to_retention(&waiting, &self.board.drawn);
            waiting.extend(lost);
        }
        let mut snapshot = self.snapshot.clone();
        snapshot.board = self.board.view();
        snapshot.views = self.board.drawn.clone();
        snapshot.first_seen = self.first_seen.clone();
        snapshot.waiting = waiting;
        snapshot.walk_complete = complete;
        snapshot.transcripts = self
            .readers
            .iter()
            .map(|(id, reader)| (id.clone(), reader.shown.clone()))
            .collect();
        self.snapshot.notice = None;
        snapshot
    }
}

impl Reader {
    fn ingest(&mut self, data: &[u8], view: Option<&Value>) {
        self.tail.extend_from_slice(data);
        let Some(last) = self.tail.iter().rposition(|b| *b == b'\n') else {
            return;
        };
        let rest = self.tail.split_off(last + 1);
        let complete = std::mem::replace(&mut self.tail, rest);
        for line in complete.split(|b| *b == b'\n') {
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            match serde_json::from_slice::<Value>(line) {
                Ok(record) => self.absorb(&record, view),
                Err(_) => self.plain.push(Block::Text {
                    who: "output".into(),
                    text: String::from_utf8_lossy(line).into_owned(),
                }),
            }
        }
    }

    /// A last line with no newline yet stays in the tail until it ends; at
    /// the end of a run there is nothing more coming, so it is shown.
    fn flush(&mut self) {
        let tail = std::mem::take(&mut self.tail);
        if !tail.iter().all(u8::is_ascii_whitespace) {
            self.ingest(&[tail, b"\n".to_vec()].concat(), None);
        }
    }

    fn absorb(&mut self, record: &Value, view: Option<&Value>) {
        if !self.decided {
            self.decided = true;
            let owner = view
                .and_then(|v| v["actions"].as_array())
                .and_then(|a| a.iter().find_map(|a| a["owner"].as_str()))
                .or_else(|| view.and_then(|v| v["runtime_detail"]["owner"].as_str()));
            let (decoder, who) = match owner.and_then(Decoder::for_harness) {
                Some(decoder) => (Some(decoder), owner.unwrap_or_default().to_owned()),
                None => sniff(record),
            };
            self.decoder = decoder;
            self.who = who;
            self.transcript = decoder.map(blocks::Transcript::new);
        }
        match (self.decoder, self.transcript.as_mut()) {
            (Some(decoder), Some(transcript)) => {
                transcript.absorb(record);
                if let Some((id, label, status)) = tool_label(decoder, record) {
                    let entry = self.labels.entry(id).or_default();
                    if !label.is_empty() {
                        entry.0 = label;
                    }
                    if !status.is_empty() {
                        entry.1 = status;
                    }
                }
            }
            _ => {
                let text = record["text"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| record.to_string());
                let who = record["source"].as_str().unwrap_or("output").to_owned();
                self.plain.push(Block::Text { who, text });
            }
        }
    }

    /// The blocks in order: consecutive text from one speaker is one block.
    fn blocks(&self) -> Vec<Block> {
        let Some(transcript) = &self.transcript else {
            return merge(self.plain.clone(), "\n");
        };
        let view = transcript.view();
        let mut out = vec![];
        for block in view["blocks"].as_array().into_iter().flatten() {
            if block[0] == "text" {
                out.push(Block::Text {
                    who: self.who.clone(),
                    text: block[1].as_str().unwrap_or("").to_owned(),
                });
                continue;
            }
            let key = key_of(&block[1]);
            let seen = &view["tools"][&key];
            let (label, status) = self.labels.get(&key).cloned().unwrap_or_default();
            out.push(Block::Tool {
                id: key,
                kind: seen["kind"].as_str().unwrap_or("tool").to_owned(),
                label,
                status,
                placement: word(&seen["placement"]),
                decided_by: word(&seen["decided_by"]),
                outcome: seen["outcome"].as_str().unwrap_or("").to_owned(),
            });
        }
        // Chunks of one message are pieces of one text (OpenCode and Codex
        // stream them); whole messages (Claude Code) are paragraphs.
        let join = if self.decoder == Some(Decoder::Assistant) {
            "\n"
        } else {
            ""
        };
        merge(out, join)
    }
}

fn merge(blocks: Vec<Block>, join: &str) -> Vec<Block> {
    let mut out: Vec<Block> = vec![];
    for block in blocks {
        if let (
            Some(Block::Text { who, text }),
            Block::Text {
                who: next,
                text: more,
            },
        ) = (out.last_mut(), &block)
            && who == next
        {
            text.push_str(join);
            text.push_str(more);
            continue;
        }
        out.push(block);
    }
    out
}

/// A tool use id as the fold keys it.
fn key_of(id: &Value) -> String {
    match id {
        Value::String(text) => text.clone(),
        Value::Null => "null".into(),
        other => other.to_string(),
    }
}

/// The decoder a record's own shape names, when no action owner does.
fn sniff(record: &Value) -> (Option<Decoder>, String) {
    if record["type"] == "session_update" || record.get("update").is_some() {
        return (Some(Decoder::SessionUpdate), "opencode".into());
    }
    if record.get("method").is_some() {
        return (Some(Decoder::Item), "codex".into());
    }
    match record["type"].as_str() {
        Some("assistant" | "user" | "system" | "result" | "stream_event") => {
            (Some(Decoder::Assistant), "claude".into())
        }
        _ => (None, String::new()),
    }
}

/// What a tool use was aimed at, from the record that announced it: the
/// title and target the harness itself gave. Never a placement.
fn tool_label(decoder: Decoder, record: &Value) -> Option<(String, String, String)> {
    let target = |input: &Value| {
        ["command", "file_path", "filePath", "path", "pattern", "url"]
            .iter()
            .find_map(|k| input[*k].as_str().map(str::to_owned))
            .unwrap_or_default()
    };
    let join = |a: &str, b: &str| match (a.is_empty(), b.is_empty()) {
        (false, false) => format!("{a}: {b}"),
        (false, true) => a.to_owned(),
        _ => b.to_owned(),
    };
    match decoder {
        Decoder::SessionUpdate => {
            let update = &record["update"];
            let id = update["toolCallId"].as_str()?.to_owned();
            let title = update["title"].as_str().unwrap_or("");
            let label = join(title, &target(&update["rawInput"]));
            Some((id, label, word_or_empty(&update["status"])))
        }
        Decoder::Assistant => {
            let block = record["message"]["content"]
                .as_array()?
                .iter()
                .find(|b| b["type"] == "tool_use")?;
            Some((
                block["id"].as_str()?.to_owned(),
                target(&block["input"]),
                String::new(),
            ))
        }
        Decoder::Item => {
            if record["method"] != "item/completed" {
                return None;
            }
            let item = &record["params"]["item"];
            let label = match item["command"].as_str() {
                Some(command) => command.to_owned(),
                None => item["changes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|c| c["path"].as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            };
            Some((
                item["id"].as_str()?.to_owned(),
                label,
                word_or_empty(&item["status"]),
            ))
        }
    }
}

fn word_or_empty(value: &Value) -> String {
    value.as_str().unwrap_or("").to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn line(value: Value) -> Vec<u8> {
        let mut bytes = serde_json::to_vec(&value).unwrap();
        bytes.push(b'\n');
        bytes
    }

    #[test]
    fn an_opencode_tool_use_is_labelled_and_left_unplaced() {
        let mut reader = Reader::default();
        let view = json!({"actions": [{"owner": "opencode", "state": "pending"}]});
        reader.ingest(
            &line(json!({"type": "session_update", "update": {
                "sessionUpdate": "tool_call", "toolCallId": "c1", "kind": "execute",
                "title": "run a command", "rawInput": {}, "status": "pending"}})),
            Some(&view),
        );
        reader.ingest(
            &line(json!({"type": "session_update", "update": {
                "sessionUpdate": "tool_call_update", "toolCallId": "c1", "kind": "execute",
                "title": "run a command", "rawInput": {"command": "git tag x"},
                "status": "in_progress"}})),
            Some(&view),
        );
        for chunk in ["fake ", "turn"] {
            reader.ingest(
                &line(json!({"type": "session_update", "update": {
                    "sessionUpdate": "agent_message_chunk", "content": {"text": chunk}}})),
                Some(&view),
            );
        }
        let blocks = reader.blocks();
        assert_eq!(blocks.len(), 2, "{blocks:?}");
        match &blocks[0] {
            Block::Tool {
                label,
                placement,
                decided_by,
                status,
                ..
            } => {
                assert_eq!(label, "run a command: git tag x");
                assert_eq!(placement, pio_client::blocks::NOT_YET);
                assert_eq!(decided_by, "unknown");
                assert_eq!(status, "in_progress");
            }
            other => panic!("not a tool: {other:?}"),
        }
        assert_eq!(
            blocks[1],
            Block::Text {
                who: "opencode".into(),
                text: "fake turn".into()
            }
        );
    }

    #[test]
    fn a_record_that_is_no_transcript_is_shown_as_it_is() {
        let mut reader = Reader::default();
        reader.ingest(
            &line(json!({"kind": "output", "source": "fake-host", "text": "started"})),
            None,
        );
        reader.ingest(b"{\"kind\":\"output\",\"source\":\"fake-host\",\"text\":\"ended\"}", None);
        assert_eq!(reader.blocks().len(), 1, "the unfinished line waits");
        reader.flush();
        assert_eq!(
            reader.blocks(),
            [Block::Text {
                who: "fake-host".into(),
                text: "started\nended".into()
            }]
        );
    }
}
