//! `pio tui`: PIO's terminal screen. The board (screen 1) and the run view
//! (screen 5) of the accepted M4 design, character tier.
//!
//! **Public API only** (M4 rule 1). Everything the screen shows or does goes
//! through `pio_client`, the same public client `pio client` uses, and this
//! crate depends on no other workspace crate; `tests/boundary.rs` holds that
//! line structurally. Quitting never stops work: the screen issues no
//! operation on the way out, and a cancel is sent only after a `y`.
mod app;
mod feed;
mod model;
mod terminal;
mod ui;

use anyhow::{Context, Result, bail};
use app::{Action, App};
use feed::{Feed, Want};
use pio_client::{Client, Credential, Failure, Options};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub const USAGE: &str = "\
pio tui [--socket PATH] [--credential-file PATH] [--grant ID] [--timeout SECONDS]

The terminal screen: the board of every run you can see, and each run's
transcript. It reads and writes only through PIO's public API. q detaches;
every run keeps working. The socket and the credential file can also come
from PIO_SOCKET and PIO_CREDENTIAL_FILE.";

struct Args {
    socket: Option<PathBuf>,
    credential: Option<PathBuf>,
    grant: Option<String>,
    timeout: u64,
}

fn parse(args: &[String]) -> Result<Option<Args>> {
    let mut parsed = Args {
        socket: None,
        credential: None,
        grant: None,
        timeout: 10,
    };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if matches!(arg.as_str(), "--help" | "-h" | "help") {
            return Ok(None);
        }
        let mut value = || {
            rest.next()
                .cloned()
                .with_context(|| format!("{arg} needs a value\n\n{USAGE}"))
        };
        match arg.as_str() {
            "--socket" => parsed.socket = Some(value()?.into()),
            "--credential-file" => parsed.credential = Some(value()?.into()),
            "--grant" => parsed.grant = Some(value()?),
            "--timeout" => {
                parsed.timeout = value()?
                    .parse()
                    .context("--timeout takes a number of seconds")?
            }
            other => bail!("unknown argument {other:?}\n\n{USAGE}"),
        }
    }
    Ok(Some(parsed))
}

fn path(given: Option<PathBuf>, option: &str, variable: &str) -> Result<PathBuf> {
    given
        .or_else(|| std::env::var_os(variable).map(PathBuf::from))
        .with_context(|| format!("{option} or {variable} is required\n\n{USAGE}"))
}

/// Runs the screen until the person detaches. The exit status: 0.
pub fn run(args: &[String]) -> Result<i32> {
    let Some(args) = parse(args)? else {
        println!("{USAGE}");
        return Ok(0);
    };
    if unsafe { libc::isatty(0) == 0 || libc::isatty(1) == 0 } {
        bail!("pio tui needs a terminal on its input and output");
    }
    let socket = path(args.socket, "--socket", "PIO_SOCKET")?;
    let credential = Credential::read(&path(
        args.credential,
        "--credential-file",
        "PIO_CREDENTIAL_FILE",
    )?)?;
    let options = Options {
        timeout: Duration::from_secs(args.timeout),
        grant: args.grant,
        caller: "pio-tui".into(),
    };
    // Connected before the terminal is taken, so a refusal is printed on a
    // terminal that was never changed.
    let client = match Client::connect(&socket, &credential, &options) {
        Ok(client) => client,
        Err(Failure::Refused(refusal)) => bail!("{refusal}"),
        Err(Failure::Transport(error)) => return Err(error),
    };
    let mut app = App::default();
    app.snap.service = feed::service(&client);
    let (want, wants) = mpsc::channel();
    let (snaps_out, snaps) = mpsc::channel();
    std::thread::Builder::new()
        .name("pio-tui-feed".into())
        .spawn(move || Feed::new(client).run(wants, snaps_out))
        .context("starting the feed")?;

    terminal::catch_signals();
    let taken = terminal::take().context("taking the terminal")?;
    let outcome = screen(&mut app, &want, &snaps);
    let _ = want.send(Want::Stop);
    drop(taken);
    outcome?;
    println!("{}", app.leaving());
    Ok(0)
}

fn screen(
    app: &mut App,
    want: &mpsc::Sender<Want>,
    snaps: &mpsc::Receiver<crate::model::Snapshot>,
) -> Result<()> {
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    terminal.clear()?;
    let mut watched: Vec<String> = vec![];
    let mut drawn = Instant::now() - Duration::from_secs(5);
    let mut dirty = true;
    let mut feed_gone = false;
    loop {
        if terminal::signalled().is_some() {
            return Ok(());
        }
        while !feed_gone {
            match snaps.try_recv() {
                Ok(snapshot) => {
                    app.absorb(snapshot);
                    dirty = true;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    feed_gone = true;
                    if app.snap.error.is_none() {
                        app.snap.error = Some("the feed stopped".into());
                    }
                    app.snap.live = false;
                    dirty = true;
                }
            }
        }
        let now_watched = app.watched();
        if now_watched != watched {
            watched = now_watched;
            let _ = want.send(Want::Watch(watched.clone()));
        }
        // Once a second regardless: the deadlines count down.
        if dirty || drawn.elapsed() >= Duration::from_secs(1) {
            let size = terminal.size()?;
            app.width = size.width;
            terminal.draw(|frame| ui::draw(frame, app))?;
            drawn = Instant::now();
            dirty = false;
        }
        let ready = match event::poll(Duration::from_millis(100)) {
            Ok(ready) => ready,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => false,
            Err(error) => return Err(error.into()),
        };
        if !ready {
            continue;
        }
        match event::read() {
            Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                test_panic(key.code);
                match app.key(key) {
                    Action::Quit => return Ok(()),
                    Action::Cancel(id) => {
                        let _ = want.send(Want::Cancel(id.clone()));
                        app.message = Some(format!("cancel for {id} sent; waiting for the service"));
                    }
                    Action::None => {}
                }
                dirty = true;
            }
            Ok(Event::Resize(..)) => dirty = true,
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
}

/// Debug builds only: `PIO_TUI_TEST_PANIC=1` makes `!` panic, so the
/// acceptance run can prove the terminal comes back from a panic. A release
/// build has no such key.
fn test_panic(code: KeyCode) {
    #[cfg(debug_assertions)]
    if code == KeyCode::Char('!') && std::env::var_os("PIO_TUI_TEST_PANIC").is_some() {
        panic!("pio tui: the test panic (PIO_TUI_TEST_PANIC, debug builds only)");
    }
    #[cfg(not(debug_assertions))]
    let _ = code;
}
