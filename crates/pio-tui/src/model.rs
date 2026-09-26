//! What the screen knows, and the words it says it in.
//!
//! Every word here is a recorded observation or says that there is none
//! (M4 rule 2). A run's state is the fold's own word
//! ([`pio_client::board::run_state`]), never recomputed here; unresolved usage
//! is a marker beside it, never the state (the orchestrator's decision of
//! 2026-09-26).
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// One block of a run's transcript as the screen draws it.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    /// What the harness said, or what the run printed.
    Text { who: String, text: String },
    /// One tool use. `placement` and `decided_by` are what a recorded
    /// observation says: `not yet classified` and `unknown` until the
    /// end-of-turn audit, and for ever on a harness that produces none.
    Tool {
        id: String,
        kind: String,
        label: String,
        status: String,
        placement: String,
        decided_by: String,
        outcome: String,
    },
}

/// Whether the end-of-turn audit has filled the blanks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Audit {
    /// The run has not exited, or its transcript is not all read yet.
    #[default]
    NotYet,
    /// `execution.exit.observed` carried the audit, and it is filled in.
    Filled,
    /// The run exited and its harness produced no audit: every placement
    /// stays `not yet classified`, and the screen says why.
    Absent,
}

/// One run's transcript, as far as it has been read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transcript {
    pub blocks: Vec<Block>,
    /// The next byte to read, and the last byte the service holds.
    pub offset: u64,
    pub end: u64,
    /// `complete`, or what the service said was lost.
    pub coverage: String,
    pub audit: Audit,
    /// The audit's containment record, when it came.
    pub containment: Value,
    /// A refusal or an unreadable record, said rather than hidden.
    pub problem: Option<String>,
}

/// Everything the screen draws, assembled by the feed from the public API.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    /// `core.describe`'s provider name and version.
    pub service: String,
    /// `execution.discovery.list`, one entry per installation.
    pub harnesses: Vec<String>,
    pub principal: String,
    /// `pio_client::board::Board::view()`.
    pub board: Value,
    /// Execution id -> the view last inspected.
    pub views: BTreeMap<String, Value>,
    /// Execution id -> when its first event was recorded (Unix seconds).
    pub first_seen: BTreeMap<String, i64>,
    /// Every request still waiting (the walk), soonest due first.
    pub waiting: Vec<Value>,
    /// False after a retention gap: the walk was completed from the views.
    pub walk_complete: bool,
    pub transcripts: BTreeMap<String, Transcript>,
    /// A subscription is open, so a change is pushed as it happens.
    pub live: bool,
    /// The feed stopped: what is shown is as it was then.
    pub error: Option<String>,
    /// What the last write said (a cancel).
    pub notice: Option<String>,
}

impl Snapshot {
    pub fn rows(&self) -> Vec<Value> {
        self.board["runs"].as_array().cloned().unwrap_or_default()
    }

    pub fn row(&self, id: &str) -> Option<Value> {
        self.rows().into_iter().find(|r| r["id"] == id)
    }

    pub fn count(&self, state: &str) -> u64 {
        self.board["counts"][state].as_u64().unwrap_or(0)
    }

    /// The requests waiting on one run.
    pub fn waiting_on(&self, id: &str) -> Vec<&Value> {
        self.waiting.iter().filter(|w| w["run"] == id).collect()
    }
}

/// The word a row shows: the fold's own, never recomputed here.
pub fn state_of(row: &Value) -> &str {
    row["state"].as_str().unwrap_or("unknown")
}

pub fn glyph(state: &str) -> &'static str {
    pio_client::board::glyph(state)
}

/// Any JSON value as a word; nothing recorded reads `unknown`.
pub fn word(value: &Value) -> String {
    match value {
        Value::Null => "unknown".into(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

pub fn exit_word(exit: &Value) -> String {
    if let Some(code) = exit["code"].as_i64() {
        return format!("exit {code}");
    }
    if let Some(signal) = exit["signal"].as_str() {
        return format!("signal {signal}");
    }
    match exit {
        Value::String(text) => format!("exit {text}"),
        _ => "exit unknown".into(),
    }
}

/// Where a tool use landed, in the screen's words.
pub fn placement_word(placement: &str) -> String {
    match placement {
        "inside_fixture" => "inside workspace".into(),
        "outside_fixture" => "outside workspace".into(),
        "not_classifiable" => "not classifiable".into(),
        other => other.replace('_', " "),
    }
}

/// Who decided a tool use. `caller` is whoever answered through the public
/// API, which need not be the person at this screen.
pub fn decider_word(decided_by: &str) -> String {
    match decided_by {
        "caller" => "decided by the caller".into(),
        "pio" => "decided by PIO".into(),
        "harness" => "decided by the harness".into(),
        "nobody was asked" => "nobody was asked".into(),
        "unknown" | "" => "decider unknown".into(),
        other => format!("decided by {other}"),
    }
}

/// A placement or a decider that no record has settled yet.
pub fn unsettled(text: &str) -> bool {
    text == pio_client::blocks::NOT_YET || text == "unknown" || text == "decider unknown"
}

/// The run's usage, from `usage.observations`. **Never zero when nothing
/// was observed**: that is `tokens unknown`.
pub fn tokens(view: Option<&Value>) -> String {
    let observations = view
        .and_then(|v| v["usage"]["observations"].as_array())
        .cloned()
        .unwrap_or_default();
    let counted: Vec<&Value> = observations
        .iter()
        .filter(|o| o["measure"].as_str().is_some_and(|m| m.contains("token")))
        .collect();
    if counted.is_empty() {
        return "tokens unknown".into();
    }
    let total: u64 = counted.iter().filter_map(|o| o["amount"].as_u64()).sum();
    let estimated = counted.iter().any(|o| o["basis"] != "measured");
    format!("{}{} tokens", if estimated { "~" } else { "" }, short(total))
}

/// 64300 -> 64.3k.
pub fn short(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{:.1}k", n as f64 / 1_000.0),
        _ => format!("{:.1}M", n as f64 / 1_000_000.0),
    }
}

/// Why a run's outcome is in doubt, in its own recorded words.
pub fn doubt(view: Option<&Value>) -> String {
    let Some(view) = view else {
        return "on the stream, not read yet".into();
    };
    let mut why = vec![];
    if view["runtime"] == "unknown" {
        why.push("host lost".to_owned());
    }
    if view["delivery"] == "ambiguous" {
        why.push("delivery ambiguous".to_owned());
    }
    if view["runtime"] == "exited" && view["exit"] == "unavailable" {
        why.push("no exit status and no result".to_owned());
    }
    if why.is_empty() {
        why.push(format!(
            "runtime {} · delivery {}",
            word(&view["runtime"]),
            word(&view["delivery"])
        ));
    }
    why.join(" · ")
}

/// What a waiting request asks for, in a few words.
pub fn asks(row: &Value) -> String {
    for key in ["command", "message", "method"] {
        if let Some(text) = row[key].as_str() {
            return text.to_owned();
        }
    }
    let classification = &row["classification"];
    let tool = classification["tool_name"].as_str().unwrap_or("");
    let target = classification["target_label"].as_str().unwrap_or("");
    let text = format!("{tool} {target}").trim().to_owned();
    if text.is_empty() {
        word(&row["action_id"])
    } else {
        text
    }
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 42 -> 42s, 150 -> 2m, 7200 -> 2h.
pub fn age(seconds: i64) -> String {
    let seconds = seconds.max(0);
    match seconds {
        0..=59 => format!("{seconds}s"),
        60..=3_599 => format!("{}m", seconds / 60),
        3_600..=86_399 => format!("{}h", seconds / 3_600),
        _ => format!("{}d", seconds / 86_400),
    }
}

/// 78 -> 01:18.
pub fn clock(seconds: i64) -> String {
    let seconds = seconds.max(0);
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn usage_never_observed_is_unknown_and_never_zero() {
        let view = json!({"usage": {"liability": "unresolved", "observations": []}});
        assert_eq!(tokens(Some(&view)), "tokens unknown");
        assert_eq!(tokens(None), "tokens unknown");
        let view = json!({"usage": {"observations": [
            {"amount": 64300, "basis": "measured", "measure": "claude.tokens.total"}]}});
        assert_eq!(tokens(Some(&view)), "64.3k tokens");
        let view = json!({"usage": {"observations": [
            {"amount": 256, "basis": "estimated", "measure": "opencode.tokens.total"}]}});
        assert_eq!(tokens(Some(&view)), "~256 tokens");
    }

    #[test]
    fn the_state_word_is_the_folds_own() {
        let row = json!({"state": "finished", "markers": ["usage unresolved"]});
        assert_eq!(state_of(&row), "finished");
        assert_eq!(state_of(&json!({})), "unknown");
    }

    #[test]
    fn doubt_names_what_is_in_doubt() {
        assert_eq!(doubt(Some(&json!({"runtime": "unknown"}))), "host lost");
        assert_eq!(
            doubt(Some(&json!({"runtime": "active", "delivery": "ambiguous"}))),
            "delivery ambiguous"
        );
        assert_eq!(doubt(None), "on the stream, not read yet");
    }

    #[test]
    fn words_for_placement_and_decider() {
        assert_eq!(placement_word("inside_fixture"), "inside workspace");
        assert_eq!(placement_word("not_classifiable"), "not classifiable");
        assert_eq!(decider_word("caller"), "decided by the caller");
        assert_eq!(decider_word("nobody was asked"), "nobody was asked");
        assert_eq!(decider_word("unknown"), "decider unknown");
        assert!(unsettled(pio_client::blocks::NOT_YET));
    }
}
