//! Scripted turns for the labeled fake app-server: a lead that calls the tools
//! of the MCP servers its thread was given, and a led run that runs one
//! command and reports a line count. Neither is a model, and nothing here is
//! evidence about Codex.
//!
//! The message shapes are 0.157.0's generated schema (`ThreadItem`'s
//! `mcpToolCall` and `commandExecution`, `McpServerElicitationRequestParams`).
//! **When** Codex asks before an MCP tool call, what it offers and how it reads
//! the answer are read from its source at `rust-v0.157.0`
//! (`codex-rs/core/src/mcp_tool_call.rs`), not measured: L3's live run is the
//! first observation. That file differs from `rust-v0.155.1`; the functions
//! that decide whether to ask, build the request and parse the answer do not.
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// One line on stdout. Both the main loop and a scripted turn write here, a
/// whole line under the lock.
pub(crate) fn emit(message: &Value) -> Result<()> {
    let out = std::io::stdout();
    let mut lock = out.lock();
    lock.write_all(&serde_json::to_vec(message)?)?;
    lock.write_all(b"\n")?;
    lock.flush()?;
    Ok(())
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// One MCP server a thread's `config.mcp_servers` named, launched the way
/// Codex launches a stdio server: `command` and `args`, with `env` on top of
/// its own environment.
pub(crate) struct McpServer {
    pub(crate) name: String,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
    /// `tools.<name>.approval_mode` from the thread config.
    modes: HashMap<String, String>,
    default_mode: Option<String>,
    /// Each tool as the server listed it, annotations included.
    tools: Vec<Value>,
    /// Tools approved for the rest of the session by a `persist` answer.
    remembered: Vec<String>,
}

impl McpServer {
    pub(crate) fn launch(name: &str, spec: &Value) -> Result<Self> {
        let mut command = Command::new(spec["command"].as_str().context("command")?);
        for arg in spec["args"].as_array().into_iter().flatten() {
            command.arg(arg.as_str().unwrap_or_default());
        }
        for (key, value) in spec["env"].as_object().into_iter().flatten() {
            command.env(key, value.as_str().unwrap_or_default());
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take().context("server stdin")?;
        let stdout = BufReader::new(child.stdout.take().context("server stdout")?);
        let modes = spec["tools"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(tool, config)| {
                config["approval_mode"]
                    .as_str()
                    .map(|mode| (tool.clone(), mode.to_owned()))
            })
            .collect();
        let mut server = Self {
            name: name.to_owned(),
            child,
            stdin,
            stdout,
            next: 0,
            modes,
            default_mode: spec["default_tools_approval_mode"]
                .as_str()
                .map(str::to_owned),
            tools: vec![],
            remembered: vec![],
        };
        server.request(
            "initialize",
            json!({"protocolVersion":"2025-06-18","capabilities":{},
                   "clientInfo":{"name":super::fake::SOURCE,"version":"0"}}),
        )?;
        server.send(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))?;
        let listed = server.request("tools/list", json!({}))?;
        server.tools = listed["result"]["tools"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        Ok(server)
    }

    fn send(&mut self, message: &Value) -> Result<()> {
        writeln!(self.stdin, "{}", serde_json::to_string(message)?)?;
        self.stdin.flush()?;
        Ok(())
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next += 1;
        let id = self.next;
        self.send(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))?;
        let mut line = String::new();
        loop {
            line.clear();
            if self.stdout.read_line(&mut line)? == 0 {
                bail!("the MCP server {} closed its output", self.name);
            }
            if let Ok(answer) = serde_json::from_str::<Value>(&line)
                && answer["id"] == id
            {
                return Ok(answer);
            }
        }
    }

    /// Whether Codex would ask before calling this tool, as its source reads
    /// at `rust-v0.157.0` (`mcp_permission_prompt_is_auto_approved`,
    /// `requires_mcp_tool_approval_for_mode`): `approve` never asks, `prompt`
    /// always does, `writes` asks unless the tool is read-only, and the
    /// default, `auto`, asks unless the tool is read-only, or both not
    /// destructive and not open-world. A missing hint counts as the risky one.
    fn asks_before(&self, tool: &str) -> bool {
        if self.remembered.iter().any(|t| t == tool) {
            return false;
        }
        let mode = self
            .modes
            .get(tool)
            .or(self.default_mode.as_ref())
            .map(String::as_str)
            .unwrap_or("auto");
        let hints = self
            .tools
            .iter()
            .find(|t| t["name"] == tool)
            .map(|t| t["annotations"].clone())
            .unwrap_or(Value::Null);
        match mode {
            "approve" => false,
            "prompt" => true,
            "writes" => hints["readOnlyHint"] != true,
            _ => {
                if hints["destructiveHint"] == true {
                    return true;
                }
                if hints["readOnlyHint"] == true {
                    return false;
                }
                hints["destructiveHint"].as_bool().unwrap_or(true)
                    || hints["openWorldHint"].as_bool().unwrap_or(true)
            }
        }
    }
}

impl Drop for McpServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Replies to the requests a scripted turn sent the client, keyed by request
/// id, routed back by the main loop to the turn that waits for them.
pub(crate) type Waiting = Arc<Mutex<HashMap<String, Sender<Value>>>>;

/// A turn a script is playing. Whoever finishes it first, the script or an
/// interrupt, sends its one `turn/completed`.
pub(crate) struct Turn {
    pub(crate) id: String,
    pub(crate) thread: String,
    pub(crate) interrupted: AtomicBool,
    finished: AtomicBool,
    /// The thread's total as last reported, and the tokens of the model step
    /// now in flight (0 when none is). Codex reports a step's usage once the
    /// step and its tool have finished, not when the step's output ends (M2
    /// R5, R6: `item/completed` for the command, then the usage), and it
    /// reports the step an interrupt cut short (M2 R1, R3: a usage after
    /// `turn/interrupt` was sent).
    usage: Mutex<(u64, u64)>,
    /// A mutant's Codex that reports no usage at all.
    pub(crate) quiet: AtomicBool,
}

/// `thread/tokenUsage/updated` as Codex sends it: the thread's running total
/// and the step just taken.
fn usage_report(thread: &str, turn: &str, total: u64, last: u64) -> Result<()> {
    let breakdown = |n: u64| json!({"cachedInputTokens":0,"inputTokens":n/2,"outputTokens":n-n/2,"reasoningOutputTokens":0,"totalTokens":n});
    emit(&json!({"method":"thread/tokenUsage/updated","params":{
        "threadId":thread,"turnId":turn,
        "tokenUsage":{"total":breakdown(total),"last":breakdown(last)}}}))
}

impl Turn {
    pub(crate) fn new(id: String, thread: String) -> Arc<Self> {
        Arc::new(Self {
            id,
            thread,
            interrupted: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            usage: Mutex::new((0, 0)),
            quiet: AtomicBool::new(false),
        })
    }

    pub(crate) fn finished(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }

    /// `turn/interrupt`: the script stops at its next step, the step in
    /// flight is reported, as Codex reports it, and the turn ends now.
    pub(crate) fn interrupt(&self) -> Result<()> {
        {
            let mut usage = self.usage.lock().expect("usage lock");
            self.interrupted.store(true, Ordering::SeqCst);
            if !self.finished() && usage.1 > 0 {
                usage.0 += usage.1;
                if !self.quiet.load(Ordering::SeqCst) {
                    usage_report(&self.thread, &self.id, usage.0, usage.1)?;
                }
                usage.1 = 0;
            }
        }
        self.complete("interrupted")
    }

    pub(crate) fn complete(&self, status: &str) -> Result<()> {
        if self.finished.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        emit(
            &json!({"method":"turn/completed","params":{"threadId":self.thread,
            "turn":{"id":self.id,"status":status,"items":[],"error":null}}}),
        )
    }
}

struct Play {
    turn: Arc<Turn>,
    waiting: Waiting,
    markers: Option<PathBuf>,
    /// A model step's tokens, and the first step's where it differs.
    step: u64,
    first: Option<u64>,
    /// How long a model step takes before it says or calls anything.
    think: Duration,
    requests: AtomicU64,
    messages: AtomicU64,
}

impl Play {
    fn stopped(&self) -> bool {
        self.turn.interrupted.load(Ordering::SeqCst)
    }

    /// A marker keyed `kind` and labeled with the fake's source, as every
    /// other marker the fake writes is, so one reader reads them all.
    fn marker(&self, mut record: Value) -> Result<()> {
        record["source"] = json!(super::fake::SOURCE);
        super::fake::marker(&self.markers, record)
    }

    /// The first model step begins with the turn.
    fn begin(&self) {
        self.turn.usage.lock().expect("usage lock").1 = self.first.unwrap_or(self.step);
    }

    /// The step in flight has finished, with its tool: report it the way
    /// Codex does, and begin the next one if there is one. Nothing is
    /// reported twice: an interrupt that came first reported it.
    fn step(&self, more: bool) -> Result<()> {
        let mut usage = self.turn.usage.lock().expect("usage lock");
        if self.stopped() {
            return Ok(());
        }
        let last = usage.1;
        usage.0 += last;
        if !self.turn.quiet.load(Ordering::SeqCst) {
            usage_report(&self.turn.thread, &self.turn.id, usage.0, last)?;
        }
        usage.1 = if more { self.step } else { 0 };
        Ok(())
    }

    /// A model step takes time before it says or calls anything. False if
    /// the turn was interrupted meanwhile.
    fn think(&self) -> bool {
        self.wait(self.think)
    }

    fn item(&self, method: &str, item: Value) -> Result<()> {
        let stamp = if method == "item/started" {
            "startedAtMs"
        } else {
            "completedAtMs"
        };
        emit(
            &json!({"method":method,"params":{"threadId":self.turn.thread,
            "turnId":self.turn.id,"item":item,stamp:now_ms()}}),
        )
    }

    /// One agent message, its own item: Codex gives every message an id of
    /// its own, and a turn can say more than one thing.
    fn say(&self, text: &str) -> Result<()> {
        let id = format!(
            "item-agent-{}",
            self.messages.fetch_add(1, Ordering::SeqCst) + 1
        );
        emit(
            &json!({"method":"item/agentMessage/delta","params":{"threadId":self.turn.thread,
            "turnId":self.turn.id,"itemId":id,"delta":text}}),
        )?;
        self.item(
            "item/completed",
            json!({"type":"agentMessage","id":id,"text":text}),
        )
    }

    /// Ask the client, and wait for its answer. `None` if the turn was
    /// interrupted first.
    fn ask(&self, method: &str, params: Value) -> Result<Option<Value>> {
        let n = self.requests.fetch_add(1, Ordering::SeqCst) + 1;
        let id = format!("fake-request-{}-{n}", self.turn.id);
        let (sender, answers) = channel();
        self.waiting
            .lock()
            .expect("waiting lock")
            .insert(id.clone(), sender);
        emit(&json!({"method":method,"id":id,"params":params}))?;
        loop {
            match answers.recv_timeout(Duration::from_millis(100)) {
                Ok(answer) => {
                    emit(&json!({"method":"serverRequest/resolved","params":{
                        "threadId":self.turn.thread,"requestId":id}}))?;
                    return Ok(Some(answer));
                }
                Err(RecvTimeoutError::Timeout) if self.stopped() => return Ok(None),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => bail!("the client went away"),
            }
        }
    }

    /// Wait, and give up early if the turn is interrupted.
    fn wait(&self, how_long: Duration) -> bool {
        let end = Instant::now() + how_long;
        while Instant::now() < end {
            if self.stopped() {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        !self.stopped()
    }
}

fn first_number(text: &str) -> Option<u64> {
    text.split(|c: char| !c.is_ascii_digit())
        .find(|part| !part.is_empty())
        .and_then(|part| part.parse().ok())
}

/// The file a prompt names, the way the OpenCode fake finds it.
fn named_file(text: &str) -> Option<String> {
    text.split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric() && c != '.' && c != '_'))
        .map(|word| word.trim_end_matches('.'))
        .find(|word| word.ends_with(".md") || word.ends_with(".env"))
        .map(str::to_owned)
}

/// The command a prompt quotes in backticks, if any.
fn quoted_command(text: &str) -> Option<String> {
    let start = text.find('`')? + 1;
    let end = start + text[start..].find('`')?;
    Some(text[start..end].to_owned())
}

/// Play the lead: its tool calls, in order, through the thread's first MCP
/// server, asking the client first wherever Codex would; then the counts it
/// read, one line per file.
pub(crate) fn lead(
    turn: Arc<Turn>,
    servers: Arc<Mutex<Vec<McpServer>>>,
    scenario: Value,
    waiting: Waiting,
    markers: Option<PathBuf>,
) {
    let play = Play {
        turn: turn.clone(),
        waiting,
        markers,
        step: scenario["lead_usage_step"]
            .as_u64()
            .or(scenario["usage_step"].as_u64())
            .unwrap_or(4096),
        first: None,
        think: think_time(&scenario),
        requests: AtomicU64::new(0),
        messages: AtomicU64::new(0),
    };
    if let Err(error) = play_lead(&play, &servers, &scenario) {
        let _ = play.marker(json!({"kind":"lead_script_failed","error":format!("{error:#}")}));
        let _ = turn.complete("failed");
    }
}

fn play_lead(play: &Play, servers: &Mutex<Vec<McpServer>>, scenario: &Value) -> Result<()> {
    let calls = scenario["lead"]["calls"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let offset = scenario["lead"]["relay_offset"].as_i64().unwrap_or(0);
    // A shape PIO does not recognise, sent whatever the approval mode says:
    // the schema allows `openai/form` and `openaiForm` beside `form`, and
    // which one Codex sends for a tool call was read from source, not
    // measured (review of L3, CH-2). A mutant sends it, before every call or
    // only before calls to `lead_asks_for`.
    let forced_mode = scenario["lead_asks_in_mode"].as_str();
    let forced_for = scenario["lead_asks_for"].as_str();
    let mut relay = Vec::new();
    play.begin();
    for (index, call) in calls.iter().enumerate() {
        let tool = call["tool"].as_str().unwrap_or_default();
        let repeat = call["repeat"].as_u64();
        let mut tries = 0;
        let (text, failed) = loop {
            // Each call is one model step, which takes time before it calls.
            if !play.think() {
                return Ok(());
            }
            let item_id = format!("call_mcp_{index}_{tries}");
            tries += 1;
            let forced = forced_mode.filter(|_| forced_for.is_none_or(|only| only == tool));
            let (asks, server_name) = {
                let servers = servers.lock().expect("servers lock");
                let server = servers.first().context("no MCP server")?;
                (
                    forced.is_some() || server.asks_before(tool),
                    server.name.clone(),
                )
            };
            if asks {
                // The request Codex builds (`build_mcp_tool_approval_elicitation_request`):
                // a form with no fields, and what it offers to remember in
                // `_meta.persist`.
                let answer = play.ask(
                    "mcpServer/elicitation/request",
                    json!({"threadId":play.turn.thread,"turnId":play.turn.id,
                           "serverName":server_name,"mode":forced.unwrap_or("form"),
                           "message":format!("Allow the {server_name} MCP server to run tool \"{tool}\"?"),
                           "requestedSchema":{"type":"object","properties":{}},
                           "_meta":{"codex_approval_kind":"mcp_tool_call",
                                    "persist":["session","always"],
                                    "tool_params":&call["arguments"]}}),
                )?;
                let Some(answer) = answer else {
                    return Ok(());
                };
                let action = answer["result"]["action"].as_str().unwrap_or("cancel");
                let persist = answer["result"]["_meta"]["persist"].as_str();
                play.marker(json!({"kind":"mcp_approval_answered","tool":tool,
                                   "action":action,"persist":persist,
                                   "refused":answer["error"]["message"]}))?;
                // How Codex reads it (`parse_mcp_tool_approval_elicitation_response`).
                match (action, persist) {
                    ("accept", Some("session" | "always")) => servers
                        .lock()
                        .expect("servers lock")
                        .first_mut()
                        .context("no MCP server")?
                        .remembered
                        .push(tool.to_owned()),
                    ("accept", _) => {}
                    ("decline", _) => {
                        play.item(
                            "item/completed",
                            json!({"type":"mcpToolCall","id":item_id,"server":server_name,
                                   "tool":tool,"arguments":&call["arguments"],"status":"failed",
                                   "result":null,
                                   "error":{"message":"user rejected MCP tool call"}}),
                        )?;
                        play.step(true)?;
                        break ("user rejected MCP tool call".to_owned(), true);
                    }
                    _ => {
                        play.turn.interrupt()?;
                        return Ok(());
                    }
                }
            }
            play.item(
                "item/started",
                json!({"type":"mcpToolCall","id":item_id,"server":server_name,"tool":tool,
                       "arguments":&call["arguments"],"status":"inProgress"}),
            )?;
            let started = Instant::now();
            let answered = servers
                .lock()
                .expect("servers lock")
                .first_mut()
                .context("no MCP server")?
                .request(
                    "tools/call",
                    json!({"name":tool,"arguments":&call["arguments"]}),
                )?;
            let text = answered["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let failed = answered["result"]["isError"] == true || !answered["error"].is_null();
            let mut item = json!({"type":"mcpToolCall","id":item_id,"server":server_name,
                "tool":tool,"arguments":&call["arguments"],
                "status":if failed { "failed" } else { "completed" },
                "durationMs":started.elapsed().as_millis() as u64});
            if failed {
                item["error"] = json!({"message":&text});
            } else {
                item["result"] = json!({"content":[{"type":"text","text":&text}],
                                        "structuredContent":null});
            }
            play.item("item/completed", item)?;
            // This step's usage, now that its tool has finished; the next
            // step begins.
            play.step(true)?;
            let done = match repeat {
                Some(times) => tries >= times,
                None => {
                    call["until"] != "exited"
                        || failed
                        || serde_json::from_str::<Value>(&text)
                            .is_ok_and(|v| v["runtime"] == "exited")
                }
            };
            if done || tries >= 480 {
                break (text, failed);
            }
        };
        play.marker(
            json!({"kind":"mcp_tool_called","tool":tool,"arguments":&call["arguments"],
                           "tries":tries,"failed":failed,"result":&text}),
        )?;
        if let Some(label) = call["report_as"].as_str() {
            // The child's answer is its last message, one to a line: its
            // first message says what it is about to run, numbers and all.
            let said = serde_json::from_str::<Value>(&text).ok().and_then(|v| {
                v["text"]
                    .as_str()
                    .and_then(|t| t.lines().last())
                    .and_then(first_number)
            });
            relay.push(match said {
                Some(n) => format!("{label}: {}", n as i64 + offset),
                None => format!("{label}: unknown"),
            });
        }
    }
    // The last step: it answers, one message per file (not measured, a
    // shape a model may send, so that an answer split across messages is
    // what the runner reads), and then its usage.
    if !play.think() {
        return Ok(());
    }
    for line in &relay {
        play.say(line)?;
    }
    play.step(false)?;
    play.turn.complete("completed")
}

/// How long a model step takes before it says or calls anything
/// (`model_step_ms`, default 1,500): long enough that a runner reading the
/// thread's usage has read each report before the next step can call.
fn think_time(scenario: &Value) -> Duration {
    Duration::from_millis(scenario["model_step_ms"].as_u64().unwrap_or(1500))
}

/// Play a led run: one command, the one its prompt quotes, run in the
/// thread's working directory, then the line count of the file it names.
/// Where the scenario says so it asks first, as a command approval.
pub(crate) fn led(
    turn: Arc<Turn>,
    scenario: Value,
    waiting: Waiting,
    markers: Option<PathBuf>,
    cwd: String,
    prompt: String,
) {
    let named = |key: &str| {
        scenario[key]
            .as_str()
            .is_some_and(|needle| !needle.is_empty() && prompt.contains(needle))
    };
    let play = Play {
        turn: turn.clone(),
        waiting,
        markers,
        step: scenario["usage_step"].as_u64().unwrap_or(4096),
        // Only the first step: the one that runs the command.
        first: named("led_heavy_if").then(|| scenario["led_heavy_step"].as_u64().unwrap_or(60_000)),
        think: scenario["led_step_ms"]
            .as_u64()
            .map(Duration::from_millis)
            .unwrap_or_else(|| think_time(&scenario)),
        requests: AtomicU64::new(0),
        messages: AtomicU64::new(0),
    };
    let delay = if named("led_delay_if") {
        scenario["led_delay_ms"].as_u64().unwrap_or(3000)
    } else {
        scenario["delay_ms"].as_u64().unwrap_or(200)
    };
    let asks = named("command_approval_if");
    let grant = named("led_permissions_if");
    let offset = scenario["led_offset"].as_i64().unwrap_or(0);
    if let Err(error) = play_led(&play, &cwd, &prompt, delay, asks, grant, offset) {
        let _ = play.marker(json!({"kind":"led_script_failed","error":format!("{error:#}")}));
        let _ = turn.complete("failed");
    }
}

#[allow(clippy::too_many_arguments)]
fn play_led(
    play: &Play,
    cwd: &str,
    prompt: &str,
    delay: u64,
    asks: bool,
    grant: bool,
    offset: i64,
) -> Result<()> {
    let file = named_file(prompt).unwrap_or_default();
    // How Codex named a command it asked about, measured in M2 R5 at 0.155.1:
    // the user's login shell wrapping it (`/bin/zsh -lc 'python3 -m unittest
    // -q'`).
    let asked_for = quoted_command(prompt).unwrap_or_else(|| format!("wc -l {file}"));
    let command = format!("/bin/zsh -lc '{asked_for}'");
    play.begin();
    if !play.think() {
        return Ok(());
    }
    // Measured on this model (M2 R5, R6): a run says what it is about to do
    // before its command, and then answers. Its preamble quotes the command,
    // numbers and all, so only its last message is its answer.
    play.say(&format!("Running `{asked_for}`."))?;
    if grant {
        // A request PIO declines by itself: a permission grant, which asks
        // for a profile rather than a decision (0.157.0's
        // `PermissionsRequestApprovalParams`). The run goes on either way.
        let answer = play.ask(
            "item/permissions/requestApproval",
            json!({"threadId":play.turn.thread,"turnId":play.turn.id,"itemId":"item-grant",
                   "cwd":cwd,"startedAtMs":now_ms(),
                   "reason":"labeled fake permission grant request",
                   "permissions":{"filesystem":{"write":[cwd]}}}),
        )?;
        let Some(answer) = answer else {
            return Ok(());
        };
        play.marker(
            json!({"kind":"permissions_answered","result":answer["result"],
                           "refused":answer["error"]["message"]}),
        )?;
    }
    if asks {
        let answer = play.ask(
            "item/commandExecution/requestApproval",
            json!({"threadId":play.turn.thread,"turnId":play.turn.id,"itemId":"item-command",
                   "command":&command,"cwd":cwd,"reason":"labeled fake approval request",
                   "kind":"command","startedAtMs":now_ms()}),
        )?;
        let Some(answer) = answer else {
            return Ok(());
        };
        let decision = answer["result"]["decision"].as_str().unwrap_or("cancel");
        play.marker(json!({"kind":"command_approval_answered","decision":decision}))?;
        if decision != "accept" {
            play.item(
                "item/completed",
                json!({"type":"commandExecution","id":"item-command","command":&command,
                       "cwd":cwd,"status":"declined","commandActions":[]}),
            )?;
            if decision == "cancel" {
                return play.turn.interrupt();
            }
            play.step(true)?;
            if !play.think() {
                return Ok(());
            }
            play.say("The command was not approved, so I could not count the lines.")?;
            play.step(false)?;
            return play.turn.complete("completed");
        }
    }
    play.item(
        "item/started",
        json!({"type":"commandExecution","id":"item-command","command":&command,"cwd":cwd,
               "status":"inProgress","commandActions":[]}),
    )?;
    let started = Instant::now();
    if !play.wait(Duration::from_millis(delay)) {
        return Ok(());
    }
    let lines = std::fs::read_to_string(Path::new(cwd).join(&file))
        .map(|text| text.lines().count() as i64)
        .ok();
    play.item(
        "item/completed",
        json!({"type":"commandExecution","id":"item-command","command":&command,"cwd":cwd,
               "status":"completed","commandActions":[],"exitCode":0,
               "aggregatedOutput":lines.map(|n| format!("{n} {file}\n")),
               "durationMs":started.elapsed().as_millis() as u64}),
    )?;
    // The first step's usage, now that its command has finished (M2 R5,
    // R6); the step that answers begins.
    play.step(true)?;
    if !play.think() {
        return Ok(());
    }
    play.say(&match lines {
        Some(n) => (n + offset).to_string(),
        None => "unknown".to_owned(),
    })?;
    play.step(false)?;
    play.turn.complete("completed")
}
