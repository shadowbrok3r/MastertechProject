use std::cell::RefCell;
use std::path::Path;

use mtech_tui::data::log_capture;
use mtech_tui::events::action_handler::{ActionHandler, WidgetEvent, WidgetId};
use mtech_tui::styling::{Theme, APP_BACKGROUND, THEME};
use mtech_tui::widgets::{
    button::{Button, ButtonState},
    input_field::InputField,
    ButtonType, HandleWidget, SHORTCUT_SET,
};
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, MouseEvent},
    layout::{Constraint, Direction, Layout, Rect},
    prelude::Backend,
    style::Style,
    text::Line,
    widgets::{Block, Borders, Paragraph, Widget, WidgetRef, Wrap},
    Frame,
};

use crate::oa3_sager::{self, H2oGeneration};

const WRAPPER_ID: &str = "Oa3Wrapper";
const BIN_ID: &str = "Oa3Bin";
const GEN_ID: &str = "Oa3Gen";
const COPY_INJECT_ID: &str = "Oa3CopyInject";
const COPY_CLEAR_ID: &str = "Oa3CopyClear";

/// OA3 (Sager H2OOAE) command-line builder: wrapper/bin paths, H2O generation,
/// inject/clear previews + clipboard copy. Mirrors the egui `ui_oa3` tab.
pub struct Oa3Tab<'a> {
    wrapper_field: InputField<'a>,
    bin_field: InputField<'a>,
    gen_btn: Button<'a>,
    copy_inject_btn: Button<'a>,
    copy_clear_btn: Button<'a>,
    generation: RefCell<H2oGeneration>,
    active_field: RefCell<WidgetId>,
    status: RefCell<String>,
}

impl<'a> Oa3Tab<'a> {
    pub fn new() -> Self {
        Self {
            wrapper_field: InputField::new("Wrapper root", WidgetId(WRAPPER_ID.to_string())),
            bin_field: InputField::new("OA3 .bin for inject", WidgetId(BIN_ID.to_string())),
            gen_btn: Button::new(H2oGeneration::H2O14.label(), WidgetId(GEN_ID.to_string()))
                .theme(Theme::TERTIARY),
            copy_inject_btn: Button::new("Copy inject", WidgetId(COPY_INJECT_ID.to_string()))
                .theme(Theme::ACCENT),
            copy_clear_btn: Button::new("Copy clear", WidgetId(COPY_CLEAR_ID.to_string()))
                .theme(Theme::ACCENT),
            generation: RefCell::new(H2oGeneration::H2O14),
            active_field: RefCell::new(WidgetId(WRAPPER_ID.to_string())),
            status: RefCell::new(String::new()),
        }
    }

    fn toggle_generation(&mut self) {
        let next = match *self.generation.borrow() {
            H2oGeneration::H2O14 => H2oGeneration::H2O12,
            H2oGeneration::H2O12 => H2oGeneration::H2O14,
        };
        *self.generation.borrow_mut() = next;
        self.gen_btn.set_label(next.label().to_string());
    }

    fn inject_preview(&self) -> String {
        let wrapper = self.wrapper_field.get_raw_text();
        let bin = self.bin_field.get_raw_text();
        oa3_sager::inject_command_line(
            *self.generation.borrow(),
            Path::new(wrapper.trim()),
            Path::new(bin.trim()),
        )
    }

    fn clear_preview(&self) -> String {
        let wrapper = self.wrapper_field.get_raw_text();
        oa3_sager::clear_command_line(*self.generation.borrow(), Path::new(wrapper.trim()))
    }

    fn copy(&self, text: String, what: &str) {
        *self.status.borrow_mut() = match log_capture::copy_text(text) {
            Ok(()) => format!("Copied {what} command to clipboard."),
            Err(e) => format!("Copy failed: {e}"),
        };
    }

    fn focus(&self, id: &WidgetId) {
        self.active_field.replace(id.clone());
        self.wrapper_field
            .set_state(state_for(id.0 == WRAPPER_ID));
        self.bin_field.set_state(state_for(id.0 == BIN_ID));
    }
}

fn state_for(active: bool) -> ButtonState {
    if active {
        ButtonState::Active
    } else {
        ButtonState::Normal
    }
}

impl<'a> Default for Oa3Tab<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> ActionHandler for Oa3Tab<'a> {
    fn widget_id(&self) -> WidgetId {
        WidgetId("Oa3Tab".to_string())
    }

    fn managed_widget_ids(&self) -> Vec<WidgetId> {
        vec![
            WidgetId(WRAPPER_ID.to_string()),
            WidgetId(BIN_ID.to_string()),
            WidgetId(GEN_ID.to_string()),
            WidgetId(COPY_INJECT_ID.to_string()),
            WidgetId(COPY_CLEAR_ID.to_string()),
        ]
    }

    fn handle_event(&mut self, event: &WidgetEvent) {
        match event {
            WidgetEvent::Active { widget_id } => {
                if widget_id.0 == WRAPPER_ID || widget_id.0 == BIN_ID {
                    self.focus(widget_id);
                }
            }
            WidgetEvent::ButtonClick { widget_id, .. } => match widget_id.0.as_str() {
                GEN_ID => self.toggle_generation(),
                COPY_INJECT_ID => {
                    let p = self.inject_preview();
                    self.copy(p, "inject");
                }
                COPY_CLEAR_ID => {
                    let p = self.clear_preview();
                    self.copy(p, "clear");
                }
                _ => {}
            },
            _ => {}
        }
    }
}

impl<'a> HandleWidget<'a> for Oa3Tab<'a> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(SHORTCUT_SET)
            .border_style(THEME.border(false))
            .title_style(THEME.title())
            .title("OA3 — Sager H2O helper");
        (&block).render(area, f.buffer_mut());
        let inner = block.inner(area);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // wrapper field
                Constraint::Length(3), // bin field
                Constraint::Length(3), // gen + copy buttons
                Constraint::Length(1), // resolved exe
                Constraint::Min(4),    // previews
                Constraint::Length(1), // status
            ])
            .margin(1)
            .split(inner);

        self.wrapper_field.render_ref(rows[0], f.buffer_mut());
        self.bin_field.render_ref(rows[1], f.buffer_mut());

        let btns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(28),
                Constraint::Length(16),
                Constraint::Length(16),
                Constraint::Min(0),
            ])
            .split(rows[2]);
        self.gen_btn.render_ref(btns[0], f.buffer_mut());
        self.copy_inject_btn.render_ref(btns[1], f.buffer_mut());
        self.copy_clear_btn.render_ref(btns[2], f.buffer_mut());

        let wrapper = self.wrapper_field.get_raw_text();
        let exe = oa3_sager::h2ooae_exe(Path::new(wrapper.trim()), *self.generation.borrow());
        f.render_widget(
            Paragraph::new(
                Line::from(format!("Resolved H2OOAE: {}", exe.display()))
                    .style(Style::default().fg(THEME.text_muted)),
            ),
            rows[3],
        );

        let preview_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(rows[4]);
        let inject = self.inject_preview();
        let clear = self.clear_preview();
        f.render_widget(
            Paragraph::new(inject)
                .wrap(Wrap { trim: false })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(THEME.border(false))
                        .title("Inject (preview)"),
                )
                .style(Style::default().fg(THEME.text).bg(APP_BACKGROUND)),
            preview_cols[0],
        );
        f.render_widget(
            Paragraph::new(clear)
                .wrap(Wrap { trim: false })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(THEME.border(false))
                        .title("Clear (preview)"),
                )
                .style(Style::default().fg(THEME.text).bg(APP_BACKGROUND)),
            preview_cols[1],
        );

        let status = self.status.borrow();
        if !status.is_empty() {
            f.render_widget(
                Paragraph::new(status.as_str()).style(Style::default().fg(THEME.success)),
                rows[5],
            );
        }
    }

    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        self.wrapper_field.handle_mouse_event(mouse_event);
        self.bin_field.handle_mouse_event(mouse_event);
        self.gen_btn.handle_mouse_event(mouse_event);
        self.copy_inject_btn.handle_mouse_event(mouse_event);
        self.copy_clear_btn.handle_mouse_event(mouse_event);
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> bool {
        match key_event.code {
            KeyCode::Tab => {
                let next = if self.active_field.borrow().0 == WRAPPER_ID {
                    WidgetId(BIN_ID.to_string())
                } else {
                    WidgetId(WRAPPER_ID.to_string())
                };
                self.focus(&next);
                true
            }
            _ => {
                if self.active_field.borrow().0 == WRAPPER_ID {
                    self.wrapper_field.input.borrow_mut().input_without_shortcuts(key_event)
                } else {
                    self.bin_field.input.borrow_mut().input_without_shortcuts(key_event)
                }
            }
        }
    }
}
