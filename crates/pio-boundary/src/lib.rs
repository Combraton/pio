//! M4 rule 1, held structurally: the screen reads and writes only through
//! the public API the command line uses.
//!
//! A crate on the public wire (pio-client, pio-client-cli, and the screen's
//! crate when it lands) is checked four ways, none of them a text search of
//! its sources, because text searches lost to every new spelling the
//! verifier tried (review of T1, round 2: a `cfg_attr` path, a spaced
//! attribute, a symlinked module, a helper module, a glob import):
//!
//! 1. **The graph**, from `cargo metadata --all-features`: no path, direct
//!    or transitive, to a PIO service crate ([`FORBIDDEN`]).
//! 2. **An allow-list** of the crate's declared dependencies: registry crates
//!    named in [`PublicWire::allowed`], workspace crates only if named in
//!    [`PublicWire::internal`] (public-wire crates themselves).
//! 3. **What the compiler compiled**: the crate is checked on its own, every
//!    target and every feature, and the dep-info rustc writes lists every
//!    source file that went in — through `mod`, a `#[path]` in any spelling,
//!    `cfg_attr`, or `include!`. Each must resolve (symlinks followed) to a
//!    file inside the crate's directory. A crate that does not build on its
//!    own declared dependencies fails here too: naming a crate it does not
//!    depend on is refused by the compiler, not by a pattern.
//! 4. **No symlink** anywhere inside the crate's directory.
//!
//! To put a crate under the boundary, add a [`PublicWire`] row to
//! `tests/boundary.rs`.
use serde_json::Value;
use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Every PIO crate that is the service (pio-core, pio-protocol) or runs
/// under it (pio-host and the adapters). A path to any one of them is a
/// path past the wire.
pub const FORBIDDEN: [&str; 6] = [
    "pio-core",
    "pio-protocol",
    "pio-host",
    "pio-codex",
    "pio-claude",
    "pio-opencode",
];

/// One crate that must stay on the public wire.
#[derive(Clone, Copy, Debug)]
pub struct PublicWire {
    pub name: &'static str,
    /// Its directory, relative to the workspace root.
    pub dir: &'static str,
    /// Registry crates it may declare (any kind but dev).
    pub allowed: &'static [&'static str],
    /// Workspace crates it may declare by path: public-wire crates only.
    pub internal: &'static [&'static str],
}

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn cargo() -> Command {
    Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
}

/// The resolved graph with every feature on.
pub fn metadata(root: &Path) -> Result<Value, String> {
    let output = cargo()
        .args(["metadata", "--format-version", "1", "--all-features"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())
}

fn package<'a>(metadata: &'a Value, name: &str) -> Option<&'a Value> {
    metadata["packages"]
        .as_array()?
        .iter()
        .find(|p| p["name"] == name)
}

/// The forbidden packages `name` reaches through normal or build
/// dependencies. Dev-dependencies are not shipped, so they are not counted.
pub fn forbidden_reach(metadata: &Value, name: &str) -> Vec<String> {
    let packages = metadata["packages"].as_array().cloned().unwrap_or_default();
    let name_of = |id: &str| {
        packages
            .iter()
            .find(|p| p["id"] == id)
            .and_then(|p| p["name"].as_str())
            .unwrap_or(id)
            .to_owned()
    };
    let nodes = metadata["resolve"]["nodes"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let Some(root) = package(metadata, name).and_then(|p| p["id"].as_str()) else {
        return vec![format!("{name} is not in the graph")];
    };
    let mut seen = BTreeSet::from([root.to_owned()]);
    let mut queue = VecDeque::from([root.to_owned()]);
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
            let found = name_of(&target);
            if FORBIDDEN.contains(&found.as_str()) {
                reached.insert(found);
            }
            queue.push_back(target);
        }
    }
    reached.into_iter().collect()
}

/// Declared dependencies (optional ones included, every kind but dev) that
/// break the crate's allow-list.
pub fn outside_allow_list(metadata: &Value, wire: &PublicWire) -> Vec<String> {
    let Some(declared) = package(metadata, wire.name) else {
        return vec![format!("{} is not in the graph", wire.name)];
    };
    let mut out = vec![];
    for dep in declared["dependencies"].as_array().into_iter().flatten() {
        if dep["kind"] == "dev" {
            continue;
        }
        let name = dep["name"].as_str().unwrap_or_default();
        let by_path = dep.get("path").is_some_and(|p| !p.is_null());
        if by_path && !wire.internal.contains(&name) {
            out.push(format!("{name} (by path)"));
        } else if !by_path && !wire.allowed.contains(&name) {
            out.push(format!("{name} (not on the allow-list)"));
        }
    }
    out
}

/// The files a dep-info file lists, as rustc wrote them (Makefile syntax:
/// one `path:` rule per source file, spaces escaped).
pub fn dep_info_files(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.strip_suffix(':'))
        .filter(|path| !path.contains(": "))
        .map(|path| PathBuf::from(path.replace("\\ ", " ").replace("\\\\", "\\")))
        .collect()
}

/// Every source file compiled into `wire`, for every target and feature,
/// resolved through symlinks. Checks the crate on its own into a target
/// directory of the checker's own (`$CARGO_TARGET_DIR/boundary`), so a
/// crate that cannot build on its declared dependencies is an error here.
pub fn compiled_sources(
    root: &Path,
    metadata: &Value,
    wire: &PublicWire,
) -> Result<Vec<PathBuf>, String> {
    let id = package(metadata, wire.name)
        .and_then(|p| p["id"].as_str())
        .ok_or_else(|| format!("{} is not in the graph", wire.name))?
        .to_owned();
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"))
        .join("boundary");
    let output = cargo()
        .args([
            "check",
            "--quiet",
            "--all-targets",
            "--all-features",
            "--message-format=json",
            "-p",
        ])
        .arg(wire.name)
        .env("CARGO_TARGET_DIR", &target)
        .current_dir(root)
        .output()
        .map_err(|e| format!("cargo check: {e}"))?;
    let messages: Vec<Value> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    if !output.status.success() {
        let rendered: Vec<String> = messages
            .iter()
            .filter(|m| m["reason"] == "compiler-message")
            .filter_map(|m| m["message"]["rendered"].as_str().map(str::to_owned))
            .collect();
        return Err(format!(
            "{}{}",
            rendered.join("\n"),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let mut files = BTreeSet::new();
    let mut units = 0;
    for message in &messages {
        if message["reason"] != "compiler-artifact" || message["package_id"] != id.as_str() {
            continue;
        }
        let Some(first) = message["filenames"][0].as_str().map(PathBuf::from) else {
            continue;
        };
        // Each unit's dep-info sits beside its output, named for the same
        // unit hash: `deps/<crate>-<hash>.d`.
        let stem = first
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let hash = stem.rsplit_once('-').map(|(_, h)| h).unwrap_or(stem);
        let dir = first.parent().unwrap_or(Path::new("."));
        let found: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.extension().is_some_and(|e| e == "d")
                    && p.file_stem()
                        .and_then(|s| s.to_str())
                        .is_some_and(|s| s.ends_with(&format!("-{hash}")))
            })
            .collect();
        if found.is_empty() {
            return Err(format!("no dep-info for {}", first.display()));
        }
        for path in found {
            let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            for file in dep_info_files(&text) {
                // Cargo runs rustc from the workspace root, so a relative
                // path in dep-info is relative to it.
                let file = if file.is_absolute() {
                    file
                } else {
                    root.join(file)
                };
                files.insert(file.canonicalize().unwrap_or(file));
            }
        }
        units += 1;
    }
    if units == 0 {
        return Err(format!("cargo compiled no unit of {}", wire.name));
    }
    Ok(files.into_iter().collect())
}

/// Every symlink inside `dir`, at any depth.
pub fn symlinks_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => out.push(path),
            Ok(meta) if meta.is_dir() => out.extend(symlinks_under(&path)),
            _ => {}
        }
    }
    out
}

/// Every way `wire` breaks the boundary, in words; empty when it holds.
pub fn check(root: &Path, metadata: &Value, wire: &PublicWire) -> Vec<String> {
    let mut out = vec![];
    let reached = forbidden_reach(metadata, wire.name);
    if !reached.is_empty() {
        out.push(format!(
            "{} reaches PIO service internals through {reached:?}",
            wire.name
        ));
    }
    let outside = outside_allow_list(metadata, wire);
    if !outside.is_empty() {
        out.push(format!(
            "{} declares dependencies outside its allow-list: {outside:?}",
            wire.name
        ));
    }
    let dir = root
        .join(wire.dir)
        .canonicalize()
        .unwrap_or(root.join(wire.dir));
    let links = symlinks_under(&dir);
    if !links.is_empty() {
        let shown: Vec<String> = links
            .iter()
            .map(|p| p.strip_prefix(root).unwrap_or(p).display().to_string())
            .collect();
        out.push(format!(
            "{} has symlinks inside its directory: {shown:?}",
            wire.name
        ));
    }
    match compiled_sources(root, metadata, wire) {
        Ok(files) => {
            let foreign: Vec<String> = files
                .iter()
                .filter(|f| !f.starts_with(&dir))
                .map(|f| f.strip_prefix(root).unwrap_or(f).display().to_string())
                .collect();
            if !foreign.is_empty() {
                out.push(format!(
                    "{} compiles source files from outside its directory: {foreign:?}",
                    wire.name
                ));
            }
        }
        Err(error) => out.push(format!(
            "{} does not build on its own declared dependencies:\n{error}",
            wire.name
        )),
    }
    out
}
