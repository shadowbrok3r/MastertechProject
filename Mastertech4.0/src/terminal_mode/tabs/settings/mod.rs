use std::sync::{Arc, Mutex};

use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent},
    layout::{Constraint, Direction, Layout, Rect},
    prelude::Backend,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Widget, WidgetRef},
    Frame,
};

use crate::terminal_mode::{
    context::TerminalContext,
    events::action_handler::{ActionHandler, WidgetEvent, WidgetId},
    styling::{ThemeRole, TuiColorScheme, THEME},
    systems::notification_system::{Notification, NotificationType},
    widgets::{button::Button, ButtonType, HandleWidget, ShrinkArea, SHORTCUT_SET},
};
use database::schema::{RecordIdExt, User};
use displays::app_state::AppState;

/// Which pane keyboard navigation is acting on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Presets,
    Slots,
}

pub struct SettingsTab<'a> {
    presets: Vec<TuiColorScheme>,
    selected_preset: usize,
    /// Scheme being previewed/edited; applied live, saved on demand.
    working: TuiColorScheme,
    slot_idx: usize,
    focus: Focus,
    /// Hex buffer while a slot is being edited.
    hex_input: Option<String>,
    status: String,
    /// Id of the user whose saved scheme is currently loaded into `working`.
    synced_user: Option<String>,
    save_btn: Button<'a>,
    reset_btn: Button<'a>,
    ctx: Arc<Mutex<TerminalContext>>,
}

fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let t = s.trim().trim_start_matches('#');
    if t.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(t, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

fn to_hex(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

impl<'a> SettingsTab<'a> {
    pub fn new(ctx: Arc<Mutex<TerminalContext>>) -> Self {
        Self {
            presets: TuiColorScheme::presets(),
            selected_preset: 0,
            working: TuiColorScheme::default(),
            slot_idx: 0,
            focus: Focus::Presets,
            hex_input: None,
            status: String::new(),
            synced_user: None,
            save_btn: Button::new("Save Theme", WidgetId("SaveTuiTheme".to_owned())).theme(ThemeRole::Accent),
            reset_btn: Button::new("Reset", WidgetId("ResetTuiTheme".to_owned())).theme(ThemeRole::Neutral),
            ctx,
        }
    }

    /// Loads the saved scheme whenever the logged-in user changes.
    fn sync_from_user(&mut self) {
        let Ok(ctx) = self.ctx.try_lock() else {
            return;
        };
        let uid = ctx.user.get_id().key_string();
        if self.synced_user.as_deref() == Some(uid.as_str()) {
            return;
        }
        let bytes = ctx.user.get_tui_color_scheme();
        drop(ctx);
        match TuiColorScheme::decode(&bytes) {
            Some(scheme) => {
                self.selected_preset = self
                    .presets
                    .iter()
                    .position(|p| p.name == scheme.name)
                    .unwrap_or(0);
                self.working = scheme;
            }
            None => {
                self.working = TuiColorScheme::default();
                self.selected_preset = 0;
            }
        }
        self.synced_user = Some(uid);
    }

    fn apply_selected_preset(&mut self) {
        if let Some(preset) = self.presets.get(self.selected_preset) {
            self.working = preset.clone();
            self.working.apply();
            self.status = format!("Previewing '{}' — Save to keep it", self.working.name);
        }
    }

    fn commit_hex(&mut self) {
        let Some(buffer) = self.hex_input.take() else {
            return;
        };
        match parse_hex(&buffer) {
            Some(rgb) => {
                self.working.set_slot(self.slot_idx, rgb);
                self.working.name = "Custom".to_string();
                self.working.apply();
                self.status = "Color updated — Save to keep it".to_string();
            }
            None => self.status = format!("Invalid hex '{buffer}' (use #RRGGBB)"),
        }
    }

    fn reset(&mut self) {
        self.working = TuiColorScheme::default();
        self.selected_preset = 0;
        self.working.apply();
        self.status = "Reset to default — Save to keep it".to_string();
    }

    fn save(&mut self) {
        let bytes = self.working.encode();
        let name = self.working.name.clone();
        let mut data_tx = None;
        let mut authenticated = false;
        if let Ok(mut ctx) = self.ctx.lock() {
            authenticated = matches!(ctx.state, AppState::Authenticated(_));
            if authenticated {
                data_tx = Some(ctx.data_sender.clone());
                ctx.user.set_tui_color_scheme(bytes.clone());
            }
        }
        if !authenticated {
            self.status = "Log in to save your theme".to_string();
            return;
        }
        self.status = format!("Saving '{name}'...");
        tokio::spawn(async move {
            let res = User::update_tui_color_scheme(bytes.into()).await;
            if let Some(tx) = data_tx {
                let notification = match res {
                    Ok(_) => Notification::new(
                        NotificationType::Info,
                        "Theme Saved",
                        &format!("'{name}' saved to your profile"),
                        4,
                    ),
                    Err(e) => Notification::new(
                        NotificationType::Error,
                        "Theme Save Failed",
                        &format!("{e}"),
                        6,
                    ),
                };
                let _ = tx.send(Box::new(notification));
            }
        });
    }

    fn draw_presets(&self, f: &mut Frame, area: Rect) {
        let focused = self.focus == Focus::Presets;
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(SHORTCUT_SET)
            .border_style(THEME.border(focused))
            .title_style(THEME.title())
            .title("Presets");
        let inner = block.inner(area);
        (&block).render(area, f.buffer_mut());

        let items: Vec<ListItem> = self
            .presets
            .iter()
            .enumerate()
            .map(|(i, preset)| {
                let active = preset.name == self.working.name;
                let marker = if active { "● " } else { "  " };
                let mut style = Style::default().fg(if active { THEME.accent } else { THEME.text });
                if i == self.selected_preset && focused {
                    style = THEME.menu_highlight();
                }
                let swatch = Span::styled(
                    "██ ",
                    Style::default().fg(Color::Rgb(preset.accent[0], preset.accent[1], preset.accent[2])),
                );
                ListItem::new(Line::from(vec![
                    Span::styled(marker.to_string(), style),
                    swatch,
                    Span::styled(preset.name.clone(), style),
                ]))
            })
            .collect();
        Widget::render(List::new(items), inner, f.buffer_mut());
    }

    fn draw_slots(&self, f: &mut Frame, area: Rect) {
        let focused = self.focus == Focus::Slots;
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(SHORTCUT_SET)
            .border_style(THEME.border(focused))
            .title_style(THEME.title())
            .title(format!("Colors — {}", self.working.name));
        let inner = block.inner(area);
        (&block).render(area, f.buffer_mut());

        let items: Vec<ListItem> = self
            .working
            .slots()
            .iter()
            .enumerate()
            .map(|(i, (label, rgb))| {
                let selected = i == self.slot_idx && focused;
                let editing = selected && self.hex_input.is_some();
                let row_style = if selected {
                    THEME.menu_highlight()
                } else {
                    Style::default().fg(THEME.text)
                };
                let value = if editing {
                    format!("{}_", self.hex_input.clone().unwrap_or_default())
                } else {
                    to_hex(*rgb)
                };
                let value_style = if editing {
                    Style::default().fg(THEME.warning).add_modifier(Modifier::BOLD)
                } else {
                    row_style
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {label:<18}"), row_style),
                    Span::styled(format!("{value:<10}"), value_style),
                    Span::styled("██████", Style::default().fg(Color::Rgb(rgb[0], rgb[1], rgb[2]))),
                ]))
            })
            .collect();
        Widget::render(List::new(items), inner, f.buffer_mut());
    }
}

impl<'a> HandleWidget<'a> for SettingsTab<'a> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        self.sync_from_user();

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Fill(1), Constraint::Length(3), Constraint::Length(1)])
            .split(area);

        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(30), Constraint::Fill(1)])
            .split(rows[0]);

        self.draw_presets(f, panes[0]);
        self.draw_slots(f, panes[1]);

        let footer = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Fill(1), Constraint::Length(14), Constraint::Length(18)])
            .split(rows[1]);

        let status = Paragraph::new(Line::styled(
            format!(" {}", self.status),
            Style::default().fg(THEME.text_muted),
        ));
        status.render(footer[0], f.buffer_mut());
        self.reset_btn.render_ref(footer[1].shrink(1, 0), f.buffer_mut());
        self.save_btn.render_ref(footer[2].shrink(1, 0), f.buffer_mut());

        let help = Paragraph::new(Line::styled(
            " Tab: switch pane · ↑/↓: navigate · Enter: apply preset / edit color · Esc: cancel edit · Ctrl+S: save",
            Style::default().fg(THEME.text_muted),
        ));
        help.render(rows[2], f.buffer_mut());
    }

    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        self.save_btn.handle_mouse_event(mouse_event);
        self.reset_btn.handle_mouse_event(mouse_event);
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> bool {
        // Hex edit mode captures all keys until commit/cancel.
        if self.hex_input.is_some() {
            match key_event.code {
                KeyCode::Esc => {
                    self.hex_input = None;
                    self.status.clear();
                }
                KeyCode::Enter => self.commit_hex(),
                KeyCode::Backspace => {
                    if let Some(buf) = self.hex_input.as_mut() {
                        buf.pop();
                    }
                }
                KeyCode::Char(c) if c.is_ascii_hexdigit() || c == '#' => {
                    if let Some(buf) = self.hex_input.as_mut() {
                        if buf.len() < 7 {
                            buf.push(c.to_ascii_uppercase());
                        }
                    }
                }
                _ => {}
            }
            return true;
        }

        if key_event.modifiers.contains(KeyModifiers::CONTROL) {
            if let KeyCode::Char('s') = key_event.code {
                self.save();
                return true;
            }
            return false;
        }

        match key_event.code {
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Presets => Focus::Slots,
                    Focus::Slots => Focus::Presets,
                };
                true
            }
            KeyCode::Up => {
                match self.focus {
                    Focus::Presets => {
                        self.selected_preset = self.selected_preset.checked_sub(1).unwrap_or(self.presets.len() - 1);
                    }
                    Focus::Slots => {
                        self.slot_idx = self.slot_idx.checked_sub(1).unwrap_or(self.working.slots().len() - 1);
                    }
                }
                true
            }
            KeyCode::Down => {
                match self.focus {
                    Focus::Presets => {
                        self.selected_preset = (self.selected_preset + 1) % self.presets.len();
                    }
                    Focus::Slots => {
                        self.slot_idx = (self.slot_idx + 1) % self.working.slots().len();
                    }
                }
                true
            }
            KeyCode::Enter => {
                match self.focus {
                    Focus::Presets => self.apply_selected_preset(),
                    // Buffer starts empty so typing a new hex value works immediately.
                    Focus::Slots => self.hex_input = Some(String::new()),
                }
                true
            }
            _ => false,
        }
    }
}

impl<'a> ActionHandler for SettingsTab<'a> {
    fn widget_id(&self) -> WidgetId {
        WidgetId("SettingsTab".to_string())
    }

    fn managed_widget_ids(&self) -> Vec<WidgetId> {
        vec![
            WidgetId("SaveTuiTheme".to_string()),
            WidgetId("ResetTuiTheme".to_string()),
        ]
    }

    fn handle_event(&mut self, event: &WidgetEvent) {
        if let WidgetEvent::ButtonClick { widget_id, .. } = event {
            match widget_id.0.as_str() {
                "SaveTuiTheme" => self.save(),
                "ResetTuiTheme" => self.reset(),
                _ => {}
            }
        }
    }
}
