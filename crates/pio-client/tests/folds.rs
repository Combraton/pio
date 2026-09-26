//! The Rust folds, held to the Python folds' answers on recorded streams.
//!
//! Each `tests/fixtures/<name>.json` was recorded off the public socket of a
//! fake-backed service by `scripts/fold_parity.py --record`, and
//! `<name>.expected.json` beside it is what the **Python** fold it was
//! ported from answers on that recording (`--write-expected`). The parity
//! script runs both folds live; this keeps the Rust side honest in `cargo
//! test` alone.
use serde_json::Value;
use std::path::Path;

fn fixtures() -> Vec<std::path::PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|p| {
            p.extension().is_some_and(|e| e == "json")
                && !p.to_string_lossy().ends_with(".expected.json")
        })
        .collect();
    paths.sort();
    paths
}

#[test]
fn every_recording_folds_as_the_python_fold_did() {
    let paths = fixtures();
    let kinds: std::collections::BTreeSet<String> = paths
        .iter()
        .map(|p| {
            let recording: Value = serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();
            recording["kind"].as_str().unwrap().to_owned()
        })
        .collect();
    assert_eq!(
        kinds.into_iter().collect::<Vec<_>>(),
        ["blocks", "board", "walk"],
        "a fold with no recording is a fold with no check"
    );
    for path in paths {
        let recording: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let expected: Value =
            serde_json::from_slice(&std::fs::read(path.with_extension("expected.json")).unwrap())
                .unwrap();
        let folded = pio_client::replay::fold(&recording).unwrap();
        assert_eq!(folded, expected, "{} folds differently", path.display());
    }
}

/// What the recordings have to contain for the check above to mean
/// anything: every decider, both deadline orders, the audit and its absence.
#[test]
fn the_recordings_cover_what_the_folds_decide() {
    let expected = |name: &str| -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(format!("{name}.expected.json"));
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    };
    let mut deciders: Vec<String> = vec![];
    for name in [
        "walk-desk-settled",
        "walk-desk-declined",
        "walk-codex-harness",
        "walk-codex-nobody",
    ] {
        for decided in expected(name)["decided"].as_array().unwrap() {
            deciders.push(decided["decided_by"].as_str().unwrap().to_owned());
            if decided["decided_by"] != "caller" && decided["decided_by"] != "pio" {
                assert_eq!(decided["decision"], Value::Null, "{name}");
                assert_eq!(decided["sent"], false, "{name}");
            }
        }
    }
    deciders.sort();
    deciders.dedup();
    assert_eq!(deciders, ["caller", "harness", "nobody", "pio"]);

    let two = expected("walk-desk-two-waiting");
    let order = |key: &str| -> Vec<Value> {
        two[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["run"].clone())
            .collect()
    };
    assert_eq!(order("walk"), ["run-2", "run-1"], "soonest due first");
    assert_eq!(order("arrival"), ["run-1", "run-2"], "the orders disagree");
    assert!(two["walk"][0]["options"].is_array());
    assert!(two["walk"][0]["if_nobody_answers"].is_object());

    assert_eq!(expected("blocks-opencode")["audited"], true);
    assert_eq!(expected("blocks-codex")["audited"], false);
    let states: Vec<Value> = expected("board-desk")["rounds"][0]["board"]["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["state"].clone())
        .collect();
    assert_eq!(states, ["needs approval", "needs approval"]);
}
