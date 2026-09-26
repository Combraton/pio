//! The terminal, taken and given back.
//!
//! Raw mode, the alternate screen and a hidden cursor while the screen
//! runs; all three undone on every way out: `q`, SIGTERM, SIGINT and SIGHUP
//! (a flag the loop reads within a tenth of a second), an error (the guard's
//! drop), and a panic (the hook, before the message is printed, so the
//! message lands on a sane terminal). Raw mode is crossterm's, which keeps
//! the terminal's own settings and puts them back byte for byte: `stty -g`
//! reads the same before and after, and the acceptance run checks it.
use ratatui::crossterm::cursor::{Hide, Show};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use std::sync::Once;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

static TAKEN: AtomicBool = AtomicBool::new(false);
static SIGNAL: AtomicI32 = AtomicI32::new(0);
static HOOK: Once = Once::new();

extern "C" fn on_signal(signal: libc::c_int) {
    SIGNAL.store(signal, Ordering::SeqCst);
}

/// SIGTERM, SIGINT and SIGHUP ask the screen to leave at its next tick,
/// having given the terminal back. Quitting never stops a run.
pub fn catch_signals() {
    let handler = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
    unsafe {
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGHUP, handler);
    }
}

/// The signal that asked the screen to leave, if one did.
pub fn signalled() -> Option<i32> {
    match SIGNAL.load(Ordering::SeqCst) {
        0 => None,
        signal => Some(signal),
    }
}

/// While this lives, the screen has the terminal.
pub struct Taken;

pub fn take() -> std::io::Result<Taken> {
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Only the screen's own thread gives the terminal back: a panic
            // on the feed's thread ends the feed, and the screen then leaves
            // and says so.
            if std::thread::current().name() == Some("main") {
                give_back();
            }
            previous(info);
        }));
    });
    enable_raw_mode()?;
    TAKEN.store(true, Ordering::SeqCst);
    execute!(std::io::stdout(), EnterAlternateScreen, Hide)?;
    Ok(Taken)
}

/// Gives the terminal back, once, however it is reached.
pub fn give_back() {
    if TAKEN.swap(false, Ordering::SeqCst) {
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen, Show);
        let _ = disable_raw_mode();
    }
}

impl Drop for Taken {
    fn drop(&mut self) {
        give_back();
    }
}
