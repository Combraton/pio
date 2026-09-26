//! The screen's own state, and what each key does to it. Nothing here talks
//! to the service: a key that needs the service returns an [`Action`] for
//! the feed.
use crate::model::{Snapshot, state_of};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Screen {
    Board,
    Run(String),
}

/// Where the preview sits on the board: beside the runs on a wide
/// terminal, underneath on a narrow one, unless `p` says otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    Auto,
    Right,
    Below,
}

/// One line of the runs card: a group header or a run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Group(String),
    Run(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    Cancel(String),
}

/// A wide terminal: at least this many columns.
pub const WIDE: u16 = 100;

pub struct App {
    pub snap: Snapshot,
    pub screen: Screen,
    /// The selected line of the runs card.
    pub selected: Option<Item>,
    /// The run the preview shows: the selected run, or the last one.
    pub previewed: Option<String>,
    pub folded: BTreeSet<String>,
    pub placement: Placement,
    /// The runs card's share of the width, side by side, in percent.
    pub split: u16,
    /// The run view's cursor, a block index; `None` follows the newest.
    pub cursor: Option<usize>,
    pub expanded: BTreeSet<usize>,
    pub evidence: bool,
    /// A cancel waiting for its confirmation.
    pub confirm: Option<String>,
    pub help: bool,
    pub message: Option<String>,
    pub width: u16,
}

impl Default for App {
    fn default() -> Self {
        Self {
            snap: Snapshot::default(),
            screen: Screen::Board,
            selected: None,
            previewed: None,
            folded: BTreeSet::new(),
            placement: Placement::Auto,
            split: 45,
            cursor: None,
            expanded: BTreeSet::new(),
            evidence: false,
            confirm: None,
            help: false,
            message: None,
            width: 80,
        }
    }
}

const NOT_HERE: &str = "is not in this build: T2 has the board and the run view";

impl App {
    pub fn absorb(&mut self, snap: Snapshot) {
        if let Some(notice) = &snap.notice {
            self.message = Some(notice.clone());
        }
        self.snap = snap;
        if self.selected.is_none()
            && let Some(first) = self.runs_in_order().into_iter().next()
        {
            self.selected = Some(Item::Run(first.clone()));
            self.previewed = Some(first);
        }
    }

    /// Runs in board order: what needs the person first (the fold's groups).
    pub fn runs_in_order(&self) -> Vec<String> {
        let mut out = vec![];
        for group in self.snap.board["groups"].as_array().into_iter().flatten() {
            for id in group["runs"].as_array().into_iter().flatten() {
                if let Some(id) = id.as_str() {
                    out.push(id.to_owned());
                }
            }
        }
        out
    }

    /// The lines of the runs card, folds applied.
    pub fn items(&self) -> Vec<Item> {
        let mut out = vec![];
        for group in self.snap.board["groups"].as_array().into_iter().flatten() {
            let state = group["state"].as_str().unwrap_or("").to_owned();
            out.push(Item::Group(state.clone()));
            if self.folded.contains(&state) {
                continue;
            }
            for id in group["runs"].as_array().into_iter().flatten() {
                if let Some(id) = id.as_str() {
                    out.push(Item::Run(id.to_owned()));
                }
            }
        }
        out
    }

    /// The runs whose transcripts the feed should read.
    pub fn watched(&self) -> Vec<String> {
        let mut out: Vec<String> = self.previewed.iter().cloned().collect();
        if let Screen::Run(id) = &self.screen
            && !out.contains(id)
        {
            out.push(id.clone());
        }
        out
    }

    pub fn wide(&self) -> bool {
        self.width >= WIDE
    }

    /// Whether the preview sits beside the runs card.
    pub fn beside(&self) -> bool {
        match self.placement {
            Placement::Auto => self.wide(),
            Placement::Right => true,
            Placement::Below => false,
        }
    }

    fn state_of_run(&self, id: &str) -> String {
        self.snap
            .row(id)
            .map(|row| state_of(&row).to_owned())
            .unwrap_or_else(|| "unknown".into())
    }

    fn select(&mut self, item: Item) {
        if let Item::Run(id) = &item {
            self.previewed = Some(id.clone());
        }
        self.selected = Some(item);
    }

    fn step(&mut self, by: isize) {
        let items = self.items();
        if items.is_empty() {
            return;
        }
        let at = self
            .selected
            .as_ref()
            .and_then(|s| items.iter().position(|i| i == s))
            .unwrap_or(0) as isize;
        let next = (at + by).clamp(0, items.len() as isize - 1) as usize;
        self.select(items[next].clone());
    }

    /// `a` and `u`: the next run in these states after the selected one,
    /// its group unfolded.
    fn next_in(&mut self, states: &[&str], what: &str) {
        let runs: Vec<String> = self
            .runs_in_order()
            .into_iter()
            .filter(|id| states.contains(&self.state_of_run(id).as_str()))
            .collect();
        if runs.is_empty() {
            self.message = Some(format!("no run {what}"));
            return;
        }
        let current = match &self.selected {
            Some(Item::Run(id)) => runs.iter().position(|r| r == id),
            _ => None,
        };
        let next = runs[current.map_or(0, |at| (at + 1) % runs.len())].clone();
        let state = self.state_of_run(&next);
        self.folded.remove(&state);
        self.select(Item::Run(next));
    }

    fn toggle_fold(&mut self) {
        let group = match &self.selected {
            Some(Item::Group(state)) => state.clone(),
            Some(Item::Run(id)) => self.state_of_run(id),
            None => return,
        };
        if !self.folded.remove(&group) {
            self.folded.insert(group.clone());
            self.selected = Some(Item::Group(group));
        }
    }

    fn open(&mut self, id: String) {
        self.previewed = Some(id.clone());
        self.selected = Some(Item::Run(id.clone()));
        self.screen = Screen::Run(id);
        self.cursor = None;
        self.expanded.clear();
        self.evidence = false;
    }

    /// The number of blocks the open run has now.
    pub fn block_count(&self) -> usize {
        match &self.screen {
            Screen::Run(id) => self.snap.transcripts.get(id).map_or(0, |t| t.blocks.len()),
            Screen::Board => 0,
        }
    }

    /// The cursor as an index, following the newest block when unset.
    pub fn cursor_at(&self) -> Option<usize> {
        let count = self.block_count();
        if count == 0 {
            return None;
        }
        Some(self.cursor.unwrap_or(count - 1).min(count - 1))
    }

    fn move_cursor(&mut self, by: isize) {
        let count = self.block_count();
        let Some(at) = self.cursor_at() else {
            return;
        };
        let next = (at as isize + by).clamp(0, count as isize - 1) as usize;
        // Back at the newest block, the cursor follows new ones again.
        self.cursor = if next + 1 == count { None } else { Some(next) };
    }

    pub fn key(&mut self, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && matches!(key.code, KeyCode::Char('c')) {
            return Action::Quit;
        }
        self.message = None;
        if let Some(id) = self.confirm.take() {
            // Cancel always asks, and only `y` sends it.
            if key.code == KeyCode::Char('y') {
                return Action::Cancel(id);
            }
            self.message = Some(format!("cancel withdrawn: nothing was sent, {id} keeps running"));
            return Action::None;
        }
        if self.help {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')) {
                self.help = false;
            }
            return Action::None;
        }
        match self.screen.clone() {
            Screen::Board => self.board_key(key, ctrl),
            Screen::Run(id) => self.run_key(key, &id),
        }
    }

    fn board_key(&mut self, key: KeyEvent, ctrl: bool) -> Action {
        match key.code {
            KeyCode::Char('q') => return Action::Quit,
            KeyCode::Up | KeyCode::Char('k') => self.step(-1),
            KeyCode::Down | KeyCode::Char('j') => self.step(1),
            KeyCode::Home => self.step(isize::MIN / 2),
            KeyCode::End => self.step(isize::MAX / 2),
            KeyCode::Enter => match self.selected.clone() {
                Some(Item::Run(id)) => self.open(id),
                Some(Item::Group(_)) => self.toggle_fold(),
                None => {}
            },
            KeyCode::Char(' ') => self.toggle_fold(),
            KeyCode::Char('p') => {
                self.placement = if self.beside() {
                    Placement::Below
                } else {
                    Placement::Right
                };
            }
            KeyCode::Left if ctrl => self.split = self.split.saturating_sub(5).max(25),
            KeyCode::Right if ctrl => self.split = (self.split + 5).min(75),
            KeyCode::Char('a') => self.next_in(&["needs approval"], "needs approval"),
            KeyCode::Char('u') => self.next_in(&["uncertain", "unknown"], "is uncertain"),
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char(c @ ('v' | 'o' | 'n' | '/' | ':' | 't')) => {
                let what = match c {
                    'v' => "split view",
                    'o' => "orchestrate",
                    'n' => "a new run",
                    '/' => "the filter",
                    ':' => "the command box",
                    _ => "switching skins",
                };
                self.message = Some(format!("{what} {NOT_HERE}"));
            }
            _ => {}
        }
        Action::None
    }

    fn run_key(&mut self, key: KeyEvent, id: &str) -> Action {
        match key.code {
            KeyCode::Char('q') => return Action::Quit,
            KeyCode::Esc | KeyCode::Left | KeyCode::Backspace => self.screen = Screen::Board,
            KeyCode::Up | KeyCode::Char('k') => self.move_cursor(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_cursor(1),
            KeyCode::PageUp => self.move_cursor(-10),
            KeyCode::PageDown => self.move_cursor(10),
            KeyCode::Home => self.move_cursor(isize::MIN / 2),
            KeyCode::End => self.move_cursor(isize::MAX / 2),
            KeyCode::Enter => {
                if let Some(at) = self.cursor_at()
                    && !self.expanded.remove(&at)
                {
                    self.expanded.insert(at);
                }
            }
            KeyCode::Char('e') => self.evidence = !self.evidence,
            KeyCode::Char('[') | KeyCode::Char(']') => {
                let runs = self.runs_in_order();
                if let Some(at) = runs.iter().position(|r| r == id) {
                    let next = if key.code == KeyCode::Char(']') {
                        (at + 1) % runs.len()
                    } else {
                        (at + runs.len() - 1) % runs.len()
                    };
                    let next = runs[next].clone();
                    self.open(next);
                }
            }
            KeyCode::Char('c') => {
                self.confirm = Some(id.to_owned());
            }
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('y') => {
                self.message = Some(format!(
                    "copy {NOT_HERE}; select with your terminal's own gesture"
                ));
            }
            KeyCode::Char('d') => self.message = Some(format!("the workspace diff {NOT_HERE}")),
            _ => {}
        }
        Action::None
    }

    /// For the detach line: runs that keep working, and requests that keep
    /// waiting.
    pub fn leaving(&self) -> String {
        let working = self
            .snap
            .rows()
            .iter()
            .filter(|r| matches!(state_of(r), "running" | "needs approval"))
            .count();
        let waiting = self.snap.waiting.len();
        let soonest = self
            .snap
            .waiting
            .iter()
            .filter_map(|w| w["due"].as_i64())
            .min();
        let mut line = format!(
            "PIO: detached. {working} run{} keep{} working, {waiting} approval{} keep{} waiting",
            if working == 1 { "" } else { "s" },
            if working == 1 { "s" } else { "" },
            if waiting == 1 { "" } else { "s" },
            if waiting == 1 { "s" } else { "" },
        );
        if let Some(due) = soonest {
            line += &format!(
                "; if nobody answers, PIO denies each at its deadline, the soonest in {}",
                crate::model::clock(due - crate::model::now())
            );
        }
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyEventKind;
    use serde_json::json;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: ratatui::crossterm::event::KeyEventState::NONE,
        }
    }

    fn app() -> App {
        let mut app = App::default();
        app.absorb(Snapshot {
            board: json!({
                "runs": [
                    {"id": "a", "state": "needs approval"},
                    {"id": "b", "state": "running"},
                    {"id": "c", "state": "needs approval"}],
                "groups": [
                    {"state": "needs approval", "runs": ["a", "c"]},
                    {"state": "running", "runs": ["b"]}],
                "counts": {"needs approval": 2, "running": 1}}),
            ..Snapshot::default()
        });
        app
    }

    #[test]
    fn cancel_asks_and_only_y_sends_it() {
        let mut app = app();
        app.key(press(KeyCode::Enter));
        assert_eq!(app.screen, Screen::Run("a".into()));
        assert_eq!(app.key(press(KeyCode::Char('c'))), Action::None);
        assert_eq!(app.key(press(KeyCode::Char('n'))), Action::None);
        assert!(app.message.as_deref().unwrap().contains("nothing was sent"));
        app.key(press(KeyCode::Char('c')));
        assert_eq!(app.key(press(KeyCode::Char('y'))), Action::Cancel("a".into()));
    }

    #[test]
    fn a_walks_the_waiting_runs_and_space_folds() {
        let mut app = app();
        assert_eq!(app.selected, Some(Item::Run("a".into())));
        app.key(press(KeyCode::Char('a')));
        assert_eq!(app.previewed.as_deref(), Some("c"));
        app.key(press(KeyCode::Char('a')));
        assert_eq!(app.previewed.as_deref(), Some("a"));
        app.key(press(KeyCode::Char(' ')));
        assert_eq!(app.selected, Some(Item::Group("needs approval".into())));
        assert_eq!(app.items().len(), 3, "the folded group keeps its header");
        app.key(press(KeyCode::Char('u')));
        assert_eq!(app.message.as_deref(), Some("no run is uncertain"));
    }

    #[test]
    fn quitting_is_q_or_ctrl_c_and_never_a_cancel() {
        let mut app = app();
        assert_eq!(app.key(press(KeyCode::Char('q'))), Action::Quit);
        let ctrl_c = KeyEvent {
            modifiers: KeyModifiers::CONTROL,
            ..press(KeyCode::Char('c'))
        };
        assert_eq!(app.key(ctrl_c), Action::Quit);
    }
}
