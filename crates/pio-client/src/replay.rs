//! The three folds over a **recorded** stream, with no service behind them.
//!
//! This is how the Rust folds are checked against the Python ones they were
//! ported from: `scripts/fold_parity.py` feeds the same recording to both
//! and requires identical output, and `tests/folds.rs` holds the Rust side
//! to the Python side's recorded answer. A recording is JSON:
//!
//! - `{"kind": "board", "rounds": [{"pages": [<core.events.read result>…],
//!   "views": {<id>: <execution.inspect result>…}}…]}`: per round, which
//!   subjects moved and the board drawn after it;
//! - `{"kind": "walk", "events": [<event>…]}`: the walk by deadline and by
//!   arrival, and every decision;
//! - `{"kind": "blocks", "harness": <owner>, "reads": [<base64>…],
//!   "exit": <execution.exit.observed event> | null}`: the blocks and tool
//!   uses, with the audit filled in if the exit carries one.
use crate::blocks::{Decoder, Transcript};
use crate::board::Board;
use crate::walk::{Order, decisions, walk};
use anyhow::{Context, Result, bail};
use base64::Engine;
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub fn fold(recording: &Value) -> Result<Value> {
    match recording["kind"].as_str() {
        Some("board") => {
            let mut board = Board::default();
            let mut rounds = vec![];
            for round in recording["rounds"].as_array().context("rounds")? {
                let mut moved = BTreeSet::new();
                for page in round["pages"].as_array().context("pages")? {
                    moved.extend(board.absorb(page));
                }
                for id in &moved {
                    *board.draws.entry(id.clone()).or_insert(0) += 1;
                    if let Some(view) = round["views"].get(id) {
                        board.drawn.insert(id.clone(), view.clone());
                    }
                }
                rounds.push(json!({"moved": moved, "board": board.view()}));
            }
            Ok(json!({"rounds": rounds}))
        }
        Some("walk") => {
            let events = recording["events"].as_array().context("events")?;
            Ok(json!({"walk": walk(events, Order::Deadline, None),
                      "arrival": walk(events, Order::Arrival, None),
                      "decided": decisions(events)}))
        }
        Some("blocks") => {
            let harness = recording["harness"].as_str().context("harness")?;
            let decoder = Decoder::for_harness(harness).context("unknown harness")?;
            let mut transcript = Transcript::new(decoder);
            for read in recording["reads"].as_array().context("reads")? {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(read.as_str().context("read")?)?;
                transcript.feed(&bytes)?;
            }
            let audited = match recording.get("exit") {
                Some(exit) if !exit.is_null() => transcript.fill_from_exit(exit)?,
                _ => false,
            };
            let mut out = transcript.view();
            out["audited"] = audited.into();
            Ok(out)
        }
        other => bail!("unknown recording kind {other:?}"),
    }
}
