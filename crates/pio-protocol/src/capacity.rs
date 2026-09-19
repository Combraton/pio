//! Test tooling for the ADR 002 projection bound. Pads a stopped service's
//! Protocol projection with inert `pio-test.filler` subjects through the same
//! bounded journal commit, so a process test can cross the real limit with a
//! single public command, then removes that filler again. Never used by the
//! service itself.
use crate::provider::{Data, Subject, key};
use anyhow::{Result, ensure};
use pio_core::{Store, projections::projection_size};
use serde_json::{Value, json};
use std::path::Path;

pub fn fill_projection_records(root: &Path, target: u64) -> Result<Value> {
    ensure!(
        root.join("journal.sqlite3").exists(),
        "fill-projection requires an existing store"
    );
    let mut store = Store::open(root)?;
    let (revision, records) = store.protocol_records()?;
    let before = projection_size(&records)?;
    let mut data = Data::from_records(&records)?;
    let filler = |n: u64| json!({"kind":"pio-test.filler","id":n.to_string()});
    let existing = data
        .subjects
        .values()
        .filter(|s| s.subject["kind"] == "pio-test.filler")
        .count() as u64;
    if target >= before.1 {
        for n in existing..existing + (target - before.1) {
            let subject = filler(n);
            data.subjects.insert(
                key(&subject),
                Subject {
                    subject,
                    revision: 1,
                    state: json!({}),
                    applied: 0,
                },
            );
        }
    } else {
        // Shrinking removes only filler subjects this tooling added.
        ensure!(
            before.1 - target <= existing,
            "only {existing} filler records can be removed"
        );
        for n in existing - (before.1 - target)..existing {
            data.subjects.remove(&key(&filler(n)));
        }
    }
    let records = data.records()?;
    let after = projection_size(&records)?;
    ensure!(after.1 == target, "filler did not reach {target} records");
    let revision = store.commit_protocol(revision, &records)?;
    Ok(
        json!({"format":"pio-projection-fill/1","test_tooling":true,"revision":revision,"before":{"bytes":before.0,"records":before.1},"after":{"bytes":after.0,"records":after.1}}),
    )
}
