//! OpenCode 2.0.11 qualification and admission (ADR 005).
//!
//! Re-pinned from 2.0.1 on 2026-09-21: npm had self-updated the owner's
//! install, exactly as ADR 005 asked whether it would. Only the top-level
//! help moved; the six subcommand helps are byte-identical.
//!
//! Two facts shape everything here, both measured:
//!
//! 1. **The owner runs their own OpenCode service.** PIO never connects to it,
//!    never restarts it and never stops it. `opencode acp` starts its own
//!    private `serve --stdio --port 0` child, so isolation is structural
//!    rather than a flag, and the owner's process is recorded before and after
//!    so a run can prove it did not move.
//! 2. **The environment does not isolate OpenCode's configuration.** With
//!    `PATH` alone and no `HOME`, a session still found the user's config and
//!    every model. PIO therefore cannot claim to have prevented the harness
//!    from reading its own credential store; it claims only that it does not
//!    read it and passes no key. `OPENCODE_CONFIG_DIR` does isolate and is the
//!    negative control.
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const PINNED_VERSION: &str = "2.0.11";

/// Command-line surface captured from the qualified executable.
pub const QUALIFIED_SURFACE: &str =
    include_str!("../../../adapters/opencode/2.0.11/surface-identity.json");

/// Helps that make up the surface identity.
pub const SURFACE_COMMANDS: &[&str] = &["<top>", "acp", "run", "models", "auth", "serve", "api"];

/// The variables a child is given. Nothing is inherited.
///
/// `OPENCODE_CONFIG_DIR` is passed only when isolating; an as-configured run
/// leaves it unset so the harness reads the user's own configuration, exactly
/// as at the keyboard.
pub const ENV_ALLOWLIST: &[&str] = &["PATH", "HOME", "USER", "OPENCODE_CONFIG_DIR"];

/// Substrings that mark a variable as carrying a credential. None may reach a
/// child: OpenCode authenticates itself and PIO supplies nothing.
pub const CREDENTIAL_MARKERS: &[&str] = &["API_KEY", "TOKEN", "SECRET", "PASSWORD", "CREDENTIAL"];

/// Flags PIO never passes. `--auto` approves everything not explicitly denied,
/// and the owner's configuration has no deny rules, so it would approve
/// everything. `--server` would reach the owner's service.
pub const FORBIDDEN_FLAGS: &[&str] = &["--auto", "--server"];

/// Settings a `pio-opencode-service/1` configuration may carry.
pub const SERVICE_SETTINGS: &[&str] = &[
    "executable",
    "env",
    "config_dir",
    "home",
    "fixture_root",
    "provider",
    "model",
    "mode",
    "labeled_fake",
    "test_only_model_exception",
    "expected_surface",
];

/// Owner decision of 2026-09-20, widened 2026-09-21: PIO may pass an explicit
/// model for the M3b OpenCode fixture runs. Test-only and dated on purpose;
/// the product still never selects a model.
pub const MODEL_EXCEPTION: &str = "owner-2026-09-20-m3b-opencode-fixture-runs";

/// The **only** provider the exception covers (owner, 2026-09-21).
///
/// The exception was widened from one model id to this provider, so runs may
/// use any model on the owner's MiniMax plan and choose the one that suits
/// the evidence. It is a provider allowlist of exactly one entry, not a
/// relaxation: every other provider is still refused, including OpenCode's
/// own free models — which is what a silently downgraded session would land
/// on — and the Juspay Grid gateway the owner excluded outright.
pub const ALLOWED_PROVIDER: &str = "minimax-coding-plan";

/// Session modes PIO may request, narrowest first.
///
/// The owner's configuration has **no permission rules**, so nothing PIO can
/// pass makes the harness ask before it acts — R2 measured a shell command
/// executed with no request reaching PIO at all. `mode` is the only
/// per-session lever that does not edit the owner's configuration, and `plan`
/// is the narrower of the two. **Narrowing is allowed; widening is not**, so
/// `build` is refused: it is the harness's own default and asking for it
/// could only ever loosen a session that was already narrower.
pub const REQUESTABLE_MODES: &[&str] = &["plan"];

/// The owner's own background service, which PIO must never touch.
pub const OWNER_SERVICE_PATTERN: &str = "serve --service";

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// The environment a child process runs with, built explicitly.
#[derive(Clone, Debug)]
pub struct ChildEnv {
    home: PathBuf,
    config_dir: Option<PathBuf>,
    user: Option<OsString>,
    path: Option<OsString>,
    extra: BTreeMap<String, String>,
}

impl ChildEnv {
    /// An isolated configuration. Measured: this yields 9 models and **no**
    /// `minimax-coding-plan` provider, which is the negative control.
    pub fn isolated(dir: &Path) -> Self {
        Self {
            home: dir.to_path_buf(),
            config_dir: Some(dir.to_path_buf()),
            user: std::env::var_os("USER"),
            path: None,
            extra: BTreeMap::new(),
        }
    }

    /// The user's own configuration, as at the keyboard.
    pub fn as_configured(home: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
            config_dir: None,
            user: std::env::var_os("USER"),
            path: None,
            extra: BTreeMap::new(),
        }
    }

    pub fn with_path(mut self, path: Option<&OsStr>) -> Self {
        self.path = path.map(OsStr::to_owned);
        self
    }

    pub fn with_extra(mut self, name: &str, value: &str) -> Self {
        self.extra.insert(name.to_owned(), value.to_owned());
        self
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn apply(&self, command: &mut Command) {
        command.env_clear();
        command.env("HOME", &self.home);
        if let Some(config) = &self.config_dir {
            command.env("OPENCODE_CONFIG_DIR", config);
        }
        if let Some(user) = &self.user {
            command.env("USER", user);
        }
        if let Some(path) = &self.path {
            command.env("PATH", path);
        }
        for (name, value) in &self.extra {
            command.env(name, value);
        }
    }
}

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
    output
        .split_whitespace()
        .find_map(|token| token.strip_prefix('v'))
        .map(str::to_owned)
}

/// Resolve the selected path to the binary that actually runs.
pub fn resolve(selected: &Path) -> Result<Value> {
    if !selected.exists() {
        bail!("no executable at {}", selected.display());
    }
    let resolved = std::fs::canonicalize(selected)?;
    Ok(json!({
        "selected":selected.display().to_string(),
        "resolved":resolved.display().to_string(),
        "binary_sha256":sha256_hex(&std::fs::read(&resolved)?),
        "install":"npm",
    }))
}

/// Canonical digest of the command-line surface.
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
        "format":"pio-opencode-surface-identity/1",
        "command_count":digests.len(),
        "commands":digests,
        "surface_listing_sha256":sha256_hex(listing.as_bytes()),
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

/// The owner's background service, by identity. Read-only, always.
pub fn owner_service() -> Vec<String> {
    let Ok(output) = Command::new("ps")
        .args(["-Ao", "pid,lstart,command"])
        .output()
    else {
        return vec![];
    };
    let mut found: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.contains(OWNER_SERVICE_PATTERN) && line.contains("opencode"))
        .map(|line| sha256_hex(line.trim().as_bytes()))
        .collect();
    found.sort();
    found
}

/// Refuse unless the session's own reported provider and model equal the ones
/// requested (owner decision, 2026-09-20).
///
/// This closes the measured hazard that a missing route does **not** refuse —
/// it silently substitutes a free built-in model, so fixture content would go
/// to a third party with nothing in the record saying so. Unlike Claude Code's
/// `system/init`, ACP reports the session configuration **before any prompt**,
/// so this check genuinely precedes delivery.
pub fn session_configuration_guard(session: &Value, requested: &str) -> Value {
    let mut unresolved = Vec::new();
    let options = session["configOptions"].as_array();
    let model = options.and_then(|options| {
        options
            .iter()
            .find(|option| option["id"] == "model")
            .map(|option| option["currentValue"].clone())
    });
    let reported = model.as_ref().and_then(Value::as_str).unwrap_or_default();
    if options.is_none() {
        unresolved.push(json!({"reason":"the session reported no configuration"}));
    } else if reported.is_empty() {
        unresolved.push(json!({"reason":"the session reported no model"}));
    } else if reported != requested {
        unresolved.push(json!({
            "reason":"the session's model is not the requested one",
            "requested":requested,"reported":reported}));
    }
    let requested_provider = requested.split('/').next().unwrap_or_default();
    let reported_provider = reported.split('/').next().unwrap_or_default();
    if !reported.is_empty() && requested_provider != reported_provider {
        unresolved.push(json!({
            "reason":"the session's provider is not the requested one",
            "requested":requested_provider,"reported":reported_provider}));
    }
    json!({
        "format":"pio-opencode-session-guard/1",
        "requested":requested,
        "reported":model,
        "requested_provider":requested_provider,
        "reported_provider":reported_provider,
        "unresolved":unresolved,
        "allowed":unresolved.is_empty(),
        // ACP reports this before a prompt, so a refusal precedes delivery.
        "checked_before_delivery":true,
    })
}

/// Qualify a selected executable against the pinned version and surface.
pub fn qualify(
    selected: &Path,
    expected_surface: &Value,
    env: &ChildEnv,
    work: &Path,
) -> Result<Value> {
    let pinned = json!({"version":PINNED_VERSION});
    let resolution = match resolve(selected) {
        Ok(resolution) => resolution,
        Err(error) => {
            return Ok(json!({
                "format":"pio-opencode-qualification/1","pinned":pinned,
                "selected":selected.display().to_string(),"qualified":false,
                "refusals":[{"reason":"unresolved_executable","detail":format!("{error:#}")}],
            }));
        }
    };
    std::fs::create_dir_all(work)?;
    let config = work.join("isolated-opencode-config");
    std::fs::create_dir_all(&config)?;
    let isolated = ChildEnv {
        home: config.clone(),
        config_dir: Some(config),
        ..env.clone()
    };
    let mut refusals = Vec::new();
    let (status, output) = run(selected, &["--version"], &isolated)?;
    let version = parse_version(&output);
    if status != 0 || version.is_none() {
        refusals.push(json!({"reason":"version_unavailable","exit":status}));
    } else if version.as_deref() != Some(PINNED_VERSION) {
        refusals.push(json!({"reason":"unsupported_version","observed":version}));
    }
    if !refusals.is_empty() {
        return Ok(json!({
            "format":"pio-opencode-qualification/1","pinned":pinned,
            "selected":selected.display().to_string(),"resolution":resolution,
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
        "format":"pio-opencode-qualification/1",
        "pinned":pinned,"selected":selected.display().to_string(),
        "resolution":resolution,"isolated_config_dir":true,"version":version,
        "surface":{
            "command_count":identity["command_count"],
            "surface_listing_sha256":identity["surface_listing_sha256"],
            "expected_surface_listing_sha256":expected_surface["surface_listing_sha256"],
            "drift_count":drift.len(),"drift":drift,
        },
        "qualified":refusals.is_empty(),"refusals":refusals,
    }))
}

fn refusal(reason: &str, detail: Value) -> Value {
    json!({"reason":reason,"detail":detail})
}

/// Every model an OpenCode configuration names outside a session's own,
/// read from the non-secret configuration files only, and nothing else in
/// them (Review 52, a static reading of OpenCode 2.0.11, not measured: its
/// title and compaction helpers use the session's own provider and model
/// unless `small_model` or a helper's own `model` says otherwise). Ported
/// from `scripts/lead_run.py`'s `helper_models`, which this now backs.
fn helper_models(config_dir: &Path) -> Result<Vec<(String, String)>> {
    let mut models = Vec::new();
    for name in ["opencode.json", "opencode.jsonc"] {
        let path = config_dir.join(name);
        if !path.exists() {
            continue;
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let body: String = raw
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let config: Value = serde_json::from_str(&body).with_context(|| {
            format!("{name} is unreadable, so its helper models cannot be checked")
        })?;
        if let Some(small) = config["small_model"].as_str() {
            models.push((format!("{name}: small_model"), small.to_owned()));
        }
        for section in ["agent", "mode"] {
            if let Some(entries) = config[section].as_object() {
                for (entry, settings) in entries {
                    if let Some(model) = settings["model"].as_str() {
                        models.push((format!("{name}: {section}.{entry}.model"), model.to_owned()));
                    }
                }
            }
        }
    }
    Ok(models)
}

/// Owner decision, 2026-09-25: refuse to start on any helper whose provider
/// is not the session's own (D18; ported from `scripts/lead_run.py`'s
/// `helpers_elsewhere`, which enforced this only for its own L3 rehearsal).
fn helper_provider_refusals(config_dir: &Path, model: &str) -> Result<Vec<Value>> {
    let provider = model.split_once('/').map_or(model, |(p, _)| p);
    let elsewhere: Vec<Value> = helper_models(config_dir)?
        .into_iter()
        .filter(|(_, m)| m.split_once('/').map_or(m.as_str(), |(p, _)| p) != provider)
        .map(|(setting, model)| json!({"setting":setting,"model":model}))
        .collect();
    Ok(if elsewhere.is_empty() {
        vec![]
    } else {
        vec![refusal("helper_elsewhere", json!(elsewhere))]
    })
}

/// Every decision a service must make before it starts a session.
pub fn service_admission(work: &Path, opencode: &Value) -> Result<Value> {
    let mut refusals = Vec::new();
    let object = opencode.as_object().context("opencode settings object")?;
    for name in object.keys() {
        if !SERVICE_SETTINGS.contains(&name.as_str()) {
            refusals.push(refusal("unsupported_setting", json!(name)));
        }
    }
    for name in ["executable", "config_dir", "home", "fixture_root"] {
        if !opencode[name]
            .as_str()
            .is_some_and(|p| Path::new(p).is_absolute())
        {
            refusals.push(refusal("setting_must_be_an_absolute_path", json!(name)));
        }
    }
    match opencode["env"].as_object() {
        None => refusals.push(refusal("env_must_be_an_object", Value::Null)),
        Some(env) => {
            if !env.values().all(Value::is_string) || !env.contains_key("PATH") {
                refusals.push(refusal(
                    "env_needs_string_values_including_path",
                    Value::Null,
                ));
            }
            for name in env.keys() {
                let upper = name.to_uppercase();
                if CREDENTIAL_MARKERS.iter().any(|m| upper.contains(m)) {
                    refusals.push(refusal("env_carries_a_credential_variable", json!(name)));
                }
                let fake_scenario = name == FAKE_SCENARIO_VAR && opencode["labeled_fake"] == true;
                if !ENV_ALLOWLIST.contains(&name.as_str()) && !fake_scenario {
                    refusals.push(refusal("env_variable_not_in_the_allowlist", json!(name)));
                }
            }
        }
    }

    // Every run passes an explicit model, because the configured default is a
    // third-party gateway the owner has excluded from PIO entirely.
    let model = opencode["model"].as_str();
    let exception = opencode["test_only_model_exception"].as_str();
    match model {
        None => refusals.push(refusal("model_required", json!(MODEL_EXCEPTION))),
        Some("") => refusals.push(refusal("model_must_be_a_non_empty_name", Value::Null)),
        Some(model) => {
            if exception != Some(MODEL_EXCEPTION) {
                refusals.push(refusal(
                    "model_requires_the_dated_test_only_exception",
                    json!({"model":model,"required":MODEL_EXCEPTION}),
                ));
            }
            // Owner decision, 2026-09-20: no PIO run uses this provider for
            // any purpose. It is excluded here, not merely unevaluated, and
            // it is named separately from the allowlist below so the record
            // says *why* rather than only that it was not allowed.
            if model.starts_with("juspay-grid/") {
                refusals.push(refusal("provider_excluded_by_the_owner", json!(model)));
            }
            // Owner decision, 2026-09-21: the exception covers the MiniMax
            // provider, so any model on the owner's plan may be used. Nothing
            // else may — including `opencode/` free models, which is exactly
            // what a silently downgraded session reports.
            else if !model.starts_with(&format!("{ALLOWED_PROVIDER}/")) {
                refusals.push(refusal(
                    "provider_not_covered_by_the_exception",
                    json!({"model":model,"allowed_provider":ALLOWED_PROVIDER}),
                ));
            }
            // Owner decision, 2026-09-25 (moved from scripts/lead_run.py's
            // L3 rehearsal into admission itself, D18, so every OpenCode
            // run gets it, not only one driven through that script): a
            // helper's `small_model`, or an `agent`/`mode` entry's own
            // `model`, on a provider other than the session's spends there
            // with nothing in the receipt to show it. Refused outright.
            if let Some(dir) = opencode["config_dir"].as_str() {
                match helper_provider_refusals(Path::new(dir), model) {
                    Ok(found) => refusals.extend(found),
                    Err(error) => refusals.push(refusal(
                        "opencode_config_unreadable",
                        json!(error.to_string()),
                    )),
                }
            }
        }
    }
    // A mode is optional. When one is named it may only be a narrowing one:
    // PIO never asks a harness to be less careful than it already is.
    if let Some(mode) = opencode.get("mode") {
        match mode.as_str() {
            Some(mode) if REQUESTABLE_MODES.contains(&mode) => {}
            Some(mode) => refusals.push(refusal(
                "mode_is_not_a_narrowing_one",
                json!({"mode":mode,"requestable":REQUESTABLE_MODES}),
            )),
            None => refusals.push(refusal("mode_must_be_a_plain_name", mode.clone())),
        }
    }
    // Qualification is computed *and acted on*. Computing a verdict and then
    // admitting anyway is how an unqualified executable would have run.
    let qualification = qualification_of(work, opencode, &refusals)?;
    if qualification["qualified"] == false {
        refusals.push(refusal(
            "opencode_not_qualified",
            qualification["refusals"].clone(),
        ));
    }
    Ok(json!({
        "format":"pio-opencode-service-admission/1",
        "adapter":"opencode",
        "labeled_fake":opencode["labeled_fake"] == true,
        "requested_model":model,
        "owner_service_before":owner_service(),
        "qualification":qualification,
        "refusals":refusals,
        "admitted":refusals.is_empty(),
        // Nothing here starts a session, sends a prompt or makes a model call.
        "session_started":false,
    }))
}

fn qualification_of(work: &Path, opencode: &Value, refusals: &[Value]) -> Result<Value> {
    if opencode["labeled_fake"] == true {
        return Ok(json!({"skipped":"labeled_fake"}));
    }
    let Some(exe) = opencode["executable"].as_str() else {
        return Ok(json!({"skipped":"no executable"}));
    };
    if !refusals.is_empty() {
        return Ok(json!({"skipped":"refused before qualification"}));
    }
    let expected: Value = match opencode["expected_surface"].as_str() {
        Some(path) => serde_json::from_slice(&std::fs::read(path)?)?,
        None => serde_json::from_str(QUALIFIED_SURFACE)?,
    };
    let path = opencode["env"]["PATH"].as_str().map(OsStr::new);
    qualify(
        Path::new(exe),
        &expected,
        &ChildEnv::isolated(&work.join("qualify-config")).with_path(path),
        &work.join("qualification"),
    )
}

/// How the labeled fake is told which scenario to play.
pub const FAKE_SCENARIO_VAR: &str = "PIO_OPENCODE_FAKE_SCENARIO";

/// The prefix of every credential PIO's own Protocol configuration issues.
pub const PIO_CREDENTIAL_PREFIX: &str = "ccred1.";

/// Why a lead-tool server spec is refused, if it is.
///
/// The spec is an ACP `McpServer` in the stdio shape measured from 2.0.11 by
/// `lead_tool_probe.py`: `{name, command, args, env: [{name, value}]}`. It
/// becomes the lead session's `mcpServers` and is **journaled** with the run,
/// so nothing secret may be in it. The lead's own Protocol credential travels
/// as a *path* to a private file, never as a value, and a variable whose name
/// marks it as a credential is refused exactly as it is for the harness's own
/// environment.
pub fn lead_tool_refusals(tool: &Value) -> Vec<Value> {
    let mut refusals = Vec::new();
    let Some(object) = tool.as_object() else {
        return vec![refusal("lead_tool_must_be_an_object", Value::Null)];
    };
    for name in object.keys() {
        if !["name", "command", "args", "env"].contains(&name.as_str()) {
            refusals.push(refusal("lead_tool_unsupported_field", json!(name)));
        }
    }
    if !tool["name"].as_str().is_some_and(|n| !n.is_empty()) {
        refusals.push(refusal("lead_tool_needs_a_name", Value::Null));
    }
    if !tool["command"]
        .as_str()
        .is_some_and(|p| Path::new(p).is_absolute())
    {
        refusals.push(refusal(
            "lead_tool_command_must_be_an_absolute_path",
            Value::Null,
        ));
    }
    let args = tool["args"].as_array();
    if !args.is_some_and(|a| a.iter().all(Value::is_string)) {
        refusals.push(refusal("lead_tool_args_must_be_strings", Value::Null));
    }
    let env = tool["env"].as_array();
    if !env.is_some_and(|e| {
        e.iter().all(|v| {
            v.as_object().is_some_and(|o| o.len() == 2)
                && v["name"].is_string()
                && v["value"].is_string()
        })
    }) {
        refusals.push(refusal(
            "lead_tool_env_must_be_name_value_pairs",
            Value::Null,
        ));
    }
    for variable in env.into_iter().flatten() {
        let name = variable["name"].as_str().unwrap_or_default();
        if CREDENTIAL_MARKERS
            .iter()
            .any(|m| name.to_uppercase().contains(m))
        {
            refusals.push(refusal("env_carries_a_credential_variable", json!(name)));
        }
    }
    let values = args
        .into_iter()
        .flatten()
        .chain(env.into_iter().flatten().map(|v| &v["value"]));
    if values
        .filter_map(Value::as_str)
        .any(|v| v.contains(PIO_CREDENTIAL_PREFIX))
    {
        refusals.push(refusal("lead_tool_carries_a_credential_value", Value::Null));
    }
    refusals
}

pub mod fake;

#[cfg(test)]
mod tests;
