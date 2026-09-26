//! G2: the approval walk, and every decision made about an approval.
//!
//! A port of `scripts/approval_desk.py` (`walk`, `due`, `decisions`),
//! checked against it on the same recorded events by
//! `scripts/fold_parity.py`. `actions[]` in the view is closed and holds
//! identity and state only; the deadline, the options the harness offered,
//! what PIO will send if nobody answers and the classification ride in the
//! payload of the `execution.runtime.changed` that marks the action pending,
//! under `pio.combraton.dev/approval`. Who decided rides on
//! `execution.action.answered`, under `pio.combraton.dev/decision`.
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const APPROVAL: &str = "pio.combraton.dev/approval";
pub const DECISION: &str = "pio.combraton.dev/decision";

/// Seconds since the Unix epoch of an RFC 3339 instant's first 19
/// characters, read as UTC (the stream's instants are UTC). `None` if it
/// does not parse.
pub fn epoch_seconds(instant: &str) -> Option<i64> {
    let text = instant.get(..19)?;
    let number = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let bytes = text.as_bytes();
    if bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    if bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// When PIO will decide if nobody does, or `None` when nothing says it
/// ever will. `assume` stands in for a missing deadline.
pub fn due(row: &Value, assume: Option<i64>) -> Option<i64> {
    let seconds = row["answer_deadline_seconds"].as_i64().or(assume)?;
    Some(epoch_seconds(row["requested_at"].as_str()?)? + seconds)
}

/// Walk order: soonest due first (the default), or as the requests arrived.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    Deadline,
    Arrival,
}

/// Every request still waiting, soonest due first. Each row is the
/// approval record as the service wrote it, plus the run, the stream
/// sequence it arrived at and when it is due.
pub fn walk(events: &[Value], order: Order, assume: Option<i64>) -> Vec<Value> {
    // Insertion order, as the Python dict keeps it: the sorts below are
    // stable, so ties keep it.
    let mut pending: Vec<(String, Value)> = vec![];
    let mut over = BTreeSet::new();
    for event in events {
        let approval = &event["payload"][APPROVAL];
        if event["type"] == "execution.runtime.changed" && truthy(approval) {
            let action = approval["action_id"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let mut row = approval.clone();
            row["run"] = event["subject"]["id"].clone();
            row["arrived"] = event["sequence"].clone();
            match pending.iter_mut().find(|(id, _)| *id == action) {
                Some(slot) => slot.1 = row,
                None => pending.push((action, row)),
            }
        }
        if event["type"] == "execution.action.answered" {
            let action = event["payload"]["action_id"].as_str().unwrap_or_default();
            pending.retain(|(id, _)| id != action);
        }
        if event["type"] == "execution.exit.observed" {
            // A run that has exited cannot answer anything, so its requests
            // leave the walk whether or not anyone decided them.
            if let Some(run) = event["subject"]["id"].as_str() {
                over.insert(run.to_owned());
            }
        }
    }
    let mut rows: Vec<Value> = pending
        .into_iter()
        .map(|(_, row)| row)
        .filter(|row| !row["run"].as_str().is_some_and(|run| over.contains(run)))
        .collect();
    rows.sort_by_key(|row| row["arrived"].as_i64().unwrap_or(0));
    if order == Order::Deadline {
        // A request that never becomes due sorts after every one that does.
        rows.sort_by_key(|row| {
            let when = due(row, assume);
            (when.is_none(), when.unwrap_or(0))
        });
    }
    for row in &mut rows {
        row["due"] = due(row, assume).map_or(Value::Null, Value::from);
    }
    rows
}

/// Every decision on the stream, in stream order, whoever made it: the
/// caller, PIO (a lapse or a decline), the harness (Codex settling a
/// request itself) or nobody (the request ended with its turn). A payload
/// without the key names no decider, so it is `unknown`.
pub fn decisions(events: &[Value]) -> Vec<Value> {
    events
        .iter()
        .filter(|event| event["type"] == "execution.action.answered")
        .map(|event| {
            let record = event["payload"]
                .get(DECISION)
                .cloned()
                .unwrap_or(Value::Null);
            let known = |key: &str| record.get(key).cloned().unwrap_or(Value::Null);
            json!({
                "run": event["subject"]["id"],
                "action_id": event["payload"].get("action_id").cloned().unwrap_or(Value::Null),
                "sequence": event["sequence"], "origin": event["origin"],
                "decided_by": record.get("decided_by").cloned().unwrap_or_else(|| "unknown".into()),
                "decision": known("decision"), "sent": known("sent"), "basis": known("basis"),
                "record": record})
        })
        .collect()
}

/// What a walk cannot know after a retention gap, said on each row.
pub const LOST: &str = "lost to retention: the request's deadline, the options the harness \
offered and what PIO will send are no longer on the stream";

/// Pending requests the stream no longer carries.
///
/// After a retention gap the `execution.runtime.changed` that carried a
/// request's details may be gone, and the walk above cannot see the
/// request at all. The run's view still lists it: `actions[]` holds every
/// action with its state. So every action a view shows `pending`, on a run
/// that has not exited, that the walk does not already have, is waiting
/// too. Its row says what is known (run, action, owner, when it was asked)
/// and that the rest was lost, rather than leaving it out or guessing.
pub fn lost_to_retention(rows: &[Value], views: &BTreeMap<String, Value>) -> Vec<Value> {
    let known: BTreeSet<&str> = rows
        .iter()
        .filter_map(|r| r["action_id"].as_str())
        .collect();
    let mut out = vec![];
    for (run, view) in views {
        if view["runtime"] == "exited" {
            continue;
        }
        for action in view["actions"].as_array().into_iter().flatten() {
            let Some(id) = action["action_id"].as_str() else {
                continue;
            };
            if action["state"] != "pending" || known.contains(id) {
                continue;
            }
            out.push(
                json!({"run": run, "action_id": id, "owner": action["owner"],
                            "requested_at": action["requested_at"], "due": null,
                            "lost_to_retention": true, "details": LOST}),
            );
        }
    }
    out
}

/// The walk and the decisions together: what the approval card draws.
pub fn desk(events: &[Value]) -> Value {
    json!({"walk": walk(events, Order::Deadline, None), "decided": decisions(events)})
}

/// Python's truthiness, for the one place the port needs it: an approval
/// key that is present but empty or null is not an approval.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Object(map) => !map.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::String(text) => !text.is_empty(),
        Value::Number(n) => n.as_f64() != Some(0.0),
    }
}

/// The events of one stream read in full, as a flat list, dropping gaps
/// and epoch changes (the walk is drawn from events alone).
pub fn events_of(pages: &[Value]) -> Vec<Value> {
    pages
        .iter()
        .flat_map(|page| page["items"].as_array().cloned().unwrap_or_default())
        .filter_map(|item| item.get("event").cloned())
        .collect()
}

/// Runs with a pending request, and how many each has.
pub fn waiting_by_run(rows: &[Value]) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for row in rows {
        if let Some(run) = row["run"].as_str() {
            *out.entry(run.to_owned()).or_insert(0) += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instants_are_read_as_utc() {
        assert_eq!(epoch_seconds("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(epoch_seconds("2026-09-26T14:58:24Z"), Some(1_790_434_704));
        assert_eq!(
            epoch_seconds("2000-02-29T23:59:59.5+00:00"),
            Some(951_868_799)
        );
        assert_eq!(epoch_seconds("not a time"), None);
    }

    #[test]
    fn a_request_the_stream_lost_is_still_listed_from_its_view() {
        let rows = vec![json!({"run": "run-1", "action_id": "run-1.action-1"})];
        let views = BTreeMap::from([
            (
                "run-1".to_owned(),
                json!({"runtime": "requires_action", "actions": [
                {"action_id": "run-1.action-1", "owner": "opencode", "state": "pending"}]}),
            ),
            (
                "run-2".to_owned(),
                json!({"runtime": "requires_action", "actions": [
                {"action_id": "run-2.action-1", "owner": "opencode", "state": "pending",
                 "requested_at": "2026-09-26T00:00:00Z"},
                {"action_id": "run-2.action-0", "owner": "opencode", "state": "answered"}]}),
            ),
            (
                "run-3".to_owned(),
                json!({"runtime": "exited", "actions": [
                {"action_id": "run-3.action-1", "owner": "opencode", "state": "pending"}]}),
            ),
        ]);
        let lost = lost_to_retention(&rows, &views);
        assert_eq!(lost.len(), 1, "{lost:?}");
        assert_eq!(lost[0]["action_id"], "run-2.action-1");
        assert_eq!(lost[0]["lost_to_retention"], true);
        assert_eq!(lost[0]["due"], Value::Null, "no deadline is invented");
    }

    #[test]
    fn the_walk_sorts_by_deadline_and_drops_exited_runs() {
        let changed = |run: &str, seq: u64, action: &str, seconds: Value| {
            json!({"type": "execution.runtime.changed", "sequence": seq,
                   "subject": {"kind": "execution.execution", "id": run},
                   "payload": {"runtime": "requires_action", APPROVAL: {
                       "action_id": action, "requested_at": "2026-09-26T00:00:00Z",
                       "answer_deadline_seconds": seconds}}})
        };
        let events = vec![
            changed("run-1", 1, "run-1.action-1", json!(300)),
            changed("run-2", 2, "run-2.action-1", json!(75)),
            changed("run-3", 3, "run-3.action-1", Value::Null),
            changed("run-4", 4, "run-4.action-1", json!(10)),
            json!({"type": "execution.exit.observed", "sequence": 5,
                   "subject": {"kind": "execution.execution", "id": "run-4"}, "payload": {}}),
        ];
        let runs = |order| {
            walk(&events, order, None)
                .iter()
                .map(|r| r["run"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(runs(Order::Deadline), ["run-2", "run-1", "run-3"]);
        assert_eq!(runs(Order::Arrival), ["run-1", "run-2", "run-3"]);
        assert_eq!(walk(&events, Order::Deadline, None)[2]["due"], Value::Null);
    }
}
