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
    let record = service_admission(
        dir.path(),
        &service(json!("juspay-grid/glm-latest"), json!({})),
    )
    .unwrap();
    assert_eq!(record["admitted"], false);
    assert!(reasons(&record).contains(&"provider_excluded_by_the_owner".to_owned()));
}

#[test]
fn a_model_requires_the_dated_exception_and_a_run_requires_a_model() {
    let dir = tempfile::tempdir().unwrap();
    let missing = service_admission(dir.path(), &service(Value::Null, json!({}))).unwrap();
    assert!(reasons(&missing).contains(&"model_required".to_owned()));

    let undated = service_admission(
        dir.path(),
        &service(
            json!("minimax-coding-plan/MiniMax-M2.7-highspeed"),
            json!({"test_only_model_exception":"something-else"}),
        ),
    )
    .unwrap();
    assert!(
        reasons(&undated).contains(&"model_requires_the_dated_test_only_exception".to_owned()),
        "{undated}"
    );
}

#[test]
fn a_credential_variable_never_reaches_a_child() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["ANTHROPIC_API_KEY", "MINIMAX_TOKEN", "OPENCODE_PASSWORD"] {
        let record = service_admission(
            dir.path(),
            &service(
                json!("minimax-coding-plan/MiniMax-M2.7-highspeed"),
                json!({"env":{"PATH":"/usr/bin:/bin", name:"x"}}),
            ),
        )
        .unwrap();
        assert!(
            reasons(&record).contains(&"env_carries_a_credential_variable".to_owned()),
            "{name} was admitted"
        );
    }
}

#[test]
fn an_admitted_configuration_starts_no_session_and_records_the_owner_service() {
    let dir = tempfile::tempdir().unwrap();
    let record = service_admission(
        dir.path(),
        &service(
            json!("minimax-coding-plan/MiniMax-M2.7-highspeed"),
            json!({}),
        ),
    )
    .unwrap();
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
