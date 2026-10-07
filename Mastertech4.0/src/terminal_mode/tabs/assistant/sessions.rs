//! The session list over the transcript: this machine's agent sessions, the technician's own, or everyone's for Root.

use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use crossbeam::channel::{Receiver, Sender, unbounded};
use database::schema::{AgentThread, ApprovalViewer, RecordId, RecordIdExt};
use displays::{PlatformSpawner, Spawner};
use ratatui::{
    Frame,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    layout::{Alignment, Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use super::{short_key, status_word, wrap};
use crate::terminal_mode::styling::{THEME, glyphs};

/// Most sessions one load lists.
const LIMIT: usize = 200;
/// How often an open list reloads.
const REFRESH: Duration = Duration::from_secs(10);
/// Widest the list grows on a wide pane.
const MAX_WIDTH: u16 = 120;
/// Cells of the status column.
const STATUS_W: usize = 14;
/// Cells of the age column.
const AGE_W: usize = 4;
/// Cells of the requester or host column.
const WHO_W: usize = 16;
/// Rows a wheel notch moves the selection.
const WHEEL_STEP: usize = 3;

/// Which sessions the list holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Machine,
    Mine,
    Everyone,
}

impl Scope {
    fn label(self) -> &'static str {
        match self {
            Self::Machine => "This machine",
            Self::Mine => "Mine",
            Self::Everyone => "Everyone",
        }
    }

    /// Scopes the viewer may list: everyone's only for Root.
    fn available(root: bool) -> &'static [Scope] {
        if root {
            &[Self::Machine, Self::Mine, Self::Everyone]
        } else {
            &[Self::Machine, Self::Mine]
        }
    }

    fn step(self, root: bool, forward: bool) -> Self {
        let all = Self::available(root);
        let at = all.iter().position(|s| *s == self).unwrap_or(0);
        let next = if forward {
            (at + 1) % all.len()
        } else {
            (at + all.len() - 1) % all.len()
        };
        all[next]
    }
}

/// What the technician picked.
#[derive(Clone, Debug, PartialEq)]
pub enum Pick {
    Open(Box<AgentThread>),
    /// Back to following this machine's newest session.
    Follow,
}

/// One line of the list.
enum Entry<'a> {
    Follow,
    Thread(&'a AgentThread),
}

type Loaded = (Scope, Result<Vec<AgentThread>, String>);

pub struct SessionPicker {
    open: bool,
    scope: Scope,
    query: String,
    rows: Vec<AgentThread>,
    selected: usize,
    offset: usize,
    /// Rows the list showed at the last redraw.
    page: usize,
    loading: bool,
    loaded_at: Option<Instant>,
    error: Option<String>,
    /// Selected once the rows holding it arrive.
    want: Option<RecordId>,
    picked: Option<Pick>,
    connection_string: String,
    viewer: Option<ApprovalViewer>,
    email: Option<String>,
    area: Rect,
    list: Rect,
    tabs: Vec<(Scope, Rect)>,
    tx: Sender<Loaded>,
    rx: Receiver<Loaded>,
}

impl SessionPicker {
    pub fn new(connection_string: String) -> Self {
        let (tx, rx) = unbounded();
        Self {
            open: false,
            scope: Scope::Machine,
            query: String::new(),
            rows: Vec::new(),
            selected: 0,
            offset: 0,
            page: 10,
            loading: false,
            loaded_at: None,
            error: None,
            want: None,
            picked: None,
            connection_string,
            viewer: None,
            email: None,
            area: Rect::default(),
            list: Rect::default(),
            tabs: Vec::new(),
            tx,
            rx,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the list and loads it, selecting `shown` once it arrives.
    pub fn open(
        &mut self,
        viewer: Option<ApprovalViewer>,
        email: Option<String>,
        shown: Option<RecordId>,
    ) {
        if !viewer.as_ref().is_some_and(|v| v.root) && self.scope == Scope::Everyone {
            self.scope = Scope::Machine;
        }
        self.viewer = viewer;
        self.email = email;
        self.open = true;
        self.query.clear();
        self.error = None;
        self.want = shown;
        self.select_wanted();
        self.loading = false;
        self.load();
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn take_pick(&mut self) -> Option<Pick> {
        self.picked.take()
    }

    /// The overlay's area at the last redraw.
    pub fn area(&self) -> Rect {
        self.area
    }

    fn root(&self) -> bool {
        self.viewer.as_ref().is_some_and(|v| v.root)
    }

    /// Takes finished loads and reloads an open list every [`REFRESH`].
    pub fn tick(&mut self) {
        while let Ok((scope, result)) = self.rx.try_recv() {
            if scope != self.scope {
                continue;
            }
            self.loading = false;
            match result {
                Ok(rows) => {
                    self.rows = rows;
                    self.error = None;
                    self.select_wanted();
                    self.select(self.selected);
                }
                Err(e) => self.error = Some(e),
            }
        }
        if self.open && !self.loading && self.loaded_at.is_none_or(|t| t.elapsed() >= REFRESH) {
            self.load();
        }
    }

    fn load(&mut self) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.loaded_at = Some(Instant::now());
        let scope = self.scope;
        let cs = self.connection_string.clone();
        let me = self.viewer.as_ref().map(|v| v.id.clone());
        let email = self.email.clone();
        let tx = self.tx.clone();
        PlatformSpawner::spawn(async move {
            let rows = match scope {
                Scope::Machine => AgentThread::list_for_connection(&cs, LIMIT).await,
                Scope::Mine => AgentThread::list_recent(LIMIT, true).await.map(|rows| {
                    rows.into_iter()
                        .filter(|t| is_mine(t, me.as_ref(), email.as_deref()))
                        .collect()
                }),
                Scope::Everyone => AgentThread::list_recent(LIMIT, true).await,
            };
            let _ = tx.send((scope, rows.map_err(|e| e.to_string())));
        });
    }

    fn set_scope(&mut self, scope: Scope) {
        if scope == self.scope {
            return;
        }
        self.scope = scope;
        self.rows.clear();
        self.selected = 0;
        self.offset = 0;
        self.loading = false;
        self.load();
    }

    fn entries(&self) -> Vec<Entry<'_>> {
        let q = self.query.to_lowercase();
        let follow = (self.scope == Scope::Machine && q.is_empty()).then_some(Entry::Follow);
        follow
            .into_iter()
            .chain(
                self.rows
                    .iter()
                    .filter(|t| matches(t, &q))
                    .map(Entry::Thread),
            )
            .collect()
    }

    fn select_wanted(&mut self) {
        let Some(want) = self.want.clone() else {
            return;
        };
        let at = self
            .entries()
            .iter()
            .position(|e| matches!(e, Entry::Thread(t) if t.id == want));
        if let Some(at) = at {
            self.want = None;
            self.selected = at;
            self.offset = at.saturating_sub(self.page / 2);
        }
    }

    /// Moves the selection to `i`, kept in the list and in view.
    fn select(&mut self, i: usize) {
        let count = self.entries().len();
        self.selected = i.min(count.saturating_sub(1));
        let page = self.page.max(1);
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + page {
            self.offset = self.selected + 1 - page;
        }
        self.offset = self.offset.min(count.saturating_sub(page));
    }

    fn pick_selected(&mut self) {
        let pick = match self.entries().get(self.selected) {
            Some(Entry::Follow) => Pick::Follow,
            Some(Entry::Thread(t)) => Pick::Open(Box::new((*t).clone())),
            None => return,
        };
        self.picked = Some(pick);
        self.open = false;
    }

    fn edit_query(&mut self, edit: impl FnOnce(&mut String)) {
        edit(&mut self.query);
        self.selected = 0;
        self.offset = 0;
    }

    /// Takes every key while the list is open.
    pub fn handle_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let page = self.page.max(1);
        match key.code {
            KeyCode::Esc if !self.query.is_empty() => self.edit_query(String::clear),
            KeyCode::Esc => self.close(),
            KeyCode::Char('s') if ctrl => self.close(),
            KeyCode::Char('r') if ctrl => {
                self.loading = false;
                self.load();
            }
            KeyCode::Enter => self.pick_selected(),
            KeyCode::Up => self.select(self.selected.saturating_sub(1)),
            KeyCode::Down => self.select(self.selected + 1),
            KeyCode::PageUp => self.select(self.selected.saturating_sub(page)),
            KeyCode::PageDown => self.select(self.selected + page),
            KeyCode::Home => self.select(0),
            KeyCode::End => self.select(usize::MAX),
            KeyCode::Tab => self.set_scope(self.scope.step(self.root(), true)),
            KeyCode::BackTab => self.set_scope(self.scope.step(self.root(), false)),
            KeyCode::Backspace => self.edit_query(|q| {
                q.pop();
            }),
            KeyCode::Char(c) if !ctrl && !alt => self.edit_query(|q| q.push(c)),
            _ => {}
        }
    }

    /// Takes every mouse event while the list is open; a click outside closes it.
    pub fn handle_mouse(&mut self, mouse: &MouseEvent) {
        let pos = Position::new(mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(scope) = self
                    .tabs
                    .iter()
                    .find(|(_, r)| r.contains(pos))
                    .map(|(s, _)| *s)
                {
                    self.set_scope(scope);
                } else if self.list.contains(pos) {
                    let i = self.offset + usize::from(mouse.row - self.list.y);
                    if i < self.entries().len() {
                        if i == self.selected {
                            self.pick_selected();
                        } else {
                            self.select(i);
                        }
                    }
                } else if !self.area.contains(pos) {
                    self.close();
                }
            }
            MouseEventKind::ScrollUp if self.area.contains(pos) => {
                self.select(self.selected.saturating_sub(WHEEL_STEP))
            }
            MouseEventKind::ScrollDown if self.area.contains(pos) => {
                self.select(self.selected + WHEEL_STEP)
            }
            _ => {}
        }
    }

    /// Draws the list centred in `within`; `shown` is the session on screen and `following` whether it follows the machine.
    pub fn draw(
        &mut self,
        f: &mut Frame,
        within: Rect,
        shown: Option<&RecordId>,
        following: bool,
        spinner: &str,
    ) {
        let width = within
            .width
            .saturating_sub(4)
            .min(MAX_WIDTH)
            .max(within.width.min(30));
        let height = within.height.saturating_sub(2).max(within.height.min(8));
        let area = Rect {
            x: within.x + (within.width - width) / 2,
            y: within.y + (within.height - height) / 2,
            width,
            height,
        };
        self.area = area;
        f.render_widget(Clear, area);
        let count = self.rows.len();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(THEME.accent))
            .title(Line::styled(" Sessions ", THEME.title()))
            .title(
                Line::styled(" Ctrl+S close ", Style::default().fg(THEME.text_muted))
                    .right_aligned(),
            )
            .style(Style::default().bg(THEME.bg));
        let inner = block.inner(area);
        f.render_widget(block, area);
        let [tabs, filter, list, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(inner);
        self.list = list;
        self.page = usize::from(list.height).max(1);

        self.draw_tabs(f, tabs);

        let muted = Style::default().fg(THEME.text_muted);
        let mut field = vec![
            Span::styled(format!("Filter {} ", glyphs::ROW_CLOSED), muted),
            Span::styled(self.query.clone(), Style::default().fg(THEME.text)),
            Span::styled(glyphs::CARET, Style::default().fg(THEME.accent)),
        ];
        if self.query.is_empty() {
            field.push(Span::styled(
                " title, host, service number, technician, status",
                Style::default().fg(THEME.overlay),
            ));
        }
        let entries = self.entries();
        let shown_count = entries
            .iter()
            .filter(|e| matches!(e, Entry::Thread(_)))
            .count();
        let tally = if self.loading && count == 0 {
            format!("{spinner} loading")
        } else if self.query.is_empty() {
            format!("{count} sessions")
        } else {
            format!("{shown_count} of {count}")
        };
        let tally_w = wrap::width(&tally) as u16 + 1;
        let [field_area, tally_area] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(tally_w)]).areas(filter);
        f.render_widget(
            Paragraph::new(wrap::fit(field, usize::from(field_area.width))),
            field_area,
        );
        f.render_widget(
            Paragraph::new(Line::styled(tally, muted)).alignment(Alignment::Right),
            tally_area,
        );

        let now = Utc::now();
        let mut lines = Vec::new();
        for (i, entry) in entries.iter().enumerate().skip(self.offset).take(self.page) {
            let selected = i == self.selected;
            let line = match entry {
                Entry::Follow => follow_line(usize::from(list.width), following),
                Entry::Thread(t) => thread_line(
                    t,
                    usize::from(list.width),
                    self.scope,
                    shown == Some(&t.id),
                    spinner,
                    now,
                ),
            };
            lines.push(if selected {
                highlight(line, usize::from(list.width))
            } else {
                line
            });
        }
        if lines.is_empty() {
            let note = match &self.error {
                Some(e) => Line::styled(
                    wrap::clip(
                        &format!("Could not load sessions: {e}"),
                        usize::from(list.width),
                    ),
                    Style::default().fg(THEME.error),
                ),
                None if self.loading => Line::styled(
                    format!("{spinner} Loading sessions{}", glyphs::ELLIPSIS),
                    muted,
                ),
                None if !self.query.is_empty() => {
                    Line::styled("No session matches the filter.", muted)
                }
                None => Line::styled("No sessions yet.", muted),
            };
            lines.push(note);
        }
        f.render_widget(Paragraph::new(lines), list);

        let hints = format!(
            "{}{} move {d} Enter open {d} Tab scope {d} type to filter {d} Esc close",
            glyphs::ARROW_UP,
            glyphs::ARROW_DOWN,
            d = glyphs::DOT
        );
        f.render_widget(
            Paragraph::new(Line::styled(
                wrap::clip(&hints, usize::from(footer.width)),
                muted,
            )),
            footer,
        );
    }

    fn draw_tabs(&mut self, f: &mut Frame, area: Rect) {
        self.tabs.clear();
        let mut x = area.x;
        let mut spans = Vec::new();
        for &scope in Scope::available(self.root()) {
            let text = format!(" {} ", scope.label());
            let w = wrap::width(&text) as u16;
            if x + w > area.right() {
                break;
            }
            let style = if scope == self.scope {
                Style::default()
                    .fg(THEME.bg)
                    .bg(THEME.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(THEME.text_muted)
            };
            self.tabs.push((
                scope,
                Rect {
                    x,
                    y: area.y,
                    width: w,
                    height: 1,
                },
            ));
            spans.push(Span::styled(text, style));
            spans.push(Span::raw(" "));
            x += w + 1;
        }
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}

/// True when `t` was asked for by, or assigned to, the viewer.
fn is_mine(t: &AgentThread, me: Option<&RecordId>, email: Option<&str>) -> bool {
    me.is_some_and(|id| t.assignee.as_ref() == Some(id))
        || email.is_some_and(|e| {
            t.requested_by
                .as_deref()
                .is_some_and(|r| r.eq_ignore_ascii_case(e))
        })
}

/// True when any of the session's words contains `q`, already lower case.
fn matches(t: &AgentThread, q: &str) -> bool {
    if q.is_empty() {
        return true;
    }
    let key = t.id.key_string();
    [
        Some(t.label()),
        t.hostname.clone(),
        t.service_number.clone(),
        t.requested_by.clone(),
        Some(t.status.clone()),
        Some(status_word(&t.status).to_string()),
        Some(t.connection_string.clone()),
        Some(key),
    ]
    .into_iter()
    .flatten()
    .any(|w| w.to_lowercase().contains(q))
}

/// Status glyph and colour of a session.
fn status_mark(status: &str, spinner: &str) -> (String, Color) {
    match status {
        "running" | "starting" => (spinner.to_string(), THEME.accent),
        "queued" => ("\u{25cc}".to_string(), THEME.accent_soft),
        "waiting_approval" => (glyphs::Glyph::Warning.as_str().to_string(), THEME.warning),
        "idle" => (glyphs::DOT_OFF.to_string(), THEME.text_muted),
        "failed" => (glyphs::Glyph::Close.as_str().to_string(), THEME.error),
        _ => (glyphs::DOT.to_string(), THEME.overlay),
    }
}

/// `now`, `12m`, `5h` or `3d` since `at`.
fn age_label(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let secs = (now - at).num_seconds().max(0);
    match secs {
        0..60 => "now".into(),
        60..3_600 => format!("{}m", secs / 60),
        3_600..86_400 => format!("{}h", secs / 3_600),
        _ => format!("{}d", secs / 86_400),
    }
}

fn thread_line(
    t: &AgentThread,
    width: usize,
    scope: Scope,
    shown: bool,
    spinner: &str,
    now: DateTime<Utc>,
) -> Line<'static> {
    let (mark, mark_color) = status_mark(&t.status, spinner);
    let muted = Style::default().fg(THEME.text_muted);
    let at = t
        .last_event_at
        .or(t.updated_at)
        .or(t.created_at)
        .map(DateTime::<Utc>::from);
    let age = at.map(|a| age_label(a, now)).unwrap_or_default();
    let who = match scope {
        Scope::Machine => t
            .requested_by
            .as_deref()
            .map(|e| e.split('@').next().unwrap_or(e).to_string()),
        Scope::Mine | Scope::Everyone => t
            .hostname
            .clone()
            .or_else(|| Some(t.connection_string.clone())),
    }
    .unwrap_or_default();
    let mut right = Vec::new();
    if width >= 44 {
        right.push(Span::styled(
            format!("{:<STATUS_W$}", status_word(&t.status)),
            Style::default().fg(mark_color),
        ));
    }
    right.push(Span::styled(format!(" {age:>AGE_W$}"), muted));
    if width >= 70 {
        right.push(Span::styled(
            format!("  {:<WHO_W$}", wrap::clip(&who, WHO_W)),
            muted,
        ));
        right.push(Span::styled(
            format!(" {}", short_key(&t.id)),
            Style::default().fg(THEME.overlay),
        ));
    }
    let right_w = wrap::spans_width(&right);
    let title_w = width.saturating_sub(4 + right_w + 1);
    let mut title = t.label();
    if let Some(sn) = t
        .service_number
        .as_deref()
        .filter(|sn| !title.contains(*sn))
    {
        title = format!("#{sn} {title}");
    }
    let title_style = if shown {
        Style::default()
            .fg(THEME.accent_soft)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(THEME.text)
    };
    let title = wrap::clip(&wrap::one_line(&title), title_w);
    let pad = title_w.saturating_sub(wrap::width(&title));
    let mut spans = vec![
        Span::raw("  "),
        Span::styled(mark, Style::default().fg(mark_color)),
        Span::raw(" "),
        Span::styled(title, title_style),
        Span::raw(" ".repeat(pad + 1)),
    ];
    spans.extend(right);
    wrap::fit(spans, width)
}

fn follow_line(width: usize, following: bool) -> Line<'static> {
    let mut spans = vec![
        Span::raw("  "),
        Span::styled(glyphs::DOT_ON, Style::default().fg(THEME.tertiary)),
        Span::raw(" "),
        Span::styled(
            "Newest session on this machine",
            Style::default()
                .fg(THEME.tertiary)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if following {
        spans.push(Span::styled(
            "  following",
            Style::default().fg(THEME.text_muted),
        ));
    }
    wrap::fit(spans, width)
}

/// `line` on the selection band with the selection marker in its first cell.
fn highlight(line: Line<'static>, width: usize) -> Line<'static> {
    let mut spans = line.spans;
    if let Some(first) = spans.first_mut() {
        *first = Span::styled(
            format!("{} ", glyphs::SELECTED),
            Style::default().fg(THEME.accent),
        );
    }
    let line = Line::from(
        spans
            .into_iter()
            .map(|s| Span::styled(s.content, s.style.add_modifier(Modifier::BOLD)))
            .collect::<Vec<_>>(),
    );
    wrap::on_bg(line, width, THEME.surface)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(key: &str, status: &str, title: &str) -> AgentThread {
        AgentThread {
            id: RecordId::new("agent_thread", key),
            status: status.into(),
            connection_string: "PC-1:abc".into(),
            hostname: None,
            service_number: None,
            store: None,
            requested_by: None,
            assignee: None,
            assist_request: None,
            service_order: None,
            computer: None,
            customer: None,
            diagnostic_session: None,
            codex_thread_id: None,
            tools_hash: None,
            model: None,
            provider: None,
            driven_by: None,
            tool_path: None,
            title: Some(title.into()),
            error: None,
            broker_node: None,
            allow_box_shell: false,
            approve_all: None,
            tokens_used: None,
            tokens_window: None,
            activity: None,
            last_seq: None,
            created_at: None,
            updated_at: None,
            last_event_at: None,
            closed_at: None,
        }
    }

    fn text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn picker(rows: Vec<AgentThread>) -> SessionPicker {
        let mut p = SessionPicker::new("PC-1:abc".into());
        p.rows = rows;
        p.open = true;
        p.page = 3;
        p
    }

    #[test]
    fn the_filter_matches_any_word_and_keeps_the_follow_row_only_unfiltered() {
        let mut p = picker(vec![
            thread("a1", "idle", "Fans loud"),
            thread("b2", "closed", "Blue screen"),
        ]);
        assert_eq!(p.entries().len(), 3);
        for c in "blue".chars() {
            p.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        let left: Vec<String> = p
            .entries()
            .iter()
            .filter_map(|e| match e {
                Entry::Thread(t) => Some(t.label()),
                Entry::Follow => None,
            })
            .collect();
        assert_eq!(left, vec!["Blue screen"]);
        assert_eq!(p.entries().len(), 1, "the follow row hides while filtering");
        p.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(
            p.query.is_empty() && p.is_open(),
            "Esc clears the filter first"
        );
        p.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!p.is_open());
    }

    #[test]
    fn enter_picks_the_selection_and_the_list_scrolls_with_it() {
        let rows: Vec<_> = (0..8)
            .map(|i| thread(&format!("t{i}"), "idle", &format!("session {i}")))
            .collect();
        let mut p = picker(rows);
        p.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(p.take_pick(), Some(Pick::Follow));
        p.open = true;
        for _ in 0..5 {
            p.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        assert_eq!((p.selected, p.offset), (5, 3));
        p.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        match p.take_pick() {
            Some(Pick::Open(t)) => assert_eq!(t.label(), "session 4"),
            other => panic!("{other:?}"),
        }
        assert!(!p.is_open());
        p.open = true;
        p.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        assert_eq!(p.selected, 8);
    }

    #[test]
    fn the_shown_session_is_selected_when_the_rows_arrive() {
        let mut p = picker(Vec::new());
        p.want = Some(RecordId::new("agent_thread", "t2"));
        p.rows = (0..4)
            .map(|i| thread(&format!("t{i}"), "idle", "x"))
            .collect();
        p.select_wanted();
        assert_eq!(p.selected, 3, "after the follow row and t0, t1");
        assert!(p.want.is_none());
    }

    #[test]
    fn scopes_cycle_through_what_the_viewer_may_list() {
        assert_eq!(Scope::Machine.step(false, true), Scope::Mine);
        assert_eq!(Scope::Mine.step(false, true), Scope::Machine);
        assert_eq!(Scope::Mine.step(true, true), Scope::Everyone);
        assert_eq!(Scope::Machine.step(true, false), Scope::Everyone);
        let me = RecordId::new("user", "tech");
        let mut t = thread("x", "idle", "x");
        assert!(!is_mine(&t, Some(&me), Some("tech@pcl.com")));
        t.requested_by = Some("Tech@PCL.com".into());
        assert!(is_mine(&t, None, Some("tech@pcl.com")));
        t.requested_by = None;
        t.assignee = Some(me.clone());
        assert!(is_mine(&t, Some(&me), None));
    }

    #[test]
    fn rows_fit_their_width_and_drop_columns_when_narrow() {
        let mut t = thread(
            "abcdef123456",
            "waiting_approval",
            &"a long title ".repeat(10),
        );
        t.requested_by = Some("tech@pcl.com".into());
        t.service_number = Some("2155485".into());
        let now = Utc::now();
        for width in [20, 44, 70, 120] {
            let line = thread_line(&t, width, Scope::Machine, false, "\u{25d0}", now);
            assert!(
                wrap::spans_width(&line.spans) <= width,
                "{width}: {}",
                text(&line)
            );
        }
        let wide = text(&thread_line(
            &t,
            120,
            Scope::Machine,
            false,
            "\u{25d0}",
            now,
        ));
        assert!(
            wide.contains("needs approval") && wide.contains("tech") && wide.contains("abcdef12"),
            "{wide}"
        );
        assert!(wide.contains("#2155485"), "{wide}");
        let narrow = text(&thread_line(&t, 30, Scope::Machine, false, "\u{25d0}", now));
        assert!(!narrow.contains("needs approval"), "{narrow}");
        assert_eq!(age_label(now - chrono::Duration::minutes(12), now), "12m");
        assert_eq!(age_label(now - chrono::Duration::days(3), now), "3d");
    }
}
