//! Small popup for editing a connected client's `friendly_name` directly
//! from the client list. A saved name sets `customer_locked`; an empty name
//! clears both, so the row falls back to the connection string and to
//! client-side auto-detection.

use crate::ui_tools::controls::{self, BarKind, IconLabel, Role, SpinnerSize, TextField};
use crate::ui_tools::{icons, theme};
use crate::{PlatformSpawner, Spawner};
use crossbeam::channel::{Receiver, Sender, unbounded};
use database::{db, schema::ConnectedClient};
use eframe::egui::{self, Context, RichText};

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
        // Locked while a name is set, so the client-side lookups leave it alone.
        let locked = name.is_some();
        PlatformSpawner::spawn(async move {
            let result: Result<_, surrealdb::Error> = db()
                .query(
                    "UPDATE $id SET friendly_name = $name, customer_locked = $locked, \
                     last_update = time::now()",
                )
                .bind(("id", id))
                .bind(("name", name))
                .bind(("locked", locked))
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
        controls::icon_window(ctx, icons::svg::PENCIL_SIMPLE, "Rename client")
            .id(egui::Id::new((
                "admin_rename_client",
                self.client.connection_string.as_str(),
            )))
            .collapsible(false)
            .resizable(false)
            .default_width(320.)
            .open(&mut open)
            .show(ctx, |ui| {
                IconLabel::new(icons::svg::DESKTOP, &self.client.connection_string)
                    .small(true)
                    .tone(theme::weak_text(ui))
                    .show(ui);
                ui.add_space(4.);
                let edit = TextField::new(&mut self.name)
                    .hint("Friendly name (empty to clear)")
                    .icon(icons::svg::TAG)
                    .invalid(self.error.is_some())
                    .width(ui.available_width())
                    .show(ui);
                let submitted = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if let Some(e) = self.error.as_deref() {
                    ui.add_space(2.);
                    IconLabel::new(icons::svg::WARNING_CIRCLE, &e)
                        .tone(theme::error(ui))
                        .small(true)
                        .wrap(true)
                        .show(ui);
                }
                ui.add_space(8.);
                controls::bar(ui, BarKind::Footer, |ui| {
                    if self.saving {
                        controls::spinner(ui, SpinnerSize::Sm);
                        ui.label(RichText::new("Saving…").weak());
                        return;
                    }
                    let clicked = controls::dialog_buttons(
                        ui,
                        &[(Role::Primary, "Save"), (Role::Cancel, "Cancel")],
                    );
                    if clicked == Some(0) || submitted {
                        self.save();
                    }
                    if clicked == Some(1) {
                        still_open = false;
                    }
                });
            });
        still_open && open
    }
}
