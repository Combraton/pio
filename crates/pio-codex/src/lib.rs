//! Codex 0.157.0 qualification. Binds the user-selected executable, the native
//! binary it actually runs and the per-file canonical app-server schema identity
//! before any native work. Qualification runs the selected executable only with
//! an isolated `CODEX_HOME`; it never reads or writes the user's Codex home and
//! makes no model call.
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Re-pinned from 0.155.1 on 2026-09-25 (owner decision): the owner's Codex
/// updated itself, and PIO drives the Codex that is installed. The evidence is
/// in `docs/work/m2/codex-qualification/`, "Re-qualification at 0.157.0".
pub const PINNED_VERSION: &str = "0.157.0";
pub const PINNED_TAG: &str = "rust-v0.157.0";
pub const PINNED_SOURCE: &str = "00c972ed5d6ff6499317fd41b7f23605b8e6850d";

/// Canonical schema identity captured from the qualified 0.157.0 binary.
pub const QUALIFIED_SCHEMA_IDENTITY: &str =
    include_str!("../../../adapters/codex/0.157.0/schema-identity.json");

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn sha256_file(path: &Path) -> Result<String> {
    Ok(sha256_hex(
        &std::fs::read(path).with_context(|| format!("read {}", path.display()))?,
    ))
}

/// Canonical parsed JSON: object members sorted by key bytes, arrays and
/// values preserved, compact. Equal for byte-different but parsed-equal files.
pub fn canonical_json(value: &Value) -> String {
    fn write(value: &Value, out: &mut String) {
        match value {
            Value::Object(map) => {
                out.push('{');
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                for (i, key) in keys.into_iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(key).expect("string key"));
                    out.push(':');
                    write(&map[key], out);
                }
                out.push('}');
            }
            Value::Array(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write(item, out);
                }
                out.push(']');
            }
            scalar => out.push_str(&serde_json::to_string(scalar).expect("scalar")),
        }
    }
    let mut out = String::new();
    write(value, &mut out);
    out
}

fn files_under(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_owned()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            ensure!(
                !kind.is_symlink(),
                "schema output contains a symlink: {}",
                entry.path().display()
            );
            if kind.is_dir() {
                pending.push(entry.path());
            } else {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Per-file identity of a generated schema directory. Only canonical digests
/// are compared; raw digests are capture provenance.
pub fn schema_identity(root: &Path) -> Result<Value> {
    let mut files = BTreeMap::new();
    for path in files_under(root)? {
        let relative = path
            .strip_prefix(root)?
            .to_str()
            .context("non-UTF-8 schema path")?
            .replace('\\', "/");
        let bytes = std::fs::read(&path)?;
        let parsed: Value = serde_json::from_slice(&bytes)
            .with_context(|| format!("schema file is not JSON: {relative}"))?;
        let canonical = sha256_hex(canonical_json(&parsed).as_bytes());
        let raw = sha256_hex(&bytes);
        files.insert(
            relative,
            json!({"canonical_sha256":canonical,"raw_sha256":raw}),
        );
    }
    // Listings follow the relative-path string order of the map.
    let mut canonical_listing = String::new();
    let mut raw_listing = String::new();
    for (relative, digests) in &files {
        canonical_listing.push_str(&format!(
            "{relative}\t{}\n",
            digests["canonical_sha256"].as_str().unwrap_or_default()
        ));
        raw_listing.push_str(&format!(
            "{relative}\t{}\n",
            digests["raw_sha256"].as_str().unwrap_or_default()
        ));
    }
    Ok(json!({
        "format":"pio-codex-schema-identity/1",
        "files":files,
        "file_count":files.len(),
        "canonical_listing_sha256":sha256_hex(canonical_listing.as_bytes()),
        "raw_listing_sha256":sha256_hex(raw_listing.as_bytes()),
    }))
}

/// Differences in file set or canonical content. Raw-only differences are not drift.
pub fn schema_drift(expected: &Value, actual: &Value) -> Vec<Value> {
    let empty = serde_json::Map::new();
    let expected = expected["files"].as_object().unwrap_or(&empty);
    let actual = actual["files"].as_object().unwrap_or(&empty);
    let mut drift = Vec::new();
    for (file, identity) in expected {
        match actual.get(file) {
            None => drift.push(json!({"file":file,"change":"removed"})),
            Some(found) if found["canonical_sha256"] != identity["canonical_sha256"] => {
                drift.push(json!({"file":file,"change":"changed"}))
            }
            _ => {}
        }
    }
    for file in actual.keys().filter(|file| !expected.contains_key(*file)) {
        drift.push(json!({"file":file,"change":"added"}));
    }
    drift
}

fn target_triple() -> Result<&'static str> {
    Ok(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-musl",
        ("linux", "aarch64") => "aarch64-unknown-linux-musl",
        (os, arch) => bail!("unsupported Codex platform {os}/{arch}"),
    })
}

fn platform_package(triple: &str) -> &'static str {
    match triple {
        "aarch64-apple-darwin" => "@openai/codex-darwin-arm64",
        "x86_64-apple-darwin" => "@openai/codex-darwin-x64",
        "x86_64-unknown-linux-musl" => "@openai/codex-linux-x64",
        _ => "@openai/codex-linux-arm64",
    }
}

/// First executable regular file named `program` on `path`, as `execvp` finds it.
fn find_on_path(program: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::env::split_paths(path?)
        .map(|dir| dir.join(program))
        .find(|candidate| {
            std::fs::metadata(candidate)
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

/// Resolve what the selected executable runs, mirroring the pinned npm
/// wrapper: `require.resolve` of the platform package from the wrapper's
/// directory, falling back to the package's own `vendor` directory.
pub fn resolve(selected: &Path, path: Option<&OsStr>) -> Result<Value> {
    let resolved = std::fs::canonicalize(selected)
        .with_context(|| format!("selected executable not found: {}", selected.display()))?;
    let head = std::fs::read(&resolved)?;
    let first_line = head.split(|b| *b == b'\n').next().unwrap_or_default();
    let is_node_wrapper = first_line.starts_with(b"#!")
        && String::from_utf8_lossy(first_line).contains("node")
        && resolved.file_name() == Some(OsStr::new("codex.js"));
    if !is_node_wrapper {
        return Ok(json!({
            "kind":"native",
            "selected":selected,
            "resolved":resolved,
            "native":{"path":resolved,"sha256":sha256_hex(&head)},
        }));
    }
    let bin = resolved.parent().context("wrapper directory")?;
    let package_root = std::fs::canonicalize(bin.join(".."))?;
    let package: Value =
        serde_json::from_slice(&std::fs::read(package_root.join("package.json"))?)?;
    ensure!(
        package["name"] == "@openai/codex",
        "wrapper package is not @openai/codex"
    );
    let triple = target_triple()?;
    let platform = platform_package(triple);
    let mut vendor_root = None;
    for dir in bin.ancestors() {
        if dir.file_name() == Some(OsStr::new("node_modules")) {
            continue;
        }
        let candidate = dir.join("node_modules").join(platform).join("package.json");
        if candidate.is_file() {
            vendor_root = Some(candidate.parent().unwrap().join("vendor"));
            break;
        }
    }
    let vendor_root = vendor_root.unwrap_or_else(|| package_root.join("vendor"));
    let native = vendor_root.join(triple).join("bin").join("codex");
    ensure!(
        native.is_file(),
        "native Codex binary missing for {triple}: {}",
        native.display()
    );
    let layout_path = vendor_root.join(triple).join("codex-package.json");
    let layout: Value = if layout_path.is_file() {
        serde_json::from_slice(&std::fs::read(&layout_path)?)?
    } else {
        Value::Null
    };
    let node = find_on_path("node", path).map(|node| std::fs::canonicalize(&node).unwrap_or(node));
    // The wrapper's `#!/usr/bin/env node` uses whichever node is first on PATH.
    let node_record = match &node {
        Some(node) => {
            let mut command = Command::new(node);
            command.arg("--version");
            if let Some(path) = path {
                command.env("PATH", path);
            }
            let version = command
                .output()
                .ok()
                .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned());
            json!({"path":node,"sha256":sha256_file(node)?,"version":version})
        }
        None => Value::Null,
    };
    Ok(json!({
        "kind":"npm_node_wrapper",
        "selected":selected,
        "resolved":resolved,
        "wrapper":{"path":resolved,"sha256":sha256_hex(&head),"package_version":package["version"]},
        "node":node_record,
        "target":triple,
        "platform_package":platform,
        "native":{"path":native,"sha256":sha256_file(&native)?,"layout":layout},
    }))
}

fn run(program: &Path, args: &[&str], home: &Path, path: Option<&OsStr>) -> Result<(i32, String)> {
    let mut command = Command::new(program);
    command.args(args).env("CODEX_HOME", home);
    match path {
        Some(path) => command.env("PATH", path),
        None => command.env_remove("PATH"),
    };
    let output = command
        .output()
        .with_context(|| format!("run {}", program.display()))?;
    Ok((
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).trim().to_owned(),
    ))
}

fn parse_version(output: &str) -> Option<String> {
    output
        .strip_prefix("codex-cli ")
        .map(|version| version.trim().to_owned())
}

/// Qualify a selected executable against the pinned version and expected
/// schema identity. Refusals are data (`qualified: false`, `refusals`), so
/// discovery can report a wrong or unsupported executable truthfully.
pub fn qualify(
    selected: &Path,
    expected_identity: &Value,
    path: Option<&OsStr>,
    work: &Path,
) -> Result<Value> {
    let pinned = json!({"version":PINNED_VERSION,"tag":PINNED_TAG,"source":PINNED_SOURCE});
    let resolution = match resolve(selected, path) {
        Ok(resolution) => resolution,
        Err(error) => {
            return Ok(json!({
                "format":"pio-codex-qualification/1","pinned":pinned,"selected":selected,
                "qualified":false,"refusals":[{"reason":"unresolved_executable","detail":format!("{error:#}")}],
            }));
        }
    };
    std::fs::create_dir_all(work)?;
    let home = work.join("isolated-codex-home");
    std::fs::create_dir_all(&home)?;
    let mut refusals = Vec::new();
    let (status, output) = run(selected, &["--version"], &home, path)?;
    let version = parse_version(&output);
    let native_path = PathBuf::from(resolution["native"]["path"].as_str().unwrap_or_default());
    let native_version = if resolution["kind"] == "npm_node_wrapper" {
        let (_, native_output) = run(&native_path, &["--version"], &home, path)?;
        parse_version(&native_output)
    } else {
        version.clone()
    };
    if status != 0 || version.is_none() {
        refusals.push(json!({"reason":"version_unavailable","exit":status}));
    } else if version.as_deref() != Some(PINNED_VERSION) {
        refusals.push(json!({"reason":"unsupported_version","observed":version}));
    }
    if native_version != version {
        refusals.push(
            json!({"reason":"native_version_mismatch","selected":version,"native":native_version}),
        );
    }
    // Never run an unqualified executable with Codex-specific arguments.
    if !refusals.is_empty() {
        return Ok(json!({
            "format":"pio-codex-qualification/1",
            "pinned":pinned,
            "selected":selected,
            "resolution":resolution,
            "isolated_codex_home":true,
            "version":{"selected":version,"native":native_version},
            "schema":{"skipped":"version_not_qualified"},
            "qualified":false,
            "refusals":refusals,
        }));
    }
    let schema_dir = work.join("schema");
    if schema_dir.exists() {
        std::fs::remove_dir_all(&schema_dir)?;
    }
    std::fs::create_dir_all(&schema_dir)?;
    let schema_out: &str = schema_dir.to_str().context("non-UTF-8 work path")?;
    let (generator_exit, _) = run(
        selected,
        &["app-server", "generate-json-schema", "--out", schema_out],
        &home,
        path,
    )?;
    let mut schema = json!({"generator_exit":generator_exit});
    if generator_exit != 0 {
        refusals.push(json!({"reason":"schema_generation_failed","exit":generator_exit}));
    } else {
        let identity = schema_identity(&schema_dir)?;
        let drift = schema_drift(expected_identity, &identity);
        schema = json!({
            "generator_exit":generator_exit,
            "file_count":identity["file_count"],
            "canonical_listing_sha256":identity["canonical_listing_sha256"],
            "raw_listing_sha256":identity["raw_listing_sha256"],
            "expected_canonical_listing_sha256":expected_identity["canonical_listing_sha256"],
            "drift_count":drift.len(),
            "drift":drift.iter().take(20).cloned().collect::<Vec<_>>(),
        });
        if !drift.is_empty() {
            refusals.push(json!({"reason":"schema_drift","files":drift.len()}));
        }
    }
    Ok(json!({
        "format":"pio-codex-qualification/1",
        "pinned":pinned,
        "selected":selected,
        "resolution":resolution,
        "isolated_codex_home":true,
        "version":{"selected":version,"native":native_version},
        "schema":schema,
        "qualified":refusals.is_empty(),
        "refusals":refusals,
    }))
}

/// Parse a TOML basic string starting at the opening quote; returns the value
/// and the rest of the input. Only the escapes a quoted project path needs.
fn toml_basic_string(input: &str) -> Option<(String, &str)> {
    let mut chars = input.strip_prefix('"')?.char_indices();
    let mut out = String::new();
    while let Some((index, c)) = chars.next() {
        match c {
            '"' => return Some((out, &input[index + 2..])),
            '\\' => match chars.next()?.1 {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                't' => out.push('\t'),
                'n' => out.push('\n'),
                _ => return None,
            },
            c => out.push(c),
        }
    }
    None
}

/// Project trust entries from `[projects."PATH"]` tables and the remaining
/// text with those tables removed. This is a targeted scan for the entry that
/// `thread/start` writes, not a general TOML parser; the raw digest still
/// detects every other change.
fn project_tables(text: &str) -> (BTreeMap<String, Option<String>>, String) {
    let mut projects = BTreeMap::new();
    let mut other = String::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            current = trimmed
                .strip_prefix("[projects.")
                .and_then(toml_basic_string)
                .filter(|(_, rest)| rest.trim() == "]")
                .map(|(path, _)| path);
            if let Some(path) = &current {
                projects.entry(path.clone()).or_insert(None);
                continue;
            }
        }
        match &current {
            Some(path) => {
                if let Some(value) = trimmed
                    .strip_prefix("trust_level")
                    .map(str::trim_start)
                    .and_then(|rest| rest.strip_prefix('='))
                    .map(str::trim_start)
                    .and_then(toml_basic_string)
                {
                    projects.insert(path.clone(), Some(value.0));
                } else if !trimmed.is_empty() {
                    other.push_str(&format!("[projects.{path:?}] {trimmed}\n"));
                }
            }
            None => {
                other.push_str(line);
                other.push('\n');
            }
        }
    }
    (projects, other)
}

/// Top-level settings that decide the thread defaults at 0.155.1 and 0.157.0, plus the
/// configured `model` so a receipt can state it. Values are the raw right-hand
/// side; `tables` lists `[permissions…]` headers and whether any table was seen
/// before a key (keys after a header belong to it).
fn top_level_settings(text: &str) -> Value {
    let mut keys = serde_json::Map::new();
    let mut permission_tables = false;
    let mut in_table = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_table = true;
            if trimmed.starts_with("[permissions") {
                permission_tables = true;
            }
            continue;
        }
        if in_table || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim();
            if matches!(
                key,
                "sandbox_mode" | "approval_policy" | "default_permissions" | "profile" | "model"
            ) {
                let value = value.trim();
                let parsed = toml_basic_string(value)
                    .filter(|(_, rest)| rest.trim().is_empty() || rest.trim().starts_with('#'))
                    .map(|(v, _)| json!(v))
                    .unwrap_or_else(|| json!({"unparsed":value}));
                keys.insert(key.to_owned(), parsed);
            }
        }
    }
    json!({"keys":keys,"permission_tables":permission_tables})
}

const SANDBOX_ORDER: &[&str] = &["read-only", "workspace-write", "danger-full-access"];
/// Fewer approval prompts is broader. `untrusted` asks most; `never` never asks.
const APPROVAL_ORDER: &[&str] = &["untrusted", "on-request", "never"];

/// Refuse requested thread settings broader than the user's configured default
/// (owner guard, 2026-09-17). An absent setting is Codex's default for a
/// trusted project, `workspace-write` and `on-request`, which the offline probe
/// measures rather than infers, at 0.155.1 and again at 0.157.0: with the project already trusted and no setting
/// configured, `thread/start` projects `workspaceWrite` and `on-request`. The
/// same probe shows an untrusted project defaults to `readOnly` and that
/// requesting `workspace-write` is what trusts it, so every run discloses the
/// trust entry it adds. Settings this scan cannot resolve — a legacy `profile`,
/// named `default_permissions`, `[permissions]` tables, a configured
/// `approval_policy = "untrusted"` that Codex refuses to start with, or a
/// non-string or unknown value — refuse rather than guess.
pub fn thread_settings_guard(snapshot: &Value, requested: &Value) -> Value {
    let settings = &snapshot["settings"];
    let mut unresolved = Vec::new();
    for key in ["profile", "default_permissions"] {
        if settings["keys"].get(key).is_some() {
            unresolved.push(json!({"setting":key,"reason":"changes defaults in ways this guard does not evaluate"}));
        }
    }
    if settings["permission_tables"] == true {
        unresolved.push(
            json!({"setting":"permissions","reason":"named permission profiles are not evaluated"}),
        );
    }
    let mut configured = serde_json::Map::new();
    let mut broader = Vec::new();
    for (key, request_key, default, order) in [
        ("sandbox_mode", "sandbox", "workspace-write", SANDBOX_ORDER),
        (
            "approval_policy",
            "approvalPolicy",
            "on-request",
            APPROVAL_ORDER,
        ),
    ] {
        let (value, source) = match settings["keys"].get(key) {
            Some(Value::String(value)) => (value.clone(), "configured"),
            Some(other) => {
                unresolved.push(json!({"setting":key,"reason":"not a plain string","value":other}));
                continue;
            }
            None => (default.to_owned(), "absent_trusted_project_default"),
        };
        if key == "approval_policy" && source == "configured" && value == "untrusted" {
            // Measured at 0.155.1 and again at 0.157.0: the app-server exits 1
            // before answering
            // `initialize` (`UnsupportedUntrustedApprovalPolicyError`). Refuse
            // rather than spawn something that cannot start. Requesting
            // `untrusted` per thread is still accepted.
            unresolved.push(json!({"setting":key,"reason":format!("Codex {PINNED_VERSION} does not start with this configured value"),"value":value}));
            continue;
        }
        let Some(configured_rank) = order.iter().position(|v| *v == value) else {
            unresolved.push(json!({"setting":key,"reason":"unknown value","value":value}));
            continue;
        };
        configured.insert(key.to_owned(), json!({"value":value,"source":source}));
        if let Some(request) = requested.get(request_key).and_then(Value::as_str) {
            match order.iter().position(|v| *v == request) {
                Some(rank) if rank <= configured_rank => {}
                Some(_) => {
                    broader.push(json!({"setting":key,"requested":request,"configured":value}))
                }
                None => unresolved.push(
                    json!({"setting":key,"reason":"unknown requested value","value":request}),
                ),
            }
        }
    }
    json!({
        "format":"pio-codex-thread-settings-guard/1",
        "config_exists":snapshot["exists"],
        "configured":configured,
        "requested":requested,
        "unresolved":unresolved,
        "broader_than_configured":broader,
        "allowed":unresolved.is_empty() && broader.is_empty(),
    })
}

/// Snapshot of `$CODEX_HOME/config.toml` for before/after disclosure. The
/// returned record holds digests and project trust entries; callers keep any
/// raw copy outside Git.
pub fn config_snapshot(codex_home: &Path) -> Result<Value> {
    let path = codex_home.join("config.toml");
    if !path.exists() {
        // A missing file compares as empty content, so creating a file that
        // holds only a trust entry is not reported as another change.
        return Ok(json!({
            "format":"pio-codex-config-snapshot/1",
            "exists":false,
            "raw_sha256":null,
            "bytes":0,
            "projects":{},
            "outside_projects_sha256":sha256_hex(b""),
            "settings":top_level_settings(""),
        }));
    }
    let bytes = std::fs::read(&path)?;
    let text = String::from_utf8(bytes.clone()).context("config.toml is not UTF-8")?;
    let (projects, other) = project_tables(&text);
    Ok(json!({
        "format":"pio-codex-config-snapshot/1",
        "exists":true,
        "raw_sha256":sha256_hex(&bytes),
        "bytes":bytes.len(),
        "projects":projects,
        "outside_projects_sha256":sha256_hex(other.as_bytes()),
        "settings":top_level_settings(&text),
    }))
}

/// Before/after difference. Project paths are reported as digests plus a label:
/// `fixture` when under `fixture_root`, otherwise `outside_fixture_root`.
pub fn config_diff(before: &Value, after: &Value, fixture_root: Option<&Path>) -> Value {
    let empty = serde_json::Map::new();
    let old = before["projects"].as_object().unwrap_or(&empty);
    let new = after["projects"].as_object().unwrap_or(&empty);
    let describe = |path: &str, trust: &Value| {
        let label = match fixture_root {
            Some(root) if Path::new(path).starts_with(root) => "fixture",
            _ => "outside_fixture_root",
        };
        json!({"path_sha256":sha256_hex(path.as_bytes()),"location":label,"trust_level":trust})
    };
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    for (path, trust) in new {
        match old.get(path) {
            None => added.push(describe(path, trust)),
            Some(previous) if previous != trust => {
                let mut entry = describe(path, trust);
                entry["previous_trust_level"] = previous.clone();
                changed.push(entry);
            }
            _ => {}
        }
    }
    for (path, trust) in old {
        if !new.contains_key(path) {
            removed.push(describe(path, trust));
        }
    }
    json!({
        "format":"pio-codex-config-diff/1",
        "unchanged":before["raw_sha256"] == after["raw_sha256"] && before["exists"] == after["exists"],
        "existed_before":before["exists"],
        "exists_after":after["exists"],
        "raw_sha256":{"before":before["raw_sha256"],"after":after["raw_sha256"]},
        "projects_added":added,
        "projects_removed":removed,
        "projects_changed":changed,
        "other_changes":before["outside_projects_sha256"] != after["outside_projects_sha256"],
    })
}

/// Codex 0.157.0's unmetered, default-on features and the per-launch
/// override that turns each off on a thread, by feature: the dotted keys a
/// `thread/start` `config` takes, each a request override Codex merges over
/// the user's `config.toml` as it merges a `-c key=value`
/// (`app-server/src/config_manager.rs:445-452`, `json_to_toml`), so it
/// replaces the user's value for that key on that thread and changes nothing
/// on disk. Read from source at `rust-v0.157.0` (00c972e), not measured
/// (review of L3, round 4, SPEND-8, SPEND-9 and the web-search and
/// image-generation gaps). Each spends outside every report PIO reads:
///
/// - **sub-agents**: `agents.enabled = false` turns them off whatever the
///   model's catalog says unless `multi_agent_v2` is on
///   (`core/src/config/mod.rs:1562-1570`); `features.multi_agent`
///   (`features/src/lib.rs:1319`) and `features.multi_agent_v2` (`:1325`)
///   false keep the override from being V1 or V2;
/// - **memories**: `features.memories = false` (`features/src/lib.rs:1147`)
///   and its legacy alias `features.memory_tool = false`
///   (`features/src/legacy.rs:41`), which sorts after it and would decide
///   it (`Features::apply_map` walks a `BTreeMap`); the pipeline starts only
///   with the feature on (`memories/write/src/start.rs:33-34`, called from
///   `app-server/src/request_processors/turn_processor.rs:689`);
/// - **goals**: `features.goals = false` (`features/src/lib.rs:1673`), which
///   hides the goal tools and stops a continuation turn
///   (`app-server/src/extensions.rs:85`, `ext/goal/src/runtime.rs:425-429`);
/// - **standalone web search**: `web_search = "disabled"`
///   (`config/src/config_toml.rs:470`), which decides the mode before any
///   feature does (`core/src/config/mod.rs:2659-2662`) and keeps `web.run`
///   unregistered (`core/src/tools/spec_plan.rs:1429-1437`);
/// - **image generation**: `features.image_generation = false`
///   (`features/src/lib.rs:1535`, checked at
///   `core/src/tools/spec_plan.rs:694-701`); with it present, the legacy
///   alias `imagegenext` is ignored (`features/src/lib.rs:645`).
///
/// Sent only under the owner's recorded decision (pio-protocol,
/// `FEATURES_OFF_DECISIONS`), or a labeled fake's own token.
pub fn features_off() -> Vec<(&'static str, &'static str, Value)> {
    vec![
        ("sub-agents", "agents.enabled", json!(false)),
        ("sub-agents", "features.multi_agent", json!(false)),
        ("sub-agents", "features.multi_agent_v2", json!(false)),
        ("memories", "features.memories", json!(false)),
        ("memories", "features.memory_tool", json!(false)),
        ("goals", "features.goals", json!(false)),
        ("standalone web search", "web_search", json!("disabled")),
        (
            "image generation",
            "features.image_generation",
            json!(false),
        ),
    ]
}

/// PATH value to hand to the selected executable, as the durable host will.
pub fn inherited_path() -> Option<OsString> {
    std::env::var_os("PATH")
}

pub mod fake;
mod fake_turn;
pub mod rpc;

#[cfg(test)]
mod tests;
