//! G3: transcript blocks as they happen, and the audit that fills them in.
//!
//! A port of `scripts/transcript_blocks.py` (`Transcript.feed`, `absorb`,
//! `fill`) and of the per-harness decoder in `scripts/proof_harness.py`,
//! checked against them on the same recorded reads by
//! `scripts/fold_parity.py`.
//!
//! Live, a tool use is visible and **where it landed is not**: it reads
//! `not yet classified`, decided by `unknown`, until the end-of-turn audit
//! on `execution.exit.observed` (`pio.combraton.dev/tool-uses`) says
//! otherwise. On a harness that produces no audit it stays that way.
use anyhow::{Result, bail};
use serde_json::{Map, Value, json};

pub const AUDIT: &str = "pio.combraton.dev/tool-uses";
pub const NOT_YET: &str = "not yet classified";

/// How a harness spools its transcript: one record per ACP
/// `session/update` (OpenCode), per stream message (Claude Code), or per
/// app-server item event (Codex).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decoder {
    SessionUpdate,
    Assistant,
    Item,
}

impl Decoder {
    /// The decoder for a harness by the name its actions carry as `owner`.
    pub fn for_harness(name: &str) -> Option<Self> {
        match name {
            "opencode" => Some(Self::SessionUpdate),
            "claude" => Some(Self::Assistant),
            "codex" => Some(Self::Item),
            _ => None,
        }
    }
}

/// One thing the screen can draw from one record.
#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    Text(String),
    Tool { id: Value, kind: Value },
}

fn text_or_empty(value: &Value) -> String {
    value.as_str().unwrap_or("").to_owned()
}

/// One spooled record into the pieces a screen would draw from it. Never a
/// placement and never a decider: neither is knowable here.
pub fn decode(decoder: Decoder, record: &Value) -> Vec<Piece> {
    match decoder {
        Decoder::SessionUpdate => {
            let update = &record["update"];
            match update["sessionUpdate"].as_str() {
                Some("agent_message_chunk") => {
                    vec![Piece::Text(text_or_empty(&update["content"]["text"]))]
                }
                Some("tool_call" | "tool_call_update") => vec![Piece::Tool {
                    id: update["toolCallId"].clone(),
                    kind: update["kind"].clone(),
                }],
                _ => vec![],
            }
        }
        Decoder::Assistant => {
            if record["type"] != "assistant" {
                return vec![];
            }
            let mut out = vec![];
            for block in record["message"]["content"]
                .as_array()
                .into_iter()
                .flatten()
            {
                match block["type"].as_str() {
                    Some("text") => out.push(Piece::Text(text_or_empty(&block["text"]))),
                    Some("tool_use") => out.push(Piece::Tool {
                        id: block["id"].clone(),
                        kind: block["name"].clone(),
                    }),
                    _ => {}
                }
            }
            out
        }
        Decoder::Item => match record["method"].as_str() {
            Some("item/agentMessage/delta") => {
                vec![Piece::Text(text_or_empty(&record["params"]["delta"]))]
            }
            Some("item/completed") => vec![Piece::Tool {
                id: record["params"]["item"]["id"].clone(),
                kind: record["params"]["item"]["type"].clone(),
            }],
            _ => vec![],
        },
    }
}

/// A tool use id as the Python fold keys it (`json.dumps` of a dict key).
fn key_of(id: &Value) -> String {
    match id {
        Value::String(text) => text.clone(),
        Value::Null => "null".into(),
        other => other.to_string(),
    }
}

/// The screen's model of one run's transcript, built only from reads.
#[derive(Clone, Debug)]
pub struct Transcript {
    pub decoder: Decoder,
    /// The next offset to read from: a live reader never re-reads.
    pub offset: u64,
    tail: Vec<u8>,
    /// Blocks in order: `["text", text]` or `["tool", id]`.
    pub blocks: Vec<Value>,
    /// Tool use id -> what the screen can say about it now, in the order
    /// the uses first appeared.
    tools: Vec<(String, Map<String, Value>)>,
}

impl Transcript {
    pub fn new(decoder: Decoder) -> Self {
        Self {
            decoder,
            offset: 0,
            tail: vec![],
            blocks: vec![],
            tools: vec![],
        }
    }

    /// Bytes as they were read, in order. A line cut across two reads waits
    /// for its end.
    pub fn feed(&mut self, data: &[u8]) -> Result<()> {
        self.tail.extend_from_slice(data);
        let Some(last) = self.tail.iter().rposition(|b| *b == b'\n') else {
            return Ok(());
        };
        let rest = self.tail.split_off(last + 1);
        let complete = std::mem::replace(&mut self.tail, rest);
        for line in complete.split(|b| *b == b'\n') {
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let record: Value = serde_json::from_slice(line)?;
            self.absorb(&record);
        }
        Ok(())
    }

    /// One spooled record becomes blocks on the screen.
    pub fn absorb(&mut self, record: &Value) {
        for piece in decode(self.decoder, record) {
            match piece {
                Piece::Text(text) => self.blocks.push(json!(["text", text])),
                Piece::Tool { id, kind } => {
                    let key = key_of(&id);
                    if !self.tools.iter().any(|(k, _)| *k == key) {
                        let mut seen = Map::new();
                        seen.insert("tool_use_id".into(), id.clone());
                        seen.insert("kind".into(), kind.clone());
                        seen.insert("placement".into(), NOT_YET.into());
                        seen.insert("decided_by".into(), "unknown".into());
                        self.tools.push((key.clone(), seen));
                    }
                    if !self
                        .blocks
                        .iter()
                        .any(|b| b[0] == "tool" && key_of(&b[1]) == key)
                    {
                        self.blocks.push(json!(["tool", id]));
                    }
                    if truthy_kind(&kind)
                        && let Some((_, seen)) = self.tools.iter_mut().find(|(k, _)| *k == key)
                    {
                        seen.insert("kind".into(), kind);
                    }
                }
            }
        }
    }

    /// The end-of-turn audit lands, and the blanks are filled in place.
    /// Anything the screen said while the run was working has to survive
    /// it; a contradiction is an error, never silently overwritten.
    pub fn fill(&mut self, audit: &Value) -> Result<()> {
        for row in audit["audit"]["tool_uses"].as_array().into_iter().flatten() {
            let key = key_of(&row["tool_use_id"]);
            let Some((_, seen)) = self.tools.iter_mut().find(|(k, _)| *k == key) else {
                continue;
            };
            let said = seen["placement"].clone();
            if said != NOT_YET && said != row["placement"] {
                bail!(
                    "the screen said {said} while the run was working; the audit says {}",
                    row["placement"]
                );
            }
            seen.insert("placement".into(), row["placement"].clone());
            let decided = match &row["decided_by"] {
                Value::Null => json!("nobody was asked"),
                Value::String(s) if s.is_empty() => json!("nobody was asked"),
                other => other.clone(),
            };
            seen.insert("decided_by".into(), decided);
            seen.insert("outcome".into(), row["outcome"].clone());
        }
        Ok(())
    }

    /// Fills from an `execution.exit.observed` event, if it carries the
    /// audit. Returns whether it did: a harness with no audit leaves every
    /// placement `not yet classified`, and the screen says so.
    pub fn fill_from_exit(&mut self, event: &Value) -> Result<bool> {
        match event["payload"].get(AUDIT) {
            Some(audit) => self.fill(audit).map(|()| true),
            None => Ok(false),
        }
    }

    /// Tool uses in the order they first appeared.
    pub fn tools(&self) -> impl Iterator<Item = &Map<String, Value>> {
        self.tools.iter().map(|(_, seen)| seen)
    }

    /// What the screen shows.
    pub fn view(&self) -> Value {
        let tools: Map<String, Value> = self
            .tools
            .iter()
            .map(|(key, seen)| (key.clone(), Value::Object(seen.clone())))
            .collect();
        json!({"blocks": self.blocks, "tools": tools})
    }
}

fn truthy_kind(kind: &Value) -> bool {
    match kind {
        Value::Null => false,
        Value::String(s) => !s.is_empty(),
        Value::Bool(b) => *b,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_cut_across_two_reads_waits_for_its_end() {
        let mut t = Transcript::new(Decoder::SessionUpdate);
        let line = br#"{"update":{"sessionUpdate":"agent_message_chunk","content":{"text":"hi"}}}"#;
        t.feed(&line[..10]).unwrap();
        assert!(t.blocks.is_empty());
        t.feed(&line[10..]).unwrap();
        assert!(t.blocks.is_empty(), "no newline yet");
        t.feed(b"\n").unwrap();
        assert_eq!(t.blocks, [json!(["text", "hi"])]);
    }

    #[test]
    fn a_tool_use_is_unplaced_until_the_audit_and_a_guess_is_refused() {
        let mut t = Transcript::new(Decoder::Assistant);
        t.absorb(&json!({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "id": "t1", "name": "Bash"}]}}));
        let seen = t.tools().next().unwrap().clone();
        assert_eq!(seen["placement"], NOT_YET);
        assert_eq!(seen["decided_by"], "unknown");
        let audit = json!({"audit": {"tool_uses": [
            {"tool_use_id": "t1", "placement": "outside_fixture", "decided_by": null,
             "outcome": "performed"}]}});
        t.fill(&audit).unwrap();
        let seen = t.tools().next().unwrap();
        assert_eq!(seen["placement"], "outside_fixture");
        assert_eq!(seen["decided_by"], "nobody was asked");
        // A screen that had guessed `inside` is contradicted, loudly.
        let mut guessed = Transcript::new(Decoder::Assistant);
        guessed.absorb(&json!({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "id": "t1", "name": "Bash"}]}}));
        guessed.tools[0]
            .1
            .insert("placement".into(), "inside".into());
        assert!(guessed.fill(&audit).is_err());
    }
}
