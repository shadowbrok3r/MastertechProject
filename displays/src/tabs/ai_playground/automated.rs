//! Diagnostic sessions no agent thread links (the ZeroClaw sweeper, MCP clients), listed for Root in the Ai tab.

use std::time::Duration;

use chrono::{DateTime, Local, Utc};
use crossbeam::channel::{Receiver, Sender};
use database::schema::{DiagnosticSession, RecordIdExt, UnthreadedSessionRef, User, UserAuthorization};
use eframe::egui::{self, Color32, RichText, ScrollArea, Ui};
use web_time::Instant;

use crate::modals::tabs::diagnostics_page::{self, DiagnosticSessionView};
use crate::ui_tools::list_row::{Lead, ListRow};
use crate::ui_tools::{icons, theme};
use crate::{PlatformSpawner, Spawner};

const LIST_POLL: Duration = Duration::from_secs(60);
const DETAIL_POLL: Duration = Duration::from_secs(30);
const LISTED: u32 = 150;
const DENIED: &str = "Only a Root user can list automated sessions.";

enum Msg {
    List(Result<Vec<UnthreadedSessionRef>, String>),
    Detail(String, Result<Option<Box<DiagnosticSessionView>>, String>),
}

pub struct AutomatedView {
    rows: Vec<UnthreadedSessionRef>,
    /// The picked session's key; its detail is polled while set.
    picked: Option<String>,
    detail: Option<DiagnosticSessionView>,
    error: Option<String>,
    loading_list: bool,
    loading_detail: bool,
    last_list: Option<Instant>,
    last_detail: Option<Instant>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl Default for AutomatedView {
    fn default() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        Self {
            rows: Vec::new(),
            picked: None,
            detail: None,
            error: None,
            loading_list: false,
            loading_detail: false,
            last_list: None,
            last_detail: None,
            tx,
            rx,
        }
    }
}

fn due(last: Option<Instant>, every: Duration) -> bool {
    last.is_none_or(|t| t.elapsed() >= every)
}

fn local(at: &database::schema::Datetime) -> String {
    DateTime::<Utc>::from(*at).with_timezone(&Local).format("%b %d %H:%M").to_string()
}

/// Whether the lowercased `needle` is in the row's machine, customer, source, status or theory.
fn mentions(row: &UnthreadedSessionRef, needle: &str) -> bool {
    needle.is_empty()
        || [
            row.hostname.as_str(),
            row.customer_name.as_deref().unwrap_or(""),
            &row.ran_by(),
            row.status.as_str(),
            row.current_theory.as_deref().unwrap_or(""),
        ]
        .iter()
        .any(|f| f.to_lowercase().contains(needle))
}

fn status_mark(ui: &Ui, status: &str) -> (&'static str, Color32) {
    match status {
        "open" => (icons::ROBOT, theme::accent(ui)),
        "escalated" => (icons::STATUS_WARN, theme::warn(ui)),
        "resolved" | "closed" => (icons::STATUS_ON, theme::success(ui)),
        _ => (icons::STATUS_OFF, theme::weak_text(ui)),
    }
}

impl AutomatedView {
    /// Applies finished loads and starts due polls.
    pub fn tick(&mut self, ui: &Ui) {
        self.drain();
        if !self.loading_list && due(self.last_list, LIST_POLL) {
            self.loading_list = true;
            self.last_list = Some(Instant::now());
            let tx = self.tx.clone();
            PlatformSpawner::spawn(async move {
                // Root is checked here, not taken from the caller's cached flag.
                let me = User::get_current_user_from_auth().await.ok().flatten();
                let root = me.is_some_and(|u| u.get_authorization() == UserAuthorization::Root);
                let list = if root {
                    DiagnosticSession::list_unthreaded(LISTED).await.map_err(|e| e.to_string())
                } else {
                    Err(DENIED.to_string())
                };
                let _ = tx.send(Msg::List(list));
            });
        }
        if let Some(key) = self.picked.clone() {
            let stale = self.detail.as_ref().is_none_or(|d| d.session.id.key_string() != key);
            if !self.loading_detail && (stale || due(self.last_detail, DETAIL_POLL)) {
                self.loading_detail = true;
                self.last_detail = Some(Instant::now());
                let tx = self.tx.clone();
                PlatformSpawner::spawn(async move {
                    let full = DiagnosticSession::get_full(&key)
                        .await
                        .map(|f| f.map(|f| Box::new(DiagnosticSessionView { session: f.session, entries: f.entries })))
                        .map_err(|e| e.to_string());
                    let _ = tx.send(Msg::Detail(key, full));
                });
            }
        }
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }

    pub fn picked(&self) -> Option<String> {
        self.picked.clone()
    }

    pub fn select(&mut self, key: &str) {
        if self.picked.as_deref() != Some(key) {
            self.picked = Some(key.to_string());
            self.error = None;
        }
    }

    pub fn deselect(&mut self) {
        self.picked = None;
    }

    /// The picked session's machine and source, for the top bar.
    pub fn title(&self) -> Option<String> {
        let key = self.picked.as_ref()?;
        let row = self.rows.iter().find(|r| &r.id.key_string() == key);
        Some(row.map_or_else(|| key.clone(), |r| format!("{} \u{00b7} {}", r.hostname, r.ran_by())))
    }

    /// The Automated group filtered by `needle`; returns the session clicked.
    pub fn list_ui(&mut self, ui: &mut Ui, needle: &str) -> Option<String> {
        let needle = needle.trim().to_lowercase();
        let rows: Vec<&UnthreadedSessionRef> = self.rows.iter().filter(|r| mentions(r, &needle)).collect();
        let open = rows.iter().filter(|r| r.status == "open" || r.status == "escalated").count();
        let header = if open > 0 {
            format!("Automated  {}  \u{00b7} {open} open", rows.len())
        } else {
            format!("Automated  {}", rows.len())
        };
        let mut clicked = None;
        egui::CollapsingHeader::new(header)
            .id_salt("enhanced_ai_automated")
            .default_open(false)
            .open((!needle.is_empty()).then_some(true))
            .show(ui, |ui| {
                if let Some(err) = &self.error {
                    ui.label(RichText::new(format!("{} {err}", icons::STATUS_ERR)).color(theme::error(ui)).small());
                }
                if rows.is_empty() {
                    let empty = if self.loading_list { "Loading\u{2026}" } else { "No automated sessions" };
                    ui.label(RichText::new(empty).color(theme::weak_text(ui)).small());
                }
                for row in rows {
                    let key = row.id.key_string();
                    let selected = self.picked.as_deref() == Some(key.as_str());
                    let (icon, color) = status_mark(ui, &row.status);
                    let title = match row.customer_name.as_deref().filter(|c| !c.is_empty()) {
                        Some(customer) => format!("{} \u{00b7} {customer}", row.hostname),
                        None => row.hostname.clone(),
                    };
                    let detail = format!("{} \u{00b7} {} \u{00b7} {}", row.ran_by(), row.status, local(&row.started_at));
                    let hover = match row.current_theory.as_deref() {
                        Some(theory) => format!("{title}\n{detail}\n{theory}"),
                        None => format!("{title}\n{detail}"),
                    };
                    let resp = ListRow::new(&title)
                        .lead(Lead::Icon(icon, Some(color)))
                        .detail(&detail)
                        .selected(selected)
                        .show(ui)
                        .on_hover_text(hover);
                    if resp.clicked() && !selected {
                        clicked = Some(key);
                    }
                }
            });
        clicked
    }

    /// The picked session, read-only.
    pub fn detail_ui(&mut self, ui: &mut Ui) {
        let Some(key) = self.picked.clone() else { return };
        if let Some(err) = &self.error {
            ui.label(RichText::new(format!("{} {err}", icons::STATUS_ERR)).color(theme::error(ui)));
        }
        let Some(view) = self.detail.as_ref().filter(|d| d.session.id.key_string() == key) else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading session\u{2026}");
            });
            return;
        };
        ui.label(
            RichText::new(format!("Read-only: run by {}, with no chat behind it.", view.session.ran_by()))
                .color(theme::weak_text(ui))
                .small(),
        );
        ScrollArea::vertical()
            .id_salt("enhanced_ai_automated_detail")
            .auto_shrink([false, false])
            .show(ui, |ui| diagnostics_page::render_session(ui, view, 0, true, None, |_| {}));
    }

    fn drain(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::List(r) => {
                    self.loading_list = false;
                    match r {
                        Ok(rows) => {
                            self.rows = rows;
                            self.error = None;
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
                Msg::Detail(key, r) => {
                    self.loading_detail = false;
                    if self.picked.as_deref() != Some(key.as_str()) {
                        continue;
                    }
                    match r {
                        Ok(Some(view)) => self.detail = Some(*view),
                        Ok(None) => self.error = Some("This session no longer exists.".to_string()),
                        Err(e) => self.error = Some(e),
                    }
                }
            }
        }
    }

    #[cfg(test)]
    pub(super) fn set_rows(&mut self, rows: Vec<UnthreadedSessionRef>) {
        self.rows = rows;
        self.last_list = Some(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::schema::{Datetime, RecordId};

    fn row(key: &str, host: &str, driven_by: Option<&str>, status: &str) -> UnthreadedSessionRef {
        UnthreadedSessionRef {
            id: RecordId::new("diagnostic_session", key),
            connection_string: format!("{host}:abc"),
            hostname: host.to_string(),
            customer_name: Some("Jacqueline Hoff".to_string()),
            tech: None,
            driven_by: driven_by.map(str::to_string),
            status: status.to_string(),
            started_at: Datetime::default(),
            last_activity_at: None,
            current_theory: Some("Known S0ix/DRIPS class".to_string()),
        }
    }

    #[test]
    fn the_search_matches_machine_customer_source_status_and_theory() {
        let r = row("a", "DESKTOP-E94IGME", Some("zeroclaw/sweeper"), "escalated");
        for needle in ["e94igme", "hoff", "sweeper", "escalated", "s0ix", ""] {
            assert!(mentions(&r, needle), "{needle}");
        }
        assert!(!mentions(&r, "green-machine"));
    }

    #[test]
    fn select_and_deselect_set_the_title() {
        let mut view = AutomatedView::default();
        view.set_rows(vec![row("a", "DESKTOP-E94IGME", Some("zeroclaw/sweeper"), "escalated")]);
        assert_eq!(view.title(), None);
        view.select("a");
        assert_eq!(view.picked().as_deref(), Some("a"));
        assert_eq!(view.title().as_deref(), Some("DESKTOP-E94IGME \u{00b7} ZeroClaw sweeper"));
        view.select("gone");
        assert_eq!(view.title().as_deref(), Some("gone"));
        view.deselect();
        assert_eq!(view.picked(), None);
    }
}
