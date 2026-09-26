//! Drawing: the board (screen 1) and the run view (screen 5), character
//! tier, Plain feel, PIO Dark. Pure functions of [`App`]; nothing here talks
//! to the service.
//!
//! Meaning never rides on colour alone: every state has its glyph and its
//! word. At under [`crate::app::WIDE`] columns the board drops workspace and
//! delivery detail first; the state words, the tokens and the two attention
//! notes never drop.
use crate::app::{App, Item, Screen};
use crate::model::{
    Audit, Block as Piece, Transcript, age, asks, clock, decider_word, doubt, exit_word, glyph,
    now, placement_word, state_of, tokens, unsettled, word,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use serde_json::Value;

/// The PIO Dark skin's tokens (design input, "Skins").
struct Skin {
    bg: Color,
    card: Color,
    fg: Color,
    dim: Color,
    head: Color,
    run: Color,
    wait: Color,
    unknown: Color,
    bad: Color,
    accent: Color,
    selection: Color,
    edge_strong: Color,
    bar_bg: Color,
    ids: [Color; 3],
}

const S: Skin = Skin {
    bg: Color::Rgb(0x0B, 0x11, 0x15),
    card: Color::Rgb(0x0F, 0x17, 0x1C),
    fg: Color::Rgb(0xC9, 0xD6, 0xDB),
    dim: Color::Rgb(0x6F, 0x80, 0x88),
    head: Color::Rgb(0x8F, 0xA3, 0xAD),
    run: Color::Rgb(0x6C, 0xCB, 0x8B),
    wait: Color::Rgb(0xF0, 0xB3, 0x54),
    unknown: Color::Rgb(0xB7, 0x9C, 0xF0),
    bad: Color::Rgb(0xF0, 0x7C, 0x6E),
    accent: Color::Rgb(0x5C, 0xC8, 0xD6),
    selection: Color::Rgb(0x18, 0x2A, 0x33),
    edge_strong: Color::Rgb(0x41, 0x54, 0x5F),
    bar_bg: Color::Rgb(0x11, 0x1B, 0x21),
    ids: [
        Color::Rgb(0x6F, 0xA8, 0xFF),
        Color::Rgb(0xF0, 0x8F, 0xB4),
        Color::Rgb(0x7F, 0xD6, 0xC2),
    ],
};

const WAITING_WORDS: &str = "WAITING FOR YOU";

fn state_style(state: &str) -> Style {
    match state {
        "needs approval" => Style::new().fg(S.wait).add_modifier(Modifier::BOLD),
        "uncertain" | "unknown" => Style::new().fg(S.unknown),
        "running" => Style::new().fg(S.run),
        "finished" => Style::new().fg(S.dim),
        "refused" | "failed" | "cancelled" => Style::new().fg(S.bad),
        _ => Style::new().fg(S.fg),
    }
}

/// Glyph and word, always together.
fn state_spans(state: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled(glyph(state).to_owned(), state_style(state)),
        Span::raw(" "),
        Span::styled(state.to_owned(), state_style(state)),
    ]
}

fn dim(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::new().fg(S.dim))
}

fn violet(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::new().fg(S.unknown))
}

fn chars(text: &str) -> usize {
    text.chars().count()
}

/// At most `width` characters, an ellipsis where it was cut.
fn fit(text: &str, width: usize) -> String {
    if chars(text) <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut out: String = text.chars().take(width - 1).collect();
    out.push('\u{2026}');
    out
}

fn pad(text: &str, width: usize) -> String {
    let text = fit(text, width);
    let n = chars(&text);
    format!("{text}{}", " ".repeat(width.saturating_sub(n)))
}

/// Words wrapped at `width`; a word longer than a line is cut.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut out = vec![];
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let mut word = word.to_owned();
            while chars(&word) > width {
                if !line.is_empty() {
                    out.push(std::mem::take(&mut line));
                }
                let head: String = word.chars().take(width).collect();
                word = word.chars().skip(width).collect();
                out.push(head);
            }
            let extra = usize::from(!line.is_empty());
            if chars(&line) + extra + chars(&word) > width {
                out.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(&word);
        }
        out.push(line);
    }
    out
}

/// Fields joined by ` · `, flowed onto as many lines as `width` needs.
/// The first line starts with `head`; the rest are indented to match.
fn flow(head: Vec<Span<'static>>, fields: Vec<Vec<Span<'static>>>, width: usize) -> Vec<Line<'static>> {
    let indent = Line::from(head.clone()).width();
    let mut lines = vec![];
    let mut current = head;
    let mut used = indent;
    let mut empty = true;
    for field in fields {
        let w = Line::from(field.clone()).width();
        let sep = if empty { 0 } else { 3 };
        if !empty && used + sep + w > width {
            lines.push(Line::from(std::mem::take(&mut current)));
            current = vec![Span::raw(" ".repeat(indent))];
            used = indent;
            empty = true;
        }
        if !empty {
            current.push(dim(" \u{b7} "));
            used += 3;
        }
        used += w;
        current.extend(field);
        empty = false;
    }
    lines.push(Line::from(current));
    lines
}

/// `left` and `right` on one line, `right` flush right, dropped if it does
/// not fit.
fn spread(left: Vec<Span<'static>>, right: Vec<Span<'static>>, width: usize) -> Line<'static> {
    let lw = Line::from(left.clone()).width();
    let rw = Line::from(right.clone()).width();
    let mut spans = left;
    if lw + 2 + rw <= width {
        spans.push(Span::raw(" ".repeat(width - lw - rw)));
        spans.extend(right);
    }
    Line::from(spans)
}

fn card(color: Color) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(color))
        .style(Style::new().bg(S.card).fg(S.fg))
}

/// Each run's identity colour, cycling three (design input, "Skins").
fn id_color(app: &App, id: &str) -> Color {
    let at = app
        .snap
        .rows()
        .iter()
        .position(|r| r["id"] == id)
        .unwrap_or(0);
    S.ids[at % S.ids.len()]
}

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(
        Block::new().style(Style::new().bg(S.bg).fg(S.fg)),
        area,
    );
    match &app.screen {
        Screen::Board => board(frame, app, area),
        Screen::Run(id) => run_view(frame, app, area, id),
    }
    if app.help {
        help(frame, app, area);
    }
}

// --- screen 1: the board -------------------------------------------------------

fn board(frame: &mut Frame, app: &App, area: Rect) {
    let notes_height = if area.height >= 16 { 4 } else { 3 };
    let [top, middle, notes, status, hint] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(notes_height),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);
    top_line(frame, app, top);
    if app.beside() {
        let runs_width = (u32::from(middle.width) * u32::from(app.split) / 100) as u16;
        let [runs, gap, preview] = Layout::horizontal([
            Constraint::Length(runs_width),
            Constraint::Length(2),
            Constraint::Fill(1),
        ])
        .areas(middle);
        runs_card(frame, app, runs);
        grip(frame, gap);
        preview_card(frame, app, preview);
    } else {
        let wanted = app.items().len() as u16 + 2;
        let height = wanted.min(middle.height.saturating_sub(4)).max(3);
        let [runs, preview] =
            Layout::vertical([Constraint::Length(height), Constraint::Fill(1)]).areas(middle);
        runs_card(frame, app, runs);
        if preview.height >= 3 {
            preview_card(frame, app, preview);
        }
    }
    notes_row(frame, app, notes);
    status_bar(frame, app, status);
    let text = if app.wide() {
        "\u{21b5} open \u{b7} space fold \u{b7} p preview \u{b7} ctrl-\u{2190}\u{2192} resize \u{b7} a approvals \u{b7} u uncertain \u{b7} ? keys \u{b7} q detach"
    } else {
        "\u{21b5} open \u{b7} space fold \u{b7} p preview \u{b7} a approvals \u{b7} u uncertain \u{b7} ? keys \u{b7} q detach"
    };
    frame.render_widget(Paragraph::new(Line::from(dim(text))), hint);
}

fn top_line(frame: &mut Frame, app: &App, area: Rect) {
    let n = app.snap.rows().len();
    let mut left = vec![
        Span::styled("pio", Style::new().fg(S.accent).add_modifier(Modifier::BOLD)),
        dim(" \u{b7} "),
        Span::raw(app.snap.service.clone()),
        dim(" \u{b7} "),
        Span::raw(if app.snap.board.is_null() {
            "reading the stream\u{2026}".to_owned()
        } else {
            format!("{n} run{}", if n == 1 { "" } else { "s" })
        }),
    ];
    if app.wide() && !app.snap.principal.is_empty() {
        left.push(dim(format!(" \u{b7} as {}", app.snap.principal)));
    }
    let right = vec![dim(app.snap.harnesses.join("   "))];
    frame.render_widget(
        Paragraph::new(spread(left, right, area.width as usize)),
        area,
    );
}

fn grip(frame: &mut Frame, area: Rect) {
    if area.height < 3 {
        return;
    }
    let at = Rect {
        y: area.y + area.height / 2,
        height: 1,
        ..area
    };
    frame.render_widget(Paragraph::new(dim("\u{22ee}\u{22ee}")), at);
}

fn runs_card(frame: &mut Frame, app: &App, area: Rect) {
    let mut counts = vec![dim("all ")];
    for state in pio_client::board::GROUPS {
        let n = app.snap.count(state);
        if n > 0 {
            counts.push(Span::styled(
                format!("{}{n} ", glyph(state)),
                state_style(state),
            ));
        }
    }
    let block = card(S.edge_strong)
        .title(Line::from(Span::styled(
            " RUNS ",
            Style::new().fg(S.head).add_modifier(Modifier::BOLD),
        )))
        .title_top(Line::from(counts).right_aligned());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let width = inner.width as usize;
    let items = app.items();
    let rows = app.snap.rows();
    let id_width = rows
        .iter()
        .map(|r| chars(r["id"].as_str().unwrap_or("")))
        .max()
        .unwrap_or(4)
        .clamp(4, 20);
    let now = now();
    let mut lines = vec![];
    let mut selected_at = 0;
    for (at, item) in items.iter().enumerate() {
        let selected = app.selected.as_ref() == Some(item);
        if selected {
            selected_at = at;
        }
        // The selection is a bar in the gutter as well as a colour, so it
        // reads on a monochrome terminal too.
        let gutter = if selected {
            Span::styled("\u{258c}", Style::new().fg(S.accent))
        } else {
            Span::raw(" ")
        };
        let mut line = match item {
            Item::Group(state) => {
                let folded = app.folded.contains(state);
                let n = app.snap.count(state);
                let mut style = Style::new().fg(S.head);
                if folded && state == "needs approval" {
                    style = state_style(state);
                }
                Line::from(vec![
                    gutter,
                    Span::styled(
                        format!(
                            "{} {state} {n}",
                            if folded { "\u{25b8}" } else { "\u{25be}" }
                        ),
                        style,
                    ),
                ])
            }
            Item::Run(id) => {
                let row = rows.iter().find(|r| r["id"] == *id).cloned().unwrap_or_default();
                let state = state_of(&row).to_owned();
                let mut spans = vec![
                    gutter,
                    Span::styled(glyph(&state).to_owned(), state_style(&state)),
                    Span::raw(" "),
                    Span::styled(pad(id, id_width), Style::new().fg(id_color(app, id))),
                    Span::raw("  "),
                    Span::styled(pad(&state, 14), state_style(&state)),
                ];
                // Usage is a marker beside the state, never the state.
                for marker in row["markers"].as_array().into_iter().flatten() {
                    spans.push(Span::raw(" "));
                    spans.push(violet(word(marker)));
                }
                let right = match app.snap.first_seen.get(id) {
                    Some(at) => vec![dim(age(now - at))],
                    None => vec![],
                };
                spread(spans, right, width)
            }
        };
        if selected {
            line = line.style(Style::new().bg(S.selection));
        }
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push(Line::from(dim(format!(
            "no runs visible to {}",
            app.snap.principal
        ))));
    }
    let height = inner.height as usize;
    let offset = (selected_at + 1).saturating_sub(height);
    frame.render_widget(
        Paragraph::new(lines).scroll((offset as u16, 0)),
        inner,
    );
}

/// The previewed run's identity and truth line, as recorded.
fn identity(app: &App, id: &str, view: &Value, width: usize) -> Vec<Line<'static>> {
    let mut fields = vec![vec![Span::raw(format!(
        "{} gen {}",
        word(&view["host"]["id"]),
        word(&view["host"]["generation"])
    ))]];
    let lease = &view["workspace"]["lease"];
    if app.wide() && lease.is_object() {
        fields.push(vec![dim(word(&lease["lease_id"]))]);
    }
    fields.push(vec![Span::raw(tokens(Some(view)))]);
    let _ = id;
    flow(vec![], fields, width)
}

fn delivery_field(app: &App, view: &Value) -> Vec<Span<'static>> {
    let delivery = word(&view["delivery"]);
    let proven = matches!(delivery.as_str(), "delivered" | "acknowledged");
    let mut spans = vec![Span::styled(
        format!(
            "delivery {delivery}{}",
            if proven { " \u{2713}" } else { "" }
        ),
        Style::new().fg(if proven {
            S.run
        } else if delivery == "ambiguous" {
            S.unknown
        } else {
            S.fg
        }),
    )];
    // Delivery detail is the first thing a narrow screen drops.
    if app.wide()
        && let Some(latest) = view["deliveries"].as_array().and_then(|d| d.last())
    {
        spans.push(dim(format!(" by {}", word(&latest["evidence"]["class"]))));
    }
    spans
}

fn truth(app: &App, view: &Value, width: usize, open: bool) -> Vec<Line<'static>> {
    let head = vec![Span::styled(
        format!("{} truth  ", if open { "\u{25be}" } else { "\u{25b8}" }),
        Style::new().fg(S.head),
    )];
    let runtime = word(&view["runtime"]);
    let mut fields = vec![
        delivery_field(app, view),
        vec![if runtime == "unknown" {
            violet("runtime unknown")
        } else {
            Span::raw(format!("runtime {runtime}"))
        }],
        vec![Span::raw(exit_word(&view["exit"]))],
        vec![Span::raw(format!("result {}", word(&view["result"])))],
    ];
    if let Some(outcome) = view["cancellation"]["outcome"].as_str() {
        fields.push(vec![Span::raw(format!("cancellation {outcome}"))]);
    }
    flow(head, fields, width)
}

fn usage(view: &Value, width: usize) -> Vec<Line<'static>> {
    let head = vec![Span::styled("  usage  ", Style::new().fg(S.head))];
    let liability = word(&view["usage"]["liability"]);
    let observations = view["usage"]["observations"].as_array().map_or(0, Vec::len);
    let mut fields = vec![
        vec![if liability == "unresolved" {
            violet("liability unresolved")
        } else {
            Span::raw(format!("liability {liability}"))
        }],
        vec![Span::raw(match observations {
            0 => "no observations".to_owned(),
            1 => "1 observation".to_owned(),
            n => format!("{n} observations"),
        })],
        vec![Span::raw(tokens(Some(view)))],
    ];
    if liability == "unresolved" {
        fields.push(vec![violet("usage unresolved")]);
    }
    flow(head, fields, width)
}

fn preview_card(frame: &mut Frame, app: &App, area: Rect) {
    let Some(id) = app.previewed.clone() else {
        let block = card(S.edge_strong);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(Paragraph::new(dim("no run selected")), inner);
        return;
    };
    let row = app.snap.row(&id).unwrap_or_default();
    let state = state_of(&row).to_owned();
    let color = id_color(app, &id);
    let mut title = vec![
        Span::styled(format!(" {id} "), Style::new().bg(color).fg(S.bg)),
        Span::raw(" "),
    ];
    title.extend(state_spans(&state));
    title.push(Span::raw(" "));
    let view = app.snap.views.get(&id).cloned();
    let block = card(color)
        .title(Line::from(title))
        .title_top(Line::from(format!(" {} ", tokens(view.as_ref()))).right_aligned());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let width = inner.width as usize;
    let Some(view) = view else {
        frame.render_widget(
            Paragraph::new(Line::from(violet(
                "not read yet: the stream names this run, and no view of it has come back",
            ))),
            inner,
        );
        return;
    };
    let mut lines = identity(app, &id, &view, width);
    lines.extend(truth(app, &view, width, false));
    let mut body = vec![];
    let transcript = app.snap.transcripts.get(&id);
    body.extend(block_lines(app, &id, transcript, width, None, true));
    let room = (inner.height as usize).saturating_sub(lines.len());
    if body.len() > room && room > 0 {
        let hidden = body.len() - room + 1;
        let mut kept = vec![Line::from(dim(format!(
            "\u{22ee} {hidden} more line{} above",
            if hidden == 1 { "" } else { "s" }
        )))];
        kept.extend(body.split_off(body.len() - (room - 1)));
        body = kept;
    }
    lines.extend(body);
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The request waiting on this tool use, if one is.
fn waiting_for<'a>(app: &'a App, run: &str, tool: &str) -> Option<&'a Value> {
    app.snap
        .waiting_on(run)
        .into_iter()
        .find(|w| w["classification"]["tool_use_id"] == tool)
}

fn waiting_spans(row: &Value) -> Vec<Span<'static>> {
    let style = Style::new().fg(S.wait).add_modifier(Modifier::BOLD);
    let when = match row["due"].as_i64() {
        Some(due) => format!(" \u{b7} {} left", clock(due - now())),
        None if row["lost_to_retention"] == true => " \u{b7} lost to retention".into(),
        None => " \u{b7} no deadline".into(),
    };
    vec![Span::styled(
        format!("{} {WAITING_WORDS}{when}", glyph("needs approval")),
        style,
    )]
}

/// Placement and decider, violet while no record has settled them.
fn settled_spans(placement: &str, decided_by: &str) -> Vec<Span<'static>> {
    let place = placement_word(placement);
    let decider = decider_word(decided_by);
    let mut spans = vec![];
    if unsettled(placement) {
        spans.push(violet(format!("\u{25c7} {place}")));
    } else {
        spans.push(dim(place));
    }
    spans.push(dim(" \u{b7} "));
    if unsettled(&decider) {
        spans.push(violet(decider));
    } else {
        spans.push(dim(decider));
    }
    spans
}

/// The lines of a run's blocks. `cursor` marks the run view's selected
/// block; `preview` keeps text short.
fn block_lines(
    app: &App,
    id: &str,
    transcript: Option<&Transcript>,
    width: usize,
    cursor: Option<usize>,
    preview: bool,
) -> Vec<Line<'static>> {
    let mut lines = vec![];
    let Some(transcript) = transcript else {
        lines.push(Line::from(dim("reading the transcript\u{2026}")));
        return lines;
    };
    if transcript.blocks.is_empty() && transcript.problem.is_none() {
        lines.push(Line::from(dim("nothing in the transcript yet")));
    }
    let mut matched = false;
    for (at, block) in transcript.blocks.iter().enumerate() {
        let open = app.expanded.contains(&at) && !preview;
        let mut these = vec![];
        match block {
            Piece::Text { who, text } => {
                these.push(Line::from(vec![
                    Span::styled("\u{23fa} ", Style::new().fg(S.accent)),
                    Span::styled(who.clone(), Style::new().fg(S.accent)),
                ]));
                let mut wrapped = wrap(text, width.saturating_sub(4));
                let limit = if preview { 3 } else { 6 };
                if !open && wrapped.len() > limit {
                    let more = wrapped.len() - limit;
                    if preview {
                        wrapped = wrapped.split_off(more);
                    } else {
                        wrapped.truncate(limit);
                        wrapped.push(format!("\u{25b8} {more} more lines (\u{21b5})"));
                    }
                }
                these.extend(wrapped.into_iter().map(|l| Line::from(format!("  {l}"))));
            }
            Piece::Tool {
                id: tool,
                kind,
                label,
                status,
                placement,
                decided_by,
                outcome,
            } => {
                let head = vec![
                    Span::styled("\u{23fa} ", Style::new().fg(S.fg)),
                    Span::styled(kind.clone(), Style::new().add_modifier(Modifier::BOLD)),
                    Span::raw("  "),
                    Span::raw(label.clone()),
                ];
                let settled = settled_spans(placement, decided_by);
                let head_width = Line::from(head.clone()).width();
                let settled_width = Line::from(settled.clone()).width();
                let beside = app.wide() && head_width + 4 + settled_width <= width;
                let said = if outcome.is_empty() { status } else { outcome };
                let said = said.replace('_', " ");
                let waiting = waiting_for(app, id, tool);
                if beside {
                    these.push(spread(head, settled, width));
                } else {
                    // Narrow: where it landed and who decided get a line
                    // of their own; they never drop.
                    these.push(Line::from(head));
                    let mut line = vec![dim("  \u{23bf} ")];
                    line.extend(settled);
                    if waiting.is_none() && !said.is_empty() {
                        line.push(dim(format!(" \u{b7} {said}")));
                    }
                    these.push(Line::from(line));
                }
                if let Some(row) = waiting {
                    matched = true;
                    let mut line = vec![dim("  \u{23bf} ")];
                    line.extend(waiting_spans(row));
                    these.push(Line::from(line));
                } else if beside && !said.is_empty() {
                    these.push(Line::from(vec![dim("  \u{23bf} "), dim(said)]));
                }
                if open {
                    these.push(Line::from(dim(format!(
                        "    tool use {tool} \u{b7} harness status {} \u{b7} outcome {}",
                        if status.is_empty() { "none" } else { status },
                        if outcome.is_empty() { "not yet recorded" } else { outcome }
                    ))));
                    if let Some(row) = waiting_for(app, id, tool) {
                        these.push(Line::from(dim(format!(
                            "    answer: pio client answer {id} {} allow|deny \u{b7} this request keeps waiting if you leave the screen",
                            word(&row["action_id"])
                        ))));
                    }
                }
            }
        }
        if cursor == Some(at) {
            these = these
                .into_iter()
                .map(|line| {
                    let mut spans = vec![Span::styled("\u{258c}", Style::new().fg(S.accent))];
                    spans.extend(line.spans);
                    Line::from(spans).style(Style::new().bg(S.selection))
                })
                .collect();
        } else if !preview {
            these = these
                .into_iter()
                .map(|line| {
                    let mut spans = vec![Span::raw(" ")];
                    spans.extend(line.spans);
                    Line::from(spans)
                })
                .collect();
        }
        lines.extend(these);
    }
    // A request the transcript has not shown a tool use for yet.
    if !matched {
        for row in app.snap.waiting_on(id) {
            let mut spans = vec![dim("  \u{23bf} ")];
            spans.extend(waiting_spans(row));
            spans.push(dim(format!(" \u{b7} {}", asks(row))));
            lines.push(Line::from(spans));
        }
    }
    if let Some(problem) = &transcript.problem {
        lines.push(Line::from(violet(format!("\u{25c7} {problem}"))));
    }
    if !transcript.coverage.is_empty() && transcript.coverage != "complete" {
        lines.push(Line::from(violet(format!(
            "\u{25c7} transcript coverage {}: part of it is no longer held",
            transcript.coverage
        ))));
    }
    if !preview {
        let has_tools = transcript
            .blocks
            .iter()
            .any(|b| matches!(b, Piece::Tool { .. }));
        let note = match transcript.audit {
            Audit::Absent if has_tools => Some(violet(
                "\u{25c7} this harness produced no end-of-turn audit: where each tool use landed stays not yet classified",
            )),
            Audit::Filled if has_tools => Some(dim(
                "the end-of-turn audit filled in where each tool use landed and who decided",
            )),
            Audit::NotYet if has_tools => Some(dim(
                "where a tool use landed is not yet classified until the end-of-turn audit",
            )),
            _ => None,
        };
        if let Some(note) = note {
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![Span::raw(" "), note]));
        }
    }
    lines
}

fn notes_row(frame: &mut Frame, app: &App, area: Rect) {
    let [left, _, right] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(area);
    // Amber: approvals waiting. They never interrupt; `a` walks them.
    let waiting = &app.snap.waiting;
    let n = waiting.len();
    let title = Line::from(vec![
        Span::styled(
            format!("{} {n} need{} you", glyph("needs approval"), if n == 1 { "s" } else { "" }),
            Style::new().fg(S.wait).add_modifier(Modifier::BOLD),
        ),
        dim(" \u{b7} press "),
        Span::styled("a ", Style::new().fg(S.accent)),
    ]);
    let block = card(S.wait).title(title);
    let inner = block.inner(left);
    frame.render_widget(block, left);
    let width = inner.width as usize;
    let mut lines = vec![];
    for row in waiting.iter().take(inner.height as usize) {
        let when = match row["due"].as_i64() {
            Some(due) => format!("{} left", clock(due - now())),
            None if row["lost_to_retention"] == true => "lost to retention".into(),
            None => "no deadline".into(),
        };
        let run = fit(&word(&row["run"]), 10);
        let room = width.saturating_sub(chars(&run) + chars(&when) + 4);
        lines.push(spread(
            vec![Span::raw(format!("{run}  {}", fit(&asks(row), room)))],
            vec![Span::styled(when, Style::new().fg(S.wait))],
            width,
        ));
    }
    if lines.is_empty() {
        lines.push(Line::from(dim("nothing is waiting for you")));
    }
    frame.render_widget(Paragraph::new(lines), inner);

    // Violet: runs whose outcome is in doubt.
    let doubtful: Vec<Value> = app
        .snap
        .rows()
        .into_iter()
        .filter(|r| matches!(state_of(r), "uncertain" | "unknown"))
        .collect();
    let title = Line::from(vec![
        Span::styled(
            format!("{} {} uncertain", glyph("uncertain"), doubtful.len()),
            Style::new().fg(S.unknown),
        ),
        dim(" \u{b7} press "),
        Span::styled("u ", Style::new().fg(S.accent)),
    ]);
    let block = card(S.unknown).title(title);
    let inner = block.inner(right);
    frame.render_widget(block, right);
    let mut lines = vec![];
    for row in doubtful.iter().take(inner.height as usize) {
        let id = word(&row["id"]);
        let why = doubt(app.snap.views.get(&id));
        lines.push(Line::from(vec![
            Span::raw(format!("{}  ", fit(&id, 10))),
            violet(fit(&why, (inner.width as usize).saturating_sub(12))),
        ]));
    }
    if lines.is_empty() {
        lines.push(Line::from(dim("no run is in doubt")));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn status_bar(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(Block::new().style(Style::new().bg(S.bar_bg)), area);
    let width = area.width as usize;
    let snap = &app.snap;
    let line = if let Some(message) = &app.message {
        Line::from(Span::styled(message.clone(), Style::new().fg(S.accent)))
    } else if let Some(error) = &snap.error {
        Line::from(violet(format!(
            "\u{25c7} the service stopped answering: {error}; the board is as it was then"
        )))
    } else {
        let mut left = vec![];
        let mut add = |text: String, style: Style| {
            if !left.is_empty() {
                left.push(dim(" \u{b7} "));
            }
            left.push(Span::styled(text, style));
        };
        add(
            format!("{} {} running", glyph("running"), snap.count("running")),
            state_style("running"),
        );
        add(
            format!("{} {} waiting", glyph("needs approval"), snap.waiting.len()),
            state_style("needs approval"),
        );
        let doubt = snap.count("uncertain") + snap.count("unknown");
        add(
            format!("{} {doubt} uncertain", glyph("uncertain")),
            state_style("uncertain"),
        );
        add(
            format!("{} {} done", glyph("finished"), snap.count("finished")),
            state_style("finished"),
        );
        for state in ["refused", "failed", "cancelled"] {
            let n = snap.count(state);
            if n > 0 {
                add(format!("{} {n} {state}", glyph(state)), state_style(state));
            }
        }
        let live = if snap.live {
            Span::styled("live", Style::new().fg(S.run))
        } else {
            dim("reading")
        };
        let mut right = vec![live];
        if !snap.walk_complete {
            right.push(violet(" \u{b7} the stream lost history"));
        }
        right.push(dim(" \u{b7} PIO Dark"));
        spread(left, right, width)
    };
    frame.render_widget(Paragraph::new(line), area);
}

// --- screen 5: the run view ------------------------------------------------------

fn run_view(frame: &mut Frame, app: &App, area: Rect, id: &str) {
    let color = id_color(app, id);
    let block = card(color);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let width = inner.width as usize;
    let row = app.snap.row(id).unwrap_or_default();
    let state = state_of(&row).to_owned();
    let view = app.snap.views.get(id).cloned().unwrap_or(Value::Null);

    let mut left = vec![
        Span::styled("\u{2190} board", Style::new().fg(S.accent)),
        Span::raw("   "),
        Span::styled(
            id.to_owned(),
            Style::new().fg(color).add_modifier(Modifier::BOLD),
        ),
    ];
    if view.is_object() {
        left.push(dim(format!(
            " \u{b7} {} gen {}",
            word(&view["host"]["id"]),
            word(&view["host"]["generation"])
        )));
        let lease = &view["workspace"]["lease"];
        if app.wide() && lease.is_object() {
            left.push(dim(format!(" \u{b7} {}", word(&lease["lease_id"]))));
        }
    }
    let mut right = state_spans(&state);
    right.push(dim(" \u{b7} "));
    right.push(Span::raw(tokens(view.is_object().then_some(&view))));
    let mut head = vec![spread(left, right, width)];
    if view.is_object() {
        head.extend(truth(app, &view, width, app.evidence));
        head.extend(usage(&view, width));
        if app.evidence {
            head.extend(evidence(app, id, &view, width));
        }
    } else {
        head.push(Line::from(violet(
            "not read yet: the stream names this run, and no view of it has come back",
        )));
    }
    head.push(Line::raw(""));

    let transcript = app.snap.transcripts.get(id);
    let cursor = app.cursor_at();
    let body = block_lines(app, id, transcript, width.saturating_sub(1), cursor, false);
    let [top, blocks, status, hint] = Layout::vertical([
        Constraint::Length(head.len() as u16),
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);
    frame.render_widget(Paragraph::new(head), top);

    // Keep the cursor's block in sight.
    let height = blocks.height as usize;
    let cursor_line = body
        .iter()
        .rposition(|l| l.spans.first().is_some_and(|s| s.content == "\u{258c}"))
        .unwrap_or(body.len().saturating_sub(1));
    let offset = (cursor_line + 1).saturating_sub(height);
    frame.render_widget(Paragraph::new(body).scroll((offset as u16, 0)), blocks);

    let count = transcript.map_or(0, |t| t.blocks.len());
    let line = if let Some(target) = &app.confirm {
        Line::from(Span::styled(
            format!("cancel {target}? It asks the harness to stop. y cancels \u{b7} any other key keeps it running"),
            Style::new().fg(S.bad).add_modifier(Modifier::BOLD),
        ))
    } else if let Some(message) = &app.message {
        Line::from(Span::styled(message.clone(), Style::new().fg(S.accent)))
    } else {
        let mut spans = state_spans(&state);
        if let Some(at) = app.snap.first_seen.get(id) {
            spans.push(dim(format!("   \u{23f1} {} since submitted", age(now() - at))));
        }
        spans.push(Span::raw(format!("   {}", tokens(view.is_object().then_some(&view)))));
        if let Some(at) = cursor {
            spans.push(dim(format!("   block {} of {count}", at + 1)));
        }
        Line::from(spans)
    };
    frame.render_widget(
        Paragraph::new(line).style(Style::new().bg(S.bar_bg)),
        status,
    );
    let text = if app.wide() {
        "\u{2191}\u{2193} blocks \u{b7} \u{21b5} open block \u{b7} e evidence \u{b7} [ ] previous or next run \u{b7} c cancel run \u{b7} esc board \u{b7} q detach"
    } else {
        "\u{2191}\u{2193} blocks \u{b7} \u{21b5} open \u{b7} e evidence \u{b7} [ ] runs \u{b7} c cancel \u{b7} esc board \u{b7} q detach"
    };
    frame.render_widget(Paragraph::new(Line::from(dim(text))), hint);
}

/// The truth line folded open: everything the view records.
fn evidence(app: &App, id: &str, view: &Value, width: usize) -> Vec<Line<'static>> {
    let mut lines = flow(
        vec![Span::raw("    ")],
        vec![
            vec![Span::raw(format!("execution {id}"))],
            vec![Span::raw(format!("revision {}", word(&view["revision"])))],
            vec![Span::raw(format!("admission {}", word(&view["admission"])))],
        ],
        width,
    );
    for delivery in view["deliveries"].as_array().into_iter().flatten() {
        lines.extend(flow(
            vec![Span::raw("    ")],
            vec![
                vec![Span::raw(format!(
                    "delivery {}: {}",
                    word(&delivery["delivery_id"]),
                    word(&delivery["delivery"])
                ))],
                vec![dim(format!("by {}", word(&delivery["evidence"]["class"])))],
                vec![dim(format!("from {}", word(&delivery["evidence"]["source"])))],
                vec![dim(format!("at {}", word(&delivery["determined_at"])))],
            ],
            width,
        ));
    }
    let lease = &view["workspace"]["lease"];
    if lease.is_object() {
        let base = word(&lease["base"]);
        lines.extend(flow(
            vec![Span::raw("    ")],
            vec![
                vec![Span::raw(format!("workspace {}", word(&lease["repository"])))],
                vec![dim(format!("base {}", fit(&base, 12)))],
                vec![dim(format!("lease {}", word(&lease["lease_id"])))],
            ],
            width,
        ));
    }
    let count = |key: &str| view[key].as_array().map_or(0, Vec::len);
    lines.extend(flow(
        vec![Span::raw("    ")],
        vec![
            vec![Span::raw(format!("recovery {}", count("recovery")))],
            vec![Span::raw(format!("obligations {}", count("obligations")))],
        ],
        width,
    ));
    if let Some(containment) = app
        .snap
        .transcripts
        .get(id)
        .map(|t| &t.containment)
        .filter(|c| c.is_object())
    {
        lines.extend(flow(
            vec![Span::raw("    ")],
            vec![
                vec![Span::raw(format!(
                    "containment {}",
                    word(&containment["mechanism"])
                ))],
                vec![Span::raw(match containment["os_sandbox_observed"].as_bool() {
                    Some(true) => "an OS sandbox observed".to_owned(),
                    Some(false) => "no OS sandbox observed".to_owned(),
                    None => "OS sandbox unknown".to_owned(),
                })],
            ],
            width,
        ));
    }
    lines
}

fn help(frame: &mut Frame, app: &App, area: Rect) {
    let keys: &[(&str, &str)] = match app.screen {
        Screen::Board => &[
            ("\u{2191} \u{2193}", "select a run or a group"),
            ("\u{21b5}", "open the run"),
            ("space", "fold or unfold a group"),
            ("p", "the preview beside or underneath"),
            ("ctrl-\u{2190} \u{2192}", "resize the cards"),
            ("a", "the next run waiting for approval"),
            ("u", "the next uncertain run"),
            ("q", "detach: every run keeps working"),
        ],
        Screen::Run(_) => &[
            ("\u{2191} \u{2193}", "step through the blocks"),
            ("\u{21b5}", "open or close a block"),
            ("e", "fold the truth line open"),
            ("[ ]", "previous or next run"),
            ("c", "cancel the run (asks first)"),
            ("esc", "back to the board"),
            ("q", "detach: every run keeps working"),
        ],
    };
    let width = area.width.min(56);
    let height = (keys.len() as u16 + 4).min(area.height);
    let at = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, at);
    let block = card(S.accent).title(Line::from(Span::styled(
        " KEYS ",
        Style::new().fg(S.head).add_modifier(Modifier::BOLD),
    )));
    let inner = block.inner(at);
    frame.render_widget(block, at);
    let mut lines: Vec<Line> = keys
        .iter()
        .map(|(key, what)| {
            Line::from(vec![
                Span::styled(pad(key, 10), Style::new().fg(S.accent)),
                Span::raw(*what),
            ])
        })
        .collect();
    lines.push(Line::from(dim("esc or ? closes this")));
    frame.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Block as Piece, Snapshot, Transcript};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn snapshot() -> Snapshot {
        let finished = json!({"runtime": "exited", "delivery": "acknowledged", "exit": {"code": 0},
            "result": "absent", "revision": 7, "admission": "admitted",
            "host": {"id": "opencode-host", "generation": 1},
            "deliveries": [{"delivery_id": "d1", "delivery": "acknowledged",
                            "evidence": {"class": "native_session_update", "source": "fake"}}],
            "usage": {"liability": "unresolved", "observations": [
                {"amount": 256, "basis": "estimated", "measure": "opencode.tokens.total"}]}});
        let waiting = json!({"runtime": "requires_action", "delivery": "acknowledged",
            "exit": "unavailable", "result": "absent", "revision": 4,
            "host": {"id": "opencode-host", "generation": 1},
            "actions": [{"action_id": "w.action-1", "owner": "opencode", "state": "pending"}],
            "usage": {"liability": "none", "observations": []}});
        let lost = json!({"runtime": "unknown", "delivery": "delivered", "exit": "unavailable",
            "result": "absent", "usage": {"liability": "none", "observations": []}});
        let mut views = BTreeMap::new();
        views.insert("done".to_owned(), finished);
        views.insert("wait".to_owned(), waiting);
        views.insert("lost".to_owned(), lost);
        let mut transcripts = BTreeMap::new();
        transcripts.insert(
            "wait".to_owned(),
            Transcript {
                blocks: vec![Piece::Tool {
                    id: "call_0".into(),
                    kind: "execute".into(),
                    label: "run a command: git tag x".into(),
                    status: "in_progress".into(),
                    placement: pio_client::blocks::NOT_YET.into(),
                    decided_by: "unknown".into(),
                    outcome: String::new(),
                }],
                coverage: "complete".into(),
                ..Transcript::default()
            },
        );
        Snapshot {
            service: "pio-journal-fake-executor 0.1.0-dev".into(),
            principal: "owner".into(),
            board: json!({
                "runs": [
                    {"id": "done", "state": "finished", "markers": ["usage unresolved"]},
                    {"id": "lost", "state": "uncertain", "markers": []},
                    {"id": "wait", "state": "needs approval", "markers": []}],
                "groups": [
                    {"state": "needs approval", "runs": ["wait"]},
                    {"state": "uncertain", "runs": ["lost"]},
                    {"state": "finished", "runs": ["done"]}],
                "counts": {"needs approval": 1, "uncertain": 1, "finished": 1},
                "notes": {"approvals": 1, "uncertain": 1}}),
            views,
            waiting: vec![json!({"run": "wait", "action_id": "w.action-1", "due": now() + 78,
                "classification": {"tool_use_id": "call_0", "tool_name": "execute"}})],
            walk_complete: true,
            transcripts,
            live: true,
            ..Snapshot::default()
        }
    }

    fn render(app: &mut App, width: u16, height: u16) -> Vec<String> {
        app.width = width;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_owned())
                    .collect::<String>()
            })
            .collect()
    }

    fn line_with<'a>(frame: &'a [String], needle: &str) -> &'a str {
        frame
            .iter()
            .find(|l| l.contains(needle))
            .unwrap_or_else(|| panic!("no line has {needle:?}:\n{}", frame.join("\n")))
    }

    #[test]
    fn the_narrow_board_keeps_every_state_word_and_both_notes() {
        let mut app = App::default();
        app.absorb(snapshot());
        let frame = render(&mut app, 80, 24);
        assert!(line_with(&frame, " done ").contains("finished"));
        assert!(line_with(&frame, " done ").contains("usage unresolved"));
        assert!(line_with(&frame, " lost ").contains("uncertain"));
        assert!(line_with(&frame, " wait ").contains("needs approval"));
        assert!(line_with(&frame, "wait  execute").contains(" left"));
        assert!(line_with(&frame, "1 uncertain \u{b7} press u").contains("uncertain"));
        assert!(line_with(&frame, "lost  host lost").contains("host lost"));
        assert!(line_with(&frame, "q detach").contains("a approvals"));
        // Narrow: the preview is underneath and delivery detail is dropped.
        assert!(!line_with(&frame, "RUNS").contains(" wait "));
        assert!(!frame.join("\n").contains("native_session_update"));
    }

    #[test]
    fn the_wide_board_puts_the_preview_beside_and_keeps_detail() {
        let mut app = App::default();
        app.absorb(snapshot());
        app.previewed = Some("done".into());
        let frame = render(&mut app, 120, 40);
        assert!(line_with(&frame, "RUNS").contains(" done "));
        assert!(frame.join("\n").contains("by native_session_update"));
        // Unresolved usage stays a marker; the state is finished.
        let title = line_with(&frame, "RUNS");
        assert!(title.contains("\u{25cb} finished"), "{title}");
    }

    #[test]
    fn the_run_view_shows_identity_and_an_unplaced_tool_use() {
        let mut app = App::default();
        app.absorb(snapshot());
        app.screen = Screen::Run("wait".into());
        let frame = render(&mut app, 80, 24);
        let text = frame.join("\n");
        for needle in [
            "\u{2190} board",
            "wait",
            "delivery acknowledged",
            "runtime requires_action",
            "exit unavailable",
            "liability none",
            "tokens unknown",
            "run a command: git tag x",
            "\u{25c7} not yet classified",
            "decider unknown",
            "WAITING FOR YOU",
        ] {
            assert!(text.contains(needle), "{needle:?} missing:\n{text}");
        }
        assert!(!text.contains("0 tokens"), "usage unknown is never zero");
    }
}
