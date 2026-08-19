//! Small popup for editing a connected client's `friendly_name` directly
//! from the client list. An empty name clears the field so the row falls
//! back to showing the connection string.

use crate::ui_tools::{icons, theme};
use crate::{PlatformSpawner, Spawner};
use crossbeam::channel::{unbounded, Receiver, Sender};
use database::{db, schema::ConnectedClient};
use eframe::egui::{self, Context, RichText, TextEdit};

pub struct RenameClientPopup {
    client: ConnectedClient,
    name: String,
    saving: bool,
    error: Option<String>,
    tx: Sender<Result<(), String>>,
    rx: Receiver<Result<(), String>>,
}

impl RenameClientPopup {
    pub fn new(client: &ConnectedClient) -> Self {
        let (tx, rx) = unbounded();
        Self {
            name: client.friendly_name.clone().unwrap_or_default(),
            client: client.clone(),
            saving: false,
            error: None,
            tx,
            rx,
        }
    }

    fn save(&mut self) {
        self.saving = true;
        self.error = None;
        let id = self.client.id.clone();
        // Empty input clears the friendly name.
        let name = Some(self.name.trim().to_string()).filter(|n| !n.is_empty());
        let tx = self.tx.clone();
        PlatformSpawner::spawn(async move {
            let result: Result<_, surrealdb::Error> = db()
                .query("UPDATE $id SET friendly_name = $name, last_update = time::now()")
                .bind(("id", id))
                .bind(("name", name))
                .await;
            let _ = tx.try_send(result.map(|_| ()).map_err(|e| e.to_string()));
        });
    }

    /// Renders the popup; returns `false` once it should be dropped.
    pub fn ui(&mut self, ctx: &Context) -> bool {
        let mut still_open = true;
        while let Ok(result) = self.rx.try_recv() {
            self.saving = false;
            match result {
                Ok(()) => still_open = false,
                Err(e) => self.error = Some(e),
            }
        }
        if !still_open {
            return false;
        }

        let mut open = true;
        egui::Window::new(format!("{} Rename client", icons::EDIT))
            .id(egui::Id::new((
                "admin_rename_client",
                self.client.connection_string.as_str(),
            )))
            .collapsible(false)
            .resizable(false)
            .default_width(320.)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(&self.client.connection_string)
                        .small()
                        .color(theme::weak_text(ui)),
                );
                ui.add_space(4.);
                let edit = ui.add(
                    TextEdit::singleline(&mut self.name)
                        .hint_text("Friendly name (empty to clear)")
                        .desired_width(f32::INFINITY),
                );
                let submitted = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if let Some(e) = self.error.as_deref() {
                    ui.add_space(2.);
                    ui.label(RichText::new(e).small().color(theme::error(ui)));
                }
                ui.add_space(6.);
                ui.horizontal(|ui| {
                    if self.saving {
                        ui.spinner();
                        ui.label(RichText::new("Saving…").weak());
                        return;
                    }
                    if ui.button(format!("{} Save", icons::SAVE)).clicked() || submitted {
                        self.save();
                    }
                    if ui.button(format!("{} Cancel", icons::CLOSE)).clicked() {
                        still_open = false;
                    }
                });
            });
        still_open && open
    }
}
