use crossbeam::channel::{Receiver, Sender};
use database::schema::{ConnectedClient, LiveTaskPayload, RecordId};
use eframe::egui::{Align, ComboBox, Layout, RichText, Ui};

use crate::chats::ChatView;
use crate::modals::task_modal::{ModalAction, TaskModal};
use crate::ui_tools::icons;
use crate::{get_database_users, DisplayModal, PlatformSpawner, Spawner};

use super::ui::WsDisplayState;
use crate::ui_tools::theme;

/// Per-client "Service Record" page. Resolves the task whose computer
/// record matches the connected client (by linked computer RecordId, or
/// by hostname parsed from the connection string) and embeds the full
/// [`TaskModal`] for it — ticket/check-in notes, recommendations, task
/// notes, computer/software info, task history, and every diagnostic
/// session linked to the task or computer — so an operator can read it all
/// without leaving the session.
pub struct ServiceRecordViewer {
    /// connection_string the current results were fetched for; gates the
    /// one-shot lookup so it doesn't re-fire every frame.
    fetched_for: Option<String>,
    loading: bool,
    error: Option<String>,
    tasks: Vec<LiveTaskPayload>,
    selected_idx: usize,
    /// Embedded modal for the selected task, rebuilt when the selection
    /// changes.
    modal: Option<TaskModal>,
    modal_task_id: Option<RecordId>,
    tasks_tx: Sender<Result<Vec<LiveTaskPayload>, String>>,
    tasks_rx: Receiver<Result<Vec<LiveTaskPayload>, String>>,
}

impl ServiceRecordViewer {
    pub fn new() -> Self {
        let (tasks_tx, tasks_rx) = crossbeam::channel::unbounded();
        Self {
            fetched_for: None,
            loading: false,
            error: None,
            tasks: Vec::new(),
            selected_idx: 0,
            modal: None,
            modal_task_id: None,
            tasks_tx,
            tasks_rx,
        }
    }

    /// Kick off the matched-task lookup for `client` unless it has already
    /// run for this connection. Prefers the linked computer RecordId and
    /// falls back to the hostname parsed from the connection string.
    fn ensure_loaded(&mut self, client: &ConnectedClient) {
        if self.fetched_for.as_deref() == Some(client.connection_string.as_str()) {
            return;
        }
        self.fetched_for = Some(client.connection_string.clone());
        self.loading = true;
        self.error = None;
        self.tasks.clear();
        self.selected_idx = 0;
        self.modal = None;
        self.modal_task_id = None;

        let computer = client.computer.clone();
        let hostname = client
            .connection_string
            .split_once(':')
            .map(|(h, _)| h.to_string())
            .unwrap_or_else(|| client.connection_string.clone());
        let tx = self.tasks_tx.clone();
        PlatformSpawner::spawn(async move {
            let mut tasks: Vec<LiveTaskPayload> = Vec::new();
            if let Some(comp_id) = computer {
                match LiveTaskPayload::get_tasks_by_computer_id(&comp_id).await {
                    Ok(found) => tasks = found,
                    Err(e) => log::error!("Service record: tasks-by-computer failed: {e:?}"),
                }
            }
            if tasks.is_empty() && !hostname.is_empty() {
                match LiveTaskPayload::get_tasks_by_hostname(&hostname).await {
                    Ok(found) => tasks = found,
                    Err(e) => log::error!("Service record: tasks-by-hostname failed: {e:?}"),
                }
            }
            let _ = tx.try_send(Ok(tasks));
        });
    }

    /// Drop cached results so the next frame re-runs the lookup.
    fn invalidate(&mut self) {
        self.fetched_for = None;
    }

    fn receive(&mut self, ctx: &eframe::egui::Context) {
        while let Ok(res) = self.tasks_rx.try_recv() {
            self.loading = false;
            match res {
                Ok(tasks) => {
                    self.tasks = tasks;
                    self.selected_idx = 0;
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
            ctx.request_repaint();
        }
    }

    /// Build or rebuild the embedded modal when the selected task changes.
    fn sync_modal(&mut self) {
        let Some(task) = self.tasks.get(self.selected_idx).cloned() else {
            return;
        };
        if self.modal_task_id.as_ref() == Some(&task.id) {
            return;
        }
        let chat = ChatView::new(get_database_users(), task.id.clone(), task.service_number.clone());
        self.modal_task_id = Some(task.id.clone());
        self.modal = Some(TaskModal::new(chat, task));
    }

    pub fn display(
        &mut self,
        ui: &mut Ui,
        client: &ConnectedClient,
        state_tx: &Sender<WsDisplayState>,
    ) {
        self.ensure_loaded(client);
        self.receive(ui.ctx());

        let labels: Vec<String> = self.tasks.iter().map(task_label).collect();

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} Service Record", icons::TASK_EXISTS))
                    .strong()
                    .color(theme::info(ui)),
            );
            if labels.len() > 1 {
                ui.separator();
                let current = labels.get(self.selected_idx).cloned().unwrap_or_default();
                ComboBox::new(("service_record_task", client.connection_string.as_str()), "")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        for (i, label) in labels.iter().enumerate() {
                            ui.selectable_value(&mut self.selected_idx, i, label.clone());
                        }
                    });
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .button(
                        RichText::new(format!("{} Close", icons::CLOSE))
                            .color(theme::warn(ui)),
                    )
                    .on_hover_text("Close the service record and return to the Home view")
                    .clicked()
                {
                    let _ = state_tx.try_send(WsDisplayState::Home);
                }
                if ui
                    .button(format!("{} Refresh", icons::REFRESH))
                    .on_hover_text("Re-check for the service task linked to this machine")
                    .clicked()
                {
                    self.invalidate();
                }
            });
        });
        ui.separator();

        if self.loading {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.spinner();
                ui.label("Looking up linked service task…");
            });
            return;
        }

        if self.tasks.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(
                    RichText::new(
                        self.error
                            .as_deref()
                            .unwrap_or("No service task is linked to this machine."),
                    )
                    .color(theme::weak_text(ui)),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "Link this client to a computer record (expand the client row → Link computer) \
                         so its service ticket, notes, and diagnostics can be matched.",
                    )
                    .small()
                    .color(theme::faint_text(ui)),
                );
            });
            return;
        }

        self.sync_modal();
        if let Some(modal) = self.modal.as_mut() {
            let result = modal.display(ui, &mut |_action: ModalAction| {});
            if matches!(result, Some(ModalAction::Close)) {
                self.modal = None;
                self.modal_task_id = None;
                self.invalidate();
                let _ = state_tx.try_send(WsDisplayState::Home);
            }
        }
    }
}

/// One-line label for a matched task in the selector.
fn task_label(t: &LiveTaskPayload) -> String {
    match &t.service_number {
        Some(sn) if !sn.is_empty() => format!("{} — #{} ({})", t.task_name, sn, t.status.as_str()),
        _ => format!("{} ({})", t.task_name, t.status.as_str()),
    }
}
