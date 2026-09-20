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
/// Stream identity captured from the qualified executable at zero tokens. A
/// help digest cannot see the wire, so the pinned interface is both artefacts.
pub const QUALIFIED_STREAM: &str =
    include_str!("../../../adapters/claude/2.1.278/stream-identity.json");

/// Fields of the stream identity that must match exactly for a qualified run.
pub const STREAM_IDENTITY_FIELDS: &[&str] = &[
    "init_keys",
    "capabilities",
    "message_sequence",
    "result_keys",
    "product_default_permission_mode",
    "product_default_model",
    "init_waits_for_stdin",
];

/// Control-protocol fields that widen permissions. PIO never sends one. Read
/// from the pinned `claude-agent-sdk-python` 0.2.153 source; see ADR 004 §9.
pub const WIDENING_RESPONSE_FIELDS: &[&str] = &["updatedPermissions"];

/// Control requests that would change the permission mode mid-session and
/// defeat the guard after the fact. PIO never sends one.
pub const WIDENING_CONTROL_SUBTYPES: &[&str] = &["set_permission_mode"];

/// The only decisions PIO forwards, the analogue of the Codex allowlist.
pub const SINGLE_USE_DECISIONS: &[&str] = &["allow", "deny"];

pub const ACCOUNT_IDENTITY_FIELDS: &[&str] = &["email", "orgId", "orgName"];

/// The only route facts PIO records.
pub const ROUTE_FIELDS: &[&str] = &["loggedIn", "authMethod", "apiProvider", "subscriptionType"];

/// Every file the user's settings can live in. Both carry permission rules.
pub const SETTINGS_FILES: &[&str] = &["settings.json", "settings.local.json"];

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// The variables a child is given. Nothing else is inherited — not the
/// caller's PATH and not the `CLAUDE_CODE_*` variables an enclosing session
/// exports. No credential variable is ever passed.
pub const ENV_ALLOWLIST: &[&str] = &["PATH", "HOME", "USER", "CLAUDE_CONFIG_DIR"];

/// The environment a child process runs with, built explicitly.
///
/// `USER` is in the allowlist because the credential route is **not observable
/// without it**: measured on 2.1.278, `auth status` reports `loggedIn: false`
/// and `authMethod: "none"` when `USER` is absent, even given the user's real
/// `HOME`. Leaving it out made PIO refuse a route that works. ADR 004 §3.
#[derive(Clone, Debug)]
pub struct ChildEnv {
    home: PathBuf,
    config_dir: Option<PathBuf>,
    user: Option<OsString>,
    path: Option<OsString>,
}

impl ChildEnv {
    /// An isolated environment. `HOME` and `CLAUDE_CONFIG_DIR` both point at a
    /// scratch directory, so none of the user's credentials is reachable and
    /// no work can touch their configuration. Used for every qualification.
    pub fn isolated(dir: &Path) -> Self {
        Self {
            home: dir.to_path_buf(),
            config_dir: Some(dir.to_path_buf()),
            user: std::env::var_os("USER"),
            path: None,
        }
    }

    /// The user's own environment, for observing the route they actually have.
    ///
    /// `CLAUDE_CONFIG_DIR` is deliberately left **unset**: measured, the product
    /// expects `.claude.json` inside the configured directory, while as the user
    /// has it that file lives at `~/.claude.json` beside `~/.claude/`. Setting
    /// the variable to `~/.claude` makes the harness report the configuration
    /// missing and the route absent.
    pub fn as_configured(home: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
            config_dir: None,
            user: std::env::var_os("USER"),
            path: None,
        }
    }

    pub fn with_path(mut self, path: Option<&OsStr>) -> Self {
        self.path = path.map(OsStr::to_owned);
        self
    }

    /// Only for tests that must prove a missing `USER` is what breaks the route.
    pub fn with_user(mut self, user: Option<OsString>) -> Self {
        self.user = user;
        self
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    fn apply(&self, command: &mut Command) {
        command.env_clear();
        command.env("HOME", &self.home);
        if let Some(config) = &self.config_dir {
            command.env("CLAUDE_CONFIG_DIR", config);
        }
        if let Some(user) = &self.user {
            command.env("USER", user);
        }
        if let Some(path) = &self.path {
            command.env("PATH", path);
        }
    }
}

/// Run the executable with an explicit environment and nothing inherited.
fn run(exe: &Path, args: &[&str], env: &ChildEnv) -> Result<(i32, String)> {
    let mut command = Command::new(exe);
    command.args(args);
    env.apply(&mut command);
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
pub fn surface_identity(exe: &Path, env: &ChildEnv) -> Result<Value> {
    let mut digests = BTreeMap::new();
    for name in SURFACE_COMMANDS {
        let args: Vec<&str> = if *name == "<top>" {
            vec!["--help"]
        } else {
            vec![name, "--help"]
        };
        let (_, out) = run(exe, &args, env)?;
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
    let isolated = ChildEnv::isolated(&config).with_path(path);
    let (status, output) = run(selected, &["--version"], &isolated)?;
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
    let identity = surface_identity(selected, &isolated)?;
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
pub fn auth_route(exe: &Path, env: &ChildEnv) -> Result<Value> {
    std::fs::create_dir_all(env.home())?;
    let (status, output) = run(exe, &["auth", "status"], env)?;
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
            unresolved.push(json!({"setting":"permissions.defaultMode","reason":"absent; measured, the product default reports as `default`, which --permission-mode does not accept"}));
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
    // The user's configuration lives in more than one file. `settings.local.json`
    // carries permission rules of its own, so a snapshot that missed it would
    // under-report the allow list it is meant to disclose. ADR 004 §7.
    let mut files = Vec::new();
    let mut allow_total = 0usize;
    let mut bash_total = 0usize;
    let mut deny_total = 0usize;
    let mut default_mode = Value::Null;
    let mut model = Value::Null;
    let mut always_thinking = Value::Null;
    let mut plugins: Vec<String> = Vec::new();
    let mut sandbox_configured = false;

    for name in SETTINGS_FILES {
        let path = config_dir.join(name);
        let exists = path.exists();
        let text = if exists {
            std::fs::read(&path)?
        } else {
            Vec::new()
        };
        let parsed: Value = if exists {
            serde_json::from_slice(&text).unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        let allow = parsed["permissions"]["allow"].as_array();
        let bash = allow
            .map(|rules| {
                rules
                    .iter()
                    .filter(|rule| rule.as_str().is_some_and(|r| r.starts_with("Bash(")))
                    .count()
            })
            .unwrap_or(0);
        allow_total += allow.map(Vec::len).unwrap_or(0);
        bash_total += bash;
        deny_total += parsed["permissions"]["deny"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0);
        if !parsed["permissions"]["defaultMode"].is_null() {
            default_mode = parsed["permissions"]["defaultMode"].clone();
        }
        if !parsed["model"].is_null() {
            model = parsed["model"].clone();
        }
        if !parsed["alwaysThinkingEnabled"].is_null() {
            always_thinking = parsed["alwaysThinkingEnabled"].clone();
        }
        if !parsed["sandbox"].is_null() {
            sandbox_configured = true;
        }
        if let Some(enabled) = parsed["enabledPlugins"].as_object() {
            plugins.extend(
                enabled
                    .iter()
                    .filter(|(_, on)| **on == Value::Bool(true))
                    .map(|(name, _)| name.clone()),
            );
        }
        files.push(json!({
            "file":name,
            "exists":exists,
            "raw_sha256":exists.then(|| sha256_hex(&text)),
            "allow_entry_count":allow.map(Vec::len),
        }));
    }
    plugins.sort();
    Ok(json!({
        "format":"pio-claude-settings-snapshot/2",
        "files":files,
        "permissions":{
            "defaultMode":default_mode,
            "allow_entry_count":allow_total,
            "bash_allow_rule_count":bash_total,
            "deny_entry_count":deny_total,
        },
        // No sandbox key means the product default, which is off. Containment
        // is then the permission rules only. ADR 004 §5.
        "sandbox":{"configured":sandbox_configured,"os_sandbox_in_effect":false},
        "model":model,
        "always_thinking_enabled":always_thinking,
        "enabled_plugins":plugins,
    }))
}

/// Name every stream-identity field that moved. The surface identity cannot
/// see the wire, so a qualified run compares both. ADR 004 §2.
pub fn stream_drift(expected: &Value, actual: &Value) -> Vec<Value> {
    STREAM_IDENTITY_FIELDS
        .iter()
        .filter(|field| expected[**field] != actual[**field])
        .map(|field| json!({"field":field,"expected":&expected[*field],"actual":&actual[*field]}))
        .collect()
}

fn inner_request_id(request: &Value) -> Result<String> {
    request["request_id"]
        .as_str()
        .map(str::to_owned)
        .context("control request carries no request_id")
}

/// Encode a permission decision for the control protocol.
///
/// Only two decisions are encodable: an `allow` that echoes the request's own
/// input back unchanged, and a `deny` carrying a reason. Every widening form
/// the protocol offers is refused here rather than in review — see ADR 004 §9.
/// The count of suggestions the harness offered is recorded and never acted on.
pub fn permission_decision(request: &Value, behavior: &str, reason: &str) -> Result<Value> {
    if !SINGLE_USE_DECISIONS.contains(&behavior) {
        bail!("PIO forwards only single-use decisions, not {behavior}");
    }
    let inner = &request["request"];
    let request_id = inner_request_id(request)?;
    let suggested = inner["permission_suggestions"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    let decision = if behavior == "allow" {
        // The original input, byte for byte. Rewriting it would change the
        // tool call the user's harness decided to make.
        json!({"behavior":"allow","updatedInput":&inner["input"]})
    } else {
        json!({"behavior":"deny","message":reason})
    };
    Ok(json!({
        "envelope": {
            "type":"control_response",
            "response":{"subtype":"success","request_id":request_id,"response":decision},
        },
        "tool_name": &inner["tool_name"],
        "single_use": true,
        "suggestions_offered": suggested,
        "suggestions_acted_on": 0,
        "widening_fields_sent": Vec::<String>::new(),
    }))
}

/// Where a tool use landed, relative to the fixture workspace.
///
/// A Bash command names no path PIO can resolve, so it is reported as
/// `not_classifiable` rather than assumed to be inside. ADR 004 §5.
fn classify_target(input: &Value, fixture: &Path) -> (Option<String>, &'static str) {
    for key in ["file_path", "path", "notebook_path"] {
        if let Some(target) = input[key].as_str() {
            let placement = if Path::new(target).starts_with(fixture) {
                "inside_fixture"
            } else {
                "outside_fixture"
            };
            return (Some(target.to_owned()), placement);
        }
    }
    (None, "not_classifiable")
}

/// Record every tool use the stream reported, by tool name and target digest,
/// whether or not it prompted.
///
/// Containment here is the harness's permission rules only. The user's settings
/// carry no sandbox key, so no OS sandbox is in effect, and a pre-approved
/// command never produces a permission request — it never reaches PIO at all.
/// A target outside the fixture is therefore an **observed effect with
/// unresolved liability**, not a declined request. ADR 004 §5.
pub fn tool_use_records(messages: &[Value], fixture: &Path) -> Value {
    let mut records = Vec::new();
    for message in messages {
        let Some(blocks) = message["message"]["content"].as_array() else {
            continue;
        };
        for block in blocks.iter().filter(|b| b["type"] == "tool_use") {
            let input = &block["input"];
            let (target, placement) = classify_target(input, fixture);
            let digest_source = target.clone().unwrap_or_else(|| input.to_string());
            records.push(json!({
                "tool": &block["name"],
                "tool_use_id": &block["id"],
                "target": target,
                "target_sha256": sha256_hex(digest_source.as_bytes()),
                "placement": placement,
            }));
        }
    }
    let count = |what: &str| records.iter().filter(|r| r["placement"] == what).count();
    let outside = count("outside_fixture");
    let unclassified = count("not_classifiable");
    json!({
        "format":"pio-claude-tool-uses/1",
        "containment":{
            "mechanism":"harness_permission_rules_only",
            "os_sandbox_observed":false,
        },
        "fixture":fixture.display().to_string(),
        "tool_uses":records,
        "out_of_fixture_effect_observed":outside > 0,
        "out_of_fixture_count":outside,
        "unclassifiable_target_count":unclassified,
        "liability":if outside > 0 || unclassified > 0 { "unresolved" } else { "none_observed" },
    })
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

pub mod fake;

#[cfg(test)]
mod tests;
