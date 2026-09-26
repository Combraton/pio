use super::*;
use std::os::unix::fs::PermissionsExt;

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn executable(path: &Path, text: &str) {
    write(path, text);
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Writing a test executable and running one race inside a single test binary:
/// a sibling test's fork inherits the still-open write descriptor, and Linux
/// then refuses the exec with `ETXTBSY`. `Command::spawn` returns only once the
/// child has exec'd, so serializing every "write a fake, then run one" region
/// closes the window. Tests that only write data do not need this.
static FAKE_EXECUTABLES: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serialized<T>(body: impl FnOnce() -> T) -> T {
    let _guard = FAKE_EXECUTABLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    body()
}

/// Labeled fake Codex CLI: prints a version and writes two schema files. It
/// appends the CODEX_HOME it was given next to itself.
fn fake_codex(dir: &Path, version: &str, a: &str, b: &str) -> PathBuf {
    let path = dir.join("fake-codex");
    executable(
        &path,
        &format!(
            "#!/bin/sh\necho \"$CODEX_HOME\" >> \"$(dirname \"$0\")/homes.log\"\ncase \"$1\" in\n  --version) echo 'codex-cli {version}' ;;\n  app-server) mkdir -p \"$4/v2\" && printf '%s' '{a}' > \"$4/a.json\" && printf '%s' '{b}' > \"$4/v2/b.json\" ;;\nesac\n"
        ),
    );
    path
}

fn expected(dir: &Path) -> Value {
    let reference = dir.join("reference");
    write(
        &reference.join("a.json"),
        r#"{"type":"object","enum":["x","y"]}"#,
    );
    write(&reference.join("v2/b.json"), r#"{"a":1,"b":[3,2,1]}"#);
    schema_identity(&reference).unwrap()
}

fn path_var() -> Option<OsString> {
    Some(OsString::from("/usr/bin:/bin"))
}

#[test]
fn canonical_json_sorts_members_and_preserves_arrays_and_values() {
    let value: Value = serde_json::from_str(r#"{"b":[2,1,{"d":true,"c":null}],"a":"\n"}"#).unwrap();
    assert_eq!(
        canonical_json(&value),
        r#"{"a":"\n","b":[2,1,{"c":null,"d":true}]}"#
    );
    let reordered: Value =
        serde_json::from_str(r#"{"a":"\n","b":[2,1,{"c":null,"d":true}]}"#).unwrap();
    assert_eq!(canonical_json(&value), canonical_json(&reordered));
    let array_order: Value =
        serde_json::from_str(r#"{"a":"\n","b":[1,2,{"c":null,"d":true}]}"#).unwrap();
    assert_ne!(canonical_json(&value), canonical_json(&array_order));
}

#[test]
fn schema_identity_ignores_raw_only_differences_and_names_real_drift() {
    let dir = tempfile::tempdir().unwrap();
    let expected = expected(dir.path());
    let raw = dir.path().join("raw");
    write(
        &raw.join("a.json"),
        "{ \"enum\" : [\"x\",\"y\"],\n \"type\":\"object\" }",
    );
    write(&raw.join("v2/b.json"), r#"{"b":[3,2,1],"a":1}"#);
    let identity = schema_identity(&raw).unwrap();
    assert_ne!(
        identity["raw_listing_sha256"],
        expected["raw_listing_sha256"]
    );
    assert_eq!(
        identity["canonical_listing_sha256"],
        expected["canonical_listing_sha256"]
    );
    assert!(schema_drift(&expected, &identity).is_empty());

    let changed = dir.path().join("changed");
    write(
        &changed.join("a.json"),
        r#"{"type":"object","enum":["x","z"]}"#,
    );
    write(&changed.join("v2/b.json"), r#"{"a":1,"b":[3,2,1]}"#);
    write(&changed.join("v2/c.json"), r#"{}"#);
    let drift = schema_drift(&expected, &schema_identity(&changed).unwrap());
    assert_eq!(
        drift,
        vec![
            json!({"file":"a.json","change":"changed"}),
            json!({"file":"v2/c.json","change":"added"})
        ]
    );
    std::fs::remove_file(changed.join("v2/b.json")).unwrap();
    assert!(
        schema_drift(&expected, &schema_identity(&changed).unwrap())
            .contains(&json!({"file":"v2/b.json","change":"removed"}))
    );
}

#[test]
fn qualify_accepts_pinned_version_and_raw_nondeterministic_schema_in_an_isolated_home() {
    let dir = tempfile::tempdir().unwrap();
    let expected = expected(dir.path());
    let work = dir.path().join("work");
    let record = serialized(|| {
        // Same content as the reference with different member order and spacing.
        let codex = fake_codex(
            dir.path(),
            PINNED_VERSION,
            r#"{ "enum":["x","y"], "type":"object" }"#,
            r#"{"b":[3,2,1],"a":1}"#,
        );
        qualify(&codex, &expected, path_var().as_deref(), &work).unwrap()
    });
    assert_eq!(record["qualified"], true, "{record:#}");
    assert_eq!(record["resolution"]["kind"], "native");
    assert_eq!(record["schema"]["file_count"], 2);
    assert_eq!(record["schema"]["drift_count"], 0);
    assert_ne!(
        record["schema"]["raw_listing_sha256"],
        expected["raw_listing_sha256"]
    );
    let homes = std::fs::read_to_string(dir.path().join("homes.log")).unwrap();
    let isolated = work.join("isolated-codex-home");
    assert!(!homes.is_empty());
    for home in homes.lines() {
        assert_eq!(
            Path::new(home),
            isolated,
            "must never use the user's Codex home"
        );
    }
}

#[test]
fn qualify_refuses_unsupported_version_without_running_codex_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let expected = expected(dir.path());
    let work = dir.path().join("work");
    let record = serialized(|| {
        let codex = fake_codex(
            dir.path(),
            "0.147.0",
            r#"{"type":"object","enum":["x","y"]}"#,
            r#"{"a":1,"b":[3,2,1]}"#,
        );
        qualify(&codex, &expected, path_var().as_deref(), &work).unwrap()
    });
    assert_eq!(record["qualified"], false);
    assert_eq!(record["refusals"][0]["reason"], "unsupported_version");
    assert_eq!(record["refusals"][0]["observed"], "0.147.0");
    // D12: the refusal names the pin, the installed version and how to
    // re-qualify, so a reader is never left to go looking for them.
    assert_eq!(record["refusals"][0]["pinned"], PINNED_VERSION);
    let detail = record["refusals"][0]["detail"].as_str().unwrap();
    assert!(detail.contains(PINNED_VERSION), "{detail}");
    assert!(detail.contains("0.147.0"), "{detail}");
    assert!(detail.contains("docs/VERSION-POLICY.md"), "{detail}");
    assert!(detail.contains("pio codex qualify"), "{detail}");
    assert_eq!(record["refusals"].as_array().unwrap().len(), 1);
    assert_eq!(record["schema"], json!({"skipped":"version_not_qualified"}));
    assert!(!work.join("schema").exists());
    // Only the version probe ran.
    let homes = std::fs::read_to_string(dir.path().join("homes.log")).unwrap();
    assert_eq!(homes.lines().count(), 1);
}

#[test]
fn qualify_refuses_meaningful_schema_change_naming_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let expected = expected(dir.path());
    let record = serialized(|| {
        let codex = fake_codex(
            dir.path(),
            PINNED_VERSION,
            r#"{"type":"object","enum":["x","z"]}"#,
            r#"{"a":1,"b":[3,2,1]}"#,
        );
        qualify(
            &codex,
            &expected,
            path_var().as_deref(),
            &dir.path().join("work"),
        )
        .unwrap()
    });
    assert_eq!(record["qualified"], false);
    assert_eq!(
        record["refusals"],
        json!([{"reason":"schema_drift","files":1}])
    );
    assert_eq!(
        record["schema"]["drift"],
        json!([{"file":"a.json","change":"changed"}])
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
            &expected(dir.path()),
            path_var().as_deref(),
            &dir.path().join("work"),
        )
        .unwrap()
    });
    assert_eq!(record["qualified"], false);
    assert_eq!(record["refusals"][0]["reason"], "unresolved_executable");
}

fn npm_layout(root: &Path, hoisted: bool, vendored: bool) -> (PathBuf, PathBuf) {
    let triple = target_triple().unwrap();
    let platform = platform_package(triple);
    let package = root.join("lib/node_modules/@openai/codex");
    write(
        &package.join("bin/codex.js"),
        "#!/usr/bin/env node\n// labeled test wrapper\n",
    );
    write(
        &package.join("package.json"),
        &format!(r#"{{"name":"@openai/codex","version":"{PINNED_VERSION}"}}"#),
    );
    let platform_root = if hoisted {
        root.join("lib/node_modules").join(platform)
    } else {
        package.join("node_modules").join(platform)
    };
    let vendor = if vendored {
        package.join("vendor")
    } else {
        write(
            &platform_root.join("package.json"),
            r#"{"name":"@openai/codex"}"#,
        );
        platform_root.join("vendor")
    };
    executable(&vendor.join(triple).join("bin/codex"), "#!/bin/sh\n");
    write(
        &vendor.join(triple).join("codex-package.json"),
        &format!(
            r#"{{"version":"{PINNED_VERSION}","target":"{triple}","entrypoint":"bin/codex"}}"#
        ),
    );
    std::fs::create_dir_all(root.join("bin")).unwrap();
    let selected = root.join("bin/codex");
    std::os::unix::fs::symlink("../lib/node_modules/@openai/codex/bin/codex.js", &selected)
        .unwrap();
    executable(&root.join("node-bin/node"), "#!/bin/sh\n");
    (
        selected,
        std::fs::canonicalize(vendor.join(triple).join("bin/codex")).unwrap(),
    )
}

#[test]
fn resolve_mirrors_the_pinned_npm_wrapper_layouts() {
    for (hoisted, vendored) in [(false, false), (true, false), (false, true)] {
        let dir = tempfile::tempdir().unwrap();
        let node_path = OsString::from(dir.path().join("node-bin"));
        let (native, resolution) = serialized(|| {
            let (selected, native) = npm_layout(dir.path(), hoisted, vendored);
            (native, resolve(&selected, Some(&node_path)).unwrap())
        });
        assert_eq!(resolution["kind"], "npm_node_wrapper");
        assert_eq!(
            Path::new(resolution["native"]["path"].as_str().unwrap()),
            native
        );
        assert_eq!(resolution["wrapper"]["package_version"], PINNED_VERSION);
        assert_eq!(resolution["native"]["layout"]["version"], PINNED_VERSION);
        assert_eq!(resolution["target"], target_triple().unwrap());
        assert!(resolution["node"]["sha256"].is_string());
    }
}

#[test]
fn checked_in_identity_is_the_qualified_schema_listing() {
    let identity: Value = serde_json::from_str(QUALIFIED_SCHEMA_IDENTITY).unwrap();
    assert_eq!(identity["format"], "pio-codex-schema-identity/1");
    // 0.157.0: 314 files (0.155.1 had 312; four added, two removed).
    assert_eq!(identity["file_count"], 314);
    assert_eq!(identity["files"].as_object().unwrap().len(), 314);
    let mut listing = String::new();
    for (file, digest) in identity["files"].as_object().unwrap() {
        listing.push_str(&format!(
            "{file}\t{}\n",
            digest["canonical_sha256"].as_str().unwrap()
        ));
    }
    assert_eq!(
        identity["canonical_listing_sha256"],
        sha256_hex(listing.as_bytes())
    );
}

/// The three pin constants and the compiled-in identity name one release:
/// the identity is the one checked in under `adapters/codex/<PINNED_VERSION>`,
/// and the committed qualification record for that version names the same
/// tag and source. A stale `PINNED_VERSION` fails closed at run time, but a
/// stale tag or source was only copied into every record's `pinned` and
/// would misstate provenance silently (review of L3, REPIN-5).
#[test]
fn pin_constants_name_the_identity_and_the_qualification_record() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let identity = std::fs::read_to_string(workspace.join(format!(
        "adapters/codex/{PINNED_VERSION}/schema-identity.json"
    )))
    .expect("an identity directory for PINNED_VERSION");
    assert_eq!(
        identity, QUALIFIED_SCHEMA_IDENTITY,
        "the compiled-in identity is not the one checked in for {PINNED_VERSION}"
    );
    let record: Value = serde_json::from_str(
        &std::fs::read_to_string(workspace.join(format!(
            "docs/work/m2/codex-qualification/qualification-{PINNED_VERSION}.json"
        )))
        .expect("a committed qualification record for PINNED_VERSION"),
    )
    .unwrap();
    assert_eq!(
        record["record"]["pinned"],
        json!({"version":PINNED_VERSION,"tag":PINNED_TAG,"source":PINNED_SOURCE})
    );
    assert_eq!(record["record"]["qualified"], true);
    assert_eq!(record["record"]["version"]["native"], PINNED_VERSION);
    assert_eq!(PINNED_TAG, format!("rust-v{PINNED_VERSION}"));
}

#[test]
fn config_diff_discloses_added_trust_entries_and_other_changes() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let fixture_root = dir.path().join("fixtures");
    let missing = config_snapshot(&home).unwrap();
    assert_eq!(missing["exists"], false);
    write(
        &home.join("config.toml"),
        &format!(
            "[projects.\"{}\"]\ntrust_level = \"trusted\"\n",
            fixture_root.join("created").display()
        ),
    );
    let created = config_diff(
        &missing,
        &config_snapshot(&home).unwrap(),
        Some(&fixture_root),
    );
    assert_eq!(created["other_changes"], false, "{created:#}");
    assert_eq!(created["projects_added"][0]["location"], "fixture");
    write(
        &home.join("config.toml"),
        "model = \"example\"\n\n[projects.\"/Users/someone/existing\"]\ntrust_level = \"trusted\"\n",
    );
    let before = config_snapshot(&home).unwrap();
    assert_eq!(
        before["projects"],
        json!({"/Users/someone/existing":"trusted"})
    );
    let fixture = fixture_root.join("run-1");
    let appended = format!(
        "model = \"example\"\n\n[projects.\"/Users/someone/existing\"]\ntrust_level = \"trusted\"\n\n[projects.\"{}\"]\ntrust_level = \"trusted\"\n",
        fixture.display()
    );
    write(&home.join("config.toml"), &appended);
    let after = config_snapshot(&home).unwrap();
    let diff = config_diff(&before, &after, Some(&fixture_root));
    assert_eq!(diff["unchanged"], false);
    assert_eq!(diff["other_changes"], false);
    assert_eq!(diff["projects_removed"], json!([]));
    let added = diff["projects_added"].as_array().unwrap();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0]["location"], "fixture");
    assert_eq!(added[0]["trust_level"], "trusted");
    assert_eq!(
        added[0]["path_sha256"],
        sha256_hex(fixture.to_str().unwrap().as_bytes())
    );
    assert!(
        !diff.to_string().contains("someone"),
        "no raw paths in the diff"
    );

    let edited = appended.replace("model = \"example\"", "model = \"other\"");
    write(&home.join("config.toml"), &edited);
    let later = config_snapshot(&home).unwrap();
    let diff = config_diff(&after, &later, Some(&fixture_root));
    assert_eq!(diff["other_changes"], true);
    assert_eq!(diff["projects_added"], json!([]));
    assert_eq!(config_diff(&later, &later, None)["unchanged"], true);
}

fn guard_for(config: Option<&str>, requested: Value) -> Value {
    let dir = tempfile::tempdir().unwrap();
    if let Some(config) = config {
        write(&dir.path().join("config.toml"), config);
    }
    thread_settings_guard(&config_snapshot(dir.path()).unwrap(), &requested)
}

#[test]
fn thread_settings_guard_refuses_broader_than_configured_defaults() {
    let plan = json!({"sandbox":"workspace-write","approvalPolicy":"on-request"});
    // Absent settings are the trusted-project defaults, so the plan is equal.
    let absent = guard_for(None, plan.clone());
    assert_eq!(absent["allowed"], true, "{absent:#}");
    assert_eq!(
        absent["configured"]["sandbox_mode"],
        json!({"value":"workspace-write","source":"absent_trusted_project_default"})
    );
    assert_eq!(
        guard_for(Some("model = \"x\"\n"), plan.clone())["allowed"],
        true
    );
    // Narrower requests are always allowed.
    let untrusted = json!({"sandbox":"read-only","approvalPolicy":"untrusted"});
    assert_eq!(guard_for(None, untrusted.clone())["allowed"], true);
    // Broader than an explicit stricter default is refused, naming the setting.
    let strict = guard_for(
        Some("sandbox_mode = \"read-only\"\n"),
        json!({"sandbox":"workspace-write","approvalPolicy":"never"}),
    );
    assert_eq!(strict["allowed"], false);
    assert_eq!(
        strict["broader_than_configured"],
        json!([
            {"setting":"sandbox_mode","requested":"workspace-write","configured":"read-only"},
            {"setting":"approval_policy","requested":"never","configured":"on-request"}
        ])
    );
    // A configured `untrusted` approval policy is not a stricter default at
    // 0.155.1 and 0.157.0: the app-server exits before `initialize`, so it is unresolved
    // rather than compared, while requesting `untrusted` per thread is fine.
    let configured_untrusted = guard_for(Some("approval_policy = \"untrusted\"\n"), plan.clone());
    assert_eq!(configured_untrusted["allowed"], false);
    assert_eq!(configured_untrusted["broader_than_configured"], json!([]));
    assert_eq!(
        configured_untrusted["unresolved"],
        json!([{"setting":"approval_policy","reason":format!("Codex {PINNED_VERSION} does not start with this configured value"),"value":"untrusted"}])
    );
    assert_eq!(
        guard_for(Some("sandbox_mode = \"read-only\"\n"), untrusted)["allowed"],
        true
    );
    // Keys inside tables are not top-level defaults.
    assert_eq!(
        guard_for(
            Some("[projects.\"/x\"]\nsandbox_mode = \"read-only\"\n"),
            plan.clone()
        )["allowed"],
        true
    );
    // Anything the guard cannot resolve refuses.
    for config in [
        "profile = \"work\"\n",
        "default_permissions = \"strict\"\n",
        "[permissions.strict]\n",
        "approval_policy = { granular = { rules = true } }\n",
        "approval_policy = \"on-failure\"\n",
    ] {
        let guard = guard_for(Some(config), plan.clone());
        assert_eq!(guard["allowed"], false, "{config}");
        assert!(
            !guard["unresolved"].as_array().unwrap().is_empty(),
            "{config}"
        );
    }
}

/// The fake's reading of how Codex 0.157.0 exposes an MCP server's tools
/// (`core/src/tools/spec_plan.rs:234-266`, `:761-772`; not measured): on a
/// code-mode-only model, only `DirectModelOnly` is in the model's own list.
/// L3's first live run sent nothing (deferred, never named); its lead sends
/// `code_mode` and `deferred` omitted. `code_mode` alone would do on this
/// model, and `deferred` alone keeps the tools inside `exec`.
#[test]
fn a_server_s_tools_reach_a_code_mode_only_model_only_when_kept_out_of_code_mode() {
    use crate::fake_turn::{exposure, in_model_list};
    let omit = |surfaces: &[&str]| surfaces.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    for (surfaces, code_mode_only, expected, listed) in [
        (&[][..], true, "deferred", false),
        (
            &["code_mode", "deferred"][..],
            true,
            "direct_model_only",
            true,
        ),
        (&["code_mode"][..], true, "direct_model_only", true),
        (&["deferred"][..], true, "direct", false),
        (&["direct"][..], true, "deferred", false),
        (&[][..], false, "deferred", true),
        (
            &["code_mode", "deferred"][..],
            false,
            "direct_model_only",
            true,
        ),
    ] {
        let found = exposure(&omit(surfaces), code_mode_only);
        assert_eq!(
            found, expected,
            "{surfaces:?}, code-mode-only {code_mode_only}"
        );
        assert_eq!(in_model_list(found, code_mode_only), listed, "{surfaces:?}");
    }
}
