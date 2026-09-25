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
/// The server request that carries an MCP tool-call approval at 0.155.1 and
/// 0.157.0 (read from source at both tags, not measured): a form-mode
/// elicitation whose `_meta.codex_approval_kind` is `mcp_tool_call`. Any
/// other elicitation asks for data or a login, which PIO never supplies.
pub const MCP_APPROVAL_METHOD: &str = "mcpServer/elicitation/request";

fn is_mcp_tool_approval(method: &str, params: &Value) -> bool {
    method == MCP_APPROVAL_METHOD
        && params["mode"] == "form"
        && params["_meta"]["codex_approval_kind"] == "mcp_tool_call"
}

/// What a decision is sent as. A command or file-change approval takes the
/// decision itself. An MCP tool-call approval is an elicitation: `accept`
/// with no `persist` in `_meta` is a single use, `decline` refuses the call,
/// and `cancel` aborts it. PIO never sends `persist`, so nothing is
/// remembered for the session or for good.
fn answer_body(elicitation: bool, decision: &str) -> Value {
    match (elicitation, decision) {
        (true, "accept") => json!({"action":"accept","content":{}}),
        (true, other) => json!({"action":other}),
        (false, other) => json!({"decision":other}),
    }
}

/// A string field, if it is one; nothing else is copied.
fn text_of(value: &Value) -> Value {
    value.as_str().map(|s| json!(s)).unwrap_or(Value::Null)
}

/// What PIO records about a server request it declines by itself, and why.
///
/// Only fields that say **what** was asked and **by whom**: the method, the
/// turn and item, and for an MCP elicitation the server, the mode, Codex's
/// approval kind and request type, and the tool's name or title where Codex
/// put one in `_meta`. For a permission grant, the kinds of permission asked
/// for, never their values. Never an argument, a form's content, a URL or a
/// message a server wrote: any of those can carry a secret or a path. The
/// reason is the true one, not "no user is attached" (review of L3, CH-2).
pub fn native_decline(method: &str, params: &Value) -> Value {
    let mut record = json!({"method":method,"turn_id":text_of(&params["turnId"]),
                            "item_id":text_of(&params["itemId"]),"decided_by":"pio",
                            "sent":"a JSON-RPC error, code -32000"});
    let reason = match method {
        REFUSED_PERMISSION_GRANT => {
            record["permission_kinds"] = params["permissions"]
                .as_object()
                .map(|kinds| json!(kinds.keys().collect::<Vec<_>>()))
                .unwrap_or(Value::Null);
            "declined by PIO: a permission grant would widen the approved thread settings"
        }
        MCP_APPROVAL_METHOD => {
            let meta = &params["_meta"];
            record["server"] = text_of(&params["serverName"]);
            record["mode"] = text_of(&params["mode"]);
            record["approval_kind"] = text_of(&meta["codex_approval_kind"]);
            record["request_type"] = text_of(&meta["codex_request_type"]);
            record["tool"] = match text_of(&meta["tool_name"]) {
                Value::Null => text_of(&meta["tool_title"]),
                name => name,
            };
            "declined by PIO: an MCP elicitation PIO does not recognise as a tool-call \
             approval (only mode form with _meta.codex_approval_kind mcp_tool_call is \
             one); PIO never supplies data, a login or a URL visit"
        }
        "item/tool/call" => {
            record["tool"] = text_of(&params["tool"]);
            "declined by PIO: a tool call PIO does not run on the user's behalf"
        }
        _ => {
            "declined by PIO: not a request PIO answers; it answers only command, \
             file-change and MCP tool-call approvals"
        }
    };
    record["reason"] = json!(reason);
    record
}

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

/// The requests the host declined while it waited for `initialize`,
/// `account/read` or `thread/start`, recorded like any other native decline
/// and so carried on the run's exit (review of L3, round 2, V-2/HR-6).
fn record_early_declines(life: &mut Lifecycle, declined: Vec<Value>) -> Result<()> {
    for mut record in declined {
        record["kind"] = json!("native_request_declined");
        record["phase"] = json!("before_turn");
        life.event(record)?;
    }
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
    let (init, early) =
        app.wait_response_declining(init, Duration::from_secs(60), native_decline)?;
    record_early_declines(life, early)?;
    if let Some(error) = response_error(&init) {
        bail!("initialize refused: {error}");
    }
    app.notify("initialized")?;
    let account = app.request("account/read", json!({"refreshToken":false}))?;
    let (account, early) =
        app.wait_response_declining(account, Duration::from_secs(60), native_decline)?;
    record_early_declines(life, early)?;
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
    // The lead tool, on this thread alone: a per-thread `config` that
    // overrides what would be read from `config.toml`, the route the M4b
    // probe saw Codex launch. Every other run's thread gets none. Its
    // `pre_allowed_tools` (the owner's decision for L3's lead, 2026-09-25)
    // set those tools' approval mode to `approve`, on this server only, so
    // Codex does not ask before calling them; nothing else is pre-allowed,
    // and nothing is written to the owner's configuration.
    if let Some(tool) = life.spec.get("lead_tool").filter(|t| t.is_object()) {
        let name = tool["name"].as_str().context("lead tool name")?.to_owned();
        let env: serde_json::Map<String, Value> = tool["env"]
            .as_array()
            .context("lead tool env")?
            .iter()
            .map(|v| {
                (
                    v["name"].as_str().unwrap_or_default().to_owned(),
                    v["value"].clone(),
                )
            })
            .collect();
        let mut server = json!({"command":tool["command"],"args":tool["args"],"env":env});
        if let Some(names) = tool["pre_allowed_tools"].as_array() {
            let tools: serde_json::Map<String, Value> = names
                .iter()
                .filter_map(Value::as_str)
                .map(|n| (n.to_owned(), json!({"approval_mode":"approve"})))
                .collect();
            server["tools"] = Value::Object(tools);
        }
        params["config"]["mcp_servers"][&name] = server;
    }
    // What goes on the wire, read back from the request itself rather than
    // from the spec: every server's name, its per-tool approval modes, and
    // any server-wide default mode. The spec said what was meant; this says
    // what was sent (review of L3, CH-1).
    let written = params["config"]["mcp_servers"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let names: Vec<&String> = written.keys().collect();
    let servers: serde_json::Map<String, Value> = written
        .iter()
        .map(|(name, server)| {
            (
                name.clone(),
                json!({"tools":server["tools"],
                       "default_tools_approval_mode":server["default_tools_approval_mode"]}),
            )
        })
        .collect();
    let pre_allowed: Value = written
        .values()
        .find_map(|server| server["tools"].as_object())
        .map(|tools| {
            json!(
                tools
                    .iter()
                    .filter(|(_, mode)| mode["approval_mode"] == "approve")
                    .map(|(tool, _)| tool)
                    .collect::<Vec<_>>()
            )
        })
        .unwrap_or(Value::Null);
    life.event(
        json!({"kind":"mcp_servers_sent","names":names,"servers":servers,
                      "pre_allowed_tools":pre_allowed}),
    )?;
    let thread = app.request("thread/start", params)?;
    // Codex attaches the thread's listener before it answers thread/start,
    // and launches the thread's MCP servers then: a request can arrive
    // before the answer does.
    let (thread, early) =
        app.wait_response_declining(thread, Duration::from_secs(120), native_decline)?;
    record_early_declines(life, early)?;
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
    // The model and provider the thread is on, from Codex's own answer and
    // before its first turn (owner decision for L3, 2026-09-25). What was
    // asked for is not evidence of what was selected; a run whose answer
    // differs ends here, having spent nothing.
    let requested = life.spec["thread"]["model"].as_str();
    let provider = life.spec["expected_model_provider"].as_str();
    let matches = requested.is_none_or(|m| result["model"].as_str() == Some(m))
        && provider.is_none_or(|p| result["modelProvider"].as_str() == Some(p));
    life.event(json!({"kind":"model_checked","requested_model":requested,
                      "expected_model_provider":provider,"model":result["model"],
                      "model_provider":result["modelProvider"],"matches":matches}))?;
    ensure!(
        matches,
        "thread_model_mismatch: asked for {requested:?} on {provider:?}, Codex answered {} on {}",
        result["model"],
        result["modelProvider"]
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
    // The caller's own delivery timeout is how long their decision may take.
    // A request nobody answers holds the turn open for ever, so, as on the
    // other two hosts, the default is a single decline, recorded as PIO's
    // (owner decision, 2026-09-25: "if I don't answer, it lapses").
    let answer_timeout = life.spec["action_answer_timeout_seconds"]
        .as_u64()
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(120));
    // Request id, whether it is an MCP tool-call elicitation, and its lapse.
    let mut pending_actions: BTreeMap<u64, (Value, bool, Instant)> = BTreeMap::new();
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
                let params = &message["params"];
                let elicitation = is_mcp_tool_approval(method, params);
                if APPROVAL_METHODS.contains(&method) || elicitation {
                    action_seq += 1;
                    // 0.155.1 added `kind` to command approvals: `command`,
                    // the default when absent, or `writeStdin`, which is
                    // input to a terminal that is already running. A
                    // decision must record which one it answered.
                    let approval_kind = if elicitation {
                        Some("mcp_tool_call".to_owned())
                    } else {
                        (method == "item/commandExecution/requestApproval")
                            .then(|| params["kind"].as_str().unwrap_or("command").to_owned())
                    };
                    pending_actions.insert(
                        action_seq,
                        (
                            message["id"].clone(),
                            elicitation,
                            Instant::now() + answer_timeout,
                        ),
                    );
                    let mut event = json!({"kind":"action_requested","action_seq":action_seq,"request_id":message["id"],"method":method,"approval_kind":approval_kind,"turn_id":params["turnId"],"item_id":params["itemId"],"command":params["command"],"cwd_digest":params["cwd"].as_str().map(|c|pio_codex::sha256_hex(c.as_bytes())),"reason":params["reason"],
                        "answer_deadline_seconds":answer_timeout.as_secs(),
                        "if_nobody_answers":{"decision":"decline","decided_by":"pio","always_option_taken":false}});
                    if elicitation {
                        // Which server and tool, in the harness's own words,
                        // and what it offered to remember, which PIO never
                        // sends.
                        event["server"] = params["serverName"].clone();
                        event["message"] = params["message"].clone();
                        event["persist_offered"] = params["_meta"]["persist"].clone();
                    }
                    if method == "item/commandExecution/requestApproval" {
                        // Where the command would run, against this run's
                        // workspace, by the classifier the other hosts use:
                        // a label and a digest, never the path. And whether
                        // Codex is asking for network access, which is a
                        // different thing from running a command (review
                        // of L3, CH-6/F6).
                        let workspace = Path::new(life.spec["cwd"].as_str().unwrap_or_default());
                        let mut placement =
                            pio_claude::classify_path(params["cwd"].as_str(), workspace, workspace);
                        placement["subject"] = json!("cwd");
                        event["classification"] = placement;
                        event["network_approval"] =
                            json!(!params["networkApprovalContext"].is_null());
                    }
                    life.event(event)?;
                } else {
                    // PIO never answers user input, elicitations, tool calls
                    // or attestation on the user's behalf, and never grants
                    // permissions beyond the approved thread settings. What
                    // it declined is recorded with enough to say what was
                    // asked, and the true reason, so a receipt can say that
                    // PIO decided it (review of L3, CH-2/F1).
                    let record = native_decline(method, params);
                    app.send(&json!({"id":message["id"],"error":{"code":-32000,
                                     "message":record["reason"]}}))?;
                    let mut event = record;
                    event["kind"] = json!("native_request_declined");
                    event["request_id"] = message["id"].clone();
                    event["phase"] = json!("turn");
                    life.event(event)?;
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
        // A request nobody answered in time: one decline, recorded as PIO's.
        let overdue: Vec<u64> = pending_actions
            .iter()
            .filter(|(_, (_, _, lapses))| Instant::now() >= *lapses)
            .map(|(seq, _)| *seq)
            .collect();
        for seq in overdue {
            let Some((rpc, elicitation, _)) = pending_actions.remove(&seq) else {
                continue;
            };
            let body = answer_body(elicitation, "decline");
            app.respond(&rpc, body.clone())?;
            life.event(json!({"kind":"request_denied_by_default","action_seq":seq,
                "decision":"decline","decided_by":"pio",
                "after_seconds":answer_timeout.as_secs(),"sent":body,
                "always_option_taken":false}))?;
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
                            Some((rpc, elicitation, _)) if ALLOWED_DECISIONS.contains(&decision) => {
                                let body = answer_body(elicitation, decision);
                                app.respond(&rpc, body.clone())?;
                                life.event(json!({"kind":"control_applied","control_id":id,"action_seq":control["action_seq"],"decision":decision,"sent":body,"always_option_taken":false}),
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
