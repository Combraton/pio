use super::*;
use std::os::unix::fs::PermissionsExt;

/// Writing a test executable and running one race inside a single test binary;
/// `Command::spawn` returns only once the child has exec'd, so serializing
/// every "write a fake, then run one" region closes the window. Learned in M2.
static FAKE_EXECUTABLES: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serialized<T>(body: impl FnOnce() -> T) -> T {
    let _guard = FAKE_EXECUTABLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    body()
}

/// Every admission runs `ps` to record the owner's service, which is a fork.
/// A fork landing between writing a fake executable and running it leaves the
/// child holding an inherited write descriptor, and the exec then fails with
/// `ETXTBSY`. So an admission takes the same lock as a write-then-run region.
/// Measured: this failed `a_version_that_is_not_the_pin_is_refused_without_
/// running_further_arguments` on a Linux runner, never on macOS.
///
/// Not for use inside a `serialized` block: the lock is not reentrant.
fn admitted(work: &Path, config: &Value) -> Value {
    serialized(|| service_admission(work, config).unwrap())
}

fn fake_opencode(dir: &Path, version: &str, top_help: &str) -> PathBuf {
    let path = dir.join("fake-opencode");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\n\
             case \"$1\" in\n\
             --version) echo 'opencode v{version}' ;;\n\
             --help) echo '{top_help}' ;;\n\
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

fn env_for(dir: &Path) -> ChildEnv {
    ChildEnv::isolated(dir).with_path(path_var().as_deref())
}

#[test]
fn a_version_that_is_not_the_pin_is_refused_without_running_further_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let record = serialized(|| {
        let exe = fake_opencode(dir.path(), "9.9.9", "top help");
        qualify(
            &exe,
            &json!({"commands":{}}),
            &env_for(&dir.path().join("env")),
            &dir.path().join("work"),
        )
        .unwrap()
    });
    assert_eq!(record["qualified"], false);
    assert_eq!(record["refusals"][0]["reason"], "unsupported_version");
    assert_eq!(record["refusals"][0]["observed"], "9.9.9");
    // D12: the refusal names the pin, the installed version and how to
    // re-qualify, so a reader is never left to go looking for them.
    assert_eq!(record["refusals"][0]["pinned"], PINNED_VERSION);
    let detail = record["refusals"][0]["detail"].as_str().unwrap();
    assert!(detail.contains(PINNED_VERSION), "{detail}");
    assert!(detail.contains("9.9.9"), "{detail}");
    assert!(detail.contains("docs/VERSION-POLICY.md"), "{detail}");
    assert!(detail.contains("pio opencode qualify"), "{detail}");
    assert_eq!(record["refusals"].as_array().unwrap().len(), 1);
    assert_eq!(record["surface"]["skipped"], "version_not_qualified");
}

#[test]
fn surface_drift_names_every_command_whose_help_moved() {
    let dir = tempfile::tempdir().unwrap();
    let (expected, drifted) = serialized(|| {
        let exe = fake_opencode(dir.path(), PINNED_VERSION, "top help v1");
        let expected = surface_identity(&exe, &env_for(&dir.path().join("a"))).unwrap();
        let exe = fake_opencode(dir.path(), PINNED_VERSION, "top help v2");
        let drifted = surface_identity(&exe, &env_for(&dir.path().join("b"))).unwrap();
        (expected, drifted)
    });
    assert!(surface_drift(&expected, &expected).is_empty());
    let drift = surface_drift(&expected, &drifted);
    assert_eq!(drift.len(), 1, "{drift:?}");
    assert_eq!(drift[0]["command"], "<top>");
    assert_eq!(drift[0]["change"], "changed");
}

fn session(model: &str) -> Value {
    json!({"sessionId":"ses_1","configOptions":[
        {"id":"model","currentValue":model,"options":[]},
        {"id":"mode","currentValue":"build","options":[]}]})
}

/// The owner's rule: refuse unless the session's own reported provider and
/// model equal the requested ones, before any prompt.
#[test]
fn a_session_reporting_the_requested_model_is_allowed() {
    let guard = session_configuration_guard(
        &session("minimax-coding-plan/MiniMax-M2.7-highspeed"),
        "minimax-coding-plan/MiniMax-M2.7-highspeed",
    );
    assert_eq!(guard["allowed"], true);
    assert_eq!(guard["unresolved"], json!([]));
    // ACP reports this before a prompt, unlike Claude Code's system/init.
    assert_eq!(guard["checked_before_delivery"], true);
}

/// The measured hazard this exists for: a missing route does not refuse, it
/// silently substitutes a free built-in model.
#[test]
fn a_silent_downgrade_to_another_provider_is_refused() {
    let guard = session_configuration_guard(
        &session("opencode/nemotron-3.5-lightning-free"),
        "minimax-coding-plan/MiniMax-M2.7-highspeed",
    );
    assert_eq!(guard["allowed"], false);
    let reasons: Vec<&str> = guard["unresolved"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["reason"].as_str().unwrap())
        .collect();
    assert!(
        reasons.contains(&"the session's model is not the requested one"),
        "{reasons:?}"
    );
    assert!(
        reasons.contains(&"the session's provider is not the requested one"),
        "{reasons:?}"
    );
    assert_eq!(guard["reported_provider"], "opencode");
}

#[test]
fn a_different_model_from_the_right_provider_is_still_refused() {
    let guard = session_configuration_guard(
        &session("minimax-coding-plan/MiniMax-M3"),
        "minimax-coding-plan/MiniMax-M2.7-highspeed",
    );
    assert_eq!(guard["allowed"], false);
    assert_eq!(guard["reported_provider"], "minimax-coding-plan");
}

#[test]
fn a_session_that_reports_nothing_is_refused_rather_than_assumed() {
    for session in [json!({}), json!({"configOptions":[]}), session("")] {
        let guard =
            session_configuration_guard(&session, "minimax-coding-plan/MiniMax-M2.7-highspeed");
        assert_eq!(guard["allowed"], false, "{session}");
    }
}

fn service(model: Value, extra: Value) -> Value {
    let mut config = json!({
        "executable":"/tmp/opencode","env":{"PATH":"/usr/bin:/bin"},
        "config_dir":"/tmp/config","home":"/tmp/home","fixture_root":"/tmp/fixtures",
        "labeled_fake":true,
        "test_only_model_exception":MODEL_EXCEPTION});
    config["model"] = model;
    for (k, v) in extra.as_object().unwrap() {
        config[k] = v.clone();
    }
    config
}

fn reasons(record: &Value) -> Vec<String> {
    record["refusals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["reason"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn the_owner_excluded_provider_is_refused_outright() {
    let dir = tempfile::tempdir().unwrap();
    let record = admitted(
        dir.path(),
        &service(json!("juspay-grid/glm-latest"), json!({})),
    );
    assert_eq!(record["admitted"], false);
    assert!(reasons(&record).contains(&"provider_excluded_by_the_owner".to_owned()));
}

/// D18: the helper-provider refusal used to exist only in
/// `scripts/lead_run.py`'s L3 rehearsal; every `serve-opencode` admission
/// must refuse it now, not only a run driven through that script.
#[test]
fn a_helper_model_on_another_provider_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("opencode.json"),
        json!({"small_model":"another-provider/helper-model"}).to_string(),
    )
    .unwrap();
    let record = admitted(
        dir.path(),
        &service(
            json!("minimax-coding-plan/MiniMax-M2.7-highspeed"),
            json!({"config_dir":dir.path().display().to_string()}),
        ),
    );
    assert_eq!(record["admitted"], false, "{record}");
    assert!(
        reasons(&record).contains(&"helper_elsewhere".to_owned()),
        "{record}"
    );
}

#[test]
fn a_helper_model_on_the_session_s_own_provider_is_admitted() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("opencode.json"),
        json!({"small_model":"minimax-coding-plan/MiniMax-M2.7-highspeed",
               "agent":{"title":{"model":"minimax-coding-plan/MiniMax-M2.7-highspeed"}}})
        .to_string(),
    )
    .unwrap();
    let record = admitted(
        dir.path(),
        &service(
            json!("minimax-coding-plan/MiniMax-M2.7-highspeed"),
            json!({"config_dir":dir.path().display().to_string()}),
        ),
    );
    assert_eq!(record["admitted"], true, "{record}");
}

#[test]
fn a_model_requires_the_dated_exception_and_a_run_requires_a_model() {
    let dir = tempfile::tempdir().unwrap();
    let missing = admitted(dir.path(), &service(Value::Null, json!({})));
    assert!(reasons(&missing).contains(&"model_required".to_owned()));

    let undated = admitted(
        dir.path(),
        &service(
            json!("minimax-coding-plan/MiniMax-M2.7-highspeed"),
            json!({"test_only_model_exception":"something-else"}),
        ),
    );
    assert!(
        reasons(&undated).contains(&"model_requires_the_dated_test_only_exception".to_owned()),
        "{undated}"
    );
}

/// D10: the owner's dated model exception is compiled out of release builds.
/// Built normally (dev, test, CI and the live runners: `test-exceptions` on,
/// the default) this configuration is admitted, exactly as
/// `a_helper_model_on_the_session_s_own_provider_is_admitted` proves without
/// the helper. Built `cargo build --no-default-features` — the release
/// configuration — OpenCode admission refuses it outright and says so with
/// one reason: OpenCode has no other path to a model at all (the owner's
/// default is a forbidden gateway), so it is test scope only in a release
/// build.
#[test]
fn a_release_build_refuses_the_model_exception_outright() {
    let dir = tempfile::tempdir().unwrap();
    let record = admitted(
        dir.path(),
        &service(
            json!("minimax-coding-plan/MiniMax-M2.7-highspeed"),
            json!({}),
        ),
    );
    if cfg!(feature = "test-exceptions") {
        assert_eq!(record["admitted"], true, "{record}");
    } else {
        assert_eq!(record["admitted"], false, "{record}");
        assert!(
            reasons(&record).contains(&"test_only_model_exception_not_compiled_in".to_owned()),
            "{record}"
        );
    }
}

#[test]
fn a_credential_variable_never_reaches_a_child() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["ANTHROPIC_API_KEY", "MINIMAX_TOKEN", "OPENCODE_PASSWORD"] {
        let record = admitted(
            dir.path(),
            &service(
                json!("minimax-coding-plan/MiniMax-M2.7-highspeed"),
                json!({"env":{"PATH":"/usr/bin:/bin", name:"x"}}),
            ),
        );
        assert!(
            reasons(&record).contains(&"env_carries_a_credential_variable".to_owned()),
            "{name} was admitted"
        );
    }
}

#[test]
fn an_admitted_configuration_starts_no_session_and_records_the_owner_service() {
    let dir = tempfile::tempdir().unwrap();
    let record = admitted(
        dir.path(),
        &service(
            json!("minimax-coding-plan/MiniMax-M2.7-highspeed"),
            json!({}),
        ),
    );
    assert_eq!(record["admitted"], true, "{record}");
    assert_eq!(record["session_started"], false);
    // The owner's own service is recorded so a run can prove it did not move.
    assert!(record["owner_service_before"].is_array());
}

#[test]
fn the_flags_pio_never_passes_are_named() {
    assert_eq!(FORBIDDEN_FLAGS, ["--auto", "--server"]);
    assert_eq!(
        ENV_ALLOWLIST,
        ["PATH", "HOME", "USER", "OPENCODE_CONFIG_DIR"]
    );
}

/// A verdict that nothing acts on is not a check. The first version of
/// `service_admission` computed qualification and admitted the configuration
/// anyway; the offline matrix caught it on an unqualified executable.
#[test]
fn an_unqualified_executable_is_refused_and_not_merely_reported() {
    let dir = tempfile::tempdir().unwrap();
    let record = serialized(|| {
        let exe = fake_opencode(
            dir.path(),
            PINNED_VERSION,
            "a help that is not the pinned one",
        );
        let mut config = service(
            json!("minimax-coding-plan/MiniMax-M2.7-highspeed"),
            json!({}),
        );
        config["labeled_fake"] = json!(false);
        config["executable"] = json!(exe.display().to_string());
        config["env"] = json!({"PATH":"/usr/bin:/bin"});
        service_admission(&dir.path().join("work"), &config).unwrap()
    });
    assert_eq!(record["qualification"]["qualified"], false, "{record}");
    assert!(
        reasons(&record).contains(&"opencode_not_qualified".to_owned()),
        "an unqualified executable was admitted: {record}"
    );
    assert_eq!(record["admitted"], false);
    assert_eq!(record["session_started"], false);
}

/// The lead tool's server spec is journaled with the run, so what it may
/// carry is narrow: the measured stdio shape, an absolute command, and no
/// credential by name or by value.
#[test]
fn a_lead_tool_spec_is_refused_for_what_it_must_not_carry() {
    let good = json!({"name":"pio-lead","command":"/usr/bin/python3",
        "args":["/opt/pio/lead_tool.py","--credential-file","/private/lead.credential"],
        "env":[{"name":"PIO_LEAD_GRANT","value":"a-grant-id"}]});
    assert!(
        lead_tool_refusals(&good).is_empty(),
        "{:?}",
        lead_tool_refusals(&good)
    );
    let reasons = |tool: Value| -> Vec<String> {
        lead_tool_refusals(&tool)
            .iter()
            .map(|r| r["reason"].as_str().unwrap().to_owned())
            .collect()
    };
    let mut relative = good.clone();
    relative["command"] = json!("python3");
    assert_eq!(
        reasons(relative),
        ["lead_tool_command_must_be_an_absolute_path"]
    );
    let mut named = good.clone();
    named["env"] = json!([{"name":"PIO_LEAD_TOKEN","value":"x"}]);
    assert_eq!(reasons(named), ["env_carries_a_credential_variable"]);
    let mut valued = good.clone();
    valued["env"] = json!([{"name":"PIO_LEAD_NOTE","value":"ccred1.lead.abc"}]);
    assert_eq!(reasons(valued), ["lead_tool_carries_a_credential_value"]);
    let mut in_args = good.clone();
    in_args["args"] = json!(["/opt/pio/lead_tool.py", "ccred1.lead.abc"]);
    assert_eq!(reasons(in_args), ["lead_tool_carries_a_credential_value"]);
    let mut extra = good.clone();
    extra["cwd"] = json!("/tmp");
    assert_eq!(reasons(extra), ["lead_tool_unsupported_field"]);
    let mut loose = good;
    loose["env"] = json!({"PIO_LEAD_GRANT":"a-grant-id"});
    assert_eq!(reasons(loose), ["lead_tool_env_must_be_name_value_pairs"]);
}
