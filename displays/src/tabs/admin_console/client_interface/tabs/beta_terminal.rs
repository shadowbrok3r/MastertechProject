use crate::remote_viewer::ratagui::{RataguiBackend, TerminalEvent};
use crate::tabs::admin_console::client_interface::tabs::command_shell::History;
use crate::ui_tools::tui_theme::{CATPPUCCIN, THEME};
use crossbeam::channel::{unbounded, Receiver, Sender};
use eframe::egui::{EventFilter, Frame, Id, Margin, Stroke, Ui};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Terminal,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use web_time::Instant;

static NEXT_BETA_TERMINAL_NONCE: AtomicUsize = AtomicUsize::new(0);

pub struct BetaTerminal {
    terminal: Terminal<RataguiBackend>,
    backend_event_rx: Receiver<TerminalEvent>,
    _backend_event_tx: Sender<TerminalEvent>,
    scrollback: Vec<Line<'static>>,
    last_history_idx: usize,
    input: String,
    cursor: usize,
    cmd_history: Vec<String>,
    cmd_history_idx: Option<usize>,
    scroll_offset: u16,
    blink_anchor: Instant,
    banner_shown: bool,
    user: String,
    host: String,
    ip: String,
    cwd: String,
    completions: Vec<String>,
    completion_idx: Option<usize>,
    apply_first_on_arrival: bool,
    focus_id: Id,
    want_focus: bool,
}

pub enum BetaTerminalAction {
    Send(String),
    SendInteractive(String),
    Quit,
    RequestCompletion(String),
}

impl BetaTerminal {
    pub fn new() -> Self {
        let (tx, rx) = unbounded();
        let backend = RataguiBackend::new(140, 38, tx.clone());
        let terminal = Terminal::new(backend).expect("ratatui terminal init");
        let nonce = NEXT_BETA_TERMINAL_NONCE.fetch_add(1, Ordering::Relaxed);
        Self {
            terminal,
            backend_event_rx: rx,
            _backend_event_tx: tx,
            scrollback: Vec::new(),
            last_history_idx: 0,
            input: String::new(),
            cursor: 0,
            cmd_history: Vec::new(),
            cmd_history_idx: None,
            scroll_offset: 0,
            blink_anchor: Instant::now(),
            banner_shown: false,
            user: default_user(),
            host: "mastertech".to_string(),
            ip: String::new(),
            cwd: "~".to_string(),
            completions: Vec::new(),
            completion_idx: None,
            apply_first_on_arrival: false,
            focus_id: Id::new(("beta_terminal_focus", nonce)),
            want_focus: false,
        }
    }

    pub fn set_session_info(&mut self, host: &str, ip: &str) {
        if !host.is_empty() {
            self.host = host.to_string();
        }
        if !ip.is_empty() {
            self.ip = ip.to_string();
        }
    }

    pub fn reset_history_cursor(&mut self, history_len: usize) {
        self.last_history_idx = history_len;
    }

    pub fn request_focus_next_frame(&mut self) {
        self.want_focus = true;
    }

    pub fn set_completions(&mut self, completions: Vec<String>) {
        if completions == self.completions {
            return;
        }
        self.completions = completions;
        self.completion_idx = None;
        if self.apply_first_on_arrival {
            if let Some(first) = self.completions.first().cloned() {
                self.apply_completion(&first);
                self.completion_idx = Some(0);
            }
            self.apply_first_on_arrival = false;
        }
    }

    pub fn sync_from_history(&mut self, history: &[History]) {
        if history.len() < self.last_history_idx {
            self.last_history_idx = history.len();
        }
        if history.len() == self.last_history_idx {
            return;
        }
        for entry in &history[self.last_history_idx..] {
            if entry.from == "You" {
                continue;
            }
            for raw in entry.message.split('\n') {
                self.scrollback.push(parse_ansi(raw, THEME.text_muted));
            }
        }
        self.last_history_idx = history.len();
        const MAX_LINES: usize = 4000;
        if self.scrollback.len() > MAX_LINES {
            let drop = self.scrollback.len() - MAX_LINES;
            self.scrollback.drain(0..drop);
        }
        self.scroll_offset = 0;
    }

    fn apply_completion(&mut self, completion: &str) {
        let ends_with_space = self.input.ends_with(char::is_whitespace);
        let trimmed = self.input.trim_end();
        let mut parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.is_empty() {
            self.input = completion.to_string();
        } else if parts.len() == 1 && !ends_with_space {
            self.input = completion.to_string();
        } else if ends_with_space {
            if !self.input.ends_with(' ') {
                self.input.push(' ');
            }
            self.input.push_str(completion);
        } else {
            if let Some(last) = parts.last_mut() {
                *last = completion;
            }
            let mut rebuilt = String::new();
            for (i, p) in parts.iter().enumerate() {
                if i > 0 {
                    rebuilt.push(' ');
                }
                rebuilt.push_str(p);
            }
            self.input = rebuilt;
        }
        self.cursor = self.input.len();
        self.blink_anchor = Instant::now();
    }

    fn echo_command(&mut self, command: &str) {
        let (top, bottom) = self.build_prompt_lines(Some(command));
        self.scrollback.push(top);
        self.scrollback.push(bottom);
    }

    fn build_prompt_lines(&self, command: Option<&str>) -> (Line<'static>, Line<'static>) {
        let mut top_spans: Vec<Span<'static>> = vec![
            Span::styled("┌─[", style(THEME.accent).add_modifier(Modifier::BOLD)),
            Span::styled(self.user.clone(), style(THEME.success).add_modifier(Modifier::BOLD)),
            Span::styled("@", style(THEME.accent)),
            Span::styled(self.host.clone(), style(THEME.accent_soft).add_modifier(Modifier::BOLD)),
        ];
        if !self.ip.is_empty() {
            top_spans.push(Span::styled(" - ", style(THEME.overlay)));
            top_spans.push(Span::styled(self.ip.clone(), style(THEME.tertiary)));
        }
        top_spans.push(Span::styled(
            "] - [",
            style(THEME.accent).add_modifier(Modifier::BOLD),
        ));
        top_spans.push(Span::styled(
            self.cwd.clone(),
            style(THEME.warning).add_modifier(Modifier::BOLD),
        ));
        top_spans.push(Span::styled(
            "]",
            style(THEME.accent).add_modifier(Modifier::BOLD),
        ));
        let top = Line::from(top_spans);

        let mut bottom_spans: Vec<Span<'static>> = vec![
            Span::styled("└─[", style(THEME.accent).add_modifier(Modifier::BOLD)),
            Span::styled("$", style(THEME.accent_soft).add_modifier(Modifier::BOLD)),
            Span::styled(
                "] ",
                style(THEME.accent).add_modifier(Modifier::BOLD),
            ),
        ];
        if let Some(cmd) = command {
            bottom_spans.push(Span::styled(
                cmd.to_string(),
                style(THEME.text).add_modifier(Modifier::BOLD),
            ));
        }
        let bottom = Line::from(bottom_spans);

        (top, bottom)
    }

    fn build_live_prompt(&self, cursor_on: bool) -> (Line<'static>, Line<'static>) {
        let (top, _) = self.build_prompt_lines(None);
        let safe_cursor = self.cursor.min(self.input.len());
        let (before, after) = self.input.split_at(safe_cursor);
        let mut after_chars = after.chars();
        let cursor_char = after_chars.next().unwrap_or(' ');
        let rest: String = after_chars.collect();
        let cursor_style = if cursor_on {
            Style::default()
                .bg(THEME.text)
                .fg(THEME.bg)
                .add_modifier(Modifier::BOLD)
        } else {
            style(THEME.text).add_modifier(Modifier::BOLD)
        };
        let bottom = Line::from(vec![
            Span::styled("└─[", style(THEME.accent).add_modifier(Modifier::BOLD)),
            Span::styled("$", style(THEME.accent_soft).add_modifier(Modifier::BOLD)),
            Span::styled(
                "] ",
                style(THEME.accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                before.to_string(),
                style(THEME.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled(cursor_char.to_string(), cursor_style),
            Span::styled(rest, style(THEME.text).add_modifier(Modifier::BOLD)),
        ]);
        (top, bottom)
    }

    pub fn ui(&mut self, ui: &mut Ui, interactive: bool) -> Option<BetaTerminalAction> {
        while self.backend_event_rx.try_recv().is_ok() {}

        if !self.banner_shown {
            self.banner_shown = true;
            self.scrollback.push(Line::from(vec![Span::styled(
                "MasterTech beta shell — Tab: completions · ↑/↓: history · Ctrl+L: clear · Ctrl+C: cancel · mouse wheel scrolls"
                    .to_string(),
                style(THEME.overlay).add_modifier(Modifier::ITALIC),
            )]));
            self.scrollback.push(Line::from(Span::raw(String::new())));
        }

        let ctx = ui.ctx().clone();
        // Register our synthetic focus id as a used widget for this frame so egui's
        // end-of-frame dead-man's switch doesn't drop our focus. `check_for_id_clash`
        // is the only public way to do this without also calling `interested_in_focus`
        // (which would surrender focus on Tab).
        let registration_rect = ui.available_rect_before_wrap();
        ctx.check_for_id_clash(self.focus_id, registration_rect, "beta_terminal_focus");

        if self.want_focus {
            ctx.memory_mut(|m| m.request_focus(self.focus_id));
            self.want_focus = false;
        }
        ctx.memory_mut(|m| {
            m.set_focus_lock_filter(
                self.focus_id,
                EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            );
        });

        let action = self.handle_input(ui, interactive);

        let cursor_on = self.blink_anchor.elapsed().as_millis() % 1000 < 500;
        let (prompt_top, prompt_bottom) = self.build_live_prompt(cursor_on);
        let scrollback = self.scrollback.clone();
        let scroll_offset = self.scroll_offset;

        let title = if self.ip.is_empty() {
            format!(" {} ", self.host)
        } else {
            format!(" {} - {} ", self.host, self.ip)
        };

        let _ = self.terminal.draw(|f| {
            let area = f.area();
            f.render_widget(Block::default().style(Style::default().bg(THEME.bg)), area);

            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(title)
                .title_style(THEME.title())
                .border_style(Style::new().fg(THEME.tertiary))
                .style(Style::new().bg(THEME.bg).fg(THEME.text));
            let body = block.inner(area);
            f.render_widget(block, area);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(2)])
                .split(body);

            let total = scrollback.len() as u16;
            let visible = chunks[0].height;
            let bottom_offset = total.saturating_sub(visible).saturating_sub(scroll_offset);
            let para = Paragraph::new(scrollback)
                .style(Style::default().fg(THEME.text_muted).bg(THEME.bg))
                .wrap(Wrap { trim: false })
                .scroll((bottom_offset, 0));
            f.render_widget(para, chunks[0]);

            let prompt = Paragraph::new(vec![prompt_top, prompt_bottom])
                .style(Style::default().bg(THEME.bg))
                .wrap(Wrap { trim: false });
            f.render_widget(prompt, chunks[1]);
        });

        // Chrome comes from the ratatui block; the egui frame only pads the grid.
        let inner = Frame::new()
            .fill(RataguiBackend::rat_to_egui_color(&THEME.bg, false))
            .stroke(Stroke::NONE)
            .inner_margin(Margin::same(4))
            .show(ui, |ui| {
                ui.add(self.terminal.backend_mut());
            });

        let resp = inner.response.interact(eframe::egui::Sense::click_and_drag());
        if resp.clicked() || resp.is_pointer_button_down_on() {
            ctx.memory_mut(|m| m.request_focus(self.focus_id));
        }

        if resp.hovered() {
            let scroll_y = ctx.input(|i| i.smooth_scroll_delta.y);
            if scroll_y.abs() > 0.5 {
                let lines = (scroll_y.abs() / 16.0).ceil() as u16;
                if scroll_y > 0.0 {
                    let total = self.scrollback.len() as u16;
                    self.scroll_offset =
                        self.scroll_offset.saturating_add(lines).min(total);
                } else {
                    self.scroll_offset = self.scroll_offset.saturating_sub(lines);
                }
            }
        }

        ctx.request_repaint_after(std::time::Duration::from_millis(120));

        action
    }

    fn handle_input(&mut self, ui: &mut Ui, interactive: bool) -> Option<BetaTerminalAction> {
        use eframe::egui::{Event, Key};
        let events: Vec<Event> = ui.input(|i| i.events.clone());
        let mut action: Option<BetaTerminalAction> = None;
        let mut absorb_keys = false;

        let cancel = |term: &mut BetaTerminal, interactive: bool| -> Option<BetaTerminalAction> {
            term.scrollback.push(Line::from(vec![Span::styled(
                "^C".to_string(),
                style(THEME.error).add_modifier(Modifier::BOLD),
            )]));
            term.input.clear();
            term.cursor = 0;
            term.completions.clear();
            term.completion_idx = None;
            if interactive {
                Some(BetaTerminalAction::Quit)
            } else {
                None
            }
        };

        for event in &events {
            match event {
                Event::Copy | Event::Cut => {
                    absorb_keys = true;
                    if let Some(a) = cancel(self, interactive) {
                        action = Some(a);
                    }
                }
                Event::Paste(text) => {
                    absorb_keys = true;
                    let clean: String = text.chars().filter(|c| *c != '\r' && !c.is_control()).collect();
                    if !clean.is_empty() {
                        self.input.insert_str(self.cursor, &clean);
                        self.cursor += clean.len();
                        self.blink_anchor = Instant::now();
                        self.scroll_offset = 0;
                    }
                }
                Event::Text(text) => {
                    absorb_keys = true;
                    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
                    if !clean.is_empty() {
                        self.input.insert_str(self.cursor, &clean);
                        self.cursor += clean.len();
                        self.blink_anchor = Instant::now();
                        self.scroll_offset = 0;
                        self.completions.clear();
                        self.completion_idx = None;
                    }
                }
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    absorb_keys = true;
                    match key {
                        Key::Enter => {
                            let line = std::mem::take(&mut self.input);
                            self.cursor = 0;
                            self.cmd_history_idx = None;
                            self.scroll_offset = 0;
                            self.completions.clear();
                            self.completion_idx = None;
                            self.apply_first_on_arrival = false;
                            if !line.is_empty()
                                && self.cmd_history.last().map(String::as_str) != Some(line.as_str())
                            {
                                self.cmd_history.push(line.clone());
                            }
                            self.echo_command(&line);
                            if !line.is_empty() {
                                self.update_cwd(&line);
                            }
                            action = Some(if interactive {
                                BetaTerminalAction::SendInteractive(line)
                            } else {
                                BetaTerminalAction::Send(line)
                            });
                        }
                        Key::Tab => {
                            if !self.completions.is_empty() {
                                let i = match self.completion_idx {
                                    None => 0,
                                    Some(i) => (i + 1) % self.completions.len(),
                                };
                                self.completion_idx = Some(i);
                                let c = self.completions[i].clone();
                                self.apply_completion(&c);
                            } else if !self.input.is_empty() {
                                self.apply_first_on_arrival = true;
                                action = Some(BetaTerminalAction::RequestCompletion(
                                    self.input.clone(),
                                ));
                            }
                        }
                        Key::Backspace => {
                            if self.cursor > 0 {
                                let prev = prev_char_boundary(&self.input, self.cursor);
                                self.input.replace_range(prev..self.cursor, "");
                                self.cursor = prev;
                                self.completions.clear();
                                self.completion_idx = None;
                            }
                        }
                        Key::Delete => {
                            if self.cursor < self.input.len() {
                                let next = next_char_boundary(&self.input, self.cursor);
                                self.input.replace_range(self.cursor..next, "");
                                self.completions.clear();
                                self.completion_idx = None;
                            }
                        }
                        Key::ArrowLeft => {
                            self.cursor = prev_char_boundary(&self.input, self.cursor);
                        }
                        Key::ArrowRight => {
                            self.cursor = next_char_boundary(&self.input, self.cursor);
                        }
                        Key::Home => self.cursor = 0,
                        Key::End => self.cursor = self.input.len(),
                        Key::ArrowUp => {
                            if !self.cmd_history.is_empty() {
                                let next = match self.cmd_history_idx {
                                    None => Some(self.cmd_history.len() - 1),
                                    Some(i) if i > 0 => Some(i - 1),
                                    Some(i) => Some(i),
                                };
                                if let Some(i) = next {
                                    self.cmd_history_idx = Some(i);
                                    self.input = self.cmd_history[i].clone();
                                    self.cursor = self.input.len();
                                }
                            }
                        }
                        Key::ArrowDown => {
                            if let Some(i) = self.cmd_history_idx {
                                if i + 1 < self.cmd_history.len() {
                                    self.cmd_history_idx = Some(i + 1);
                                    self.input = self.cmd_history[i + 1].clone();
                                    self.cursor = self.input.len();
                                } else {
                                    self.cmd_history_idx = None;
                                    self.input.clear();
                                    self.cursor = 0;
                                }
                            }
                        }
                        Key::C if modifiers.ctrl => {
                            if let Some(a) = cancel(self, interactive) {
                                action = Some(a);
                            }
                        }
                        Key::L if modifiers.ctrl => {
                            self.scrollback.clear();
                            self.scroll_offset = 0;
                        }
                        Key::U if modifiers.ctrl => {
                            self.input.clear();
                            self.cursor = 0;
                            self.completions.clear();
                            self.completion_idx = None;
                        }
                        Key::PageUp => {
                            self.scroll_offset = self.scroll_offset.saturating_add(5);
                        }
                        Key::PageDown => {
                            self.scroll_offset = self.scroll_offset.saturating_sub(5);
                        }
                        Key::Escape => {
                            self.completions.clear();
                            self.completion_idx = None;
                            self.apply_first_on_arrival = false;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        if self.cursor > self.input.len() {
            self.cursor = self.input.len();
        }

        if absorb_keys {
            ui.input_mut(|i| {
                i.events.retain(|e| {
                    !matches!(
                        e,
                        Event::Key { .. }
                            | Event::Text(_)
                            | Event::Copy
                            | Event::Cut
                            | Event::Paste(_)
                    )
                });
            });
        }

        action
    }

    fn update_cwd(&mut self, command: &str) {
        let trimmed = command.trim();
        let rest = if trimmed == "cd" {
            ""
        } else if let Some(r) = trimmed.strip_prefix("cd ") {
            r.trim()
        } else if let Some(r) = trimmed.strip_prefix("cd\t") {
            r.trim()
        } else {
            return;
        };
        if rest.is_empty() || rest == "~" {
            self.cwd = "~".to_string();
        } else if rest == ".." {
            self.cwd_up();
        } else if rest == "." {
            return;
        } else if rest.starts_with('/') || rest.starts_with('~') {
            self.cwd = rest.to_string();
        } else if self.cwd.ends_with('/') {
            self.cwd.push_str(rest);
        } else {
            self.cwd.push('/');
            self.cwd.push_str(rest);
        }
    }

    fn cwd_up(&mut self) {
        if self.cwd == "~" || self.cwd == "/" {
            return;
        }
        if let Some(idx) = self.cwd.rfind('/') {
            if idx == 0 {
                self.cwd = "/".to_string();
            } else {
                self.cwd.truncate(idx);
            }
        } else {
            self.cwd = "~".to_string();
        }
    }
}

fn style(c: Color) -> Style {
    Style::default().fg(c)
}

fn prev_char_boundary(s: &str, idx: usize) -> usize {
    if idx == 0 {
        return 0;
    }
    let mut i = idx - 1;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn next_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    let mut i = idx + 1;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

fn default_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "operator".to_string())
}

fn parse_ansi(input: &str, default_fg: Color) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut buf = String::new();
    let mut current = Style::default().fg(default_fg);
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            let mut code = String::new();
            while let Some(&n) = chars.peek() {
                if n.is_ascii_digit() || n == ';' {
                    code.push(n);
                    chars.next();
                } else {
                    break;
                }
            }
            let final_byte = chars.next();
            if final_byte == Some('m') {
                if !buf.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut buf), current));
                }
                current = apply_sgr(current, &code, default_fg);
            }
            continue;
        }
        if c == '\r' {
            continue;
        }
        if c.is_control() {
            continue;
        }
        buf.push(c);
    }
    if !buf.is_empty() {
        spans.push(Span::styled(buf, current));
    }
    if spans.is_empty() {
        spans.push(Span::raw(String::new()));
    }
    Line::from(spans)
}

fn apply_sgr(mut s: Style, code: &str, default_fg: Color) -> Style {
    let mut it = code
        .split(';')
        .filter_map(|v| v.parse::<u32>().ok())
        .peekable();
    while let Some(n) = it.next() {
        match n {
            0 => s = Style::default().fg(default_fg),
            1 => s = s.add_modifier(Modifier::BOLD),
            2 => s = s.add_modifier(Modifier::DIM),
            3 => s = s.add_modifier(Modifier::ITALIC),
            4 => s = s.add_modifier(Modifier::UNDERLINED),
            7 => s = s.add_modifier(Modifier::REVERSED),
            22 => s = s.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => s = s.remove_modifier(Modifier::ITALIC),
            24 => s = s.remove_modifier(Modifier::UNDERLINED),
            27 => s = s.remove_modifier(Modifier::REVERSED),
            30..=37 => s = s.fg(ansi_basic(n - 30)),
            90..=97 => s = s.fg(ansi_basic(n - 90)),
            40..=47 => s = s.bg(ansi_basic(n - 40)),
            100..=107 => s = s.bg(ansi_basic(n - 100)),
            38 | 48 => {
                let is_fg = n == 38;
                match it.next() {
                    Some(5) => {
                        if let Some(idx) = it.next() {
                            let c = Color::Indexed(idx as u8);
                            s = if is_fg { s.fg(c) } else { s.bg(c) };
                        }
                    }
                    Some(2) => {
                        let r = it.next().unwrap_or(0);
                        let g = it.next().unwrap_or(0);
                        let b = it.next().unwrap_or(0);
                        let c = Color::Rgb(r as u8, g as u8, b as u8);
                        s = if is_fg { s.fg(c) } else { s.bg(c) };
                    }
                    _ => {}
                }
            }
            39 => s = s.fg(default_fg),
            49 => s = s.bg(THEME.bg),
            _ => {}
        }
    }
    s
}

fn ansi_basic(n: u32) -> Color {
    match n {
        0 => CATPPUCCIN.surface1,
        1 => CATPPUCCIN.red,
        2 => CATPPUCCIN.green,
        3 => CATPPUCCIN.yellow,
        4 => CATPPUCCIN.blue,
        5 => CATPPUCCIN.pink,
        6 => CATPPUCCIN.teal,
        _ => CATPPUCCIN.text,
    }
}
