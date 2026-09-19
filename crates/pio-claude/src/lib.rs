//! Claude Code 2.1.278 qualification. Binds the user-selected executable, the
//! binary it actually resolves to, and the command-line surface it exposes,
//! before any native work. Qualification runs the executable only with an
//! isolated `CLAUDE_CONFIG_DIR`; it never reads the user's credentials, never
//! writes their configuration, and makes no model call.
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const PINNED_VERSION: &str = "2.1.278";

/// Command-line surface captured from the qualified 2.1.278 executable. Claude
/// Code publishes no schemas, so the interface PIO can pin is the surface it
/// drives. See [ADR 004](../../../docs/decisions/004-claude-code-adapter.md).
pub const QUALIFIED_SURFACE: &str =
    include_str!("../../../adapters/claude/2.1.278/surface-identity.json");

/// Helps that make up the surface identity. The top-level help plus every
/// subcommand PIO might touch, so a self-update that changes the interface
/// moves the digest.
pub const SURFACE_COMMANDS: &[&str] = &[
    "<top>", "auth", "mcp", "plugin", "project", "doctor", "agents", "install",
];

/// `claude auth status` prints these; they identify the account and are dropped
/// at this boundary, never recorded.
pub const ACCOUNT_IDENTITY_FIELDS: &[&str] = &["email", "orgId", "orgName"];

/// The only route facts PIO records.
pub const ROUTE_FIELDS: &[&str] = &["loggedIn", "authMethod", "apiProvider", "subscriptionType"];

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Run the executable with an isolated configuration directory and an explicit
/// environment. No credential variable is ever passed.
fn run(exe: &Path, args: &[&str], config: &Path, path: Option<&OsStr>) -> Result<(i32, String)> {
    let mut command = Command::new(exe);
    command.args(args).env_clear();
    command.env("CLAUDE_CONFIG_DIR", config);
    command.env("HOME", config);
    if let Some(path) = path {
        command.env("PATH", path);
    }
    let output = command
        .output()
        .with_context(|| format!("running {}", exe.display()))?;
    Ok((
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    ))
}

fn parse_version(output: &str) -> Option<String> {
    output.split_whitespace().next().map(str::to_owned)
}

/// Resolve the selected path to the binary that actually runs. On this
/// workstation the Homebrew symlink points into a cask directory whose name is
/// the version, so a self-update moves the resolved path.
pub fn resolve(selected: &Path) -> Result<Value> {
    if !selected.exists() {
        bail!("no executable at {}", selected.display());
    }
    let resolved = std::fs::canonicalize(selected)
        .with_context(|| format!("resolving {}", selected.display()))?;
    let bytes =
        std::fs::read(&resolved).with_context(|| format!("reading {}", resolved.display()))?;
    let directory = resolved
        .parent()
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned());
    Ok(json!({
        "selected": selected,
        "resolved": resolved,
        "binary_sha256": sha256_hex(&bytes),
        "resolved_parent": directory,
    }))
}

/// Digest every help in [`SURFACE_COMMANDS`] and the listing over them.
pub fn surface_identity(exe: &Path, config: &Path, path: Option<&OsStr>) -> Result<Value> {
    let mut digests = BTreeMap::new();
    for name in SURFACE_COMMANDS {
        let args: Vec<&str> = if *name == "<top>" {
            vec!["--help"]
        } else {
            vec![name, "--help"]
        };
        let (_, out) = run(exe, &args, config, path)?;
        digests.insert((*name).to_owned(), sha256_hex(out.as_bytes()));
    }
    let listing: String = digests
        .iter()
        .map(|(name, digest)| format!("{name}\t{digest}\n"))
        .collect();
    Ok(json!({
        "format": "pio-claude-surface-identity/1",
        "command_count": digests.len(),
        "commands": digests,
        "surface_listing_sha256": sha256_hex(listing.as_bytes()),
    }))
}

/// Name every command whose help changed, appeared or disappeared.
pub fn surface_drift(expected: &Value, actual: &Value) -> Vec<Value> {
    let empty = serde_json::Map::new();
    let before = expected["commands"].as_object().unwrap_or(&empty);
    let after = actual["commands"].as_object().unwrap_or(&empty);
    let mut drift = Vec::new();
    for (name, digest) in before {
        match after.get(name) {
            None => drift.push(json!({"command":name,"change":"removed"})),
            Some(current) if current != digest => {
                drift.push(json!({"command":name,"change":"changed"}))
            }
            Some(_) => {}
        }
    }
    for name in after.keys() {
        if !before.contains_key(name) {
            drift.push(json!({"command":name,"change":"added"}));
        }
    }
    drift
}

/// Qualify a selected executable against the pinned version and surface.
/// Refusals are data, so discovery can report a wrong or self-updated
/// executable truthfully instead of crashing.
pub fn qualify(
    selected: &Path,
    expected_surface: &Value,
    path: Option<&OsStr>,
    work: &Path,
) -> Result<Value> {
    let pinned = json!({"version":PINNED_VERSION});
    let resolution = match resolve(selected) {
        Ok(resolution) => resolution,
        Err(error) => {
            return Ok(json!({
                "format":"pio-claude-qualification/1","pinned":pinned,"selected":selected,
                "qualified":false,
                "refusals":[{"reason":"unresolved_executable","detail":format!("{error:#}")}],
            }));
        }
    };
    std::fs::create_dir_all(work)?;
    let config = work.join("isolated-claude-config");
    std::fs::create_dir_all(&config)?;
    let mut refusals = Vec::new();
    let (status, output) = run(selected, &["--version"], &config, path)?;
    let version = parse_version(&output);
    if status != 0 || version.is_none() {
        refusals.push(json!({"reason":"version_unavailable","exit":status}));
    } else if version.as_deref() != Some(PINNED_VERSION) {
        refusals.push(json!({"reason":"unsupported_version","observed":version}));
    }
    // Never run an unqualified executable with Claude-specific arguments.
    if !refusals.is_empty() {
        return Ok(json!({
            "format":"pio-claude-qualification/1","pinned":pinned,"selected":selected,
            "resolution":resolution,"isolated_config_dir":true,
            "version":version,"surface":{"skipped":"version_not_qualified"},
            "qualified":false,"refusals":refusals,
        }));
    }
    let identity = surface_identity(selected, &config, path)?;
    let drift = surface_drift(expected_surface, &identity);
    if !drift.is_empty() {
        refusals.push(json!({"reason":"surface_drift","commands":drift.len()}));
    }
    Ok(json!({
        "format":"pio-claude-qualification/1",
        "pinned":pinned,
        "selected":selected,
        "resolution":resolution,
        "isolated_config_dir":true,
        "version":version,
        "surface":{
            "command_count":identity["command_count"],
            "surface_listing_sha256":identity["surface_listing_sha256"],
            "expected_surface_listing_sha256":expected_surface["surface_listing_sha256"],
            "drift_count":drift.len(),
            "drift":drift,
        },
        "qualified":refusals.is_empty(),
        "refusals":refusals,
    }))
}

/// Observe which credential route is configured, recording only
/// [`ROUTE_FIELDS`]. The account identity fields are dropped here and never
/// reach a receipt, a journal record or a log.
pub fn auth_route(exe: &Path, config: &Path, path: Option<&OsStr>) -> Result<Value> {
    std::fs::create_dir_all(config)?;
    let (status, output) = run(exe, &["auth", "status"], config, path)?;
    let Ok(status_json) = serde_json::from_str::<Value>(&output) else {
        return Ok(json!({"exit":status,"parsed":false,"usable":false}));
    };
    let mut observed = serde_json::Map::new();
    for field in ROUTE_FIELDS {
        if let Some(value) = status_json.get(*field) {
            observed.insert((*field).to_owned(), value.clone());
        }
    }
    let dropped: Vec<&&str> = ACCOUNT_IDENTITY_FIELDS
        .iter()
        .filter(|field| status_json.get(**field).is_some())
        .collect();
    Ok(json!({
        "exit":status,"parsed":true,
        "observed":observed,
        "account_identity_fields_dropped":dropped,
        "usable":status_json["loggedIn"] == true || status_json["apiKeySource"] == "apiKey",
    }))
}

/// The one permission mode PIO may never request, whatever anything else says.
pub const FORBIDDEN_MODE: &str = "bypassPermissions";

/// Refuse a requested permission mode that is not exactly the user's configured
/// default (owner guard, carried from ADR 003 §6 and the Codex thread-settings
/// guard). The breadth ordering of Claude Code's six modes is **not
/// established**: a documentation pass produced one that contradicted itself,
/// placing `dontAsk`, which auto-denies everything that would prompt, above
/// `acceptEdits`. Rather than encode a guess as a safety property, this guard
/// compares for equality. A mode joins a narrower-than set only with a
/// measurement. An absent or unreadable default refuses rather than assuming
/// the product's own default, which is likewise unmeasured.
pub fn permission_mode_guard(settings: &Value, requested: &str) -> Value {
    let mut unresolved = Vec::new();
    if requested == FORBIDDEN_MODE {
        unresolved.push(json!({"setting":"requested","reason":"PIO never requests this mode","value":requested}));
    }
    let configured = match &settings["permissions"]["defaultMode"] {
        Value::String(mode) if mode == FORBIDDEN_MODE => {
            unresolved.push(json!({"setting":"permissions.defaultMode","reason":"PIO never requests this mode, even when it is configured","value":mode}));
            None
        }
        Value::String(mode) => Some(mode.clone()),
        Value::Null => {
            unresolved.push(json!({"setting":"permissions.defaultMode","reason":"absent, and the product default is not measured"}));
            None
        }
        other => {
            unresolved.push(json!({"setting":"permissions.defaultMode","reason":"not a plain string","value":other}));
            None
        }
    };
    let matches = configured.as_deref() == Some(requested);
    if !matches && unresolved.is_empty() {
        unresolved.push(json!({
            "setting":"permissions.defaultMode","reason":"requested mode is not the configured default and no breadth ordering is established",
            "configured":configured,"requested":requested}));
    }
    json!({
        "format":"pio-claude-permission-guard/1",
        "configured":configured,
        "requested":requested,
        "unresolved":unresolved,
        "allowed":unresolved.is_empty() && matches,
    })
}

/// Read the user's settings without recording anything that identifies them.
/// Returns the facts the guard and the receipts need, plus digests.
pub fn settings_snapshot(config_dir: &Path) -> Result<Value> {
    let settings_path = config_dir.join("settings.json");
    let exists = settings_path.exists();
    let text = if exists {
        std::fs::read(&settings_path)?
    } else {
        Vec::new()
    };
    let parsed: Value = if exists {
        serde_json::from_slice(&text).unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let plugins: Vec<String> = parsed["enabledPlugins"]
        .as_object()
        .map(|p| {
            p.iter()
                .filter(|(_, enabled)| **enabled == Value::Bool(true))
                .map(|(name, _)| name.clone())
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({
        "format":"pio-claude-settings-snapshot/1",
        "exists":exists,
        "raw_sha256":exists.then(|| sha256_hex(&text)),
        "permissions":{
            "defaultMode":parsed["permissions"]["defaultMode"],
            "allow_entry_count":parsed["permissions"]["allow"].as_array().map(Vec::len),
        },
        "model":parsed["model"],
        "always_thinking_enabled":parsed["alwaysThinkingEnabled"],
        "enabled_plugins":plugins,
    }))
}

/// Path helper used by callers that pass an explicit `PATH`.
pub fn path_of(value: &str) -> OsString {
    OsString::from(value)
}

/// Where the adapter looks for the user's Claude configuration.
pub fn default_config_dir() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude")))
}

#[cfg(test)]
mod tests;
