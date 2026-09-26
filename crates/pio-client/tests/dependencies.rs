//! M4 rule 1: the screen reads and writes only through the public API the
//! command line uses. `pio-client` is that API's only Rust implementation,
//! so it may not reach PIO's service internals by any path, and neither may
//! the command-line code built on it.
//!
//! Four checks, because the first cut had one and the verifier went round
//! it twice (review of T1: a feature-gated optional dependency with a
//! re-export, and a source file of the service's included by path):
//!
//! 1. the graph, with **every feature on**: no path to a service crate;
//! 2. an **allow-list** of pio-client's own dependencies: nothing from the
//!    workspace and nothing by path, and nothing new without a review;
//! 3. pio-client's **sources**: no module path attribute and no include
//!    macro that reaches outside the crate;
//! 4. the **command line** (`client_cli.rs` and `ledger.rs`): no service
//!    crate named, and nothing borrowed from the rest of the binary.
use serde_json::Value;
use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
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

/// pio-client's own dependencies, all of them from the registry. Adding one
/// is a decision, so it fails here until it is written down here.
const ALLOWED: [&str; 7] = [
    "anyhow",
    "base64",
    "libc",
    "serde",
    "serde_json",
    "sha2",
    "uuid",
];

fn metadata() -> Value {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--all-features"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

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

/// pio-client's declared dependencies (optional ones included, every
/// kind but dev) that break the allow-list: by path, or not on it.
fn outside_allow_list(metadata: &Value) -> Vec<String> {
    let client = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "pio-client")
        .expect("pio-client in metadata");
    let mut out = vec![];
    for dep in client["dependencies"].as_array().into_iter().flatten() {
        if dep["kind"] == "dev" {
            continue;
        }
        let name = dep["name"].as_str().unwrap_or_default();
        if dep.get("path").is_some_and(|p| !p.is_null()) {
            out.push(format!("{name} (by path)"));
        } else if !ALLOWED.contains(&name) {
            out.push(format!("{name} (not on the allow-list)"));
        }
    }
    out
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(rust_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out
}

/// A module path attribute, or an include macro whose argument is not a
/// plain file beside the including one. The needles are built at run time
/// so this file does not match itself.
fn reaches_outside(source: &str) -> Vec<String> {
    let attribute = format!("#[{}", "path");
    let spaced = format!("#[ {}", "path");
    let includes = ["include!", "include_str!", "include_bytes!"];
    let mut out = vec![];
    for (number, line) in source.lines().enumerate() {
        let code = line.split("//").next().unwrap_or("");
        if code.contains(&attribute) || code.contains(&spaced) {
            out.push(format!("line {}: {}", number + 1, line.trim()));
            continue;
        }
        for include in includes {
            if let Some(at) = code.find(include) {
                let argument = &code[at + include.len()..];
                if argument.contains("..")
                    || argument.contains("(\"/")
                    || argument.contains("env!")
                    || argument.contains("concat!")
                {
                    out.push(format!("line {}: {}", number + 1, line.trim()));
                }
            }
        }
    }
    out
}

/// A service crate named in command-line code, or a path into the rest of
/// the binary (`crate::` other than the two client modules, or `super::`).
fn borrows_the_service(source: &str) -> Vec<String> {
    let names: Vec<String> = FORBIDDEN.iter().map(|n| n.replace('-', "_")).collect();
    let mut out = vec![];
    for (number, line) in source.lines().enumerate() {
        let code = line.split("//").next().unwrap_or("");
        let named = names.iter().any(|n| code.contains(n.as_str()));
        let reached = code.match_indices("crate::").any(|(at, _)| {
            let rest = &code[at + "crate::".len()..];
            !rest.starts_with("ledger") && !rest.starts_with("client_cli")
        }) || code.contains("super::") && !code.contains("use super::*");
        if named || reached {
            out.push(format!("line {}: {}", number + 1, line.trim()));
        }
    }
    out
}

#[test]
fn pio_client_depends_on_no_service_crate_with_every_feature_on() {
    let metadata = metadata();
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

#[test]
fn pio_client_depends_only_on_what_the_allow_list_names() {
    let outside = outside_allow_list(&metadata());
    assert!(
        outside.is_empty(),
        "pio-client declares dependencies outside its allow-list: {outside:?}"
    );
}

#[test]
fn pio_client_sources_reach_nothing_outside_the_crate() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found = vec![];
    for dir in ["src", "tests", "examples", "benches"] {
        for file in rust_files(&root.join(dir)) {
            for hit in reaches_outside(&std::fs::read_to_string(&file).unwrap()) {
                let shown = file.strip_prefix(root).unwrap_or(&file);
                found.push(format!("{}: {hit}", shown.display()));
            }
        }
    }
    assert!(!rust_files(&root.join("src")).is_empty());
    assert!(
        found.is_empty(),
        "pio-client sources reach outside the crate: {found:?}"
    );
}

#[test]
fn the_command_line_reaches_the_service_only_through_pio_client() {
    let cli = Path::new(env!("CARGO_MANIFEST_DIR")).join("../pio-cli/src");
    let mut found = vec![];
    for name in ["client_cli.rs", "ledger.rs"] {
        let source = std::fs::read_to_string(cli.join(name)).unwrap();
        assert!(
            source.contains("pio_client"),
            "{name} does not use pio_client"
        );
        for hit in borrows_the_service(&source)
            .into_iter()
            .chain(reaches_outside(&source))
        {
            found.push(format!("{name}: {hit}"));
        }
    }
    assert!(
        found.is_empty(),
        "the command line reaches past the public client: {found:?}"
    );
}

/// The checkers themselves, on inputs where each must fire and must not.
#[test]
fn each_checker_fires_on_what_it_forbids() {
    let graph = |kind: &str| {
        serde_json::json!({
            "packages": [
                {"id": "c", "name": "pio-client", "dependencies": [
                    {"name": "middle", "kind": null, "path": "/x/middle"},
                    {"name": "serde_json", "kind": null}]},
                {"id": "m", "name": "middle"},
                {"id": "k", "name": "pio-core"}],
            "resolve": {"nodes": [
                {"id": "c", "deps": [{"pkg": "m", "dep_kinds": [{"kind": kind}]}]},
                {"id": "m", "deps": [{"pkg": "k", "dep_kinds": [{"kind": null}]}]},
                {"id": "k", "deps": []}]}})
    };
    assert_eq!(forbidden_reach(&graph("build"), "pio-client"), ["pio-core"]);
    assert!(forbidden_reach(&graph("dev"), "pio-client").is_empty());
    assert_eq!(outside_allow_list(&graph("build")), ["middle (by path)"]);

    let attribute = format!("#[{} = \"../../pio-protocol/src/encoding.rs\"]", "path");
    assert_eq!(reaches_outside(&format!("{attribute}\nmod leak;")).len(), 1);
    let include = format!("{}(\"../../pio-core/src/lib.rs\");", "include!");
    assert_eq!(reaches_outside(&include).len(), 1);
    assert!(reaches_outside("let x = 1; // mod leak;").is_empty());

    assert_eq!(borrows_the_service("use pio_core::digest;").len(), 1);
    assert_eq!(borrows_the_service("crate::main_helper()").len(), 1);
    assert!(borrows_the_service("use crate::ledger;\nuse pio_client::Client;").is_empty());
}
