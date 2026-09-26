//! A durable place in the event stream, kept in the client's own state
//! file, so a follower that detaches and comes back resumes where it left
//! off: nothing lost, nothing shown twice. This is the foundation of the
//! screen's "leaving and coming back" (screen 7).
//!
//! The Protocol's cursor is per page, not per event, so the state keeps
//! two things: the cursor a page was read **from**, and the last event
//! (epoch, sequence) actually delivered. A follower that stops half-way
//! through a page resumes from that page's cursor and skips what it has
//! already delivered. A different stream (a new store) starts over.
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;

pub const FORMAT: &str = "pio-client-watch/1";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WatchState {
    /// The stream this position belongs to.
    pub stream: Option<String>,
    /// Where the next read starts.
    pub cursor: Option<String>,
    /// The last event delivered, as (epoch, sequence).
    pub last: Option<(u64, u64)>,
}

impl WatchState {
    /// The saved position, or the start of the stream if there is none.
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", path.display()));
            }
        };
        let saved: Value = serde_json::from_slice(&bytes)
            .with_context(|| format!("{} is not a watch state", path.display()))?;
        anyhow::ensure!(
            saved["format"] == FORMAT,
            "{} is not a {FORMAT} file",
            path.display()
        );
        let last = match (
            saved["last"]["epoch"].as_u64(),
            saved["last"]["sequence"].as_u64(),
        ) {
            (Some(epoch), Some(sequence)) => Some((epoch, sequence)),
            _ => None,
        };
        Ok(Self {
            stream: saved["stream"].as_str().map(str::to_owned),
            cursor: saved["cursor"].as_str().map(str::to_owned),
            last,
        })
    }

    /// Replaces the file atomically: a reader, or a crash, sees the old
    /// position or the new one, never half of either.
    pub fn save(&self, path: &Path) -> Result<()> {
        let value = json!({
            "format": FORMAT, "stream": self.stream, "cursor": self.cursor,
            "last": self.last.map(|(epoch, sequence)| json!({"epoch": epoch, "sequence": sequence}))});
        let temporary = path.with_extension("tmp");
        {
            let mut file = std::fs::File::create(&temporary)
                .with_context(|| format!("writing {}", temporary.display()))?;
            file.write_all(&serde_json::to_vec_pretty(&value)?)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
        }
        std::fs::rename(&temporary, path)?;
        Ok(())
    }

    /// Takes the stream a page (or a probe) came from. A saved position
    /// that belongs to another stream (another store, or one rebuilt) means
    /// nothing here: it is dropped and the follower starts from the
    /// beginning. Returns whether it was dropped, so the caller can say so.
    pub fn adopt(&mut self, stream: &str) -> bool {
        let other = self.stream.as_deref().is_some_and(|saved| saved != stream);
        if other {
            self.cursor = None;
            self.last = None;
        }
        self.stream = Some(stream.to_owned());
        other
    }

    /// Whether a stream item has not been delivered yet: an event, a
    /// retention gap or an epoch change, each at its place in the stream.
    pub fn is_new(&self, item: &Value) -> bool {
        match point(item) {
            Some(at) => self.last.is_none_or(|last| at > last),
            None => true,
        }
    }

    pub fn delivered(&mut self, item: &Value) {
        if let Some(at) = point(item) {
            self.last = Some(at);
        }
    }
}

/// How one page went for a follower.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageEnd {
    /// Events delivered from this page (gaps and epoch changes not counted).
    pub events: u64,
    /// Whether every item was delivered, so the cursor moved past the page.
    pub finished: bool,
}

/// Delivers one page (a `core.events.read` result) to a follower.
///
/// Every item not yet delivered — an event, a retention gap, an epoch
/// change — goes to `deliver`, in order, and the position is saved after
/// each (when `path` is given). `deliver` returns `false` to stop there, and
/// the page stops after `limit` events. Until the whole page is delivered
/// the cursor stays at the page's start, so a follower that stopped inside
/// it resumes there and skips, by position, everything it already had:
/// events and notices alike.
pub fn deliver_page(
    state: &mut WatchState,
    path: Option<&Path>,
    page: &Value,
    limit: Option<u64>,
    deliver: &mut dyn FnMut(&Value) -> Result<bool>,
) -> Result<PageEnd> {
    if let Some(stream) = page["stream"]["id"].as_str() {
        state.adopt(stream);
    }
    let items = page["items"].as_array().cloned().unwrap_or_default();
    let mut events = 0;
    for (index, item) in items.iter().enumerate() {
        if !state.is_new(item) {
            continue;
        }
        let keep_going = deliver(item)?;
        state.delivered(item);
        if let Some(path) = path {
            state.save(path)?;
        }
        if item.get("event").is_some() {
            events += 1;
        }
        let stop = !keep_going || limit.is_some_and(|limit| events >= limit);
        // Stopped on the page's last item, the page is delivered: the
        // cursor moves past it. Left at the page's start, a resumed read of
        // a page that ended in a gap is answered with a new gap reaching
        // further, whose notice prints again and folds events away.
        if stop && index + 1 < items.len() {
            return Ok(PageEnd {
                events,
                finished: false,
            });
        }
        if stop {
            break;
        }
    }
    if let Some(next) = page["next_cursor"].as_str() {
        state.cursor = Some(next.to_owned());
        if let Some(path) = path {
            state.save(path)?;
        }
    }
    Ok(PageEnd {
        events,
        finished: true,
    })
}

/// Where a stream item sits, as (epoch, sequence): an event at its own
/// place; a retention gap at the position its snapshot is as of (the last
/// place it covers); an epoch change just before the new epoch's first
/// event. An item may be a page item (`{"event": …}`) or the event itself.
pub fn point(item: &Value) -> Option<(u64, u64)> {
    let at = |position: &Value| Some((position["epoch"].as_u64()?, position["sequence"].as_u64()?));
    if let Some(event) = item.get("event") {
        return at(event);
    }
    if let Some(gap) = item.get("gap") {
        return at(&gap["to"]);
    }
    if let Some(change) = item.get("epoch_change") {
        return Some((change["to_epoch"].as_u64()?, 0));
    }
    at(item)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(sequence: u64) -> Value {
        json!({"event": {"stream": "s", "epoch": 1, "sequence": sequence}})
    }

    #[test]
    fn a_position_survives_a_restart_and_skips_what_was_delivered() {
        let dir = std::env::temp_dir().join(format!("pio-watch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("watch.json");
        let mut state = WatchState::load(&path).unwrap();
        assert_eq!(state, WatchState::default());
        assert!(!state.adopt("s"), "nothing saved, nothing dropped");
        assert!(state.is_new(&event(1)));
        state.delivered(&event(1));
        state.cursor = Some("page-1".into());
        state.save(&path).unwrap();
        let mut again = WatchState::load(&path).unwrap();
        assert_eq!(again, state);
        assert!(!again.adopt("s"));
        assert!(!again.is_new(&event(1)), "delivered once is delivered");
        assert!(again.is_new(&event(2)));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_position_from_another_stream_is_dropped() {
        let mut state = WatchState {
            stream: Some("store-a".into()),
            cursor: Some("store-a:1:9".into()),
            last: Some((1, 9)),
        };
        assert!(state.adopt("store-b"));
        assert_eq!(state.cursor, None);
        assert_eq!(state.last, None);
        assert_eq!(state.stream.as_deref(), Some("store-b"));
        assert!(state.is_new(&event(1)));
    }

    #[test]
    fn a_follower_that_stops_inside_a_page_repeats_nothing_on_resuming() {
        let page = json!({"stream": {"id": "s", "epoch": 2}, "next_cursor": "s:2:2", "items": [
            {"epoch_change": {"from_epoch": 1, "to_epoch": 2, "vouched_through": 9}},
            {"event": {"stream": "s", "epoch": 2, "sequence": 1}},
            {"gap": {"kind": "retention", "from": {"epoch": 2, "sequence": 2},
                     "to": {"epoch": 2, "sequence": 5}, "snapshot": {"subjects": []}}},
            {"event": {"stream": "s", "epoch": 2, "sequence": 6}}]});
        let mut state = WatchState {
            cursor: Some("s:1:9".into()),
            ..WatchState::default()
        };
        let mut seen = vec![];
        let mut record = |item: &Value| -> Result<bool> {
            seen.push(point(item).unwrap());
            Ok(true)
        };
        let first = deliver_page(&mut state, None, &page, Some(1), &mut record).unwrap();
        assert_eq!(
            first,
            PageEnd {
                events: 1,
                finished: false
            }
        );
        assert_eq!(
            state.cursor.as_deref(),
            Some("s:1:9"),
            "still at the page's start"
        );
        let second = deliver_page(&mut state, None, &page, None, &mut record).unwrap();
        assert_eq!(
            second,
            PageEnd {
                events: 1,
                finished: true
            }
        );
        assert_eq!(state.cursor.as_deref(), Some("s:2:2"));
        assert_eq!(
            seen,
            [(2, 0), (2, 1), (2, 5), (2, 6)],
            "each item exactly once"
        );
    }

    #[test]
    fn a_stop_on_the_last_item_moves_past_the_page() {
        let page = json!({"stream": {"id": "s", "epoch": 1}, "next_cursor": "s:1:9", "items": [
            {"event": {"stream": "s", "epoch": 1, "sequence": 3}},
            {"gap": {"kind": "retention", "from": {"epoch": 1, "sequence": 4},
                     "to": {"epoch": 1, "sequence": 9}, "snapshot": {"subjects": []}}}]});
        let mut state = WatchState {
            cursor: Some("s:1:2".into()),
            ..WatchState::default()
        };
        // A signal arrives while the gap, the last item, is printed.
        let mut stop_on_gap = |item: &Value| -> Result<bool> { Ok(item.get("gap").is_none()) };
        let end = deliver_page(&mut state, None, &page, None, &mut stop_on_gap).unwrap();
        assert_eq!(
            end,
            PageEnd {
                events: 1,
                finished: true
            }
        );
        assert_eq!(
            state.cursor.as_deref(),
            Some("s:1:9"),
            "past the page, not at its start"
        );
        // The same for a limit reached on the last item.
        let mut state = WatchState {
            cursor: Some("s:1:2".into()),
            ..WatchState::default()
        };
        let events_only = json!({"stream": {"id": "s", "epoch": 1}, "next_cursor": "s:1:3",
                                 "items": [{"event": {"stream": "s", "epoch": 1, "sequence": 3}}]});
        let mut all = |_: &Value| -> Result<bool> { Ok(true) };
        let end = deliver_page(&mut state, None, &events_only, Some(1), &mut all).unwrap();
        assert!(end.finished);
        assert_eq!(state.cursor.as_deref(), Some("s:1:3"));
    }

    #[test]
    fn gaps_and_epoch_changes_are_delivered_once_too() {
        let gap = json!({"gap": {"kind": "retention", "from": {"epoch": 1, "sequence": 1},
                                 "to": {"epoch": 1, "sequence": 7}, "snapshot": {}}});
        let change = json!({"epoch_change": {"from_epoch": 1, "to_epoch": 2,
                                             "vouched_through": 9}});
        let mut state = WatchState::default();
        assert!(state.is_new(&gap));
        state.delivered(&gap);
        assert!(
            !state.is_new(&gap),
            "a resumed watcher does not repeat the gap"
        );
        assert!(!state.is_new(&event(7)));
        assert!(state.is_new(&event(8)));
        state.delivered(&event(9));
        assert!(state.is_new(&change));
        state.delivered(&change);
        assert!(!state.is_new(&change));
        assert!(state.is_new(&json!({"event": {"epoch": 2, "sequence": 1}})));
    }
}
