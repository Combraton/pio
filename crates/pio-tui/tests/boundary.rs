//! M4 rule 1 for the screen, held structurally: `pio tui` reads and writes
//! only through the public API the command line uses.
//!
//! A text scan of the sources was the first line of defence for pio-client
//! and was gone round several ways (a `cfg_attr` path, `# [path` with a
//! space, a symlinked module, `use super::*`). So the screen's crate is held
//! by what the toolchain itself knows, not by what its text looks like:
//!
//! 1. **the graph** (`cargo metadata`, every feature on): the only workspace
//!    crate pio-tui reaches, by any dependency but dev, is pio-client; and
//!    its own declared dependencies are pio-client by path and an allow-list
//!    of registry crates, nothing new without a review;
//! 2. **rustc's dep-info**: every source file rustc compiled into pio-tui,
//!    however it got there (`mod`, `#[path]`, `include!`, a macro), is under
//!    `crates/pio-tui/`, the workspace manifest aside;
//! 3. **no symlink** anywhere under `crates/pio-tui/`.
//!
//! The helpers take the package, its directory and its library name, so the
//! same checks can hold any other crate that must stay on the public client.
use serde_json::Value;
use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;

const PACKAGE: &str = "pio-tui";
const LIBRARY: &str = "pio_tui";
/// The one workspace crate the screen may reach: the public client.
const WORKSPACE_ALLOWED: [&str; 1] = ["pio-client"];
/// Its registry dependencies. Adding one is a decision, so it fails here
/// until it is written down here.
const REGISTRY_ALLOWED: [&str; 4] = ["anyhow", "libc", "ratatui", "serde_json"];

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

/// (1) Every workspace crate `package` reaches through normal or build
/// dependencies, transitively.
fn workspace_reach(metadata: &Value, package: &str) -> BTreeSet<String> {
    let packages = metadata["packages"].as_array().expect("packages");
    let members: BTreeSet<&str> = metadata["workspace_members"]
        .as_array()
        .expect("workspace members")
        .iter()
        .filter_map(Value::as_str)
        .collect();
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
            if members.contains(target.as_str()) {
                reached.insert(name_of(&target));
            }
            queue.push_back(target);
        }
    }
    reached
}

/// (1) `package`'s declared dependencies (optional ones too, every kind but
/// dev) that break its allow-lists.
fn outside_allow_list(
    metadata: &Value,
    package: &str,
    by_path: &[&str],
    registry: &[&str],
) -> Vec<String> {
    let manifest = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == package)
        .expect("package in metadata");
    let mut out = vec![];
    for dep in manifest["dependencies"].as_array().into_iter().flatten() {
        if dep["kind"] == "dev" {
            continue;
        }
        let name = dep["name"].as_str().unwrap_or_default();
        let is_path = dep.get("path").is_some_and(|p| !p.is_null());
        if is_path && !by_path.contains(&name) {
            out.push(format!("{name} (by path)"));
        } else if !is_path && !registry.contains(&name) {
            out.push(format!("{name} (not on the allow-list)"));
        }
    }
    out
}

/// (2) The files a dep-info file names. rustc writes one phony target line,
/// `path:`, for every file it read, relative to where it ran (the workspace
/// root) unless absolute, with a space escaped as `\ `.
fn dep_info_files(text: &str, root: &Path) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim_end)
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.strip_suffix(':'))
        .map(|path| root.join(path.replace("\\ ", " ")))
        .collect()
}

/// (2) Every file rustc compiled into `library`, from every dep-info file of
/// it beside this test binary (`target/<profile>/deps/LIBRARY-HASH.d`).
/// Cargo built the library for this very test, so its dep-info is there.
fn compiled_files(library: &str, root: &Path) -> (usize, BTreeSet<PathBuf>) {
    let exe = std::env::current_exe().expect("the test binary");
    let deps = exe.parent().expect("the deps directory");
    let mut infos = 0;
    let mut files = BTreeSet::new();
    for entry in std::fs::read_dir(deps).expect("the deps directory").flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&format!("{library}-")) && name.ends_with(".d") {
            infos += 1;
            let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
            files.extend(dep_info_files(&text, root));
        }
    }
    (infos, files)
}

/// (2) The compiled files that are not under `dir`, the workspace manifest
/// aside (rustc lists it for the inherited package fields; it is not code).
fn outside_dir(files: &BTreeSet<PathBuf>, dir: &Path, manifest: &Path) -> Vec<String> {
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_owned());
    let manifest = manifest
        .canonicalize()
        .unwrap_or_else(|_| manifest.to_owned());
    files
        .iter()
        .filter_map(|file| {
            // Resolved, so a path that goes through a link is judged by
            // where it lands.
            let real = file.canonicalize().unwrap_or_else(|_| file.clone());
            (real != manifest && !real.starts_with(&dir)).then(|| real.display().to_string())
        })
        .collect()
}

/// (3) Every symlink under `dir`, not followed.
fn symlinks(dir: &Path) -> Vec<String> {
    let mut out = vec![];
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            out.push(path.display().to_string());
        } else if meta.is_dir() {
            out.extend(symlinks(&path));
        }
    }
    out
}

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn the_screen_reaches_no_workspace_crate_but_the_public_client() {
    let metadata = metadata();
    // The graph is the real one: the CLI is in it and does reach the
    // service, so a checker that saw nothing would fail here first.
    assert!(workspace_reach(&metadata, "pio-cli").contains("pio-core"));
    let reached = workspace_reach(&metadata, PACKAGE);
    let allowed: BTreeSet<String> = WORKSPACE_ALLOWED.iter().map(|s| s.to_string()).collect();
    let outside: Vec<&String> = reached.difference(&allowed).collect();
    assert!(
        outside.is_empty(),
        "pio-tui reaches workspace crates other than pio-client: {outside:?}; \
         the screen must use the public client only"
    );
    assert!(reached.contains("pio-client"), "pio-tui is built on pio-client");
}

#[test]
fn the_screen_declares_only_what_its_allow_lists_name() {
    let outside = outside_allow_list(&metadata(), PACKAGE, &WORKSPACE_ALLOWED, &REGISTRY_ALLOWED);
    assert!(
        outside.is_empty(),
        "pio-tui declares dependencies outside its allow-lists: {outside:?}"
    );
}

#[test]
fn every_file_rustc_compiled_into_the_screen_is_its_own() {
    let metadata = metadata();
    let root = PathBuf::from(metadata["workspace_root"].as_str().expect("workspace root"));
    let (infos, files) = compiled_files(LIBRARY, &root);
    assert!(infos > 0, "no dep-info for {LIBRARY} beside the test binary");
    assert!(
        files.iter().any(|f| f.ends_with("crates/pio-tui/src/lib.rs")),
        "the dep-info does not name pio-tui's own lib.rs: {files:?}"
    );
    let outside = outside_dir(&files, &crate_dir(), &root.join("Cargo.toml"));
    assert!(
        outside.is_empty(),
        "rustc compiled files from outside crates/pio-tui into the screen: {outside:?}"
    );
}

#[test]
fn nothing_under_the_screens_crate_is_a_symlink() {
    let found = symlinks(&crate_dir());
    assert!(
        found.is_empty(),
        "pio-tui's directory holds a symlink: {found:?}"
    );
}

/// The checkers themselves, on inputs where each must fire and must not.
#[test]
fn each_checker_fires_on_what_it_forbids() {
    let graph = |kind: &str| {
        serde_json::json!({
            "workspace_members": ["t", "c", "k"],
            "packages": [
                {"id": "t", "name": "pio-tui", "dependencies": [
                    {"name": "pio-client", "kind": null, "path": "/x/pio-client"},
                    {"name": "middle", "kind": null},
                    {"name": "pio-core", "kind": null, "path": "/x/pio-core"}]},
                {"id": "c", "name": "pio-client"},
                {"id": "m", "name": "middle"},
                {"id": "k", "name": "pio-core"}],
            "resolve": {"nodes": [
                {"id": "t", "deps": [
                    {"pkg": "c", "dep_kinds": [{"kind": null}]},
                    {"pkg": "m", "dep_kinds": [{"kind": kind}]}]},
                {"id": "c", "deps": []},
                {"id": "m", "deps": [{"pkg": "k", "dep_kinds": [{"kind": null}]}]},
                {"id": "k", "deps": []}]}})
    };
    // Through a registry crate in the middle, still reached.
    assert_eq!(
        workspace_reach(&graph("build"), "pio-tui"),
        BTreeSet::from(["pio-client".to_owned(), "pio-core".to_owned()])
    );
    assert_eq!(
        workspace_reach(&graph("dev"), "pio-tui"),
        BTreeSet::from(["pio-client".to_owned()])
    );
    assert_eq!(
        outside_allow_list(&graph("build"), "pio-tui", &["pio-client"], &["anyhow"]),
        ["middle (not on the allow-list)", "pio-core (by path)"]
    );

    let root = Path::new("/w");
    let info = "/w/target/debug/deps/x-1.d: crates/a/src/lib.rs /w/crates/pio-core/src/x.rs\n\n\
                crates/a/src/lib.rs:\n/w/crates/pio-core/src/x.rs:\n/tmp/a\\ b/c.rs:\n\n\
                # env-dep:X=1\n";
    let files: BTreeSet<PathBuf> = dep_info_files(info, root).into_iter().collect();
    assert_eq!(
        files,
        BTreeSet::from([
            PathBuf::from("/w/crates/a/src/lib.rs"),
            PathBuf::from("/w/crates/pio-core/src/x.rs"),
            PathBuf::from("/tmp/a b/c.rs"),
        ])
    );
    assert_eq!(
        outside_dir(&files, Path::new("/w/crates/a"), Path::new("/w/Cargo.toml")),
        ["/tmp/a b/c.rs", "/w/crates/pio-core/src/x.rs"]
    );
}
