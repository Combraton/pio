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

    /// Whether an event has not been delivered yet. An event from another
    /// stream than the saved one resets the position.
    pub fn is_new(&mut self, event: &Value) -> bool {
        let stream = event["stream"].as_str().map(str::to_owned);
        if stream.is_some() && self.stream.is_some() && stream != self.stream {
            self.last = None;
            self.cursor = None;
        }
        if stream.is_some() {
            self.stream = stream;
        }
        let at = (
            event["epoch"].as_u64().unwrap_or(0),
            event["sequence"].as_u64().unwrap_or(0),
        );
        self.last.is_none_or(|last| at > last)
    }

    pub fn delivered(&mut self, event: &Value) {
        self.last = Some((
            event["epoch"].as_u64().unwrap_or(0),
            event["sequence"].as_u64().unwrap_or(0),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(sequence: u64) -> Value {
        json!({"stream": "s", "epoch": 1, "sequence": sequence})
    }

    #[test]
    fn a_position_survives_a_restart_and_skips_what_was_delivered() {
        let dir = std::env::temp_dir().join(format!("pio-watch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("watch.json");
        let mut state = WatchState::load(&path).unwrap();
        assert_eq!(state, WatchState::default());
        assert!(state.is_new(&event(1)));
        state.delivered(&event(1));
        state.cursor = Some("page-1".into());
        state.save(&path).unwrap();
        let mut again = WatchState::load(&path).unwrap();
        assert_eq!(again, state);
        assert!(!again.is_new(&event(1)), "delivered once is delivered");
        assert!(again.is_new(&event(2)));
        // A new store is a new stream: start over.
        assert!(again.is_new(&json!({"stream": "t", "epoch": 1, "sequence": 1})));
        assert_eq!(again.cursor, None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
