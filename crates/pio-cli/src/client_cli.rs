//! `pio client`: the command line over PIO's public API.
//!
//! Every command here goes through `pio_client`, the same public client the
//! M4 screen uses, so what the command line can see and do is exactly what
//! the screen can (M4 rule 1). Human-readable output by default, `--json`
//! for scripts. A refusal is printed as the service sent it, the whole
//! error object, and exits 3; it is never reported as a success.
use crate::ledger;
use anyhow::{Context, Result, bail};
use pio_client::board::{Board, glyph, run_state};
use pio_client::walk::{Order, decisions, lost_to_retention, walk};
use pio_client::watch::{WatchState, deliver_page};
use pio_client::{Client, Credential, Failure, Options, Pinned, Position, Reply, decision_word};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const USAGE: &str = "\
pio client COMMAND [--socket PATH] [--credential-file PATH] [--grant ID] [--json]

  list                          the board: every run you can see, by state
  inspect ID                    one run's view
  output ID [--follow] [--offset N]
                                the run's transcript bytes; --follow until it exits
  approvals                     every request waiting, soonest due first, and
                                every decision made about one
  answer RUN ACTION allow|deny [--epoch N] [--revision N]
                                answer one request, once
  cancel ID [--reason TEXT] [--epoch N] [--revision N]
  steer ID TEXT [--epoch N] [--revision N]
  claim [--host ID] [--epoch N] claim the controller; older epochs are then refused
  watch --state FILE [--kinds K,..] [--max-events N] [--idle-exit SECONDS]
                                follow the event stream; the position is kept in
                                FILE, so detaching and coming back resumes
  submit --store DIR --request FILE [--basis FILE]
  reconcile --store DIR         the caller ledger: submit once, recover after a loss

The socket and the credential file can also come from PIO_SOCKET and
PIO_CREDENTIAL_FILE. Exit status: 0 done; 3 refused by the service (the
refusal is printed as it came, whole); 4 (submit and reconcile only) the
outcome is not known yet, the service answered retry after_reconcile or a
reconcile could not establish it: the caller ledger keeps it pending, run
pio client reconcile; 2 anything else.";

const FLAGS: [&str; 2] = ["--json", "--follow"];

struct Args {
    positional: Vec<String>,
    options: BTreeMap<String, String>,
    flags: BTreeSet<String>,
}

impl Args {
    fn parse(args: &[String]) -> Result<Self> {
        let mut parsed = Self {
            positional: vec![],
            options: BTreeMap::new(),
            flags: BTreeSet::new(),
        };
        let mut rest = args.iter();
        while let Some(arg) = rest.next() {
            if FLAGS.contains(&arg.as_str()) {
                parsed.flags.insert(arg.clone());
            } else if arg.starts_with("--") {
                let value = rest
                    .next()
                    .with_context(|| format!("{arg} needs a value"))?;
                parsed.options.insert(arg.clone(), value.clone());
            } else {
                parsed.positional.push(arg.clone());
            }
        }
        Ok(parsed)
    }

    fn opt(&self, name: &str) -> Option<&str> {
        self.options.get(name).map(String::as_str)
    }

    fn num(&self, name: &str) -> Result<Option<u64>> {
        self.opt(name)
            .map(|v| v.parse().with_context(|| format!("{name} takes a number")))
            .transpose()
    }

    fn json(&self) -> bool {
        self.flags.contains("--json")
    }

    fn arg(&self, index: usize, what: &str) -> Result<&str> {
        self.positional
            .get(index)
            .map(String::as_str)
            .with_context(|| format!("missing {what}\n\n{USAGE}"))
    }

    fn pinned(&self) -> Result<Pinned> {
        Ok(Pinned {
            revision: self.num("--revision")?,
            epoch: self.num("--epoch")?,
        })
    }
}

/// The service said no. Printed whole, as it came, and never as a success.
fn refused(json: bool, refusal: &pio_client::Refusal) -> ! {
    if json {
        println!(
            "{}",
            json!({"refused": {"operation": refusal.operation, "error": refusal.error}})
        );
    }
    eprintln!(
        "PIO: {} refused by the service: {}",
        refusal.operation,
        serde_json::to_string(&refusal.error).unwrap_or_default()
    );
    std::process::exit(3);
}

fn check<T>(json: bool, reply: Reply<T>) -> Result<T> {
    match reply {
        Ok(value) => Ok(value),
        Err(Failure::Refused(refusal)) => refused(json, &refusal),
        Err(Failure::Transport(error)) => Err(error),
    }
}

fn path_from(args: &Args, option: &str, variable: &str) -> Result<PathBuf> {
    args.opt(option)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os(variable).map(PathBuf::from))
        .with_context(|| format!("{option} or {variable} is required"))
}

fn connect(args: &Args) -> Result<Client> {
    let socket = path_from(args, "--socket", "PIO_SOCKET")?;
    let credential = Credential::read(&path_from(
        args,
        "--credential-file",
        "PIO_CREDENTIAL_FILE",
    )?)?;
    let options = Options {
        timeout: Duration::from_secs(args.num("--timeout")?.unwrap_or(10)),
        grant: args.opt("--grant").map(str::to_owned),
        caller: "pio-cli".into(),
    };
    check(args.json(), Client::connect(&socket, &credential, &options))
}

// --- detaching ---------------------------------------------------------------

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_stop(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

/// SIGINT and SIGTERM ask a follower to stop at the next event boundary,
/// after its position is saved, rather than killing it mid-write.
fn stop_on_signal() {
    let handler = on_stop as extern "C" fn(libc::c_int) as libc::sighandler_t;
    unsafe {
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
    }
}

fn stopped() -> bool {
    STOP.load(Ordering::SeqCst)
}

// --- words ---------------------------------------------------------------------

fn word(value: &Value) -> String {
    match value {
        Value::Null => "unknown".into(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn exit_word(exit: &Value) -> String {
    if let Some(code) = exit["code"].as_i64() {
        return format!("exit {code}");
    }
    if let Some(signal) = exit["signal"].as_str() {
        return format!("signal {signal}");
    }
    match exit {
        Value::String(text) => format!("exit {text}"),
        _ => "exit unknown".into(),
    }
}

fn plural(n: usize, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// --- commands --------------------------------------------------------------------

pub fn run(args: &[String]) -> Result<()> {
    let Some(command) = args.first() else {
        bail!("{USAGE}");
    };
    let parsed = Args::parse(&args[1..])?;
    match command.as_str() {
        "submit" | "reconcile" => ledger_command(command, &parsed),
        "list" => list(&parsed),
        "inspect" => inspect(&parsed),
        "output" => output(&parsed),
        "approvals" => approvals(&parsed),
        "answer" => answer(&parsed),
        "cancel" => cancel(&parsed),
        "steer" => steer(&parsed),
        "claim" => claim(&parsed),
        "watch" => watch(&parsed),
        "help" | "--help" => {
            println!("{USAGE}");
            Ok(())
        }
        other => bail!("unknown client command {other:?}\n\n{USAGE}"),
    }
}

fn ledger_command(command: &str, args: &Args) -> Result<()> {
    let path = |name: &str| -> Result<&Path> {
        args.opt(name)
            .map(Path::new)
            .with_context(|| format!("{command} needs {name}"))
    };
    let request = if command == "submit" {
        Some(path("--request")?)
    } else {
        None
    };
    let socket = path_from(args, "--socket", "PIO_SOCKET")?;
    let credential = path_from(args, "--credential-file", "PIO_CREDENTIAL_FILE")?;
    let record = ledger::run(
        path("--store")?,
        &socket,
        &credential,
        request,
        args.opt("--basis").map(Path::new),
    )?;
    // How it ended, from the service's own answer: 0 taken, 3 refused, 4
    // not known yet (the ledger holds it pending until a reconcile says).
    // A submit the ledger already holds, unacknowledged, reconciles instead
    // of sending again, and answers as a reconcile does.
    let reconciled = record.get("operations").is_some();
    let operations: Vec<Value> = match record["operations"].as_array() {
        Some(operations) => operations.clone(),
        None => vec![record.clone()],
    };
    let status = operations.iter().map(ledger_status).max().unwrap_or(0);
    if args.json() {
        println!("{record}");
    } else {
        if reconciled {
            println!(
                "{} pending in the caller ledger",
                plural(operations.len(), "operation")
            );
        }
        for operation in &operations {
            let indent = if reconciled { "  " } else { "" };
            println!(
                "{indent}caller operation {}: {}",
                word(&operation["caller_operation"]),
                describe_ledger(operation)
            );
        }
    }
    if status != 0 {
        std::process::exit(status);
    }
    Ok(())
}

/// A caller-ledger outcome's exit status: 0 the service took it (or
/// replayed its first answer), 3 the service refused it and said so, 4 the
/// outcome is not known: the service answered `retry: after_reconcile`
/// (an `internal_error` on a submit is exactly that), or a reconcile could
/// not establish it. A 4 stays pending in the ledger: run `pio client
/// reconcile`.
fn ledger_status(outcome: &Value) -> i32 {
    let response = &outcome["response"];
    if let Some(error) = response.get("error") {
        return if error["data"]["retry"] == "after_reconcile" {
            4
        } else {
            3
        };
    }
    if response.get("result").is_some() {
        return 0;
    }
    4
}

fn describe_ledger(outcome: &Value) -> String {
    let response = &outcome["response"];
    if let Some(error) = response.get("error") {
        let verbatim = serde_json::to_string(error).unwrap_or_default();
        return if ledger_status(outcome) == 4 {
            format!("pending: the outcome is not known ({verbatim}); run pio client reconcile")
        } else {
            format!("refused by the service: {verbatim}")
        };
    }
    if let Some(result) = response.get("result") {
        let replay = if outcome["caller_replay"] == true || result["replay"] == true {
            " (a replay of the first answer)"
        } else {
            ""
        };
        return format!(
            "{} {}{replay}",
            word(&result["acknowledgment"]["subject"]["id"]),
            word(&result["outcome"]["admission"])
        );
    }
    format!(
        "pending: {}; run pio client reconcile again later",
        word(&outcome["reason"])
    )
}

fn list(args: &Args) -> Result<()> {
    let mut client = connect(args)?;
    let mut board = Board::default();
    check(args.json(), board.refresh(&mut client))?;
    let view = board.view();
    if args.json() {
        println!("{view}");
        return Ok(());
    }
    let runs = view["runs"].as_array().cloned().unwrap_or_default();
    if runs.is_empty() {
        println!("no runs visible to {}", client.principal());
        return Ok(());
    }
    println!(
        "{} · {} waiting · {} uncertain",
        plural(runs.len(), "run"),
        plural(
            view["notes"]["approvals"].as_u64().unwrap_or(0) as usize,
            "approval"
        ),
        view["notes"]["uncertain"]
    );
    for group in view["groups"].as_array().into_iter().flatten() {
        let state = group["state"].as_str().unwrap_or("");
        println!("{} {state}", glyph(state));
        for id in group["runs"].as_array().into_iter().flatten() {
            let row = runs
                .iter()
                .find(|r| r["id"] == *id)
                .cloned()
                .unwrap_or_default();
            let detail = match state {
                "needs approval" => format!(
                    "waiting on {}",
                    row["pending_actions"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(word)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                "finished" | "cancelled" => exit_word(&row["exit"]),
                "unknown" => "not read: no view from the service".into(),
                "uncertain" => format!(
                    "the outcome is in doubt: delivery {} · runtime {} · {}",
                    word(&row["delivery"]),
                    word(&row["runtime"]),
                    exit_word(&row["exit"])
                ),
                _ => format!(
                    "runtime {} · delivery {} · admission {}",
                    word(&row["runtime"]),
                    word(&row["delivery"]),
                    word(&row["admission"])
                ),
            };
            // Usage is a marker beside the state, never the state itself.
            let markers: Vec<String> = row["markers"]
                .as_array()
                .into_iter()
                .flatten()
                .map(word)
                .collect();
            let markers = if markers.is_empty() {
                String::new()
            } else {
                format!(" · {}", markers.join(" · "))
            };
            println!("  {}  r{}  {detail}{markers}", word(id), row["revision"]);
        }
    }
    Ok(())
}

fn inspect(args: &Args) -> Result<()> {
    let id = args.arg(0, "the run id")?;
    let mut client = connect(args)?;
    let view = check(args.json(), client.inspect(id))?;
    if args.json() {
        println!("{view}");
        return Ok(());
    }
    let state = run_state(Some(&view));
    println!("{id}  {} {state}", glyph(state));
    println!(
        "  admission {} · runtime {} · delivery {} · {}",
        word(&view["admission"]),
        word(&view["runtime"]),
        word(&view["delivery"]),
        exit_word(&view["exit"])
    );
    let observations = view["usage"]["observations"].as_array().map_or(0, Vec::len);
    println!(
        "  usage liability {} ({})",
        word(&view["usage"]["liability"]),
        plural(observations, "observation")
    );
    println!(
        "  host {} generation {} · revision {}",
        word(&view["host"]["id"]),
        word(&view["host"]["generation"]),
        word(&view["revision"])
    );
    for action in view["actions"].as_array().into_iter().flatten() {
        println!(
            "  action {} ({}) {}",
            word(&action["action_id"]),
            word(&action["owner"]),
            word(&action["state"])
        );
    }
    for delivery in view["deliveries"].as_array().into_iter().flatten() {
        println!(
            "  delivery {}: {} by {} from {}",
            word(&delivery["delivery_id"]),
            word(&delivery["delivery"]),
            word(&delivery["evidence"]["class"]),
            word(&delivery["evidence"]["source"])
        );
    }
    if let Some(cancellation) = view.get("cancellation") {
        println!(
            "  cancellation {} · outcome {}",
            word(&cancellation["receipt"]["state"]),
            word(&cancellation["outcome"])
        );
    }
    Ok(())
}

fn output(args: &Args) -> Result<()> {
    let id = args.arg(0, "the run id")?;
    let follow = args.flags.contains("--follow");
    let json = args.json();
    let mut client = connect(args)?;
    if follow {
        stop_on_signal();
    }
    let mut offset = args.num("--offset")?.unwrap_or(0);
    let mut exited = false;
    let mut warned = false;
    let mut out = std::io::stdout().lock();
    loop {
        let chunk = check(json, client.output_read(id, offset, 65536))?;
        if json {
            writeln!(
                out,
                "{}",
                json!({"offset": chunk.offset, "next_offset": chunk.next_offset,
                       "end_offset": chunk.end_offset, "coverage": chunk.coverage,
                       "lost_ranges": chunk.lost_ranges,
                       "data_base64": base64_of(&chunk.data)})
            )?;
        } else {
            out.write_all(&chunk.data)?;
            if chunk.coverage != "complete" && !warned {
                warned = true;
                eprintln!(
                    "PIO: output coverage {}: lost ranges {}",
                    chunk.coverage, chunk.lost_ranges
                );
            }
        }
        out.flush()?;
        offset = chunk.next_offset;
        if !chunk.data.is_empty() {
            continue;
        }
        if !follow || exited || stopped() {
            return Ok(());
        }
        let view = check(json, client.inspect(id))?;
        if view["runtime"] == "exited" {
            // One more read drains what landed before the exit.
            exited = true;
            continue;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn base64_of(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

/// Every event of the given kinds, from the start, and the retention gaps
/// met on the way.
fn all_events(client: &mut Client, json: bool, kinds: &[&str]) -> Result<(Vec<Value>, usize)> {
    let mut events = vec![];
    let mut gaps = 0;
    let mut from = Position::Start;
    loop {
        let page = check(json, client.events_read(&from, kinds, 1000))?;
        let items = page["items"].as_array().cloned().unwrap_or_default();
        gaps += items
            .iter()
            .filter(|item| item.get("gap").is_some())
            .count();
        events.extend(items.iter().filter_map(|item| item.get("event").cloned()));
        match page["next_cursor"].as_str() {
            Some(cursor) if !items.is_empty() => from = Position::Cursor(cursor.into()),
            _ => return Ok((events, gaps)),
        }
    }
}

fn approvals(args: &Args) -> Result<()> {
    let json = args.json();
    let mut client = connect(args)?;
    let (events, gaps) = all_events(&mut client, json, &[pio_client::client::EXECUTION])?;
    let mut waiting = walk(&events, Order::Deadline, None);
    // After a retention gap the walk cannot be trusted to be whole: the
    // event that carried a request may be gone. The runs' views still list
    // their pending actions, so the walk is completed from them, and every
    // row it could not fill says what was lost. Never a guess, never silence.
    let lost = if gaps > 0 {
        let mut board = Board::default();
        check(json, board.refresh(&mut client))?;
        lost_to_retention(&waiting, &board.drawn)
    } else {
        vec![]
    };
    waiting.extend(lost.iter().cloned());
    let decided = decisions(&events);
    if json {
        println!(
            "{}",
            json!({"waiting": waiting, "decided": decided, "gaps": gaps,
                   "walk_complete": gaps == 0, "lost_to_retention": lost.len(),
                   "now": now()})
        );
        return Ok(());
    }
    if gaps > 0 {
        println!(
            "the stream no longer holds all of its history ({}): the walk is completed from \
             the runs' views, and decisions made before the gap are not listed",
            plural(gaps, "retention gap")
        );
    }
    if waiting.is_empty() {
        println!("no approvals waiting");
    } else {
        println!(
            "{} waiting, soonest due first",
            plural(waiting.len(), "approval")
        );
    }
    for row in &waiting {
        println!(
            "{} {} · {}",
            glyph("needs approval"),
            word(&row["run"]),
            word(&row["action_id"])
        );
        if row["lost_to_retention"] == true {
            println!(
                "    asked {} of {}; {}",
                word(&row["requested_at"]),
                word(&row["owner"]),
                pio_client::walk::LOST
            );
            println!(
                "    answer: pio client answer {} {} allow|deny",
                word(&row["run"]),
                word(&row["action_id"])
            );
            continue;
        }
        let asks = ["command", "message", "server", "approval_kind", "method"]
            .iter()
            .filter_map(|key| row[*key].as_str().map(|v| format!("{key} {v}")))
            .collect::<Vec<_>>();
        if !asks.is_empty() {
            println!("    asks: {}", asks.join(" · "));
        }
        let classification = &row["classification"];
        if classification.is_object() {
            println!(
                "    lands: {} ({})",
                word(&classification["placement"]),
                word(&classification["disposition"])
            );
        }
        let kinds: Vec<String> = row["options"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|o| word(&o["kind"]))
            .collect();
        if !kinds.is_empty() {
            println!("    offered: {}", kinds.join(", "));
        }
        if kinds.iter().any(|k| k.contains("always")) {
            println!("    always-allow: offered, and never sent by PIO");
        }
        let fallback = &row["if_nobody_answers"];
        let sends = fallback["option_kind"]
            .as_str()
            .or_else(|| fallback["decision"].as_str())
            .or_else(|| fallback["behavior"].as_str());
        match row["due"].as_i64() {
            Some(due) => println!(
                "    due in {}s: if nobody answers, PIO sends {} and it is recorded as PIO's decision",
                (due - now()).max(0),
                sends.unwrap_or("a refusal")
            ),
            None => println!("    no deadline: it waits for an answer"),
        }
        println!(
            "    answer: pio client answer {} {} allow|deny",
            word(&row["run"]),
            word(&row["action_id"])
        );
    }
    if !decided.is_empty() {
        println!("decided");
        for decision in &decided {
            let by = match decision["decided_by"].as_str() {
                Some("pio") => "PIO".to_owned(),
                Some(other) => other.to_owned(),
                None => "unknown".to_owned(),
            };
            let mut line = format!(
                "  {} on {}: decided by {by}",
                word(&decision["action_id"]),
                word(&decision["run"])
            );
            if !decision["decision"].is_null() {
                line += &format!(", {}", word(&decision["decision"]));
            }
            if !decision["basis"].is_null() {
                line += &format!(" ({})", word(&decision["basis"]));
            }
            if decision["sent"] == false {
                line += ", nothing sent";
            }
            println!("{line}");
        }
    }
    Ok(())
}

fn answer(args: &Args) -> Result<()> {
    let run = args.arg(0, "the run id")?;
    let action = args.arg(1, "the action id")?;
    let allow = match args.arg(2, "allow or deny")? {
        "allow" => true,
        "deny" => false,
        other => bail!("an answer is allow or deny, not {other:?}: nothing wider is ever sent"),
    };
    let json = args.json();
    let mut client = connect(args)?;
    let mut sent = "";
    let result = check(
        json,
        client.fenced(run, args.pinned()?, 3, |client, fence, view| {
            // The harness's own word, from the action's owner on this run.
            // An action this run does not have finds no owner; the service
            // refuses it on its own terms.
            let owner = view["actions"]
                .as_array()
                .and_then(|actions| actions.iter().find(|a| a["action_id"] == action))
                .and_then(|a| a["owner"].as_str())
                .unwrap_or("");
            sent = decision_word(owner, allow);
            client.respond_action(run, action, sent, fence)
        }),
    )?;
    if json {
        println!("{}", json!({"answered": result, "sent": sent}));
    } else {
        println!(
            "answered {action} on {run}: {} (sent as {sent}, once) · response effect {}",
            if allow { "allow" } else { "deny" },
            word(&result["outcome"]["response_effect"])
        );
    }
    Ok(())
}

fn cancel(args: &Args) -> Result<()> {
    let id = args.arg(0, "the run id")?;
    let json = args.json();
    let reason = args.opt("--reason");
    let mut client = connect(args)?;
    let result = check(
        json,
        client.fenced(id, args.pinned()?, 3, |client, fence, _| {
            client.cancel(id, fence, reason)
        }),
    )?;
    if json {
        println!("{result}");
    } else {
        println!(
            "cancel {} for {id} (operation {}); its outcome is on the run: pio client inspect {id}",
            word(&result["outcome"]["receipt"]["state"]),
            word(&result["acknowledgment"]["operation_ref"])
        );
    }
    Ok(())
}

fn steer(args: &Args) -> Result<()> {
    let id = args.arg(0, "the run id")?;
    let text = args.arg(1, "the message")?;
    let json = args.json();
    let mut client = connect(args)?;
    let result = check(
        json,
        client.fenced(id, args.pinned()?, 3, |client, fence, _| {
            client.steer(id, text, fence)
        }),
    )?;
    if json {
        println!("{result}");
    } else if result["outcome"]["request"] == "recorded" {
        println!(
            "steer {} recorded for {id}, delivery {}: recorded is not delivered; watch the run for its delivery",
            word(&result["outcome"]["steer_id"]),
            word(&result["outcome"]["delivery_id"])
        );
    } else {
        println!(
            "steer not supported for {id}: {}",
            word(&result["outcome"]["alternative"])
        );
    }
    Ok(())
}

fn claim(args: &Args) -> Result<()> {
    let json = args.json();
    let mut client = connect(args)?;
    let epochs = check(json, client.controller_epochs())?;
    let host = match args.opt("--host") {
        Some(host) => host.to_owned(),
        None if epochs.len() == 1 => epochs.keys().next().cloned().unwrap_or_default(),
        None => {
            // A controller never claimed has no events; the runs name it.
            let mut board = Board::default();
            check(json, board.refresh(&mut client))?;
            let hosts: BTreeSet<String> = board
                .drawn
                .values()
                .filter_map(|v| v["host"]["id"].as_str().map(str::to_owned))
                .collect();
            match hosts.len() {
                1 => hosts.into_iter().next().unwrap_or_default(),
                _ => bail!("name the controller with --host; seen: {hosts:?}"),
            }
        }
    };
    let epoch = match args.num("--epoch")? {
        Some(epoch) => epoch,
        None => epochs.get(&host).copied().unwrap_or(0),
    };
    let result = check(json, client.claim_controller(&host, epoch))?;
    if json {
        println!("{result}");
    } else {
        println!(
            "claimed controller {host}: authority epoch {}; a command fenced at an older epoch is now refused",
            word(&result["outcome"]["epoch"])
        );
    }
    Ok(())
}

fn print_event(json: bool, event: &Value) -> Result<()> {
    let mut out = std::io::stdout().lock();
    if json {
        writeln!(out, "{event}")?;
    } else {
        let mut payload = event["payload"].to_string();
        if payload.len() > 200 {
            let mut end = 200;
            while !payload.is_char_boundary(end) {
                end -= 1;
            }
            payload.truncate(end);
            payload.push('…');
        }
        writeln!(
            out,
            "#{} {} {} {}:{} r{} {payload}",
            word(&event["sequence"]),
            word(&event["recorded_at"]),
            word(&event["type"]),
            word(&event["subject"]["kind"]),
            word(&event["subject"]["id"]),
            word(&event["revision"])
        )?;
    }
    out.flush()?;
    Ok(())
}

fn print_notice(json: bool, item: &Value) -> Result<()> {
    let mut out = std::io::stdout().lock();
    if json {
        writeln!(out, "{item}")?;
    } else if let Some(gap) = item.get("gap") {
        let subjects = gap["snapshot"]["subjects"].as_array().map_or(0, Vec::len);
        writeln!(
            out,
            "-- retention gap: events {}:{} to {}:{} are no longer kept; a snapshot of {} as of {}:{} stands in",
            word(&gap["from"]["epoch"]),
            word(&gap["from"]["sequence"]),
            word(&gap["to"]["epoch"]),
            word(&gap["to"]["sequence"]),
            plural(subjects, "subject"),
            word(&gap["snapshot"]["as_of"]["epoch"]),
            word(&gap["snapshot"]["as_of"]["sequence"])
        )?;
    } else if let Some(change) = item.get("epoch_change") {
        writeln!(
            out,
            "-- epoch change: {} to {}, vouched through {}",
            word(&change["from_epoch"]),
            word(&change["to_epoch"]),
            word(&change["vouched_through"])
        )?;
    } else {
        writeln!(out, "-- {item}")?;
    }
    out.flush()?;
    Ok(())
}

fn watch(args: &Args) -> Result<()> {
    let json = args.json();
    let path = PathBuf::from(
        args.opt("--state")
            .context("watch needs --state FILE: the position is kept there")?,
    );
    let kinds: Vec<String> = args
        .opt("--kinds")
        .map(|k| k.split(',').map(str::to_owned).collect())
        .unwrap_or_default();
    let kinds: Vec<&str> = kinds.iter().map(String::as_str).collect();
    let most = args.num("--max-events")?;
    let idle = args.num("--idle-exit")?.map(Duration::from_secs);
    stop_on_signal();
    let mut client = connect(args)?;
    let mut state = WatchState::load(&path)?;
    // A saved cursor belongs to one stream. Against another store it is
    // refused `invalid_cursor` on every read, so the stream is compared
    // first, and a position from elsewhere is dropped, and said so.
    if state.stream.is_some() || state.cursor.is_some() {
        let probe = check(json, client.events_read(&Position::Now, &kinds, 1))?;
        let stream = probe["stream"]["id"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let saved = state.stream.clone().unwrap_or_default();
        if state.adopt(&stream) {
            let notice = format!(
                "the saved position belongs to stream {saved}, and this service's stream is \
                 {stream}: starting from the beginning"
            );
            if json {
                println!("{}", json!({"notice": notice}));
            } else {
                eprintln!("PIO: {notice}");
            }
            state.save(&path)?;
        }
    }
    let mut delivered = 0u64;
    let mut quiet_since = Instant::now();
    let mut subscription: Option<String> = None;
    'follow: while !stopped() {
        let from = match &state.cursor {
            Some(cursor) => Position::Cursor(cursor.clone()),
            None => Position::Start,
        };
        let page = check(json, client.events_read(&from, &kinds, 1000))?;
        let remaining = most.map(|most| most.saturating_sub(delivered));
        let end = deliver_page(&mut state, Some(&path), &page, remaining, &mut |item| {
            match item.get("event") {
                Some(event) => print_event(json, event)?,
                // A retention gap or an epoch change: said, not hidden.
                None => print_notice(json, item)?,
            }
            Ok(!stopped())
        })?;
        delivered += end.events;
        if end.events > 0 {
            quiet_since = Instant::now();
        }
        if !end.finished || most.is_some_and(|most| delivered >= most) {
            break 'follow;
        }
        let items = page["items"].as_array().cloned().unwrap_or_default();
        if !items.is_empty() {
            continue;
        }
        if idle.is_some_and(|idle| quiet_since.elapsed() >= idle) {
            break;
        }
        // Woken by a push, or after a short wait regardless: the read above
        // is what delivers, so a push that races the read loses nothing.
        if subscription.is_none() {
            let opened = check(json, client.events_subscribe(&Position::Now, &kinds))?;
            subscription = opened["subscription"].as_str().map(str::to_owned);
        }
        check(json, client.notification(Duration::from_millis(250)))?;
    }
    if let Some(subscription) = subscription {
        let _ = client.events_unsubscribe(&subscription);
    }
    state.save(&path)?;
    if !json {
        eprintln!(
            "PIO: detached after {}; the position is saved in {}",
            plural(delivered as usize, "event"),
            path.display()
        );
    }
    Ok(())
}
