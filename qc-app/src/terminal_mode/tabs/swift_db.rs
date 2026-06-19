use std::cell::RefCell;
use std::path::Path;

use mtech_tui::events::action_handler::{ActionHandler, WidgetEvent, WidgetId};
use mtech_tui::styling::{Theme, APP_BACKGROUND, THEME};
use mtech_tui::widgets::{
    button::{Button, ButtonState},
    input_field::InputField,
    ButtonType, HandleWidget, ShrinkArea, SHORTCUT_SET,
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

use crate::db;

const PATH_ID: &str = "SwiftDbPath";
const OPEN_ID: &str = "SwiftDbOpen";

/// Swift driver SQLite catalog: path field, open/migrate button, table-count
/// summary. Mirrors the egui `ui_database` tab.
pub struct SwiftDbTab<'a> {
    path_field: InputField<'a>,
    open_btn: Button<'a>,
    active_field: RefCell<Option<WidgetId>>,
    summary: RefCell<String>,
}

impl<'a> SwiftDbTab<'a> {
    pub fn new() -> Self {
        let path_field = InputField::new("Database file", WidgetId(PATH_ID.to_string()));
        path_field.set_text(&db::default_sqlite_path().to_string_lossy());
        Self {
            path_field,
            open_btn: Button::new("Open / create & migrate", WidgetId(OPEN_ID.to_string()))
                .theme(Theme::ACCENT),
            active_field: RefCell::new(Some(WidgetId(PATH_ID.to_string()))),
            summary: RefCell::new(String::new()),
        }
    }

    fn run_open(&self) {
        let path_text = self.path_field.get_raw_text();
        let path = Path::new(path_text.trim());
        let msg = match db::open_or_create(path).and_then(|c| db::table_stats(&c)) {
            Ok(rows) => rows
                .iter()
                .map(|(n, c)| format!("{n}: {c}"))
                .collect::<Vec<_>>()
                .join("    "),
            Err(e) => format!("Error: {e:#}"),
        };
        *self.summary.borrow_mut() = msg;
    }
}

impl<'a> Default for SwiftDbTab<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> ActionHandler for SwiftDbTab<'a> {
    fn widget_id(&self) -> WidgetId {
        WidgetId("SwiftDbTab".to_string())
    }

    fn managed_widget_ids(&self) -> Vec<WidgetId> {
        vec![WidgetId(PATH_ID.to_string()), WidgetId(OPEN_ID.to_string())]
    }

    fn handle_event(&mut self, event: &WidgetEvent) {
        match event {
            WidgetEvent::Active { widget_id } if widget_id.0 == PATH_ID => {
                self.active_field.replace(Some(widget_id.clone()));
                self.path_field.set_state(ButtonState::Active);
            }
            WidgetEvent::ButtonClick { widget_id, .. } if widget_id.0 == OPEN_ID => {
                self.run_open();
            }
            _ => {}
        }
    }
}

impl<'a> HandleWidget<'a> for SwiftDbTab<'a> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(SHORTCUT_SET)
            .border_style(THEME.border(false))
            .title_style(THEME.title())
            .title("Swift driver DB (local SQLite)");
        (&block).render(area, f.buffer_mut());
        let inner = block.inner(area);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(1),
            ])
            .margin(1)
            .split(inner);

        f.render_widget(
            Paragraph::new(
                Line::from("Schema from db_creation_script.sql (MySQL), adapted for SQLite.")
                    .style(Style::default().fg(THEME.text_muted)),
            ),
            rows[0],
        );
        self.path_field.render_ref(rows[1], f.buffer_mut());
        self.open_btn.render_ref(rows[2].shrink(2, 0), f.buffer_mut());

        let summary = self.summary.borrow();
        if !summary.is_empty() {
            f.render_widget(
                Paragraph::new(summary.as_str())
                    .wrap(Wrap { trim: true })
                    .style(Style::default().fg(THEME.text).bg(APP_BACKGROUND)),
                rows[3],
            );
        }
    }

    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        self.path_field.handle_mouse_event(mouse_event);
        self.open_btn.handle_mouse_event(mouse_event);
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> bool {
        match key_event.code {
            KeyCode::Enter => {
                self.run_open();
                true
            }
            _ => self.path_field.input.borrow_mut().input_without_shortcuts(key_event),
        }
    }
}
