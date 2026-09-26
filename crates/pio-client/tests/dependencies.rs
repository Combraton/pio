//! M4 rule 1: the screen reads and writes only through the public API the
//! command line uses. `pio-client` is that API's only Rust implementation,
//! so it may not reach PIO's service internals by any path: not pio-core's
//! store, not pio-protocol's provider or persistence, and not a host or an
//! adapter crate. This reads the real dependency graph from `cargo metadata`
//! and fails on any such edge, direct or transitive.
use serde_json::Value;
use std::collections::{BTreeSet, VecDeque};
use std::process::Command;

/// Every PIO crate that is not the client. Each of them either is the
/// service (pio-core, pio-protocol) or runs under it (pio-host and the
/// adapters), so a path to any one of them is a path past the wire.
const FORBIDDEN: [&str; 6] = [
    "pio-core",
    "pio-protocol",
    "pio-host",
    "pio-codex",
    "pio-claude",
    "pio-opencode",
];

/// The forbidden packages `package` reaches through normal or build
/// dependencies. Dev-dependencies are not shipped, so they are not counted.
fn forbidden_reach(metadata: &Value, package: &str) -> Vec<String> {
    let packages = metadata["packages"].as_array().expect("packages");
    let name_of = |id: &str| {
        packages
            .iter()
            .find(|p| p["id"] == id)
            .and_then(|p| p["name"].as_str())
            .unwrap_or(id)
            .to_owned()
    };
    let nodes = metadata["resolve"]["nodes"].as_array().expect("resolve");
    let root = packages
        .iter()
        .find(|p| p["name"] == package)
        .and_then(|p| p["id"].as_str())
        .expect("package in metadata")
        .to_owned();
    let mut seen = BTreeSet::from([root.clone()]);
    let mut queue = VecDeque::from([root]);
    let mut reached = BTreeSet::new();
    while let Some(id) = queue.pop_front() {
        let node = nodes.iter().find(|n| n["id"] == id.as_str());
        for dep in node
            .and_then(|n| n["deps"].as_array())
            .into_iter()
            .flatten()
        {
            let shipped = dep["dep_kinds"]
                .as_array()
                .is_none_or(|kinds| kinds.is_empty() || kinds.iter().any(|k| k["kind"] != "dev"));
            let target = dep["pkg"].as_str().unwrap_or_default().to_owned();
            if !shipped || !seen.insert(target.clone()) {
                continue;
            }
            let name = name_of(&target);
            if FORBIDDEN.contains(&name.as_str()) {
                reached.insert(name);
            }
            queue.push_back(target);
        }
    }
    reached.into_iter().collect()
}

#[test]
fn pio_client_depends_on_no_service_crate() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value = serde_json::from_slice(&output.stdout).unwrap();
    // The graph is the real one: the CLI is in it and does reach the
    // service, so a checker that saw nothing would fail here first.
    assert!(!forbidden_reach(&metadata, "pio-cli").is_empty());
    let reached = forbidden_reach(&metadata, "pio-client");
    assert!(
        reached.is_empty(),
        "pio-client reaches PIO service internals through {reached:?}; \
         the screen and the CLI must use the public wire only"
    );
}

/// The checker itself, on a graph where the client reaches pio-core only
/// through an intermediate crate, and on one where the edge is dev-only.
#[test]
fn the_checker_sees_a_transitive_edge_and_ignores_a_dev_only_one() {
    let graph = |kind: &str| {
        serde_json::json!({
            "packages": [
                {"id": "c", "name": "pio-client"},
                {"id": "m", "name": "middle"},
                {"id": "k", "name": "pio-core"}],
            "resolve": {"nodes": [
                {"id": "c", "deps": [{"pkg": "m", "dep_kinds": [{"kind": kind}]}]},
                {"id": "m", "deps": [{"pkg": "k", "dep_kinds": [{"kind": null}]}]},
                {"id": "k", "deps": []}]}})
    };
    assert_eq!(forbidden_reach(&graph("build"), "pio-client"), ["pio-core"]);
    assert!(forbidden_reach(&graph("dev"), "pio-client").is_empty());
}
