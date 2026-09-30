//! QC view: the bench-QC surfaces from qc-app, for a connected client.
//!
//! Three read-only sections:
//!   - **Order** — resolves the order from this machine's serials (or a manual
//!     lookup), then shows the status gate, line items and which of them have
//!     serials attached, plus federated serial history on demand.
//!   - **Firmware** — installed BIOS against the catalog's latest, Secure Boot,
//!     TPM and Windows activation. Needs a client new enough to answer
//!     [`Cmd::GatherQcFirmware`].
//!   - **Driver check** — the machine's newest driver snapshot compared per part
//!     category against the fleet driver catalog.
//!
//! Order and catalog traffic is admin-side, so both render for a client that is
//! offline; only the firmware reading needs the client on the wire.

use std::collections::{HashMap, HashSet};

use crossbeam::channel::{unbounded, Receiver, Sender};
use database::orders::{
    is_placeholder_serial, resolve_any, BackendKind, GateOutcome, OrderKey, QcBackend, QcOrder,
    SerialHistorySummary,
};
use database::schema::driver_catalog::{
    bios_for_baseboard, build_driver_check, gpu_driver_for_device, package_drivers_for_baseboard,
    CatalogBios, DriverCheckRow, DriverStatus, TargetDriver,
};
use database::schema::driver_intel::DriverSnapshot;
use database::schema::{ConnectedClient, SystemInformation};
use eframe::egui::{Button, RichText, ScrollArea, TextEdit, Ui};
use web_time::Instant;

use crate::ui_tools::info_card::{badge, kv_row, section_card};
use crate::ui_tools::{glass_card, icons, theme};
use crate::{Cmd, PlatformSpawner, QcFirmware, Spawner};

/// How long a `GatherQcFirmware` may go unanswered before the section says so
/// instead of spinning forever (an older client never replies at all).
const FIRMWARE_TIMEOUT_SECS: u64 = 20;
const FIELD_WIDTH: f32 = 190.0;

enum QcMsg {
    Order(Box<Result<QcOrder, String>>),
    /// Auto-resolution finished; `None` means no order matched the serials.
    AutoResolved(Option<String>),
    SerialHistory { serial: String, result: Box<Result<SerialHistorySummary, String>> },
    DriverCheck(Box<Result<Vec<DriverCheckRow>, String>>),
    CatalogBios(Box<Option<CatalogBios>>),
    Status(String),
}

pub struct QcViewer {
    tx: Sender<QcMsg>,
    rx: Receiver<QcMsg>,
    status: String,

    order_input: String,
    order: Option<Result<QcOrder, String>>,
    order_busy: bool,
    /// Serial-driven auto-resolution runs once per session.
    auto_resolve_started: bool,
    serial_history: HashMap<String, Result<SerialHistorySummary, String>>,
    /// Lookups in flight; several serials on one line item resolve together.
    serial_busy: HashSet<String>,

    firmware: Option<QcFirmware>,
    firmware_requested_at: Option<Instant>,

    catalog_bios: Option<CatalogBios>,
    catalog_bios_board: Option<String>,

    driver_check: Option<Result<Vec<DriverCheckRow>, String>>,
    driver_check_busy: bool,
    /// Board the loaded driver check was built for, so a later firmware reply
    /// with a different board re-runs it.
    driver_check_board: Option<String>,
}

impl Default for QcViewer {
    fn default() -> Self {
        Self::new()
    }
}

impl QcViewer {
    pub fn new() -> Self {
        let (tx, rx) = unbounded();
        Self {
            tx,
            rx,
            status: String::new(),
            order_input: String::new(),
            order: None,
            order_busy: false,
            auto_resolve_started: false,
            serial_history: HashMap::new(),
            serial_busy: HashSet::new(),
            firmware: None,
            firmware_requested_at: None,
            catalog_bios: None,
            catalog_bios_board: None,
            driver_check: None,
            driver_check_busy: false,
            driver_check_board: None,
        }
    }

    /// Store the client's answer to `GatherQcFirmware`.
    pub fn set_firmware(&mut self, firmware: QcFirmware) {
        self.firmware_requested_at = None;
        self.firmware = Some(firmware);
    }

    /// True while a firmware request is outstanding and not yet timed out.
    fn firmware_pending(&self) -> bool {
        self.firmware_requested_at
            .is_some_and(|t| t.elapsed().as_secs() < FIRMWARE_TIMEOUT_SECS)
    }

    fn firmware_timed_out(&self) -> bool {
        self.firmware_requested_at
            .is_some_and(|t| t.elapsed().as_secs() >= FIRMWARE_TIMEOUT_SECS)
    }

    fn request_firmware(&mut self, cmd_tx: &Sender<Cmd>) {
        if cmd_tx.try_send(Cmd::GatherQcFirmware).is_ok() {
            self.firmware_requested_at = Some(Instant::now());
        }
    }

    /// Board product for catalog lookups: the client's firmware reading first,
    /// then the live sysinfo the session already streams.
    fn baseboard_product(&self, sysinfo: Option<&SystemInformation>) -> Option<String> {
        self.firmware
            .as_ref()
            .and_then(|f| f.baseboard_product.clone())
            .or_else(|| sysinfo.map(|s| s.motherboard_name.clone()))
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
    }

    /// Serials this machine can be identified by, best first. Placeholders are
    /// shared across a whole model, so they would resolve someone else's order.
    fn machine_serials(&self, sysinfo: Option<&SystemInformation>) -> Vec<String> {
        let from_firmware = self
            .firmware
            .as_ref()
            .map(|f| [f.system_serial.clone(), f.board_serial.clone()])
            .unwrap_or_default();
        let from_sysinfo = sysinfo
            .map(|s| [Some(s.product_serial.clone()), Some(s.motherboard_serial.clone())])
            .unwrap_or_default();

        let mut out: Vec<String> = Vec::new();
        for serial in from_firmware.into_iter().chain(from_sysinfo).flatten() {
            let serial = serial.trim().to_string();
            if !is_placeholder_serial(&serial) && !out.contains(&serial) {
                out.push(serial);
            }
        }
        out
    }

    // ---- loads ------------------------------------------------------

    fn start_auto_resolve(&mut self, serials: Vec<String>) {
        if serials.is_empty() {
            return;
        }
        self.auto_resolve_started = true;
        let tx = self.tx.clone();
        PlatformSpawner::spawn(async move {
            let found = resolve_any(&serials).await.map(|s| s.lookup_input());
            let _ = tx.try_send(QcMsg::AutoResolved(found));
        });
    }

    fn start_order_load(&mut self, input: String) {
        let Some(key) = OrderKey::parse(&input) else {
            self.order = Some(Err(
                "Enter a PS order (2…), Everest doc (5…), Shopify order # or XBS- serial.".into(),
            ));
            return;
        };
        self.order_busy = true;
        self.serial_history.clear();
        self.serial_busy.clear();
        let tx = self.tx.clone();
        PlatformSpawner::spawn(async move {
            let backend = QcBackend::for_key_resolved(&key).await;
            let result = backend.find_order(&key).await.map_err(|e| format!("{e:#}"));
            let _ = tx.try_send(QcMsg::Order(Box::new(result)));
        });
    }

    fn start_serial_history(&mut self, serial: String) {
        if self.serial_history.contains_key(&serial) || !self.serial_busy.insert(serial.clone()) {
            return;
        }
        let tx = self.tx.clone();
        PlatformSpawner::spawn(async move {
            let backend = QcBackend::shopify();
            let result = backend.serial_history(&serial).await.map_err(|e| format!("{e:#}"));
            let _ = tx.try_send(QcMsg::SerialHistory { serial, result: Box::new(result) });
        });
    }

    fn start_catalog_bios(&mut self, product: String) {
        self.catalog_bios_board = Some(product.clone());
        let tx = self.tx.clone();
        PlatformSpawner::spawn(async move {
            match bios_for_baseboard(&product).await {
                Ok(b) => {
                    let _ = tx.try_send(QcMsg::CatalogBios(Box::new(b)));
                }
                Err(e) => {
                    let _ = tx.try_send(QcMsg::Status(format!("Catalog BIOS lookup failed: {e}")));
                }
            }
        });
    }

    /// Newest driver snapshot for this client compared against the catalog.
    fn start_driver_check(&mut self, connection_string: String, product: Option<String>) {
        self.driver_check_busy = true;
        self.driver_check_board = product.clone();
        let gpu_codes = self
            .firmware
            .as_ref()
            .map(|f| f.gpu_device_codes.clone())
            .unwrap_or_default();
        let tx = self.tx.clone();
        PlatformSpawner::spawn(async move {
            let result = load_driver_check(&connection_string, product, gpu_codes).await;
            let _ = tx.try_send(QcMsg::DriverCheck(Box::new(result)));
        });
    }

    // ---- render -----------------------------------------------------

    pub fn display(
        &mut self,
        ui: &mut Ui,
        client: &ConnectedClient,
        sysinfo: Option<&SystemInformation>,
        cmd_tx: &Sender<Cmd>,
    ) {
        let mut repaint = false;
        while let Ok(msg) = self.rx.try_recv() {
            repaint = true;
            match msg {
                QcMsg::Order(result) => {
                    self.order_busy = false;
                    self.order = Some(*result);
                }
                QcMsg::AutoResolved(found) => match found {
                    Some(input) => {
                        self.order_input = input.clone();
                        self.start_order_load(input);
                    }
                    None => {
                        self.status =
                            "No order matched this machine's serials — look one up above.".into();
                    }
                },
                QcMsg::SerialHistory { serial, result } => {
                    self.serial_busy.remove(&serial);
                    self.serial_history.insert(serial, *result);
                }
                QcMsg::DriverCheck(result) => {
                    self.driver_check_busy = false;
                    self.driver_check = Some(*result);
                }
                QcMsg::CatalogBios(bios) => self.catalog_bios = *bios,
                QcMsg::Status(s) => self.status = s,
            }
        }
        if repaint {
            ui.ctx().request_repaint();
        }

        // One firmware request per session; everything else keys off its answer.
        if self.firmware.is_none() && self.firmware_requested_at.is_none() && client.connected {
            self.request_firmware(cmd_tx);
        }
        let product = self.baseboard_product(sysinfo);
        if let Some(p) = product.clone() {
            if self.catalog_bios_board.as_deref() != Some(p.as_str()) {
                self.start_catalog_bios(p);
            }
        }
        if !self.driver_check_busy
            && (self.driver_check.is_none() || self.driver_check_board != product)
        {
            self.start_driver_check(client.connection_string.clone(), product.clone());
        }
        if !self.auto_resolve_started && self.order.is_none() {
            let serials = self.machine_serials(sysinfo);
            if !serials.is_empty() {
                self.start_auto_resolve(serials);
            }
        }

        ScrollArea::vertical()
            .id_salt("qc_viewer_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if !self.status.is_empty() {
                    ui.label(RichText::new(&self.status).small().color(theme::weak_text(ui)));
                    ui.add_space(4.0);
                }
                self.render_order(ui);
                ui.add_space(6.0);
                self.render_firmware(ui, cmd_tx, product.as_deref());
                ui.add_space(6.0);
                self.render_driver_check(ui, client, cmd_tx, product.as_deref());
            });
    }

    fn render_order(&mut self, ui: &mut Ui) {
        section_card(ui, icons::PACKAGE, "Order — items & serials", None, |ui| {
            let mut submit = false;
            ui.horizontal_wrapped(|ui| {
                let response = ui.add(
                    TextEdit::singleline(&mut self.order_input)
                        .hint_text("PS order / Everest doc / #Shopify / XBS-…")
                        .desired_width(FIELD_WIDTH),
                );
                submit |= response.lost_focus() && ui.input(|i| i.key_pressed(eframe::egui::Key::Enter));
                submit |= ui
                    .add_enabled(!self.order_busy, Button::new(format!("{} Load", icons::SEARCH)))
                    .clicked();
                if self.order_busy {
                    ui.spinner();
                }
            });
            if submit && !self.order_input.trim().is_empty() {
                let input = self.order_input.trim().to_string();
                self.start_order_load(input);
            }

            ui.add_space(4.0);
            match self.order.as_ref() {
                None => {
                    ui.label(
                        RichText::new(
                            "Resolving this machine's order from its serials — or look one up above.",
                        )
                        .small()
                        .color(theme::weak_text(ui)),
                    );
                }
                Some(Err(e)) => {
                    ui.colored_label(theme::error(ui), RichText::new(e).small());
                }
                Some(Ok(order)) => {
                    render_order_header(ui, order);
                    ui.add_space(6.0);
                    for serial in render_items(ui, order) {
                        self.start_serial_history(serial);
                    }
                    self.render_serial_history(ui);
                }
            }
        });
    }

    fn render_serial_history(&self, ui: &mut Ui) {
        if self.serial_history.is_empty() && self.serial_busy.is_empty() {
            return;
        }
        ui.add_space(6.0);
        ui.separator();
        ui.label(RichText::new("Serial history").strong().small());
        if !self.serial_busy.is_empty() {
            let mut pending: Vec<&str> = self.serial_busy.iter().map(String::as_str).collect();
            pending.sort_unstable();
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    RichText::new(format!("looking up {}…", pending.join(", ")))
                        .small()
                        .color(theme::weak_text(ui)),
                );
            });
        }
        for (serial, result) in &self.serial_history {
            match result {
                Ok(h) => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(serial).monospace().small().strong());
                        if !h.found {
                            ui.colored_label(
                                theme::warn(ui),
                                RichText::new("not found in any system").small(),
                            );
                        }
                        if let Some(order) = h.current_order.as_ref() {
                            ui.label(RichText::new(format!("installed on {order}")).small());
                        }
                        if let Some(lot) = h.odoo_lot.as_ref() {
                            ui.label(
                                RichText::new(format!("Odoo: {lot}"))
                                    .small()
                                    .color(theme::weak_text(ui)),
                            );
                        }
                        if h.prestashop_allocations > 0 {
                            ui.label(
                                RichText::new(format!("PS allocs: {}", h.prestashop_allocations))
                                    .small()
                                    .color(theme::weak_text(ui)),
                            );
                        }
                    });
                    for flag in &h.flags {
                        ui.colored_label(
                            theme::error(ui),
                            RichText::new(format!("{} {flag}", icons::STATUS_WARN)).small(),
                        );
                    }
                }
                Err(e) => {
                    ui.colored_label(theme::error(ui), RichText::new(format!("{serial}: {e}")).small());
                }
            }
        }
    }

    fn render_firmware(&mut self, ui: &mut Ui, cmd_tx: &Sender<Cmd>, product: Option<&str>) {
        section_card(ui, icons::MONITOR, "BIOS & firmware", None, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !self.firmware_pending(),
                        Button::new(format!("{} Re-read firmware", icons::REFRESH)),
                    )
                    .clicked()
                {
                    self.firmware = None;
                    self.request_firmware(cmd_tx);
                }
                if self.firmware_pending() {
                    ui.spinner();
                    ui.label(
                        RichText::new("reading…").small().color(theme::weak_text(ui)),
                    );
                }
            });
            ui.add_space(4.0);

            let Some(f) = self.firmware.as_ref() else {
                let msg = if self.firmware_timed_out() {
                    "The client did not answer. A MasterTech build older than this console cannot \
                     report firmware — push a self-update from the Transfer menu."
                } else {
                    "Waiting for the client's firmware reading."
                };
                ui.label(RichText::new(msg).small().color(theme::weak_text(ui)));
                return;
            };

            kv_row(ui, "Installed BIOS", &opt(&f.bios_version));
            if f.bios_date.is_some() || f.bios_vendor.is_some() {
                kv_row(
                    ui,
                    "BIOS released",
                    &format!("{} · {}", opt(&f.bios_date), opt(&f.bios_vendor)),
                );
            }
            match self.catalog_bios.as_ref() {
                Some(bios) => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new("Latest (catalog)")
                                .small()
                                .color(theme::weak_text(ui)),
                        );
                        if let Some(file) = bios.file_name.as_ref() {
                            ui.label(RichText::new(file).monospace().small());
                        }
                        if !bios.url_webpage.is_empty() {
                            ui.hyperlink_to("manufacturer page", &bios.url_webpage);
                        }
                    });
                }
                None if product.is_some() => {
                    ui.label(
                        RichText::new("No catalog BIOS entry for this board.")
                            .small()
                            .color(theme::weak_text(ui)),
                    );
                }
                None => {}
            }

            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                let (boot_color, boot_text) = if f.boot_mode.starts_with("UEFI") {
                    (theme::success(ui), f.boot_mode.clone())
                } else {
                    (theme::warn(ui), f.boot_mode.clone())
                };
                badge(ui, &boot_text, boot_color);
                badge(ui, &format!("Secure Boot {}", tri(f.secure_boot_enabled)), flag_color(ui, f.secure_boot_enabled));
                let tpm_label = match (f.tpm_present, f.tpm_enabled) {
                    (false, _) => "TPM absent".to_string(),
                    (true, Some(true)) => match f.tpm_spec_version.as_deref() {
                        Some(v) => format!("TPM {} on", v.split(',').next().unwrap_or(v).trim()),
                        None => "TPM on".to_string(),
                    },
                    (true, Some(false)) => "TPM off".to_string(),
                    (true, None) => "TPM present".to_string(),
                };
                let tpm_color = if f.tpm_present && f.tpm_enabled != Some(false) {
                    theme::success(ui)
                } else {
                    theme::warn(ui)
                };
                badge(ui, &tpm_label, tpm_color);
                badge(
                    ui,
                    &format!("Windows {}", match f.windows_activated {
                        Some(true) => "activated",
                        Some(false) => "NOT activated",
                        None => "activation unknown",
                    }),
                    flag_color(ui, f.windows_activated),
                );
                badge(
                    ui,
                    if f.oa3_key_present { "OA3 key present" } else { "no OA3 key" },
                    if f.oa3_key_present { theme::success(ui) } else { theme::weak_text(ui) },
                );
            });

            ui.add_space(4.0);
            kv_row(ui, "Board", &opt(&f.baseboard_product));
            kv_row(ui, "System serial", &opt(&f.system_serial));
            kv_row(ui, "Board serial", &opt(&f.board_serial));
            if let Some(manufacturer) = f.tpm_manufacturer.as_ref() {
                kv_row(ui, "TPM vendor", manufacturer);
            }
        });
    }

    fn render_driver_check(
        &mut self,
        ui: &mut Ui,
        client: &ConnectedClient,
        cmd_tx: &Sender<Cmd>,
        product: Option<&str>,
    ) {
        section_card(ui, icons::HARD_DRIVE, "Driver check", None, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        !self.driver_check_busy,
                        Button::new(format!("{} Re-check", icons::REFRESH)),
                    )
                    .clicked()
                {
                    self.driver_check = None;
                    let cs = client.connection_string.clone();
                    let p = product.map(str::to_string);
                    self.start_driver_check(cs, p);
                }
                if ui
                    .add_enabled(client.connected, Button::new(format!("{} New snapshot", icons::HARD_DRIVE)))
                    .on_hover_text(
                        "Capture a fresh driver inventory on the client via \
                         com.mastertech.driverstore, then re-check.",
                    )
                    .on_disabled_hover_text("The client is not connected.")
                    .clicked()
                {
                    let _ = cmd_tx.try_send(Cmd::CallRemotePluginTool {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        plugin_id: "com.mastertech.driverstore".to_string(),
                        tool_name: "snapshot".to_string(),
                        args_json: "{}".to_string(),
                    });
                    self.status =
                        "Snapshot requested — press Re-check once it lands.".to_string();
                }
                if self.driver_check_busy {
                    ui.spinner();
                }
                if let Some(p) = product {
                    ui.label(RichText::new(p).monospace().small().color(theme::weak_text(ui)));
                }
            });
            ui.add_space(4.0);

            match self.driver_check.as_ref() {
                None => {}
                Some(Err(e)) => {
                    ui.colored_label(theme::error(ui), RichText::new(e).small());
                }
                Some(Ok(rows)) if rows.is_empty() => {
                    ui.label(
                        RichText::new("No catalog driver mapping for this board.")
                            .small()
                            .color(theme::weak_text(ui)),
                    );
                }
                Some(Ok(rows)) => render_driver_rows(ui, rows),
            }
        });
    }
}

fn opt(v: &Option<String>) -> String {
    v.clone().unwrap_or_else(|| "—".to_string())
}

fn tri(v: Option<bool>) -> &'static str {
    match v {
        Some(true) => "on",
        Some(false) => "OFF",
        None => "unknown",
    }
}

fn flag_color(ui: &Ui, v: Option<bool>) -> eframe::egui::Color32 {
    match v {
        Some(true) => theme::success(ui),
        Some(false) => theme::error(ui),
        None => theme::weak_text(ui),
    }
}

fn render_order_header(ui: &mut Ui, order: &QcOrder) {
    glass_card::group(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&order.reference).strong());
            let backend = match order.backend {
                Some(BackendKind::Shopify) => "Shopify",
                Some(BackendKind::Prestashop) => "PrestaShop",
                None => "unknown backend",
            };
            badge(ui, backend, theme::info(ui));
            badge(ui, order.kind.as_str(), theme::accent(ui));
            if !order.status.name.is_empty() {
                badge(ui, &order.status.name, theme::weak_text(ui));
            }
        });
        if !order.customer_name.is_empty() {
            kv_row(ui, "Customer", &order.customer_name);
        }
        if !order.total_paid.is_empty() {
            kv_row(ui, "Total", &order.total_paid);
        }

        if let Some(reason) = order.truncation_reason() {
            ui.colored_label(
                theme::error(ui),
                RichText::new(format!("{} {reason}", icons::CRITICAL)).small(),
            );
        }
        if let Some(key) = order.key.as_ref() {
            let gate = QcBackend::for_key(key).status_gate(order);
            let (color, label) = match gate.outcome {
                GateOutcome::GoodToMove { .. } => (theme::success(ui), "good to move"),
                GateOutcome::RefuseToMove => (theme::error(ui), "REFUSE TO MOVE"),
                GateOutcome::Neutral => (theme::warn(ui), "neutral"),
            };
            ui.horizontal_wrapped(|ui| {
                badge(ui, label, color);
                if !gate.message.is_empty() {
                    ui.label(RichText::new(&gate.message).small().color(theme::weak_text(ui)));
                }
            });
        }
    });
}

/// Line items with serial-attach status. Returns the serials the operator asked
/// to look up.
fn render_items(ui: &mut Ui, order: &QcOrder) -> Vec<String> {
    if order.items.is_empty() {
        ui.label(RichText::new("No line items on this order.").small().color(theme::weak_text(ui)));
        return Vec::new();
    }
    let attached = order.items.iter().filter(|i| i.serial_attached()).count();
    ui.label(
        RichText::new(format!(
            "{attached}/{} items have serials attached",
            order.items.len()
        ))
        .small()
        .color(theme::weak_text(ui)),
    );

    let unattached: Vec<_> = order.items.iter().filter(|i| !i.serial_attached()).collect();
    if !unattached.is_empty() {
        glass_card::group(ui, |ui| {
            ui.colored_label(
                theme::warn(ui),
                RichText::new(format!(
                    "{} {} item(s) not yet committed (no serial)",
                    icons::STATUS_WARN,
                    unattached.len()
                ))
                .strong()
                .small(),
            );
            for it in &unattached {
                ui.label(RichText::new(format!("• {} ({})", it.name, it.reference)).small());
            }
        });
        ui.add_space(4.0);
    }

    let is_shopify = order.backend == Some(BackendKind::Shopify);
    let mut lookup = Vec::new();
    egui_extras::TableBuilder::new(ui)
        .id_salt("qc_items_table")
        .striped(true)
        .column(egui_extras::Column::exact(22.0))
        .column(egui_extras::Column::initial(160.0).at_least(80.0).clip(true))
        .column(egui_extras::Column::initial(120.0).at_least(90.0).clip(true))
        .column(egui_extras::Column::exact(34.0))
        .column(egui_extras::Column::remainder().at_least(110.0))
        .header(20.0, |mut header| {
            header.col(|_| {});
            header.col(|ui| {
                ui.strong("Item");
            });
            header.col(|ui| {
                ui.strong("Ref");
            });
            header.col(|ui| {
                ui.strong("Qty");
            });
            header.col(|ui| {
                ui.strong("Serial");
            });
        })
        .body(|mut body| {
            for item in &order.items {
                body.row(20.0, |mut row| {
                    row.col(|ui| {
                        if item.serial_attached() {
                            ui.colored_label(theme::success(ui), icons::CHECK);
                        } else {
                            ui.colored_label(theme::warn(ui), icons::STATUS_WAIT);
                        }
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(&item.name).small());
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(&item.reference).monospace().small());
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(format!("{:.0}", item.quantity)).small());
                    });
                    row.col(|ui| {
                        if item.serials.is_empty() {
                            ui.label(RichText::new("—").color(theme::weak_text(ui)));
                            return;
                        }
                        ui.horizontal(|ui| {
                            // Federated history only exists on the Shopify/XBM side.
                            if is_shopify
                                && ui
                                    .small_button(icons::SEARCH)
                                    .on_hover_text("Serial history (Shopify · Odoo · PrestaShop)")
                                    .clicked()
                            {
                                lookup = item.serials.clone();
                            }
                            ui.label(RichText::new(item.serials.join(", ")).monospace().small());
                        });
                    });
                });
            }
        });
    lookup
}

fn render_driver_rows(ui: &mut Ui, rows: &[DriverCheckRow]) {
    let missing: Vec<&str> = rows
        .iter()
        .filter(|r| r.status == DriverStatus::Missing)
        .map(|r| r.category.as_str())
        .collect();
    let outdated: Vec<&str> = rows
        .iter()
        .filter(|r| r.status == DriverStatus::Outdated)
        .map(|r| r.category.as_str())
        .collect();
    if missing.is_empty() && outdated.is_empty() {
        ui.colored_label(
            theme::success(ui),
            format!("{} All drivers present and current", icons::CHECK),
        );
    } else {
        if !missing.is_empty() {
            ui.colored_label(
                theme::error(ui),
                format!("{} Missing: {}", icons::CRITICAL, missing.join(", ")),
            );
        }
        if !outdated.is_empty() {
            ui.colored_label(
                theme::warn(ui),
                format!("{} Outdated: {}", icons::STATUS_WARN, outdated.join(", ")),
            );
        }
    }
    ui.add_space(4.0);

    egui_extras::TableBuilder::new(ui)
        .id_salt("qc_driver_check_table")
        .striped(true)
        .column(egui_extras::Column::initial(130.0).at_least(70.0).clip(true))
        .column(egui_extras::Column::remainder().at_least(150.0).clip(true))
        .column(egui_extras::Column::initial(160.0).at_least(90.0).clip(true))
        .column(egui_extras::Column::exact(76.0))
        .header(20.0, |mut h| {
            h.col(|ui| {
                ui.strong("Part");
            });
            h.col(|ui| {
                ui.strong("Installed");
            });
            h.col(|ui| {
                ui.strong("Catalog target");
            });
            h.col(|ui| {
                ui.strong("Status");
            });
        })
        .body(|mut body| {
            for r in rows {
                body.row(20.0, |mut row| {
                    row.col(|ui| {
                        ui.label(RichText::new(&r.category).strong().small());
                    });
                    row.col(|ui| {
                        let text = match (&r.installed_name, &r.installed_version) {
                            (Some(n), Some(v)) => format!("{n}  v{v}"),
                            (Some(n), None) => n.clone(),
                            _ => "—".to_string(),
                        };
                        ui.label(RichText::new(text).small());
                    });
                    row.col(|ui| {
                        let text = match (r.target_file.as_deref(), r.target_version.as_deref()) {
                            (Some(f), Some(v)) => format!("{f}  (v{v})"),
                            (Some(f), None) => f.to_string(),
                            _ => "—".to_string(),
                        };
                        ui.label(RichText::new(text).monospace().small());
                    });
                    row.col(|ui| {
                        let (color, text) = match r.status {
                            DriverStatus::Installed => (theme::success(ui), "installed"),
                            DriverStatus::Outdated => (theme::warn(ui), "OUTDATED"),
                            DriverStatus::Missing => (theme::error(ui), "MISSING"),
                            DriverStatus::NoTarget => (theme::weak_text(ui), "info"),
                        };
                        ui.colored_label(color, RichText::new(text).small());
                    });
                });
            }
        });
}
/// Newest snapshot + catalog targets, compared. Errors carry what is missing so
/// the operator knows which half to fix.
///
/// Refuses to render a comparison with no targets at all: every part would come
/// back `Installed` for want of anything to check it against, which reads as a
/// clean pass rather than as an unanswered question.
async fn load_driver_check(
    connection_string: &str,
    product: Option<String>,
    gpu_codes: Vec<String>,
) -> Result<Vec<DriverCheckRow>, String> {
    let Some(product) = product else {
        return Err(
            "No board product reported yet — the driver check needs the client's firmware \n             reading, or its live system info."
                .to_string(),
        );
    };

    let snapshots = DriverSnapshot::list_for_connection(connection_string, 1)
        .await
        .map_err(|e| format!("snapshot load failed: {e}"))?;
    let Some(snapshot) = snapshots.into_iter().next() else {
        return Err(
            "No driver snapshot for this machine yet — take one with New snapshot (needs \n             com.mastertech.driverstore deployed on the client)."
                .to_string(),
        );
    };

    let package = package_drivers_for_baseboard(&product)
        .await
        .map_err(|e| format!("catalog lookup failed: {e}"))?;
    let Some(package) = package else {
        return Err(format!(
            "'{product}' is not in the driver catalog, so there is nothing to check against. \n             Import the catalog (database-tools --bin import-driver-catalog), or add the board."
        ));
    };

    let mut gpu_targets: Vec<TargetDriver> = Vec::new();
    for code in gpu_codes {
        if let Ok(Some(target)) = gpu_driver_for_device(&code).await {
            gpu_targets.push(target);
        }
    }

    if package.is_empty() && gpu_targets.is_empty() {
        return Err(format!(
            "'{product}' is in the catalog but has no driver targets mapped, so every part would \n             report as current without being checked."
        ));
    }

    Ok(build_driver_check(&snapshot.drivers, &package, &gpu_targets))
}
