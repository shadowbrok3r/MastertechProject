//! Read-only detail windows for the `customer` / `computer` rows a connected
//! client links to. Opened by clicking either value in a client row's details
//! grid; each record gets its own window, fetched once on open.

use crate::ui_tools::{icons, theme};
use crate::{PlatformSpawner, Spawner};
use crossbeam::channel::{unbounded, Receiver, Sender};
use database::{
    db,
    schema::{ComputerData, CustomerData, RecordId, RecordIdExt},
};
use eframe::egui::{self, Context, Grid, RichText, ScrollArea, Ui};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    Customer,
    Computer,
}

impl RecordKind {
    fn label(self) -> &'static str {
        match self {
            Self::Customer => "Customer",
            Self::Computer => "Computer",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Customer => icons::RELINK,
            Self::Computer => icons::DESKTOP,
        }
    }
}

pub enum RecordPayload {
    Customer(Box<CustomerData>),
    Computer(Box<ComputerData>),
}

struct RecordWindow {
    kind: RecordKind,
    id: RecordId,
    payload: Option<RecordPayload>,
    error: Option<String>,
    open: bool,
}

type FetchResult = (String, Result<RecordPayload, String>);

/// Open record windows plus the channel their fetches report back on.
pub struct RecordViewer {
    windows: Vec<RecordWindow>,
    tx: Sender<FetchResult>,
    rx: Receiver<FetchResult>,
}

impl Default for RecordViewer {
    fn default() -> Self {
        let (tx, rx) = unbounded();
        Self { windows: Vec::new(), tx, rx }
    }
}

impl RecordViewer {
    /// Opens a window for `id`, fetching it in the background. A record that is
    /// already open is re-fetched rather than duplicated.
    pub fn open(&mut self, kind: RecordKind, id: RecordId) {
        let key = id.key_string().to_string();
        match self.windows.iter_mut().find(|w| w.id.key_string() == key) {
            Some(existing) => {
                existing.open = true;
                existing.payload = None;
                existing.error = None;
            }
            None => self.windows.push(RecordWindow {
                kind,
                id: id.clone(),
                payload: None,
                error: None,
                open: true,
            }),
        }

        let tx = self.tx.clone();
        PlatformSpawner::spawn(async move {
            let result = match kind {
                RecordKind::Customer => {
                    let row: Result<Option<CustomerData>, surrealdb::Error> =
                        db().select(id.clone()).await;
                    row.map_err(|e| e.to_string()).and_then(|r| {
                        r.map(|c| RecordPayload::Customer(Box::new(c)))
                            .ok_or_else(|| "no such record".to_string())
                    })
                }
                RecordKind::Computer => {
                    let row: Result<Option<ComputerData>, surrealdb::Error> =
                        db().select(id.clone()).await;
                    row.map_err(|e| e.to_string()).and_then(|r| {
                        r.map(|c| RecordPayload::Computer(Box::new(c)))
                            .ok_or_else(|| "no such record".to_string())
                    })
                }
            };
            let _ = tx.send((key, result));
        });
    }

    pub fn ui(&mut self, ctx: &Context) {
        while let Ok((key, result)) = self.rx.try_recv() {
            let Some(window) = self.windows.iter_mut().find(|w| w.id.key_string() == key) else {
                continue;
            };
            match result {
                Ok(payload) => {
                    window.payload = Some(payload);
                    window.error = None;
                }
                Err(e) => {
                    window.payload = None;
                    window.error = Some(e);
                }
            }
        }

        for window in self.windows.iter_mut() {
            let key = window.id.key_string().to_string();
            let mut open = window.open;
            egui::Window::new(format!(
                "{}  {} — {key}",
                window.kind.icon(),
                window.kind.label()
            ))
            .id(egui::Id::new(("admin_record_viewer", key.as_str())))
            .default_size([460., 520.])
            .open(&mut open)
            .show(ctx, |ui| {
                ScrollArea::vertical().show(ui, |ui| match (&window.payload, &window.error) {
                    (Some(payload), _) => render_payload(ui, payload),
                    (None, Some(e)) => {
                        ui.label(RichText::new(e).color(theme::error(ui)));
                    }
                    (None, None) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new("Loading…").weak());
                        });
                    }
                });
            });
            window.open = open;
        }
        self.windows.retain(|w| w.open);
    }
}

fn render_payload(ui: &mut Ui, payload: &RecordPayload) {
    match payload {
        RecordPayload::Customer(c) => render_customer(ui, c),
        RecordPayload::Computer(c) => render_computer(ui, c),
    }
}

fn render_customer(ui: &mut Ui, c: &CustomerData) {
    Grid::new("record_viewer_customer")
        .num_columns(2)
        .spacing([10., 3.])
        .show(ui, |ui| {
            field(ui, "ID", &c.id.key_string());
            field(ui, "Name", &c.name);
            field(ui, "Customer code", &c.cust_code);
            field(ui, "Phone", &c.phone_number);
            if !c.phone_number_2.is_empty() {
                field(ui, "Phone 2", &c.phone_number_2);
            }
            field(ui, "Email", &c.email);
            if !c.num_inv.is_empty() {
                field(ui, "Invoices", &c.num_inv);
            }
            if !c.li_doc.is_empty() {
                field(ui, "Last invoice", &c.li_doc);
            }
            if !c.li_amnt.is_empty() {
                field(ui, "Last amount", &c.li_amnt);
            }
        });

    if let Some(links) = c.part_order_links.as_ref().filter(|l| !l.is_empty()) {
        ui.add_space(6.);
        ui.label(RichText::new("Part orders").strong());
        for link in links {
            ui.label(RichText::new(link).small());
        }
    }
}

fn render_computer(ui: &mut Ui, c: &ComputerData) {
    if database::schema::entity_link::is_placeholder_computer(c) {
        ui.label(
            RichText::new(
                "Placeholder record — built from an order, not from client hardware. \
                 No specs were ever gathered.",
            )
            .small()
            .color(theme::warn(ui)),
        );
        ui.add_space(4.);
    }

    Grid::new("record_viewer_computer")
        .num_columns(2)
        .spacing([10., 3.])
        .show(ui, |ui| {
            field(ui, "ID", &c.id.key_string());
            field(
                ui,
                "Customer",
                &c.customer
                    .as_ref()
                    .map(|id| id.key_string().to_string())
                    .unwrap_or_else(|| "—".into()),
            );
            field(ui, "Hostname", &c.hostname);
            field(ui, "Operating system", &c.operating_system);
            if let Some(active) = c.windows_active {
                field(ui, "Windows active", if active { "yes" } else { "no" });
            }
            if let Some(key) = c.oa3_key.as_deref().filter(|k| !k.is_empty()) {
                field(ui, "OA3 key", key);
            }
            ui.end_row();

            field(ui, "CPU", &c.cpu);
            field(ui, "GPU", &c.gpu);
            field(ui, "RAM", &c.ram);
            field(ui, "Motherboard", &c.motherboard_name);
            opt_field(ui, "Device name", c.device_name.as_deref());
            opt_field(ui, "Device mfg", c.device_mfg.as_deref());
            opt_field(ui, "Device model", c.device_model.as_deref());
            opt_field(ui, "Device serial", c.device_serial.as_deref());
        });

    if !c.drives.is_empty() {
        ui.add_space(6.);
        ui.label(RichText::new("Drives").strong());
        Grid::new("record_viewer_drives")
            .num_columns(3)
            .spacing([10., 3.])
            .show(ui, |ui| {
                for drive in &c.drives {
                    ui.label(RichText::new(&drive.drive_letter).small());
                    ui.label(RichText::new(&drive.drive_type).small());
                    ui.label(
                        RichText::new(format!("{} / {} Gb", drive.space_left, drive.total_size))
                            .small(),
                    );
                    ui.end_row();
                }
            });
    }

    if !c.current_antivirus.is_empty() {
        ui.add_space(6.);
        ui.label(RichText::new("Security products").strong());
        for product in &c.current_antivirus {
            ui.label(RichText::new(&product.name).small());
        }
    }
}

fn field(ui: &mut Ui, key: &str, value: &str) {
    ui.label(RichText::new(key).small().color(theme::weak_text(ui)));
    let shown = if value.trim().is_empty() { "—" } else { value };
    ui.add(egui::Label::new(RichText::new(shown).small()).wrap());
    ui.end_row();
}

fn opt_field(ui: &mut Ui, key: &str, value: Option<&str>) {
    field(ui, key, value.unwrap_or("—"));
}
