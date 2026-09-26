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
    // Serialized like every other spawn here: a missing executable still
    // forks before it fails, and a fork inside another test's write-then-run
    // region is what makes an exec fail with `ETXTBSY` on Linux.
    let record = serialized(|| {
        qualify(
            &dir.path().join("absent"),
            &json!({"commands":{}}),
            &ChildEnv::isolated(&dir.path().join("work")).with_path(path_var().as_deref()),
            &dir.path().join("work"),
        )
        .unwrap()
    });
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

    // An absent default is the product's own, which is not `acceptEdits`;
    // an unreadable one refuses rather than assuming anything.
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

/// D2. Most installations configure no `permissions.defaultMode`, and the
/// guard refused every one of them. Absent means the product's own default,
/// measured as `default` in `system/init`; the guard compares against that,
/// and requesting it passes no flag, because `--permission-mode` does not
/// accept the name.
#[test]
fn an_absent_default_mode_is_the_product_default_and_is_compared_like_any_other() {
    assert_eq!(product_default_permission_mode(), "default");
    for settings in [json!({}), json!({"permissions":{"allow":["Bash(cat)"]}})] {
        let guard = permission_mode_guard(&settings, "default");
        assert_eq!(guard["allowed"], true, "{guard:#}");
        assert_eq!(guard["configured"], "default");
        assert_eq!(guard["configured_source"], "product_default");
        assert_eq!(guard["unresolved"], json!([]));
        // Still equality: an absent default admits nothing broader, and
        // nothing narrower either, since no ordering is established.
        for requested in ["acceptEdits", "plan", "manual", FORBIDDEN_MODE] {
            let refused = permission_mode_guard(&settings, requested);
            assert_eq!(refused["allowed"], false, "{requested}: {refused:#}");
        }
    }
    // A configured value still comes from the user's settings.
    let configured = permission_mode_guard(
        &json!({"permissions":{"defaultMode":"acceptEdits"}}),
        "acceptEdits",
    );
    assert_eq!(configured["configured_source"], "user_settings");
    // The product default has no flag spelling; every other mode is passed.
    assert!(permission_mode_args("default").is_empty());
    assert_eq!(
        permission_mode_args("acceptEdits"),
        ["--permission-mode", "acceptEdits"]
    );
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

/// The pin and the compiled-in identities name one release: both identities
/// are the ones checked in under `adapters/claude/<PINNED_VERSION>`, and the
/// committed zero-token re-qualification for that version measured the same
/// executable version, qualified with no findings, against the same surface
/// and stream. A stale `PINNED_VERSION` fails closed at run time, but a pin
/// pointing at another release's identity would qualify the wrong interface
/// silently. The Codex adapter has the same test.
#[test]
fn pin_names_the_identity_directory_and_the_requalification_record() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let directory = workspace.join(format!("adapters/claude/{PINNED_VERSION}"));
    let surface = std::fs::read_to_string(directory.join("surface-identity.json"))
        .expect("a surface identity for PINNED_VERSION");
    let stream = std::fs::read_to_string(directory.join("stream-identity.json"))
        .expect("a stream identity for PINNED_VERSION");
    assert_eq!(
        surface, QUALIFIED_SURFACE,
        "the compiled-in surface identity is not the one checked in for {PINNED_VERSION}"
    );
    assert_eq!(
        stream, QUALIFIED_STREAM,
        "the compiled-in stream identity is not the one checked in for {PINNED_VERSION}"
    );
    let surface: Value = serde_json::from_str(&surface).unwrap();
    let stream: Value = serde_json::from_str(&stream).unwrap();
    assert_eq!(surface["version"], PINNED_VERSION);
    assert_eq!(stream["version"], PINNED_VERSION);
    let record: Value = serde_json::from_str(
        &std::fs::read_to_string(workspace.join(format!(
            "docs/work/m3/claude-qualification/requalification-{PINNED_VERSION}.json"
        )))
        .expect("a committed re-qualification record for PINNED_VERSION"),
    )
    .unwrap();
    assert_eq!(record["executable"]["version"], PINNED_VERSION);
    assert_eq!(record["qualified"], true);
    assert_eq!(record["findings"], json!([]));
    assert_eq!(record["model_calls"], 0);
    assert_eq!(
        record["cli_surface"]["surface_listing_sha256"],
        surface["surface_listing_sha256"]
    );
    assert_eq!(record["stream_identity"], stream);
}

/// D11. The product holds each run's `system/init` to the pinned stream
/// identity: the key set and the capability list, with both digests
/// recorded. A labeled fake may carry its label key and nothing else.
#[test]
fn init_is_held_to_the_pinned_stream_identity() {
    let pinned: Value = serde_json::from_str(QUALIFIED_STREAM).unwrap();
    let mut init = serde_json::Map::new();
    for key in pinned["init_keys"].as_array().unwrap() {
        init.insert(key.as_str().unwrap().to_owned(), Value::Null);
    }
    init.insert("capabilities".into(), pinned["capabilities"].clone());
    let init = Value::Object(init);

    let same = init_identity(&init, &pinned, false);
    assert_eq!(same["matches"], true, "{same:#}");
    assert_eq!(same["observed_sha256"], same["pinned_sha256"]);
    assert_eq!(same["pinned_version"], PINNED_VERSION);

    // A key the pinned release never sent, as a self-update would add.
    let mut added = init.clone();
    added["next_release_key"] = json!(true);
    let drifted = init_identity(&added, &pinned, false);
    assert_eq!(drifted["matches"], false);
    assert_ne!(drifted["observed_sha256"], drifted["pinned_sha256"]);
    assert_eq!(
        drifted["drift"],
        json!([{"field":"init_keys","added":["next_release_key"],"removed":[],"reordered":false}])
    );
    // A key that went away.
    let mut removed = init.clone();
    removed.as_object_mut().unwrap().remove("permissionMode");
    let drifted = init_identity(&removed, &pinned, false);
    assert_eq!(drifted["drift"][0]["removed"], json!(["permissionMode"]));
    // A capability that moved.
    let mut capabilities = init.clone();
    capabilities["capabilities"] = json!(["interrupt_receipt_v1"]);
    let drifted = init_identity(&capabilities, &pinned, false);
    assert_eq!(drifted["drift"][0]["field"], "capabilities");
    assert_eq!(drifted["matches"], false);

    // The fake's label is the one key a labeled fake may add; a real harness
    // sending it has drifted like any other.
    let mut labeled = init.clone();
    labeled[FAKE_LABEL_KEY] = json!(fake::SOURCE);
    assert_eq!(init_identity(&labeled, &pinned, true)["matches"], true);
    assert_eq!(init_identity(&labeled, &pinned, false)["matches"], false);
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

/// A fixture workspace and a sibling directory outside it, both real, because
/// symlink resolution only means anything against the filesystem.
fn workspace(dir: &Path) -> (PathBuf, PathBuf) {
    let fixture = dir.join("fixture");
    let outside = dir.join("outside");
    std::fs::create_dir_all(fixture.join("src")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), "not yours\n").unwrap();
    std::fs::write(fixture.join("src/calc.py"), "print(1)\n").unwrap();
    (
        std::fs::canonicalize(fixture).unwrap(),
        std::fs::canonicalize(outside).unwrap(),
    )
}

fn placement_of(input: Value, fixture: &Path, cwd: &Path) -> String {
    let record = tool_use_records(
        &[tool_use("Read", "t1", input)],
        Some(&json!([])),
        &Value::Null,
        fixture,
        cwd,
    );
    record["tool_uses"][0]["placement"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// A prefix test on the raw string is wrong in both directions. Both of these
/// were mislabeled before the reviewer's probe.
#[test]
fn a_traversal_out_of_the_fixture_is_not_inside_it() {
    let dir = tempfile::tempdir().unwrap();
    let (fixture, _) = workspace(dir.path());
    let escape = format!("{}/../outside/secret.txt", fixture.display());
    assert_eq!(
        placement_of(json!({"file_path":escape}), &fixture, &fixture),
        "outside_fixture"
    );
    // The same path without the traversal is inside, so the test is about `..`
    // and not about the fixture being unreadable.
    assert_eq!(
        placement_of(
            json!({"file_path":format!("{}/src/calc.py", fixture.display())}),
            &fixture,
            &fixture
        ),
        "inside_fixture"
    );
}

#[test]
fn a_relative_target_resolves_against_the_session_working_directory() {
    let dir = tempfile::tempdir().unwrap();
    let (fixture, outside) = workspace(dir.path());
    assert_eq!(
        placement_of(json!({"file_path":"src/calc.py"}), &fixture, &fixture),
        "inside_fixture"
    );
    assert_eq!(
        placement_of(
            json!({"file_path":"./src/../src/calc.py"}),
            &fixture,
            &fixture
        ),
        "inside_fixture"
    );
    // The same relative name is outside when the session runs elsewhere.
    assert_eq!(
        placement_of(json!({"file_path":"secret.txt"}), &fixture, &outside),
        "outside_fixture"
    );
}

#[test]
fn a_symlink_pointing_out_of_the_fixture_is_followed() {
    let dir = tempfile::tempdir().unwrap();
    let (fixture, outside) = workspace(dir.path());
    std::os::unix::fs::symlink(&outside, fixture.join("escape")).unwrap();
    assert_eq!(
        placement_of(json!({"file_path":"escape/secret.txt"}), &fixture, &fixture),
        "outside_fixture"
    );
    // A link that stays inside is still inside.
    std::os::unix::fs::symlink(fixture.join("src"), fixture.join("inside")).unwrap();
    assert_eq!(
        placement_of(json!({"file_path":"inside/calc.py"}), &fixture, &fixture),
        "inside_fixture"
    );
}

/// A link that is not the last component of the path (review of L3, round
/// 2, HR-1). The resolver used to put the rest of the path on the stack
/// above the link's target, so it walked the rest first and then threw it
/// away: a path through a link inside the fixture and then out of it read
/// `inside_fixture`. These escapes are real on Codex, which joins a
/// command's working directory without canonicalizing it, and on Claude
/// Code and OpenCode, whose tool targets go through the same resolver.
#[test]
fn a_symlink_before_the_last_component_is_followed_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let (fixture, outside) = workspace(dir.path());
    std::os::unix::fs::symlink(fixture.join("src"), fixture.join("inside")).unwrap();
    // Through a link that stays inside, then out by `..`.
    assert_eq!(
        placement_of(
            json!({"file_path":"inside/../../outside/secret.txt"}),
            &fixture,
            &fixture
        ),
        "outside_fixture"
    );
    // No `..` at all: a link to the fixture itself, then a link out of it.
    std::os::unix::fs::symlink(".", fixture.join("self")).unwrap();
    std::os::unix::fs::symlink(&outside, fixture.join("esc2")).unwrap();
    assert_eq!(
        placement_of(
            json!({"file_path":"self/esc2/secret.txt"}),
            &fixture,
            &fixture
        ),
        "outside_fixture"
    );
    // A link that stays inside, to a directory holding a link out.
    std::fs::create_dir_all(fixture.join("sub")).unwrap();
    std::os::unix::fs::symlink(&outside, fixture.join("sub/esc")).unwrap();
    std::os::unix::fs::symlink(fixture.join("sub"), fixture.join("link-in")).unwrap();
    assert_eq!(
        placement_of(
            json!({"file_path":"link-in/esc/secret.txt"}),
            &fixture,
            &fixture
        ),
        "outside_fixture"
    );
    // The same classifier on a directory, as the Codex host uses it for a
    // command's working directory: out through the two links, and the
    // label of a path that stays inside keeps everything after the link.
    let workdir = fixture.join("self/esc2");
    assert_eq!(
        classify_path(workdir.to_str(), &fixture, &fixture)["placement"],
        "outside_fixture"
    );
    let deeper = classify_path(fixture.join("inside/deeper").to_str(), &fixture, &fixture);
    assert_eq!(deeper["placement"], "inside_fixture");
    assert_eq!(deeper["target_label"], "<fixture>/src/deeper");
    // And the decision the Claude and OpenCode hosts make on a permission
    // request: a target out through the links is declined, not surfaced.
    let request = json!({"request":{"input":{"file_path":"self/esc2/secret.txt"},
                                    "tool_name":"Read","tool_use_id":"t-link"}});
    let decided = classify_permission_request(&request, &fixture, &fixture);
    assert_eq!(decided["placement"], "outside_fixture");
    assert_eq!(decided["disposition"], "decline");
}

/// A loop has no end to classify: it is unresolvable, never inside, and
/// the Claude and OpenCode hosts put it to the caller rather than deciding
/// it (review of L3, round 3, R3-HC-1).
#[test]
fn a_symlink_loop_is_unresolvable() {
    let dir = tempfile::tempdir().unwrap();
    let (fixture, _) = workspace(dir.path());
    std::os::unix::fs::symlink(fixture.join("b"), fixture.join("a")).unwrap();
    std::os::unix::fs::symlink(fixture.join("a"), fixture.join("b")).unwrap();
    assert_eq!(resolve_target("a", &fixture), None);
    let placed = classify_path(fixture.join("a/x").to_str(), &fixture, &fixture);
    assert_eq!(placed["placement"], "not_classifiable");
    assert!(placed["target_label"].is_null(), "{placed}");
    let request = json!({"request":{"input":{"file_path":"a/x"},
                                    "tool_name":"Read","tool_use_id":"t-loop"}});
    let decided = classify_permission_request(&request, &fixture, &fixture);
    assert_eq!(decided["placement"], "not_classifiable");
    assert_eq!(decided["disposition"], "surface_as_action");
}

/// Links chained one to the next are followed to the end, however many
/// there are within the budget: six, the last pointing out of the fixture
/// (review of L3, round 3, R3-HC-2; with a budget of two, this read inside).
#[test]
fn a_chain_of_links_is_followed_to_its_end() {
    let dir = tempfile::tempdir().unwrap();
    let (fixture, outside) = workspace(dir.path());
    std::fs::create_dir_all(fixture.join("sub")).unwrap();
    std::os::unix::fs::symlink(&outside, fixture.join("sub/esc")).unwrap();
    std::os::unix::fs::symlink("sub", fixture.join("e6")).unwrap();
    for n in (1..6).rev() {
        std::os::unix::fs::symlink(format!("e{}", n + 1), fixture.join(format!("e{n}"))).unwrap();
    }
    // The kernel lands outside, through all six and then `esc`.
    assert!(fixture.join("e1/esc/secret.txt").exists());
    let out = classify_path(fixture.join("e1/esc").to_str(), &fixture, &fixture);
    assert_eq!(out["placement"], "outside_fixture", "{out}");
    let inside = classify_path(fixture.join("e1").to_str(), &fixture, &fixture);
    assert_eq!(inside["placement"], "inside_fixture", "{inside}");
    assert_eq!(inside["target_label"], "<fixture>/sub");
}

/// A chain longer than the link budget is not walked on as if it ended
/// where the budget did: it is unresolvable. Forty-one links, the kernel's
/// own limit being lower (32 on macOS, 40 on Linux).
#[test]
fn a_chain_past_the_link_budget_is_unresolvable() {
    let dir = tempfile::tempdir().unwrap();
    let (fixture, outside) = workspace(dir.path());
    std::fs::create_dir_all(fixture.join("sub")).unwrap();
    std::os::unix::fs::symlink(&outside, fixture.join("sub/esc")).unwrap();
    std::os::unix::fs::symlink("sub", fixture.join("c41")).unwrap();
    for n in (1..41).rev() {
        std::os::unix::fs::symlink(format!("c{}", n + 1), fixture.join(format!("c{n}"))).unwrap();
    }
    assert_eq!(resolve_target("c1/esc", &fixture), None);
    for path in ["c1/esc", "c1"] {
        let placed = classify_path(fixture.join(path).to_str(), &fixture, &fixture);
        assert_eq!(placed["placement"], "not_classifiable", "{path}: {placed}");
    }
}

/// Make `names` as a chain of nested directories under `dir`, each relative
/// to the one before by its descriptor, so no call names a long path; and
/// return the descriptor of the deepest.
fn nested_dirs(dir: std::fs::File, names: &[String]) -> std::fs::File {
    use std::os::fd::{AsRawFd, FromRawFd};
    names.iter().fold(dir, |parent, name| {
        let name = std::ffi::CString::new(name.as_str()).unwrap();
        // SAFETY: a valid descriptor and a NUL-terminated name; the new
        // descriptor is owned by the File returned.
        unsafe {
            assert_eq!(libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o755), 0);
            let fd = libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY,
            );
            assert!(fd >= 0);
            std::fs::File::from_raw_fd(fd)
        }
    })
}

fn symlink_at(target: &str, dir: &std::fs::File, name: &str) {
    use std::os::fd::AsRawFd;
    let (target, name) = (
        std::ffi::CString::new(target).unwrap(),
        std::ffi::CString::new(name).unwrap(),
    );
    // SAFETY: a valid descriptor and NUL-terminated strings.
    assert_eq!(
        unsafe { libc::symlinkat(target.as_ptr(), dir.as_raw_fd(), name.as_ptr()) },
        0
    );
}

/// A link reached through a resolved path longer than `PATH_MAX`: `readlink`
/// refuses the long path (`ENAMETOOLONG`), while the kernel, resolving one
/// component at a time from the directory it has reached, follows the link
/// out. Two links, each under `PATH_MAX` by itself and together past it,
/// with the second inside the directory the first leads to: `L1/L2/esc`.
/// Taking the refusal for "not a link" read this inside the fixture (review
/// of L3, round 3, R3-HC-1).
#[test]
fn a_link_reached_past_path_max_is_unresolvable() {
    let dir = tempfile::tempdir().unwrap();
    let (fixture, outside) = workspace(dir.path());
    let path_max = libc::PATH_MAX as usize;
    // As many 250-character components as one link's text holds under
    // PATH_MAX (three on macOS, sixteen on Linux).
    let per_link = (path_max - 64) / 251;
    let names = |prefix: char| -> Vec<String> {
        (0..per_link)
            .map(|n| format!("{prefix}{n:02}{}", "d".repeat(247)))
            .collect()
    };
    let (first, second) = (names('a'), names('b'));
    std::fs::create_dir(fixture.join("t")).unwrap();
    let a = nested_dirs(std::fs::File::open(fixture.join("t")).unwrap(), &first);
    std::os::unix::fs::symlink(format!("t/{}", first.join("/")), fixture.join("L1")).unwrap();
    let b = nested_dirs(a.try_clone().unwrap(), &second);
    symlink_at(&second.join("/"), &a, "L2");
    symlink_at(outside.to_str().unwrap(), &b, "esc");
    // What the resolver would build is past PATH_MAX; the kernel follows it
    // out regardless.
    assert!(fixture.as_os_str().len() + 2 * per_link * 251 > path_max);
    assert!(fixture.join("L1/L2/esc/secret.txt").exists());
    assert_eq!(resolve_target("L1/L2/esc", &fixture), None);
    let placed = classify_path(fixture.join("L1/L2/esc").to_str(), &fixture, &fixture);
    assert_eq!(placed["placement"], "not_classifiable", "{placed}");
    assert!(placed["target_label"].is_null(), "{placed}");
    let request = json!({"request":{"input":{"file_path":"L1/L2/esc/secret.txt"},
                                    "tool_name":"Read","tool_use_id":"t-long"}});
    let decided = classify_permission_request(&request, &fixture, &fixture);
    assert_eq!(decided["placement"], "not_classifiable");
    assert_eq!(decided["disposition"], "surface_as_action");
}

/// Gives a directory its permissions back when a test ends, however it
/// ends, so the temporary directory around it can be removed.
struct Unlock(PathBuf);

impl Drop for Unlock {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
    }
}

/// A link out of the fixture, held in a directory this user cannot search
/// (review of L3, round 4, R4-HC-3): `readlink` refuses it (`EACCES`), so
/// where it leads cannot be seen, and the path is unresolvable, never
/// inside. Taking that refusal for "not a link" read it inside the fixture.
/// The kernel refuses it too, now; a directory unlocked before a command
/// runs is the deferred time-of-check case (R3-HC-8).
#[test]
fn a_link_in_an_unsearchable_directory_is_unresolvable() {
    let dir = tempfile::tempdir().unwrap();
    let (fixture, outside) = workspace(dir.path());
    let locked = fixture.join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::os::unix::fs::symlink(&outside, locked.join("esc")).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let _unlock = Unlock(locked.clone());
    // The case itself: this user may not look inside. Run as root, it could,
    // and the test would prove nothing, so it says so.
    let refused = std::fs::read_link(locked.join("esc")).unwrap_err();
    assert_eq!(
        refused.kind(),
        std::io::ErrorKind::PermissionDenied,
        "{refused}"
    );
    assert_eq!(resolve_target("locked/esc", &fixture), None);
    for path in ["locked/esc", "locked/esc/secret.txt"] {
        let placed = classify_path(fixture.join(path).to_str(), &fixture, &fixture);
        assert_eq!(placed["placement"], "not_classifiable", "{path}: {placed}");
        assert!(placed["target_label"].is_null(), "{placed}");
    }
    let request = json!({"request":{"input":{"file_path":"locked/esc/secret.txt"},
                                    "tool_name":"Read","tool_use_id":"t-locked"}});
    let decided = classify_permission_request(&request, &fixture, &fixture);
    assert_eq!(decided["placement"], "not_classifiable");
    assert_eq!(decided["disposition"], "surface_as_action");
}

/// A path through a file (`file.txt/x`): `readlink` answers `ENOTDIR`, and
/// the path is unresolvable, never inside (review of L3, round 4,
/// R4-HC-3). Taking that answer for "not a link" read it inside.
#[test]
fn a_path_through_a_file_is_unresolvable() {
    let dir = tempfile::tempdir().unwrap();
    let (fixture, _) = workspace(dir.path());
    std::fs::write(fixture.join("file.txt"), "text\n").unwrap();
    let refused = std::fs::read_link(fixture.join("file.txt/x")).unwrap_err();
    assert_eq!(refused.raw_os_error(), Some(libc::ENOTDIR), "{refused}");
    assert_eq!(resolve_target("file.txt/x", &fixture), None);
    let placed = classify_path(fixture.join("file.txt/x").to_str(), &fixture, &fixture);
    assert_eq!(placed["placement"], "not_classifiable", "{placed}");
    assert!(placed["target_label"].is_null(), "{placed}");
}

#[test]
fn a_tool_use_the_harness_refused_is_an_attempt_and_not_an_effect() {
    let dir = tempfile::tempdir().unwrap();
    let (fixture, outside) = workspace(dir.path());
    let messages = vec![
        tool_use("Read", "t1", json!({"file_path":"src/calc.py"})),
        tool_use(
            "Read",
            "t2",
            json!({"file_path":outside.join("marker.txt")}),
        ),
    ];
    // The `result` names what the harness refused, by tool use id. Measured on
    // R6: the read outside the workspace was refused outright, and PIO
    // reported it as an observed effect with unresolved liability anyway.
    let denials = json!([{"tool_name":"Read","tool_use_id":"t2",
                          "tool_input":{"file_path":"…"}}]);
    let record = tool_use_records(&messages, Some(&denials), &Value::Null, &fixture, &fixture);
    let uses = record["tool_uses"].as_array().unwrap();
    // Both are still recorded: an attempt is worth knowing about.
    assert_eq!(uses.len(), 2);
    assert_eq!(uses[0]["denied_by_harness"], false);
    assert_eq!(uses[0]["outcome"], "performed");
    assert_eq!(uses[1]["denied_by_harness"], true);
    assert_eq!(uses[1]["outcome"], "attempted_and_denied");
    assert_eq!(uses[1]["placement"], "outside_fixture");
    // Nothing happened outside the workspace, so there is no effect and no
    // liability. Without the denial list this same record said otherwise.
    assert_eq!(record["out_of_fixture_effect_observed"], false);
    assert_eq!(record["out_of_fixture_count"], 0);
    assert_eq!(record["denied_by_harness_count"], 1);
    assert_eq!(record["liability"], "none_observed");
}

/// `result.permission_denials` says a tool use was refused and never says by
/// whom. R3c is the run that showed why that matters: the caller answered
/// deny, the harness recorded the denial, and the record called it the
/// harness's own refusal with "PIO was not asked" beside it.
#[test]
fn a_refusal_is_attributed_to_whoever_decided_it() {
    let dir = tempfile::tempdir().unwrap();
    let (fixture, outside) = workspace(dir.path());
    let messages = vec![
        tool_use("Bash", "t1", json!({"command":"git tag x"})),
        tool_use(
            "Read",
            "t2",
            json!({"file_path":outside.join("marker.txt")}),
        ),
        tool_use("Bash", "t3", json!({"command":"git tag y"})),
    ];
    // All three were refused, by three different deciders.
    let denials = json!([{"tool_use_id":"t1"},{"tool_use_id":"t2"},{"tool_use_id":"t3"}]);
    let decided = json!({
        "t1": {"by":"caller","decision":"deny"},
        "t2": {"by":"pio","decision":"deny","reason":"target_outside_the_fixture_workspace"},
    });
    let record = tool_use_records(&messages, Some(&denials), &decided, &fixture, &fixture);
    let uses = record["tool_uses"].as_array().unwrap();
    assert_eq!(uses[0]["outcome"], "denied_by_caller");
    assert_eq!(uses[0]["decided_by"], "caller");
    assert_eq!(uses[0]["denied_by_harness"], false);
    assert_eq!(uses[1]["outcome"], "declined_by_pio");
    assert_eq!(uses[1]["decided_by"], "pio");
    assert_eq!(uses[1]["denied_by_harness"], false);
    // Nobody was asked about the third, so it is the harness's own refusal.
    assert_eq!(uses[2]["outcome"], "attempted_and_denied");
    assert_eq!(uses[2]["decided_by"], Value::Null);
    assert_eq!(uses[2]["denied_by_harness"], true);

    assert_eq!(record["denied_by_caller_count"], 1);
    assert_eq!(record["declined_by_pio_count"], 1);
    assert_eq!(record["denied_by_harness_count"], 1);
    // None of them ran, whoever decided, so none is an effect.
    assert_eq!(record["out_of_fixture_effect_observed"], false);
    assert_eq!(record["liability"], "none_observed");
}

/// D3. A turn killed or crashed before its final `result` has no denial list.
/// The audit used to read that absence as "nothing was refused" and record
/// PIO's own decline as `performed`, `denied: false`. A deny PIO sent is a
/// refusal whether or not a `result` confirms it; a use nobody denied, with
/// no `result`, is `unknown` and leaves the liability unresolved.
#[test]
fn without_a_result_a_denied_use_stays_denied_and_the_rest_are_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let (fixture, outside) = workspace(dir.path());
    let messages = vec![
        tool_use(
            "Read",
            "t1",
            json!({"file_path":outside.join("marker.txt")}),
        ),
        tool_use("Bash", "t2", json!({"command":"git tag x"})),
        tool_use("Read", "t3", json!({"file_path":"src/calc.py"})),
        tool_use("Bash", "t4", json!({"command":"git tag y"})),
    ];
    let decided = json!({
        "t1": {"by":"pio","decision":"deny","reason":"target_outside_the_fixture_workspace"},
        "t2": {"by":"caller","decision":"deny"},
        "t4": {"by":"caller","decision":"allow"},
    });
    let record = tool_use_records(&messages, None, &decided, &fixture, &fixture);
    let uses = record["tool_uses"].as_array().unwrap();
    assert_eq!(uses[0]["outcome"], "declined_by_pio", "{record}");
    assert_eq!(uses[0]["denied"], true);
    assert_eq!(uses[0]["denied_by_harness"], false);
    assert_eq!(uses[1]["outcome"], "denied_by_caller");
    assert_eq!(uses[1]["denied"], true);
    // Nobody denied these, and no `result` says whether they ran: an allow is
    // a decision, not an observation that the use completed.
    assert_eq!(uses[2]["outcome"], "unknown");
    assert_eq!(uses[2]["denied"], false);
    assert_eq!(uses[3]["outcome"], "unknown");
    assert!(uses.iter().all(|u| u["outcome"] != "performed"), "{record}");
    assert_eq!(record["result_observed"], false);
    assert_eq!(record["unknown_outcome_count"], 2);
    assert_eq!(record["declined_by_pio_count"], 1);
    assert_eq!(record["denied_by_caller_count"], 1);
    // PIO's decline outside the workspace never ran, so it is no effect.
    assert_eq!(record["out_of_fixture_effect_observed"], false);
    assert_eq!(record["liability"], "unresolved");

    // The same uses with a `result` that refused nothing: the undecided
    // inside-fixture read ran, and PIO's decline is still a decline.
    let settled = tool_use_records(
        &messages[..3],
        Some(&json!([])),
        &decided,
        &fixture,
        &fixture,
    );
    let uses = settled["tool_uses"].as_array().unwrap();
    assert_eq!(uses[0]["outcome"], "declined_by_pio");
    assert_eq!(uses[2]["outcome"], "performed");
    assert_eq!(settled["result_observed"], true);
    assert_eq!(settled["unknown_outcome_count"], 0);
    assert_eq!(settled["liability"], "none_observed", "{settled}");
}

#[test]
fn every_tool_use_is_recorded_and_targets_outside_the_fixture_are_flagged() {
    let dir = tempfile::tempdir().unwrap();
    let (fixture, outside) = workspace(dir.path());
    let messages = vec![
        tool_use("Read", "t1", json!({"file_path":"src/calc.py"})),
        tool_use(
            "Edit",
            "t2",
            json!({"file_path":outside.join("secret.txt")}),
        ),
    ];
    let record = tool_use_records(
        &messages,
        Some(&json!([])),
        &Value::Null,
        &fixture,
        &fixture,
    );
    let uses = record["tool_uses"].as_array().unwrap();
    assert_eq!(uses.len(), 2, "a tool use went unrecorded");
    assert_eq!(uses[0]["placement"], "inside_fixture");
    assert_eq!(uses[1]["placement"], "outside_fixture");
    assert_eq!(uses[1]["tool"], "Edit");
    assert_eq!(record["out_of_fixture_effect_observed"], true);
    assert_eq!(record["out_of_fixture_count"], 1);
    assert_eq!(record["liability"], "unresolved");
    assert_eq!(
        record["containment"],
        json!({"mechanism":"harness_permission_rules_only","os_sandbox_observed":false})
    );
}

/// A receipt names a target by digest and a fixture-relative label. Neither a
/// raw path nor the absolute fixture path may appear anywhere in it.
#[test]
fn a_receipt_carries_labels_and_digests_rather_than_paths() {
    let dir = tempfile::tempdir().unwrap();
    let (fixture, outside) = workspace(dir.path());
    let messages = vec![
        tool_use("Read", "t1", json!({"file_path":"src/calc.py"})),
        tool_use(
            "Edit",
            "t2",
            json!({"file_path":outside.join("secret.txt")}),
        ),
    ];
    let record = tool_use_records(
        &messages,
        Some(&json!([])),
        &Value::Null,
        &fixture,
        &fixture,
    );
    let uses = record["tool_uses"].as_array().unwrap();
    assert_eq!(uses[0]["target_label"], "<fixture>/src/calc.py");
    assert_eq!(uses[1]["target_label"], "<outside>");
    assert!(uses[0]["target_sha256"].is_string());
    assert_ne!(uses[0]["target_sha256"], uses[1]["target_sha256"]);
    let text = serde_json::to_string(&record).unwrap();
    for leaked in [fixture.display().to_string(), outside.display().to_string()] {
        assert!(!text.contains(&leaked), "receipt leaked {leaked} in {text}");
    }
}

/// A shell command names no path PIO can resolve. Reporting it as inside the
/// fixture would be a claim the adapter cannot support.
#[test]
fn a_shell_command_is_not_classifiable_rather_than_assumed_contained() {
    let dir = tempfile::tempdir().unwrap();
    let (fixture, _) = workspace(dir.path());
    let messages = vec![tool_use("Bash", "t1", json!({"command":"cat /etc/passwd"}))];
    let record = tool_use_records(
        &messages,
        Some(&json!([])),
        &Value::Null,
        &fixture,
        &fixture,
    );
    let uses = record["tool_uses"].as_array().unwrap();
    assert_eq!(uses[0]["placement"], "not_classifiable");
    assert!(uses[0]["target_label"].is_null());
    assert_eq!(record["unclassifiable_target_count"], 1);
    assert_eq!(record["out_of_fixture_effect_observed"], false);
    assert_eq!(record["liability"], "unresolved");
}

#[test]
fn a_clean_run_inside_the_fixture_reports_no_outstanding_liability() {
    let dir = tempfile::tempdir().unwrap();
    let (fixture, _) = workspace(dir.path());
    let messages = vec![tool_use("Read", "t1", json!({"file_path":"src/calc.py"}))];
    let record = tool_use_records(
        &messages,
        Some(&json!([])),
        &Value::Null,
        &fixture,
        &fixture,
    );
    assert_eq!(record["liability"], "none_observed");
    assert_eq!(record["out_of_fixture_effect_observed"], false);
}

/// PIO declines an out-of-fixture request itself, and never auto-allows
/// anything: an allow is always somebody's decision.
#[test]
fn pio_declines_out_of_fixture_requests_and_surfaces_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let (fixture, outside) = workspace(dir.path());
    let request = |input: Value| {
        json!({"type":"control_request","request_id":"req_1",
               "request":{"subtype":"can_use_tool","tool_name":"Read",
                          "input":input,"tool_use_id":"toolu_1"}})
    };
    let outside_request = classify_permission_request(
        &request(json!({"file_path":outside.join("secret.txt")})),
        &fixture,
        &fixture,
    );
    assert_eq!(outside_request["disposition"], "decline");
    assert_eq!(
        outside_request["reason"],
        "target_outside_the_fixture_workspace"
    );
    assert_eq!(outside_request["target_label"], "<outside>");

    // A traversal is declined for the same reason, not admitted by a prefix test.
    let traversal = classify_permission_request(
        &request(json!({"file_path":"../outside/secret.txt"})),
        &fixture,
        &fixture,
    );
    assert_eq!(traversal["disposition"], "decline");

    // A shell command is surfaced, never auto-allowed.
    let shell = classify_permission_request(
        &request(json!({"command":"cat README.md"})),
        &fixture,
        &fixture,
    );
    assert_eq!(shell["disposition"], "surface_as_action");
    assert_eq!(shell["placement"], "not_classifiable");

    let inside = classify_permission_request(
        &request(json!({"file_path":"src/calc.py"})),
        &fixture,
        &fixture,
    );
    assert_eq!(inside["disposition"], "surface_as_action");
    for classification in [&outside_request, &traversal, &shell, &inside] {
        assert_eq!(classification["auto_allowed"], false);
    }
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
    // Set explicitly rather than inherited: a test that depends on the ambient
    // USER passes or fails on the runner's environment, not on the adapter.
    let present = ChildEnv::isolated(&config).with_user(Some(OsString::from("somebody")));
    assert_eq!(route(present)["usable"], true);
    assert_eq!(
        route(ChildEnv::isolated(&config).with_user(None))["usable"],
        false,
        "a child without USER saw a route it should not have"
    );
}

/// A home, a configuration directory and **this run's workspace**, which sits
/// inside the area fixtures are created in rather than being it.
fn durable_workspace(dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let home = dir.join("home");
    let config = home.join(".claude");
    let workspace = dir.join("fixtures").join("workspace");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(
        config.join("settings.json"),
        r#"{"permissions":{"defaultMode":"acceptEdits","allow":["Bash(cat)"]}}"#,
    )
    .unwrap();
    (home, config, workspace)
}

/// `~/.claude.json` holds the account's email, name and organization under
/// `oauthAccount`. It is never recorded, not even as a list of its keys.
#[test]
fn a_durable_snapshot_never_records_the_account() {
    let dir = tempfile::tempdir().unwrap();
    let (home, config, workspace) = durable_workspace(dir.path());
    std::fs::write(
        home.join(".claude.json"),
        json!({
            "numStartups": 7,
            "oauthAccount": {"emailAddress":"person@example.invalid",
                             "organizationName":"Their Organization"},
            "projects": {}
        })
        .to_string(),
    )
    .unwrap();
    let snapshot = durable_snapshot(&home, &config, &workspace).unwrap();
    let text = serde_json::to_string(&snapshot).unwrap();
    for leaked in [
        "person@example.invalid",
        "Their Organization",
        "oauthAccount",
    ] {
        assert!(!text.contains(leaked), "snapshot leaked {leaked}");
    }
    assert_eq!(snapshot["claude_json"]["account_fields_recorded"], false);
    // The key count excludes it, so the digest cannot be reversed into it.
    assert_eq!(snapshot["claude_json"]["top_level_key_count"], 2);
}

/// A run creates the fixture's project entry, and `--print` trusts the
/// directory without asking. Both are disclosed rather than glossed.
#[test]
fn a_diff_separates_what_the_run_caused_from_bookkeeping() {
    let dir = tempfile::tempdir().unwrap();
    let (home, config, workspace) = durable_workspace(dir.path());
    let state = home.join(".claude.json");
    std::fs::write(&state, json!({"numStartups":7,"projects":{}}).to_string()).unwrap();
    let before = durable_snapshot(&home, &config, &workspace).unwrap();

    std::fs::write(
        &state,
        json!({"numStartups": 8, "promptQueueUseCount": 3,
               "projects": {workspace.display().to_string(): {
                   "hasTrustDialogAccepted": true,
                   "lastTotalInputTokens": 120, "lastCost": 0.01}}})
        .to_string(),
    )
    .unwrap();
    let after = durable_snapshot(&home, &config, &workspace).unwrap();
    let diff = durable_diff(&before, &after);

    assert_eq!(diff["workspace_project_created"], true);
    assert_eq!(diff["workspace_project_trusted_without_asking"], true);
    assert_eq!(diff["claude_json_key_count_changed"], true);
    // PIO edits and removes nothing, so the settings must never move.
    assert_eq!(diff["settings_changed"], false);
    assert_eq!(diff["usage_secondary"]["last_total_input_tokens"], 120);
}

/// The harness slugs the session's working directory. R1 slugged the area
/// fixtures are created in, a path the harness never writes to, so the listing
/// was empty before and after and the diff reported nothing written while a
/// 194 KB session file and a `memory` directory sat on disk.
#[test]
fn the_transcript_listing_follows_the_session_working_directory() {
    let dir = tempfile::tempdir().unwrap();
    let (home, config, workspace) = durable_workspace(dir.path());
    let before = durable_snapshot(&home, &config, &workspace).unwrap();
    assert_eq!(before["transcripts"]["exists"], false);
    assert_eq!(before["transcripts"]["entry_count"], 0);

    let slug = |path: &Path| path.display().to_string().replace(['/', '.'], "-");
    // What the harness writes: a session file and a directory beside it. The
    // directory is empty in the run this was measured from; a file is put in
    // it here because a listing that does not descend would otherwise report
    // the same count and this assertion would prove nothing.
    let written = config.join("projects").join(slug(&workspace));
    std::fs::create_dir_all(written.join("memory")).unwrap();
    std::fs::write(written.join("session.jsonl"), "{}\n").unwrap();
    std::fs::write(written.join("memory").join("note.md"), "nested\n").unwrap();
    // What a snapshot of the parent would have read instead. Nothing here may
    // ever reach the listing, however much of it there is.
    let parent = config
        .join("projects")
        .join(slug(workspace.parent().unwrap()));
    std::fs::create_dir_all(&parent).unwrap();
    for name in ["decoy-a.jsonl", "decoy-b.jsonl", "decoy-c.jsonl"] {
        std::fs::write(parent.join(name), "{}\n").unwrap();
    }

    let after = durable_snapshot(&home, &config, &workspace).unwrap();
    assert_eq!(after["transcripts"]["exists"], true);
    // The file, the directory beside it and the file within: three, not the
    // two a listing that stops at the top would report, and never the decoys.
    assert_eq!(after["transcripts"]["entry_count"], 3);
    assert_eq!(durable_diff(&before, &after)["new_transcript_entries"], 3);
    // Names never appear, only digests of paths relative to the listing root.
    let text = serde_json::to_string(&after).unwrap();
    for name in ["session.jsonl", "memory", "note.md", "decoy-a.jsonl"] {
        assert!(!text.contains(name), "the listing named {name}");
    }
}

#[test]
fn a_settings_change_is_reported_because_pio_must_never_cause_one() {
    let dir = tempfile::tempdir().unwrap();
    let (home, config, workspace) = durable_workspace(dir.path());
    let before = durable_snapshot(&home, &config, &workspace).unwrap();
    std::fs::write(
        config.join("settings.json"),
        r#"{"permissions":{"defaultMode":"acceptEdits","allow":["Bash(cat)","Bash(rm)"]}}"#,
    )
    .unwrap();
    let after = durable_snapshot(&home, &config, &workspace).unwrap();
    assert_eq!(durable_diff(&before, &after)["settings_changed"], true);
}
