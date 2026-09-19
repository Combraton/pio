use super::*;
use std::os::unix::fs::PermissionsExt;

/// Writing a test executable and running one race inside a single test binary:
/// a sibling test's fork inherits the still-open write descriptor, and Linux
/// then refuses the exec with `ETXTBSY`. `Command::spawn` returns only once the
/// child has exec'd, so serializing every "write a fake, then run one" region
/// closes the window. Learned in M2; see VERIFICATION.
static FAKE_EXECUTABLES: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serialized<T>(body: impl FnOnce() -> T) -> T {
    let _guard = FAKE_EXECUTABLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    body()
}

/// Labeled fake Claude Code: prints a version, a distinct help per command, and
/// an `auth status` document. It appends the config directory it was given next
/// to itself, so a test can prove isolation.
fn fake_claude(dir: &Path, version: &str, top_help: &str, auth: &str) -> PathBuf {
    let path = dir.join("fake-claude");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\n\
             echo \"$CLAUDE_CONFIG_DIR\" >> \"$(dirname \"$0\")/configs.log\"\n\
             case \"$1\" in\n\
             --version) echo '{version} (Claude Code)' ;;\n\
             --help) echo '{top_help}' ;;\n\
             auth) [ \"$2\" = status ] && printf '%s' '{auth}' ;;\n\
             *) echo \"help for $1\" ;;\n\
             esac\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn path_var() -> Option<OsString> {
    Some(OsString::from("/usr/bin:/bin"))
}

fn expected_surface(dir: &Path, top_help: &str) -> Value {
    let config = dir.join("expected-config");
    std::fs::create_dir_all(&config).unwrap();
    let fake = fake_claude(dir, PINNED_VERSION, top_help, "{}");
    surface_identity(&fake, &config, path_var().as_deref()).unwrap()
}

#[test]
fn surface_identity_digests_every_command_and_its_listing() {
    let dir = tempfile::tempdir().unwrap();
    let identity = serialized(|| expected_surface(dir.path(), "top help v1"));
    assert_eq!(identity["command_count"], SURFACE_COMMANDS.len());
    let commands = identity["commands"].as_object().unwrap();
    assert!(commands.contains_key("<top>") && commands.contains_key("auth"));
    // Every subcommand help differs, so no two digests collide.
    let distinct: std::collections::BTreeSet<&str> =
        commands.values().map(|d| d.as_str().unwrap()).collect();
    assert_eq!(distinct.len(), commands.len());
    let listing: String = commands
        .iter()
        .map(|(name, digest)| format!("{name}\t{}\n", digest.as_str().unwrap()))
        .collect();
    assert_eq!(
        identity["surface_listing_sha256"],
        sha256_hex(listing.as_bytes())
    );
}

#[test]
fn surface_drift_names_changed_added_and_removed_commands() {
    let before = json!({"commands":{"<top>":"a","auth":"b","mcp":"c"}});
    let after = json!({"commands":{"<top>":"a","auth":"CHANGED","plugin":"d"}});
    let mut drift = surface_drift(&before, &after);
    drift.sort_by_key(|d| d["command"].as_str().unwrap().to_owned());
    assert_eq!(
        drift,
        vec![
            json!({"command":"auth","change":"changed"}),
            json!({"command":"mcp","change":"removed"}),
            json!({"command":"plugin","change":"added"}),
        ]
    );
}

#[test]
fn qualify_accepts_the_pinned_version_in_an_isolated_config() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("work");
    let record = serialized(|| {
        let expected = expected_surface(dir.path(), "top help v1");
        let claude = fake_claude(dir.path(), PINNED_VERSION, "top help v1", "{}");
        qualify(&claude, &expected, path_var().as_deref(), &work).unwrap()
    });
    assert_eq!(record["qualified"], true, "{record:#}");
    assert_eq!(record["surface"]["drift_count"], 0);
    assert_eq!(record["version"], PINNED_VERSION);
    assert_eq!(record["isolated_config_dir"], true);
    assert!(record["resolution"]["binary_sha256"].is_string());
    // The user's own configuration directory is never used.
    let configs = std::fs::read_to_string(dir.path().join("configs.log")).unwrap();
    let isolated = work.join("isolated-claude-config");
    assert!(!configs.is_empty());
    for line in configs.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        assert!(
            Path::new(line) == isolated || Path::new(line) == dir.path().join("expected-config"),
            "must never use the user's Claude configuration: {line}"
        );
    }
}

#[test]
fn qualify_refuses_an_unsupported_version_without_running_claude_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let record = serialized(|| {
        let expected = expected_surface(dir.path(), "top help v1");
        std::fs::remove_file(dir.path().join("configs.log")).unwrap();
        let claude = fake_claude(dir.path(), "2.1.279", "top help v1", "{}");
        qualify(
            &claude,
            &expected,
            path_var().as_deref(),
            &dir.path().join("work"),
        )
        .unwrap()
    });
    assert_eq!(record["qualified"], false);
    assert_eq!(
        record["refusals"],
        json!([{"reason":"unsupported_version","observed":"2.1.279"}])
    );
    assert_eq!(
        record["surface"],
        json!({"skipped":"version_not_qualified"})
    );
    // Only the version probe ran: one invocation, not nine.
    let configs = std::fs::read_to_string(dir.path().join("configs.log")).unwrap();
    assert_eq!(configs.lines().filter(|l| !l.trim().is_empty()).count(), 1);
}

#[test]
fn qualify_refuses_surface_drift_naming_the_command() {
    let dir = tempfile::tempdir().unwrap();
    let record = serialized(|| {
        let expected = expected_surface(dir.path(), "top help v1");
        // A self-update that changes the top-level help and nothing else.
        let claude = fake_claude(dir.path(), PINNED_VERSION, "top help v2", "{}");
        qualify(
            &claude,
            &expected,
            path_var().as_deref(),
            &dir.path().join("work"),
        )
        .unwrap()
    });
    assert_eq!(record["qualified"], false);
    assert_eq!(
        record["refusals"],
        json!([{"reason":"surface_drift","commands":1}])
    );
    assert_eq!(
        record["surface"]["drift"],
        json!([{"command":"<top>","change":"changed"}])
    );
}

#[test]
fn qualify_refuses_a_missing_executable_as_data() {
    let dir = tempfile::tempdir().unwrap();
    let record = qualify(
        &dir.path().join("absent"),
        &json!({"commands":{}}),
        path_var().as_deref(),
        &dir.path().join("work"),
    )
    .unwrap();
    assert_eq!(record["qualified"], false);
    assert_eq!(record["refusals"][0]["reason"], "unresolved_executable");
}

#[test]
fn auth_route_records_the_route_and_drops_account_identity() {
    let dir = tempfile::tempdir().unwrap();
    let status = r#"{"loggedIn":true,"authMethod":"claude.ai","apiProvider":"firstParty","subscriptionType":"max","email":"person@example.invalid","orgId":"org-1","orgName":"Their Organization"}"#;
    let route = serialized(|| {
        let claude = fake_claude(dir.path(), PINNED_VERSION, "top help v1", status);
        auth_route(
            &claude,
            &dir.path().join("route-config"),
            path_var().as_deref(),
        )
        .unwrap()
    });
    assert_eq!(route["parsed"], true);
    assert_eq!(route["usable"], true);
    assert_eq!(
        route["observed"],
        json!({"loggedIn":true,"authMethod":"claude.ai","apiProvider":"firstParty","subscriptionType":"max"})
    );
    assert_eq!(
        route["account_identity_fields_dropped"],
        json!(["email", "orgId", "orgName"])
    );
    // The identity must not survive anywhere in the record.
    let text = serde_json::to_string(&route).unwrap();
    for leaked in ["person@example.invalid", "org-1", "Their Organization"] {
        assert!(!text.contains(leaked), "leaked {leaked} in {text}");
    }
}

#[test]
fn auth_route_reports_a_missing_route_as_unusable() {
    let dir = tempfile::tempdir().unwrap();
    let route = serialized(|| {
        let claude = fake_claude(
            dir.path(),
            PINNED_VERSION,
            "top help v1",
            r#"{"loggedIn":false,"authMethod":"none","apiProvider":"firstParty"}"#,
        );
        auth_route(
            &claude,
            &dir.path().join("route-config"),
            path_var().as_deref(),
        )
        .unwrap()
    });
    assert_eq!(route["usable"], false);
    assert_eq!(route["observed"]["authMethod"], "none");
    assert_eq!(route["account_identity_fields_dropped"], json!([]));
}

#[test]
fn permission_mode_guard_requires_the_configured_default() {
    let accept = json!({"permissions":{"defaultMode":"acceptEdits"}});
    let allowed = permission_mode_guard(&accept, "acceptEdits");
    assert_eq!(allowed["allowed"], true, "{allowed:#}");
    assert_eq!(allowed["configured"], "acceptEdits");

    // No breadth ordering is established, so every other value refuses, even
    // ones that plausibly deny more.
    for requested in ["plan", "dontAsk", "manual", "auto", "invented"] {
        let guard = permission_mode_guard(&accept, requested);
        assert_eq!(guard["allowed"], false, "{requested}");
        assert_eq!(
            guard["unresolved"][0]["reason"],
            "requested mode is not the configured default and no breadth ordering is established"
        );
    }
    // The forbidden mode refuses from either side.
    let requested_bypass = permission_mode_guard(&accept, FORBIDDEN_MODE);
    assert_eq!(requested_bypass["allowed"], false);
    assert_eq!(
        requested_bypass["unresolved"][0]["reason"],
        "PIO never requests this mode"
    );
    let configured_bypass = permission_mode_guard(
        &json!({"permissions":{"defaultMode":FORBIDDEN_MODE}}),
        FORBIDDEN_MODE,
    );
    assert_eq!(configured_bypass["allowed"], false);

    // An absent or unreadable default refuses rather than assuming one.
    for settings in [
        json!({}),
        json!({"permissions":{}}),
        json!({"permissions":{"defaultMode":{"mode":"acceptEdits"}}}),
    ] {
        let guard = permission_mode_guard(&settings, "acceptEdits");
        assert_eq!(guard["allowed"], false, "{settings}");
        assert!(!guard["unresolved"].as_array().unwrap().is_empty());
    }
}

#[test]
fn settings_snapshot_reports_the_facts_the_guard_and_receipts_need() {
    let dir = tempfile::tempdir().unwrap();
    let settings = r#"{"permissions":{"defaultMode":"acceptEdits","allow":["Bash(a)","Bash(b)"]},
      "model":"opus[1m]","alwaysThinkingEnabled":true,
      "enabledPlugins":{"one@market":true,"two@market":true,"off@market":false}}"#;
    std::fs::write(dir.path().join("settings.json"), settings).unwrap();
    let snapshot = settings_snapshot(dir.path()).unwrap();
    assert_eq!(snapshot["exists"], true);
    assert_eq!(snapshot["permissions"]["defaultMode"], "acceptEdits");
    assert_eq!(snapshot["permissions"]["allow_entry_count"], 2);
    assert_eq!(snapshot["model"], "opus[1m]");
    assert_eq!(snapshot["always_thinking_enabled"], true);
    assert_eq!(
        snapshot["enabled_plugins"],
        json!(["one@market", "two@market"])
    );
    assert!(snapshot["raw_sha256"].is_string());

    let absent = settings_snapshot(&dir.path().join("nowhere")).unwrap();
    assert_eq!(absent["exists"], false);
    assert_eq!(absent["raw_sha256"], Value::Null);
    assert_eq!(
        permission_mode_guard(&absent, "acceptEdits")["allowed"],
        false
    );
}

#[test]
fn checked_in_surface_identity_is_its_own_listing() {
    let identity: Value = serde_json::from_str(QUALIFIED_SURFACE).unwrap();
    assert_eq!(identity["format"], "pio-claude-surface-identity/1");
    assert_eq!(identity["command_count"], SURFACE_COMMANDS.len());
    let commands = identity["commands"].as_object().unwrap();
    assert_eq!(commands.len(), SURFACE_COMMANDS.len());
    for name in SURFACE_COMMANDS {
        assert!(commands.contains_key(*name), "missing {name}");
    }
    let listing: String = commands
        .iter()
        .map(|(name, digest)| format!("{name}\t{}\n", digest.as_str().unwrap()))
        .collect();
    assert_eq!(
        identity["surface_listing_sha256"],
        sha256_hex(listing.as_bytes())
    );
}
