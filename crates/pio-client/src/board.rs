//! G1 and G6: the board, drawn from the events fold.
//!
//! A port of `scripts/board_fold.py` (`Board.absorb`, `run_state`,
//! `board_view`), checked against it on the same recorded pages by
//! `scripts/fold_parity.py`. Nothing lists the executions a caller may see;
//! `core.events.read {from: start, kinds: [execution.execution]}` does,
//! because every execution's first event is visible to a grant that can read
//! it. `execution.inspect` is then called only for subjects whose revision
//! moved, so a settled run is never read again.
use crate::client::{Client, EXECUTION, Position};
use crate::wire::Reply;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// The design's one vocabulary: a glyph beside every word.
pub fn glyph(state: &str) -> &'static str {
    match state {
        "needs approval" => "\u{25d0}",
        "uncertain" | "unknown" => "\u{25c7}",
        "running" => "\u{25cf}",
        "refused" | "failed" | "cancelled" => "\u{2715}",
        "finished" => "\u{25cb}",
        _ => "?",
    }
}

/// The order runs are grouped in: what needs the person first.
pub const GROUPS: [&str; 8] = [
    "needs approval",
    "uncertain",
    "unknown",
    "running",
    "refused",
    "failed",
    "cancelled",
    "finished",
];

/// One word for one run, from its view alone; first match wins. No view
/// (seen on the stream, never read) is `unknown`, not a guess.
pub fn run_state(view: Option<&Value>) -> &'static str {
    let Some(view) = view else {
        return "unknown";
    };
    let pending = view["actions"]
        .as_array()
        .is_some_and(|actions| actions.iter().any(|a| a["state"] == "pending"));
    if pending {
        return "needs approval";
    }
    if view["delivery"] == "ambiguous"
        || view["usage"]["liability"] == "unresolved"
        || view["runtime"] == "unknown"
    {
        return "uncertain";
    }
    if view["admission"] == "refused" {
        return "refused";
    }
    if view["delivery"] == "not_delivered" || view["delivery"] == "failed_before_delivery" {
        return "failed";
    }
    if view["cancellation"]["outcome"] == "cancelled" {
        return "cancelled";
    }
    if view["runtime"] == "exited" {
        return "finished";
    }
    "running"
}

#[derive(Clone, Debug, Default)]
pub struct Board {
    pub cursor: Option<String>,
    /// Execution id -> the highest revision the fold reported.
    pub seen: BTreeMap<String, u64>,
    /// Execution id -> the view last inspected.
    pub drawn: BTreeMap<String, Value>,
    /// Execution id -> how many times it was inspected.
    pub draws: BTreeMap<String, u64>,
    pub gaps: Vec<Value>,
}

impl Board {
    /// One page of the stream (a `core.events.read` result, or a
    /// `core.events.notify` params): which subjects moved.
    pub fn absorb(&mut self, page: &Value) -> BTreeSet<String> {
        let mut moved = BTreeSet::new();
        let mut note = |seen: &mut BTreeMap<String, u64>, id: &str, revision: &Value| {
            let revision = revision.as_u64().unwrap_or(0);
            if seen.get(id).is_none_or(|&known| revision > known) {
                seen.insert(id.to_owned(), revision);
                moved.insert(id.to_owned());
            }
        };
        for item in page["items"].as_array().into_iter().flatten() {
            if let Some(gap) = item.get("gap") {
                // A retention gap is a roster, not a hole.
                self.gaps.push(gap.clone());
                for entry in gap["snapshot"]["subjects"].as_array().into_iter().flatten() {
                    if let Some(id) = entry["subject"]["id"].as_str() {
                        note(&mut self.seen, id, &entry["revision"]);
                    }
                }
                continue;
            }
            let Some(event) = item.get("event") else {
                continue;
            };
            if event["subject"]["kind"] != EXECUTION {
                continue;
            }
            if let Some(id) = event["subject"]["id"].as_str() {
                note(&mut self.seen, id, &event["revision"]);
            }
        }
        if let Some(cursor) = page["next_cursor"].as_str() {
            self.cursor = Some(cursor.to_owned());
        }
        moved
    }

    /// Reads the stream from where the board left off (or from the start)
    /// until a page comes back empty.
    pub fn fold(&mut self, client: &mut Client) -> Reply<BTreeSet<String>> {
        let mut moved = BTreeSet::new();
        loop {
            let from = match &self.cursor {
                Some(cursor) => Position::Cursor(cursor.clone()),
                None => Position::Start,
            };
            let page = client.events_read(&from, &[EXECUTION], 1000)?;
            moved.extend(self.absorb(&page));
            if page["items"].as_array().is_none_or(Vec::is_empty) {
                return Ok(moved);
            }
        }
    }

    /// Inspects what moved, and nothing else.
    pub fn draw(&mut self, client: &mut Client, moved: &BTreeSet<String>) -> Reply<()> {
        for id in moved {
            *self.draws.entry(id.clone()).or_insert(0) += 1;
            match client.inspect(id) {
                Ok(view) => {
                    self.drawn.insert(id.clone(), view);
                }
                // As the Python fold does: a refused read keeps the last
                // view it had, whose `drawn_revision` then trails the
                // revision the stream reported, so the row shows its age.
                Err(crate::wire::Failure::Refused(_)) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    /// Fold, then draw what moved: one refresh.
    pub fn refresh(&mut self, client: &mut Client) -> Reply<BTreeSet<String>> {
        let moved = self.fold(client)?;
        self.draw(client, &moved)?;
        Ok(moved)
    }

    /// The board as the screen draws it.
    pub fn view(&self) -> Value {
        let mut rows = vec![];
        let mut counts: BTreeMap<&str, u64> = BTreeMap::new();
        for (id, revision) in &self.seen {
            let view = self.drawn.get(id);
            let state = run_state(view);
            *counts.entry(state).or_insert(0) += 1;
            let field = |key: &str| view.map(|v| v[key].clone()).unwrap_or(Value::Null);
            let pending: Vec<Value> = view
                .and_then(|v| v["actions"].as_array())
                .into_iter()
                .flatten()
                .filter(|a| a["state"] == "pending")
                .map(|a| a["action_id"].clone())
                .collect();
            rows.push(json!({
                "id": id, "revision": revision, "state": state, "glyph": glyph(state),
                "drawn_revision": field("revision"), "admission": field("admission"),
                "runtime": field("runtime"), "delivery": field("delivery"),
                "liability": view.map(|v| v["usage"]["liability"].clone()).unwrap_or(Value::Null),
                "exit": field("exit"), "pending_actions": pending}));
        }
        let groups: Vec<Value> = GROUPS
            .iter()
            .filter(|state| counts.contains_key(*state))
            .map(|state| {
                let ids: Vec<&Value> = rows
                    .iter()
                    .filter(|r| r["state"] == *state)
                    .map(|r| &r["id"])
                    .collect();
                json!({"state": state, "runs": ids})
            })
            .collect();
        let approvals: usize = rows
            .iter()
            .map(|r| r["pending_actions"].as_array().map_or(0, Vec::len))
            .sum();
        let uncertain = counts.get("uncertain").copied().unwrap_or(0)
            + counts.get("unknown").copied().unwrap_or(0);
        let counts: Map<String, Value> = counts
            .into_iter()
            .map(|(state, n)| (state.to_owned(), n.into()))
            .collect();
        json!({"runs": rows, "groups": groups, "counts": counts,
               "notes": {"approvals": approvals, "uncertain": uncertain},
               "cursor": self.cursor, "gaps": self.gaps.len()})
    }
}
