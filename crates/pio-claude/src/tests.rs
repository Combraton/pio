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

/// A labeled fake that reports a route only when `USER` reaches it, the way
/// the real 2.1.278 does. Its help and version stay qualified so the test is
/// about the environment and nothing else.
fn fake_claude_reading_user(dir: &Path) -> PathBuf {
    let path = dir.join("fake-claude-user");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\n\
             case \"$1\" in\n\
             --version) echo '{PINNED_VERSION} (Claude Code)' ;;\n\
             auth) if [ -n \"$USER\" ]; then\n\
                     printf '%s' '{{\"loggedIn\":true,\"authMethod\":\"claude.ai\"}}'\n\
                   else\n\
                     printf '%s' '{{\"loggedIn\":false,\"authMethod\":\"none\"}}'\n\
                   fi ;;\n\
             *) echo 'help' ;;\n\
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
    surface_identity(
        &fake,
        &ChildEnv::isolated(&config).with_path(path_var().as_deref()),
    )
    .unwrap()
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
        qualify(
            &claude,
            &expected,
            &ChildEnv::isolated(&work).with_path(path_var().as_deref()),
            &work,
        )
        .unwrap()
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
            &ChildEnv::isolated(&dir.path().join("work")).with_path(path_var().as_deref()),
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
            &ChildEnv::isolated(&dir.path().join("work")).with_path(path_var().as_deref()),
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
        &ChildEnv::isolated(&dir.path().join("work")).with_path(path_var().as_deref()),
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
            &ChildEnv::isolated(&dir.path().join("route-config")).with_path(path_var().as_deref()),
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
            &ChildEnv::isolated(&dir.path().join("route-config")).with_path(path_var().as_deref()),
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
    // The second settings file carries permission rules of its own. A snapshot
    // that read only the first would under-report the user's allow list.
    std::fs::write(
        dir.path().join("settings.local.json"),
        r#"{"permissions":{"allow":["Bash(cat)","Read(/tmp/**)"],"deny":["Bash(rm:*)"]}}"#,
    )
    .unwrap();
    let snapshot = settings_snapshot(dir.path()).unwrap();
    assert_eq!(snapshot["permissions"]["defaultMode"], "acceptEdits");
    assert_eq!(snapshot["permissions"]["allow_entry_count"], 4);
    assert_eq!(snapshot["permissions"]["bash_allow_rule_count"], 3);
    assert_eq!(snapshot["permissions"]["deny_entry_count"], 1);
    assert_eq!(snapshot["model"], "opus[1m]");
    assert_eq!(snapshot["always_thinking_enabled"], true);
    assert_eq!(
        snapshot["enabled_plugins"],
        json!(["one@market", "two@market"])
    );
    let files = snapshot["files"].as_array().unwrap();
    assert_eq!(files.len(), SETTINGS_FILES.len());
    for file in files {
        assert_eq!(file["exists"], true, "{file}");
        assert!(file["raw_sha256"].is_string(), "{file}");
    }
    // No sandbox key means the product default, which is off, so containment
    // is the permission rules only.
    assert_eq!(
        snapshot["sandbox"],
        json!({"configured":false,"os_sandbox_in_effect":false})
    );

    let absent = settings_snapshot(&dir.path().join("nowhere")).unwrap();
    for file in absent["files"].as_array().unwrap() {
        assert_eq!(file["exists"], false);
        assert_eq!(file["raw_sha256"], Value::Null);
    }
    assert_eq!(absent["permissions"]["allow_entry_count"], 0);
    assert_eq!(
        permission_mode_guard(&absent, "acceptEdits")["allowed"],
        false
    );
}

/// The user's second settings file is not optional evidence: a run that read
/// only `settings.json` would report a smaller allow list than the one in force.
#[test]
fn the_local_settings_file_contributes_to_the_disclosed_allow_list() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("settings.json"),
        r#"{"permissions":{"defaultMode":"acceptEdits","allow":["Bash(a)"]}}"#,
    )
    .unwrap();
    let alone = settings_snapshot(dir.path()).unwrap();
    std::fs::write(
        dir.path().join("settings.local.json"),
        r#"{"permissions":{"allow":["Bash(b)","Bash(c)"]}}"#,
    )
    .unwrap();
    let both = settings_snapshot(dir.path()).unwrap();
    assert_eq!(alone["permissions"]["allow_entry_count"], 1);
    assert_eq!(both["permissions"]["allow_entry_count"], 3);
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

/// The stream identity is the half of the interface a help digest cannot see.
#[test]
fn checked_in_stream_identity_carries_every_compared_field() {
    let identity: Value = serde_json::from_str(QUALIFIED_STREAM).unwrap();
    assert_eq!(identity["format"], "pio-claude-stream-identity/1");
    assert_eq!(identity["version"], PINNED_VERSION);
    for field in STREAM_IDENTITY_FIELDS {
        assert!(!identity[*field].is_null(), "stream identity lacks {field}");
    }
    // Measured: init does not arrive until something is written to stdin, so
    // the effective-mode check cannot precede release of the brief.
    assert_eq!(identity["init_waits_for_stdin"], true);
    // Measured: the product default with an empty configuration. The model
    // equals the owner's configured model, so a receipt showing Opus proves
    // nothing about fidelity; the name lists do that instead.
    assert_eq!(identity["product_default_permission_mode"], "default");
    assert_eq!(identity["capabilities"][0], "interrupt_receipt_v1");
}

#[test]
fn stream_drift_names_every_field_that_moved() {
    let expected: Value = serde_json::from_str(QUALIFIED_STREAM).unwrap();
    assert!(stream_drift(&expected, &expected).is_empty());
    let mut moved = expected.clone();
    moved["capabilities"] = json!(["interrupt_receipt_v1"]);
    moved["product_default_model"] = json!("something-else");
    let drift = stream_drift(&expected, &moved);
    let fields: Vec<&str> = drift.iter().map(|d| d["field"].as_str().unwrap()).collect();
    assert_eq!(fields, ["capabilities", "product_default_model"]);
}

fn permission_request() -> Value {
    json!({
        "type":"control_request","request_id":"req_1_abcd",
        "request":{
            "subtype":"can_use_tool","tool_name":"Bash",
            "input":{"command":"ls -la","description":"list"},
            "tool_use_id":"toolu_1",
            "permission_suggestions":[
                {"type":"addRules","destination":"userSettings",
                 "behavior":"allow","rules":[{"tool_name":"Bash"}]}],
        }
    })
}

#[test]
fn an_allow_echoes_the_original_input_and_widens_nothing() {
    let request = permission_request();
    let decision = permission_decision(&request, "allow", "").unwrap();
    let response = &decision["envelope"]["response"]["response"];
    assert_eq!(response["behavior"], "allow");
    assert_eq!(response["updatedInput"], request["request"]["input"]);
    assert_eq!(decision["envelope"]["response"]["request_id"], "req_1_abcd");
    for field in WIDENING_RESPONSE_FIELDS {
        assert!(response[*field].is_null(), "decision carried {field}");
    }
    // The harness offered a rule update. It is recorded and not acted on.
    assert_eq!(decision["suggestions_offered"], 1);
    assert_eq!(decision["suggestions_acted_on"], 0);
}

#[test]
fn a_deny_carries_a_reason_and_no_interrupt() {
    let decision =
        permission_decision(&permission_request(), "deny", "outside the fixture").unwrap();
    let response = &decision["envelope"]["response"]["response"];
    assert_eq!(response["behavior"], "deny");
    assert_eq!(response["message"], "outside the fixture");
    assert!(response["interrupt"].is_null());
    assert!(response["updatedInput"].is_null());
}

#[test]
fn no_decision_outside_the_single_use_allowlist_is_encodable() {
    for behavior in ["always_allow", "addRules", "setMode", "bypass", ""] {
        assert!(
            permission_decision(&permission_request(), behavior, "x").is_err(),
            "{behavior} was encodable"
        );
    }
    assert_eq!(SINGLE_USE_DECISIONS, ["allow", "deny"]);
    assert_eq!(WIDENING_CONTROL_SUBTYPES, ["set_permission_mode"]);
}

#[test]
fn a_request_without_an_id_is_refused_rather_than_answered_blindly() {
    let mut request = permission_request();
    request["request_id"] = Value::Null;
    assert!(permission_decision(&request, "allow", "").is_err());
}

fn tool_use(name: &str, id: &str, input: Value) -> Value {
    json!({"type":"assistant","message":{"content":[
        {"type":"text","text":"working"},
        {"type":"tool_use","name":name,"id":id,"input":input}]}})
}

#[test]
fn every_tool_use_is_recorded_and_targets_outside_the_fixture_are_flagged() {
    let fixture = Path::new("/tmp/fixture");
    let messages = vec![
        tool_use("Read", "t1", json!({"file_path":"/tmp/fixture/README.md"})),
        tool_use("Edit", "t2", json!({"file_path":"/etc/hosts"})),
    ];
    let record = tool_use_records(&messages, fixture);
    let uses = record["tool_uses"].as_array().unwrap();
    assert_eq!(uses.len(), 2, "a tool use went unrecorded");
    assert_eq!(uses[0]["placement"], "inside_fixture");
    assert_eq!(uses[1]["placement"], "outside_fixture");
    assert_eq!(uses[1]["tool"], "Edit");
    assert_eq!(record["out_of_fixture_effect_observed"], true);
    assert_eq!(record["out_of_fixture_count"], 1);
    assert_eq!(record["liability"], "unresolved");
    // The receipt states what containment actually is, every time.
    assert_eq!(
        record["containment"],
        json!({"mechanism":"harness_permission_rules_only","os_sandbox_observed":false})
    );
}

/// A shell command names no path PIO can resolve. Reporting it as inside the
/// fixture would be a claim the adapter cannot support.
#[test]
fn a_shell_command_is_not_classifiable_rather_than_assumed_contained() {
    let fixture = Path::new("/tmp/fixture");
    let messages = vec![tool_use("Bash", "t1", json!({"command":"cat /etc/passwd"}))];
    let record = tool_use_records(&messages, fixture);
    let uses = record["tool_uses"].as_array().unwrap();
    assert_eq!(uses[0]["placement"], "not_classifiable");
    assert!(uses[0]["target"].is_null());
    assert_eq!(record["unclassifiable_target_count"], 1);
    assert_eq!(record["out_of_fixture_effect_observed"], false);
    assert_eq!(record["liability"], "unresolved");
}

#[test]
fn a_clean_run_inside_the_fixture_reports_no_outstanding_liability() {
    let messages = vec![tool_use(
        "Read",
        "t1",
        json!({"file_path":"/tmp/fixture/a.txt"}),
    )];
    let record = tool_use_records(&messages, Path::new("/tmp/fixture"));
    assert_eq!(record["liability"], "none_observed");
    assert_eq!(record["out_of_fixture_effect_observed"], false);
}

/// The environment is an allowlist, and `USER` earns its place there.
///
/// Measured on the real 2.1.278: without `USER`, `auth status` reports
/// `loggedIn: false` even given the user's real `HOME`, so a child that does
/// not get it makes a working login look absent. The committed adapter did
/// exactly that and refused a usable route. This pins the shape that fixes it.
#[test]
fn the_child_environment_is_an_allowlist_that_carries_user() {
    assert_eq!(ENV_ALLOWLIST, ["PATH", "HOME", "USER", "CLAUDE_CONFIG_DIR"]);
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");

    // As configured: the user's own HOME, and CLAUDE_CONFIG_DIR left unset,
    // because the product expects `.claude.json` inside a configured dir while
    // the user has it beside `~/.claude/`.
    let configured = ChildEnv::as_configured(&home);
    assert_eq!(configured.home(), home);

    // The isolated control differs in exactly one thing: the configuration the
    // harness can see. Both carry USER, so a refusal cannot be an artefact of
    // the environment rather than of the missing credential.
    let isolated = ChildEnv::isolated(&home);
    assert_eq!(isolated.home(), home);
}

/// A fake that reports the route only when `USER` reaches it reproduces the
/// defect: the adapter must pass it, or every route reads as absent.
#[test]
fn a_route_that_depends_on_user_is_observed_only_when_user_is_passed() {
    let dir = tempfile::tempdir().unwrap();
    let route = |env: ChildEnv| {
        serialized(|| {
            let claude = fake_claude_reading_user(dir.path());
            auth_route(&claude, &env.with_path(path_var().as_deref())).unwrap()
        })
    };
    let config = dir.path().join("route-config");
    assert_eq!(route(ChildEnv::isolated(&config))["usable"], true);
    assert_eq!(
        route(ChildEnv::isolated(&config).with_user(None))["usable"],
        false,
        "a child without USER saw a route it should not have"
    );
}
