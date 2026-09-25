//! Durable host for the Codex app-server (ADR 003). It keeps the M1 launch
//! fences, owns the app-server's stdio, and records native observations in an
//! fsync'd append-only events file. Controls from the service arrive in a
//! controls file and are applied at most once.
use crate::harness::{self, Lifecycle};
use crate::{append_json, identity};
use anyhow::{Context, Result, bail, ensure};
use pio_codex::rpc::AppServer;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

pub const APPROVAL_METHODS: &[&str] = &[
    "item/commandExecution/requestApproval",
    "item/fileChange/requestApproval",
];
/// `item/permissions/requestApproval` asks for a granted permission profile,
/// not `accept`, `decline` or `cancel`: its response carries `permissions` and a
/// `scope` that defaults to the whole turn. Answering it would widen the
/// settings the thread-settings guard approved, and PIO cannot express a grant
/// through a Protocol action response, so it is refused natively with a reason.
pub const REFUSED_PERMISSION_GRANT: &str = "item/permissions/requestApproval";
/// Decisions PIO may forward. Session-wide or policy-amending approvals widen
/// standing permissions and are never sent.
pub const ALLOWED_DECISIONS: &[&str] = &["accept", "decline", "cancel"];

/// The adapter label. It prefixes this host's event and control files, so the
/// shared lifecycle produces exactly the paths the service already reads.
pub const ADAPTER: &str = "codex";

pub fn events_path(root: &Path, invocation: &str) -> PathBuf {
    harness::events_path(root, ADAPTER, invocation)
}
pub fn controls_path(root: &Path, invocation: &str) -> PathBuf {
    harness::controls_path(root, ADAPTER, invocation)
}
pub use harness::read_jsonl;

pub fn source(spec: &Value) -> &'static str {
    if spec["labeled_fake"] == true {
        pio_codex::fake::SOURCE
    } else {
        "codex-app-server"
    }
}

/// Append a control for the host. Ids make repeated appends harmless.
pub fn append_control(root: &Path, invocation: &str, control: &Value) -> Result<()> {
    harness::append_control(root, ADAPTER, invocation, control)
}

fn child_pids(pid: u32) -> Vec<u32> {
    let Ok(output) = Command::new("ps").args(["-axo", "pid=,ppid="]).output() else {
        return vec![];
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let child: u32 = fields.next()?.parse().ok()?;
            let parent: u32 = fields.next()?.parse().ok()?;
            (parent == pid).then_some(child)
        })
        .collect()
}

fn executable_path(pid: u32) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_link(format!("/proc/{pid}/exe")).ok()
    }
    #[cfg(target_os = "macos")]
    {
        let mut buffer = vec![0u8; 4096];
        let n = unsafe {
            libc::proc_pidpath(pid as i32, buffer.as_mut_ptr().cast(), buffer.len() as u32)
        };
        (n > 0).then(|| PathBuf::from(String::from_utf8_lossy(&buffer[..n as usize]).into_owned()))
    }
}

/// Bind the running native process to the qualification record: the spawned
/// process itself for a native executable, or a child of the npm wrapper.
fn verify_native(spec: &Value, spawned: u32) -> Result<Value> {
    let qualification = &spec["qualification"];
    let expected_path = qualification["native_path"]
        .as_str()
        .context("qualification native path")?;
    let expected = std::fs::canonicalize(expected_path)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut candidates = vec![spawned];
        candidates.extend(child_pids(spawned));
        for pid in candidates {
            if let Some(path) = executable_path(pid)
                && std::fs::canonicalize(&path).ok().as_deref() == Some(expected.as_path())
            {
                let sha = pio_codex::sha256_file(&expected)?;
                ensure!(
                    Some(sha.as_str()) == qualification["native_sha256"].as_str(),
                    "native_binary_changed"
                );
                return Ok(
                    json!({"identity":identity(pid)?,"path_matches_qualification":true,"sha256":sha}),
                );
            }
        }
        ensure!(
            Instant::now() < deadline,
            "native_process_not_observed: no process runs the qualified binary"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// For the npm wrapper, the launched process becomes the Node interpreter
/// that `#!/usr/bin/env node` found on the host PATH. It must be the Node the
/// service qualified.
fn verify_interpreter(spec: &Value, spawned: u32) -> Result<Value> {
    let Some(expected) = spec["qualification"]["node_path"].as_str() else {
        return Ok(json!({"applicable":false}));
    };
    let expected = std::fs::canonicalize(expected)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let observed = executable_path(spawned).and_then(|p| std::fs::canonicalize(p).ok());
        if observed.as_deref() == Some(expected.as_path()) {
            return Ok(json!({"applicable":true,"path_matches_qualification":true}));
        }
        ensure!(
            Instant::now() < deadline,
            "interpreter_mismatch: launched process runs {observed:?}, not the qualified Node"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn recheck_executable(spec: &Value) -> Result<()> {
    let qualification = &spec["qualification"];
    let path = spec["env"]["PATH"].as_str().map(std::ffi::OsStr::new);
    let resolution = pio_codex::resolve(
        Path::new(spec["executable"].as_str().context("executable")?),
        path,
    )?;
    ensure!(
        resolution["native"]["sha256"] == qualification["native_sha256"]
            && resolution["native"]["path"] == qualification["native_path"]
            && resolution["wrapper"]["sha256"] == qualification["wrapper_sha256"]
            && resolution["node"]["sha256"] == qualification["node_sha256"],
        "executable_changed_since_qualification"
    );
    Ok(())
}

fn response_error(message: &Value) -> Option<Value> {
    message.get("error").cloned()
}

pub fn codex_host(root: &Path, command: &str, invocation_id: &str) -> Result<()> {
    let mut life = Lifecycle::claim(root, command, invocation_id, ADAPTER, |spec| {
        source(spec).to_owned()
    })?;
    let mut server: Option<AppServer> = None;
    let outcome = run_turn(&mut life, &mut server);
    if let Err(error) = &outcome {
        life.fail(error, || {
            if let Some(app) = server.as_mut() {
                let _ = app.child.kill();
                let _ = app.child.wait();
            }
        });
    }
    outcome
}

/// Everything specific to the Codex app-server: its handshake, its message
/// shapes and its controls. The lifecycle around this is shared.
fn run_turn(life: &mut Lifecycle, server: &mut Option<AppServer>) -> Result<()> {
    if life.spec["qualification"].is_object() {
        recheck_executable(&life.spec)?;
    }
    let codex_home = PathBuf::from(life.spec["codex_home"].as_str().context("codex_home")?);
    let before = pio_codex::config_snapshot(&codex_home)?;
    life.event(json!({"kind":"config_before","snapshot":before}))?;
    // Owner guard: never request thread settings broader than the user's
    // configured default. Refused before the app-server is started.
    let guard = pio_codex::thread_settings_guard(&before, &life.spec["thread"]);
    life.event(json!({"kind":"thread_settings_guard","guard":guard}))?;
    ensure!(
        guard["allowed"] == true,
        "thread_settings_refused: requested {} is broader than or not comparable with the configured default {}",
        guard["requested"],
        json!({"configured":guard["configured"],"broader":guard["broader_than_configured"],"unresolved":guard["unresolved"]})
    );
    let env: Vec<(String, String)> = life.spec["env"]
        .as_object()
        .context("env")?
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
        .collect();
    let stderr = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(life.root.join(format!("codex-{}.stderr", life.invocation)))?;
    let app = server.insert(AppServer::spawn(
        Path::new(life.spec["executable"].as_str().context("executable")?),
        &env,
        stderr,
    )?);
    let spawned = app.child.id();
    let child_identity = identity(spawned)?;
    let native = if life.spec["qualification"].is_object() {
        let mut native = verify_native(&life.spec, spawned)?;
        native["interpreter"] = verify_interpreter(&life.spec, spawned)?;
        native
    } else {
        json!({"verified":false,"reason":"labeled fake app-server has no qualification record"})
    };
    life.spawned(&serde_json::to_value(&child_identity)?, &native)?;
    let init = app.request(
            "initialize",
            json!({"clientInfo":{"name":"pio","title":"PIO standalone execution host","version":env!("CARGO_PKG_VERSION")}}),
        )?;
    let init = app.wait_response(init, Duration::from_secs(60), |_| Ok(()))?;
    if let Some(error) = response_error(&init) {
        bail!("initialize refused: {error}");
    }
    app.notify("initialized")?;
    let account = app.request("account/read", json!({"refreshToken":false}))?;
    let account = app.wait_response(account, Duration::from_secs(60), |_| Ok(()))?;
    // Only the authentication type; never email, plan or tokens.
    life.event(json!({"kind":"account","authentication_type":account["result"]["account"]["type"],"requires_openai_auth":account["result"]["requiresOpenaiAuth"],"error":response_error(&account)}),
        )?;
    let mut params = life.spec["thread"].clone();
    if !params.is_object() {
        params = json!({});
    }
    params["cwd"] = life.spec["cwd"].clone();
    // Owner decision, 2026-09-22: **`approvalsReviewer` is never set.** The
    // app-server's own schema describes it as "override where approval
    // requests are routed for review on this thread and subsequent turns",
    // with `user | auto_review | guardian_subagent`. Two of those three send
    // approvals somewhere other than the person, which is the one thing a
    // lead must never arrange. The whole `thread` object is operator
    // configuration that reaches `thread/start` unchanged, so the refusal is
    // here, at the wire, rather than a promise made elsewhere.
    ensure!(
        params.get("approvalsReviewer").is_none(),
        "approvals_reviewer_never_set: PIO does not route approvals away from the user"
    );
    let thread = app.request("thread/start", params)?;
    let thread = app.wait_response(thread, Duration::from_secs(120), |_| Ok(()))?;
    if let Some(error) = response_error(&thread) {
        bail!("thread_start_refused: {error}");
    }
    let result = &thread["result"];
    let thread_id = result["thread"]["id"]
        .as_str()
        .context("thread id")?
        .to_owned();
    let sandbox = &result["sandbox"];
    life.event(json!({"kind":"thread_started","thread_id":thread_id,"configured_model":before["settings"]["keys"]["model"],"requested_model":life.spec["thread"]["model"],"model":result["model"],"model_provider":result["modelProvider"],"sandbox":sandbox,"approval_policy":result["approvalPolicy"],"approvals_reviewer":result["approvalsReviewer"],"instruction_sources":result["instructionSources"].as_array().map(|a|a.len())}),
        )?;
    // And the harness's own answer, on every run: approvals come to the
    // person. Recorded in the event above and asserted here, because a
    // field that is never read is not a check. **Absent is not `user`.**
    // `ThreadStartResponse` lists `approvalsReviewer` as required at 0.155.1
    // and at 0.157.0,
    // so a response without it did not come from the qualified app-server,
    // and a run whose routing nobody stated is not a run that asserted it.
    ensure!(
        result["approvalsReviewer"].as_str() == Some("user"),
        "approvals_reviewer_not_user: {}",
        result["approvalsReviewer"]
    );
    ensure!(
        !matches!(
            sandbox["type"].as_str(),
            Some("dangerFullAccess" | "externalSandbox")
        ) && sandbox["networkAccess"] != true,
        "restricted_sandbox_required: effective sandbox {sandbox}"
    );
    life.park(child_identity)?;
    let brief = pio_core::spool::Spool::open(&life.root)?.read(
        life.spec["brief"]["digest"]
            .as_str()
            .context("brief digest")?,
    )?;
    let text = String::from_utf8(brief).context("brief is not UTF-8 text")?;
    // The release and the first native write happen together under the
    // controller gate, so a controller never sees a released invocation
    // whose brief has not been sent.
    let gate = life.release()?;
    let turn_request = app.request(
        "turn/start",
        json!({"threadId":thread_id,"clientUserMessageId":life.invocation,"input":[{"type":"text","text":text}]}),
    )?;
    life.event(json!({"kind":"turn_start_sent","request_id":turn_request,"input_sha256":pio_codex::sha256_hex(text.as_bytes())}))?;
    drop(gate);

    let spool = pio_core::spool::Spool::open(&life.root)?;
    let refs = life
        .root
        .join(format!("output-{}.refs.jsonl", life.invocation));
    let mut output_offset = 0u64;
    let mut all_output = Vec::new();
    let mut turn_id: Option<String> = None;
    let mut turn_status: Option<Value> = None;
    let mut pending_actions: BTreeMap<u64, Value> = BTreeMap::new();
    let mut action_seq = 0u64;
    let mut requests: BTreeMap<u64, String> = BTreeMap::new();
    while turn_status.is_none() {
        if let Some(message) = app.receive(Duration::from_millis(25))? {
            let method = message["method"].as_str().unwrap_or("");
            let has_id = message.get("id").is_some();
            if method.is_empty() && has_id {
                let id = message["id"].as_u64();
                if id == Some(turn_request) {
                    match message["result"]["turn"]["id"].as_str() {
                        Some(turn) => {
                            turn_id = Some(turn.to_owned());
                            life.event(json!({"kind":"turn_acknowledged","turn_id":turn}))?;
                        }
                        None => {
                            life.event(json!({"kind":"turn_start_failed","error":response_error(&message)}),
                                )?;
                            turn_status = Some(json!("failed"));
                        }
                    }
                } else if let Some(control) = id.and_then(|id| requests.remove(&id)) {
                    life.event(json!({"kind":"control_response","control_id":control,"result":message.get("result"),"error":response_error(&message)}),
                        )?;
                }
                continue;
            }
            if has_id {
                if APPROVAL_METHODS.contains(&method) {
                    action_seq += 1;
                    let params = &message["params"];
                    // 0.155.1 added `kind` to command approvals: `command`,
                    // the default when absent, or `writeStdin`, which is
                    // input to a terminal that is already running. A
                    // decision must record which one it answered.
                    let approval_kind = (method == "item/commandExecution/requestApproval")
                        .then(|| params["kind"].as_str().unwrap_or("command").to_owned());
                    pending_actions.insert(action_seq, message["id"].clone());
                    life.event(json!({"kind":"action_requested","action_seq":action_seq,"request_id":message["id"],"method":method,"approval_kind":approval_kind,"turn_id":params["turnId"],"item_id":params["itemId"],"command":params["command"],"cwd_digest":params["cwd"].as_str().map(|c|pio_codex::sha256_hex(c.as_bytes())),"reason":params["reason"]}),
                        )?;
                } else {
                    // PIO never answers user input, elicitations, tool calls
                    // or attestation on the user's behalf, and never grants
                    // permissions beyond the approved thread settings.
                    let reason = if method == REFUSED_PERMISSION_GRANT {
                        "declined by PIO: a permission grant would widen the approved thread settings"
                    } else {
                        "declined by PIO: no user is attached to answer this request"
                    };
                    app.send(
                        &json!({"id":message["id"],"error":{"code":-32000,"message":reason}}),
                    )?;
                    life.event(
                        json!({"kind":"native_request_declined","method":method,"reason":reason}),
                    )?;
                }
                continue;
            }
            match method {
                    "turn/started" => life.event(json!({"kind":"turn_started","turn_id":message["params"]["turn"]["id"]}),
                    )?,
                    "item/agentMessage/delta"
                    | "item/commandExecution/outputDelta"
                    | "item/completed"
                    | "turn/diff/updated" => {
                        let mut line = serde_json::to_vec(
                            &json!({"method":method,"params":message["params"]}),
                        )?;
                        line.push(b'\n');
                        let digest = spool.put(&line)?;
                        append_json(
                            &refs,
                            &json!({"digest":digest,"offset":output_offset,"length":line.len()}),
                        )?;
                        output_offset += line.len() as u64;
                        all_output.extend_from_slice(&line);
                        if method == "item/completed" {
                            let item = &message["params"]["item"];
                            life.event(json!({"kind":"item_completed","item_type":item["type"],"item_id":item["id"],"status":item["status"]}),
                            )?;
                        }
                    }
                    "thread/tokenUsage/updated" => life.event(json!({"kind":"usage","turn_id":message["params"]["turnId"],"total":message["params"]["tokenUsage"]["total"],"last":message["params"]["tokenUsage"]["last"]}),
                    )?,
                    "serverRequest/resolved" => life.event(json!({"kind":"request_resolved","request_id":message["params"]["requestId"]}),
                    )?,
                    "turn/completed" => {
                        let turn = &message["params"]["turn"];
                        life.event(json!({"kind":"turn_completed","turn_id":turn["id"],"status":turn["status"],"error":turn["error"]}),
                        )?;
                        turn_status = Some(turn["status"].clone());
                    }
                    "error" => life.event(json!({"kind":"native_error","error":message["params"]["error"]}),
                    )?,
                    _ => {}
                }
        }
        // Controls are deduplicated by the lifecycle; each arrives once.
        for control in life.controls()? {
            let id = control["id"].as_str().unwrap_or_default().to_owned();
            match control["kind"].as_str() {
                    Some("respond_action") => {
                        let decision = control["decision"].as_str().unwrap_or("");
                        let rpc = control["action_seq"]
                            .as_u64()
                            .and_then(|seq| pending_actions.remove(&seq));
                        match rpc {
                            Some(rpc) if ALLOWED_DECISIONS.contains(&decision) => {
                                app.respond(&rpc, json!({"decision":decision}))?;
                                life.event(json!({"kind":"control_applied","control_id":id,"action_seq":control["action_seq"],"decision":decision}),
                                )?;
                            }
                            _ => life.event(json!({"kind":"control_rejected","control_id":id,"reason":"no pending action or decision not allowed"}),
                            )?,
                        }
                    }
                    Some("interrupt") => match &turn_id {
                        Some(turn) => {
                            let request = app.request(
                                "turn/interrupt",
                                json!({"threadId":thread_id,"turnId":turn}),
                            )?;
                            requests.insert(request, id.clone());
                            life.event(json!({"kind":"control_sent","control_id":id,"method":"turn/interrupt"}),
                            )?;
                        }
                        None => life.event(json!({"kind":"control_rejected","control_id":id,"reason":"no acknowledged turn"}),
                        )?,
                    },
                    Some("steer") => match (
                        &turn_id,
                        spool
                            .read(control["digest"].as_str().unwrap_or_default())
                            .ok()
                            .and_then(|b| String::from_utf8(b).ok()),
                    ) {
                        (Some(turn), Some(text)) => {
                            let request = app.request("turn/steer", json!({"threadId":thread_id,"expectedTurnId":turn,"clientUserMessageId":id,"input":[{"type":"text","text":text}]}))?;
                            requests.insert(request, id.clone());
                            life.event(json!({"kind":"control_sent","control_id":id,"method":"turn/steer"}),
                            )?;
                        }
                        _ => life.event(json!({"kind":"control_rejected","control_id":id,"reason":"no acknowledged turn or steering text unavailable"}),
                        )?,
                    },
                    _ => life.event(json!({"kind":"control_rejected","control_id":id,"reason":"unknown control"}),
                    )?,
                }
        }
    }
    app.close_stdin();
    let exit = life.stop(&mut app.child)?;
    let after = pio_codex::config_snapshot(&codex_home)?;
    let fixture_root = life.spec["fixture_root"].as_str().map(Path::new);
    let diff = pio_codex::config_diff(&before, &after, fixture_root);
    // Ordered deliberately, as the Claude and OpenCode hosts are: the exit
    // event is what turns the runtime to `exited`, so everything a caller
    // must see on a finished execution is recorded first. This host had it
    // the other way round, and a reader that saw `exited` could find no
    // `config_after` yet (review 47: `j1_turn_completes`, `IndexError`).
    life.event(json!({"kind":"config_after","snapshot":after,"diff":diff}))?;
    life.event(json!({"kind":"app_server_exited","code":exit}))?;
    let receipt = json!({"source":source(&life.spec),"kind":"native_turn_completed","turn_status":turn_status,"app_server_exit":exit,"output_digest":pio_core::digest(&all_output),"output_bytes":output_offset,"completion_is_acceptance":false});
    life.complete(receipt)?;
    Ok(())
}
