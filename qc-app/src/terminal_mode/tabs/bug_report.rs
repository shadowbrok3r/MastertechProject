use crossbeam::channel::{unbounded, Receiver, Sender};
use database::orders::{QcBackend, TechIdentity};
use mtech_ui::github::{build_github_issue_body, create_new_issue};

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

const EMP_ID: &str = "BugEmployee";
const TITLE_ID: &str = "BugTitle";
const DESC_ID: &str = "BugDesc";
const LOOKUP_ID: &str = "BugLookup";
const SUBMIT_ID: &str = "BugSubmit";

enum BugReportEvent {
    Resolved(TechIdentity),
    LookupFailed(String),
    Submitted,
    SubmitFailed(String),
}

/// Files a GitHub issue with recent logs attached, resolving the submitter
/// against the Shopify staff roster. Mirrors the egui `BugReportPanel`.
pub struct BugReportTab<'a> {
    employee_field: InputField<'a>,
    title_field: InputField<'a>,
    description_field: InputField<'a>,
    lookup_btn: Button<'a>,
    submit_btn: Button<'a>,
    resolved: Option<TechIdentity>,
    status: String,
    busy: bool,
    active_field: WidgetId,
    channel: (Sender<BugReportEvent>, Receiver<BugReportEvent>),
}

impl<'a> BugReportTab<'a> {
    pub fn new() -> Self {
        Self {
            employee_field: InputField::new("Employee name or ID", WidgetId(EMP_ID.to_string())),
            title_field: InputField::new("Issue title", WidgetId(TITLE_ID.to_string())),
            description_field: InputField::new("Describe the bug", WidgetId(DESC_ID.to_string())),
            lookup_btn: Button::new("Look up", WidgetId(LOOKUP_ID.to_string())).theme(Theme::TERTIARY),
            submit_btn: Button::new("Submit bug report", WidgetId(SUBMIT_ID.to_string()))
                .theme(Theme::ACCENT),
            resolved: None,
            status: String::new(),
            busy: false,
            active_field: WidgetId(EMP_ID.to_string()),
            channel: unbounded(),
        }
    }

    fn poll(&mut self) {
        while let Ok(ev) = self.channel.1.try_recv() {
            match ev {
                BugReportEvent::Resolved(id) => {
                    self.busy = false;
                    self.status = format!("Matched {} (employee #{})", id.name, id.id_employee);
                    self.resolved = Some(id);
                }
                BugReportEvent::LookupFailed(e) => {
                    self.busy = false;
                    self.resolved = None;
                    self.status = format!("Lookup failed: {e}");
                }
                BugReportEvent::Submitted => {
                    self.busy = false;
                    self.status = "Bug report submitted — thank you!".to_string();
                    self.title_field.set_text("");
                    self.description_field.set_text("");
                }
                BugReportEvent::SubmitFailed(e) => {
                    self.busy = false;
                    self.status = format!("Submit failed: {e}");
                }
            }
        }
    }

    fn focus(&mut self, id: WidgetId) {
        self.employee_field.set_state(state_for(id.0 == EMP_ID));
        self.title_field.set_state(state_for(id.0 == TITLE_ID));
        self.description_field.set_state(state_for(id.0 == DESC_ID));
        self.active_field = id;
    }

    fn kick_off_lookup(&mut self) {
        let input = self.employee_field.get_raw_text().trim().to_string();
        if self.busy || input.is_empty() {
            return;
        }
        self.busy = true;
        self.status = "Looking up employee…".to_string();
        let tx = self.channel.0.clone();
        tokio::spawn(async move {
            let backend = QcBackend::shopify();
            match backend.authenticate_tech(&input, "").await {
                Ok(id) => {
                    let _ = tx.send(BugReportEvent::Resolved(id));
                }
                Err(e) => {
                    let _ = tx.send(BugReportEvent::LookupFailed(format!("{e:#}")));
                }
            }
        });
    }

    fn kick_off_submit(&mut self) {
        let title = self.title_field.get_raw_text().trim().to_string();
        let description = self.description_field.get_raw_text();
        if self.busy || title.is_empty() || description.trim().is_empty() {
            self.status = "Title and description are required.".to_string();
            return;
        }
        self.busy = true;
        self.status = "Submitting…".to_string();

        let typed = self.employee_field.get_raw_text().trim().to_string();
        let logs = mtech_ui::egui_logger::get_logs_for_issue();
        let (name, contact) = match self.resolved.clone() {
            Some(id) => {
                let contact = if id.email.is_empty() {
                    format!("employee #{}", id.id_employee)
                } else {
                    id.email
                };
                (id.name, contact)
            }
            None => {
                let name = if typed.is_empty() { "unknown".to_string() } else { typed };
                (name, "employee not resolved".to_string())
            }
        };
        let body = build_github_issue_body(&description, &name, &contact, &logs);
        let tx = self.channel.0.clone();
        tokio::spawn(async move {
            let client = reqwest::Client::new();
            match create_new_issue(title, body, client).await {
                Ok(res) => {
                    log::info!("bug report submitted: {res}");
                    let _ = tx.send(BugReportEvent::Submitted);
                }
                Err(e) => {
                    let _ = tx.send(BugReportEvent::SubmitFailed(format!("{e:#}")));
                }
            }
        });
    }
}

fn state_for(active: bool) -> ButtonState {
    if active {
        ButtonState::Active
    } else {
        ButtonState::Normal
    }
}

impl<'a> Default for BugReportTab<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> ActionHandler for BugReportTab<'a> {
    fn widget_id(&self) -> WidgetId {
        WidgetId("BugReportTab".to_string())
    }

    fn managed_widget_ids(&self) -> Vec<WidgetId> {
        vec![
            WidgetId(EMP_ID.to_string()),
            WidgetId(TITLE_ID.to_string()),
            WidgetId(DESC_ID.to_string()),
            WidgetId(LOOKUP_ID.to_string()),
            WidgetId(SUBMIT_ID.to_string()),
        ]
    }

    fn handle_event(&mut self, event: &WidgetEvent) {
        match event {
            WidgetEvent::Active { widget_id } => {
                if matches!(widget_id.0.as_str(), EMP_ID | TITLE_ID | DESC_ID) {
                    self.focus(widget_id.clone());
                }
            }
            WidgetEvent::ButtonClick { widget_id, .. } => match widget_id.0.as_str() {
                LOOKUP_ID => self.kick_off_lookup(),
                SUBMIT_ID => self.kick_off_submit(),
                _ => {}
            },
            _ => {}
        }
    }
}

impl<'a> HandleWidget<'a> for BugReportTab<'a> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        self.poll();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(SHORTCUT_SET)
            .border_style(THEME.border(false))
            .title_style(THEME.title())
            .title("Bug report");
        (&block).render(area, f.buffer_mut());
        let inner = block.inner(area);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // hint
                Constraint::Length(3), // employee + lookup
                Constraint::Length(1), // resolved
                Constraint::Length(3), // title
                Constraint::Min(4),    // description
                Constraint::Length(3), // submit
                Constraint::Length(1), // status
            ])
            .margin(1)
            .split(inner);

        f.render_widget(
            Paragraph::new(
                Line::from("Files a GitHub issue. Recent app logs are attached automatically.")
                    .style(Style::default().fg(THEME.text_muted)),
            ),
            rows[0],
        );

        let emp_row = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(20), Constraint::Length(14)])
            .split(rows[1]);
        self.employee_field.render_ref(emp_row[0], f.buffer_mut());
        self.lookup_btn.render_ref(emp_row[1], f.buffer_mut());

        if let Some(id) = &self.resolved {
            f.render_widget(
                Paragraph::new(
                    Line::from(format!("{} — employee #{}", id.name, id.id_employee))
                        .style(Style::default().fg(THEME.success)),
                ),
                rows[2],
            );
        }

        self.title_field.render_ref(rows[3], f.buffer_mut());
        self.description_field.render_ref(rows[4], f.buffer_mut());
        self.submit_btn.render_ref(rows[5].shrink(2, 0), f.buffer_mut());

        if !self.status.is_empty() {
            let color = if self.status.contains("failed") {
                THEME.error
            } else if self.status.contains("submitted") {
                THEME.success
            } else {
                THEME.text_muted
            };
            f.render_widget(
                Paragraph::new(self.status.as_str())
                    .wrap(Wrap { trim: true })
                    .style(Style::default().fg(color).bg(APP_BACKGROUND)),
                rows[6],
            );
        }
    }

    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        self.employee_field.handle_mouse_event(mouse_event);
        self.title_field.handle_mouse_event(mouse_event);
        self.description_field.handle_mouse_event(mouse_event);
        self.lookup_btn.handle_mouse_event(mouse_event);
        self.submit_btn.handle_mouse_event(mouse_event);
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> bool {
        match key_event.code {
            KeyCode::Tab => {
                let next = match self.active_field.0.as_str() {
                    EMP_ID => TITLE_ID,
                    TITLE_ID => DESC_ID,
                    _ => EMP_ID,
                };
                self.focus(WidgetId(next.to_string()));
                true
            }
            KeyCode::Enter if self.active_field.0 == EMP_ID => {
                self.kick_off_lookup();
                true
            }
            _ => match self.active_field.0.as_str() {
                EMP_ID => self.employee_field.input.borrow_mut().input_without_shortcuts(key_event),
                TITLE_ID => self.title_field.input.borrow_mut().input_without_shortcuts(key_event),
                _ => self.description_field.input.borrow_mut().input_without_shortcuts(key_event),
            },
        }
    }
}
