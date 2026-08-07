//! Fleet Intel view: crash-signature intelligence + driver time machine for one client.
//!
//! Crash side: browse/search fleet `crash_signature` rows, inspect sightings and
//! verdicts, record new verdicts, and drive dump-decode analysis on the client.
//! Driver side: take pnputil snapshots, review snapshot history and drift, and
//! maintain the `known_bad_driver` blocklist.

use crossbeam::channel::{unbounded, Receiver, Sender};
use database::schema::{
    crash_intel::{CrashIngest, CrashSighting, CrashSignature, CrashVerdict},
    driver_intel::{
        diff_driver_sets, DriverChange, DriverRecord, DriverSnapshot, KnownBadDriver, KnownBadHit,
    },
    ConnectedClient, RecordId, RecordIdExt,
};
use eframe::egui::{
    Align, Button, CollapsingHeader, Color32, ComboBox, Frame, Layout, Margin, RichText, ScrollArea,
    TextEdit, Ui, Vec2,
};
use std::collections::HashSet;

use crate::ui_tools::info_card::{
    badge, expandable_text, fmt_date, fmt_date_time, kv_row, markup_leak, section_card,
    truncate_chars, wrapped_text,
};
use crate::ui_tools::{icons, theme};
use crate::{Cmd, PlatformSpawner, Spawner};
use crate::ui_tools::glass_card;

const DUMP_DECODE: &str = "com.mastertech.dump-decode";
const DRIVERSTORE: &str = "com.mastertech.driverstore";
const SNAPSHOT_LABELS: [&str; 4] = ["intake", "pre_service", "post_service", "manual"];
/// `known_bad_driver.severity` domain.
const SEVERITIES: [&str; 3] = ["info", "warn", "critical"];
const CRASH_SPLIT_BREAKPOINT: f32 = 900.0;
const STAT_STRIP_BREAKPOINT: f32 = 620.0;
const CRASH_LIST_MAX_HEIGHT: f32 = 360.0;
const COMBO_WIDTH: f32 = 110.0;
const FIELD_MIN_WIDTH: f32 = 150.0;
const FIELD_MAX_WIDTH: f32 = 320.0;
const PROSE_LINES: usize = 3;
const SIGNATURE_TITLE_CHARS: usize = 44;
const MODULE_LABEL_CHARS: usize = 28;
const INLINE_META_CHARS: usize = 40;
const DRIFT_ROWS_SHOWN: usize = 12;

enum FleetIntelMsg {
    Signatures(Vec<(CrashSignature, Vec<CrashVerdict>)>),
    Sightings(Vec<CrashSighting>),
    Snapshots(Vec<DriverSnapshot>),
    Blocklist(Vec<KnownBadDriver>),
    Status(String),
}

pub struct FleetIntelViewer {
    tx: Sender<FleetIntelMsg>,
    rx: Receiver<FleetIntelMsg>,
    loaded_once: bool,
    status: String,

    search: String,
    signatures: Vec<(CrashSignature, Vec<CrashVerdict>)>,
    selected_signature: Option<usize>,
    sightings: Vec<CrashSighting>,
    verdict_text: String,
    verdict_fix: String,
    verdict_confidence: String,

    triage_with_ai: bool,
    snapshots: Vec<DriverSnapshot>,
    snapshot_label: String,
    blocklist: Vec<KnownBadDriver>,
    kbd_module: String,
    kbd_versions: String,
    kbd_symptom: String,
    kbd_fix: String,
    kbd_severity: String,
}

impl Default for FleetIntelViewer {
    fn default() -> Self {
        Self::new()
    }
}

impl FleetIntelViewer {
    pub fn new() -> Self {
        let (tx, rx) = unbounded();
        Self {
            tx,
            rx,
            loaded_once: false,
            status: String::new(),
            search: String::new(),
            signatures: Vec::new(),
            selected_signature: None,
            sightings: Vec::new(),
            verdict_text: String::new(),
            verdict_fix: String::new(),
            verdict_confidence: "medium".to_string(),
            triage_with_ai: true,
            snapshots: Vec::new(),
            snapshot_label: "intake".to_string(),
            blocklist: Vec::new(),
            kbd_module: String::new(),
            kbd_versions: String::new(),
            kbd_symptom: String::new(),
            kbd_fix: String::new(),
            kbd_severity: "warn".to_string(),
        }
    }

    fn load_signatures(&self, term: String) {
        let tx = self.tx.clone();
        PlatformSpawner::spawn(async move {
            let result = if term.trim().is_empty() {
                CrashSignature::recent(30).await
            } else {
                CrashSignature::search(term.trim(), 30).await
            };
            match result {
                Ok(sigs) => {
                    let mut rows = Vec::with_capacity(sigs.len());
                    for sig in sigs {
                        let verdicts = CrashSignature::verdicts(&sig.id, 3).await.unwrap_or_default();
                        rows.push((sig, verdicts));
                    }
                    let _ = tx.try_send(FleetIntelMsg::Signatures(rows));
                }
                Err(e) => {
                    let _ = tx.try_send(FleetIntelMsg::Status(format!("Signature load failed: {e}")));
                }
            }
        });
    }

    fn load_sightings(&self, signature: &CrashSignature) {
        let tx = self.tx.clone();
        let sig_id = signature.id.clone();
        PlatformSpawner::spawn(async move {
            match CrashSignature::sightings(&sig_id, 15).await {
                Ok(s) => {
                    let _ = tx.try_send(FleetIntelMsg::Sightings(s));
                }
                Err(e) => {
                    let _ = tx.try_send(FleetIntelMsg::Status(format!("Sighting load failed: {e}")));
                }
            }
        });
    }

    fn load_driver_data(&self, connection_string: String) {
        let tx = self.tx.clone();
        PlatformSpawner::spawn(async move {
            match DriverSnapshot::list_for_connection(&connection_string, 10).await {
                Ok(s) => {
                    let _ = tx.try_send(FleetIntelMsg::Snapshots(s));
                }
                Err(e) => {
                    let _ = tx.try_send(FleetIntelMsg::Status(format!("Snapshot load failed: {e}")));
                }
            }
            match KnownBadDriver::active().await {
                Ok(b) => {
                    let _ = tx.try_send(FleetIntelMsg::Blocklist(b));
                }
                Err(e) => {
                    let _ = tx.try_send(FleetIntelMsg::Status(format!("Blocklist load failed: {e}")));
                }
            }
        });
    }

    fn call_remote_tool(cmd_tx: &Sender<Cmd>, plugin_id: &str, tool_name: &str) {
        let _ = cmd_tx.try_send(Cmd::CallRemotePluginTool {
            request_id: uuid::Uuid::new_v4().to_string(),
            plugin_id: plugin_id.to_string(),
            tool_name: tool_name.to_string(),
            args_json: "{}".to_string(),
        });
    }

    fn refresh_all(&mut self, connection_string: &str) {
        self.load_signatures(self.search.clone());
        self.load_driver_data(connection_string.to_string());
    }

    pub fn display(&mut self, ui: &mut Ui, client: &ConnectedClient, cmd_tx: &Sender<Cmd>) {
        let mut repaint = false;
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                FleetIntelMsg::Signatures(rows) => {
                    self.signatures = rows;
                    self.selected_signature = None;
                    self.sightings.clear();
                }
                FleetIntelMsg::Sightings(s) => self.sightings = s,
                FleetIntelMsg::Snapshots(s) => self.snapshots = s,
                FleetIntelMsg::Blocklist(b) => self.blocklist = b,
                FleetIntelMsg::Status(s) => self.status = s,
            }
            repaint = true;
        }
        if repaint {
            ui.ctx().request_repaint();
        }
        if !self.loaded_once {
            self.loaded_once = true;
            self.refresh_all(&client.connection_string);
        }

        ScrollArea::vertical()
            .id_salt("fleet_intel_scroll")
            .show(ui, |ui| {
                self.page_header(ui, client, cmd_tx);
                if !self.status.is_empty() {
                    ui.label(RichText::new(&self.status).color(theme::warn(ui)).small());
                }
                ui.add_space(6.0);
                self.crash_section(ui, client, cmd_tx);
                ui.add_space(12.0);
                self.driver_section(ui, client, cmd_tx);
            });
    }

    fn page_header(&mut self, ui: &mut Ui, client: &ConnectedClient, cmd_tx: &Sender<Cmd>) {
        glass_card::group(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{} Fleet Intel", icons::DIAGNOSTICS))
                        .color(theme::accent(ui))
                        .heading(),
                );
                ui.add_space(12.0);
                let running = crate::plugins::intake_autopilot::triage_running();
                let label = if running {
                    format!("{} Triage running…", icons::STATUS_WAIT)
                } else {
                    format!("{} Run Intake Triage", icons::LIGHTBULB)
                };
                if ui
                    .add_enabled(!running, Button::new(label))
                    .on_hover_text(
                        "Full intake suite: crash survey, driver inventory, DriverStore snapshot, WinDbg batch analysis, fleet-intel matching, and an AI-drafted verdict into the diagnostic session.",
                    )
                    .clicked()
                {
                    crate::plugins::intake_autopilot::run_intake_triage(
                        client.clone(),
                        cmd_tx.clone(),
                        self.triage_with_ai,
                    );
                }
                ui.checkbox(&mut self.triage_with_ai, "AI verdict draft");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("conn: {}", client.connection_string))
                            .small()
                            .monospace()
                            .color(theme::weak_text(ui)),
                    );
                });
            });
            ui.allocate_space(Vec2::new(ui.available_width(), 0.0));
        });
    }

    fn crash_section(&mut self, ui: &mut Ui, client: &ConnectedClient, cmd_tx: &Sender<Cmd>) {
        section_card(ui, icons::DIAGNOSTICS, "Crash Intelligence", None, |ui| {
            self.crash_toolbar(ui, cmd_tx);
            self.crash_summary_strip(ui);
            let ingests =
                crate::plugins::crash_intel_hooks::latest_ingests(&client.connection_string);
            if !ingests.is_empty() {
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Latest analysis on this client")
                        .strong()
                        .color(theme::strong_text(ui)),
                );
                for ingest in &ingests {
                    self.ingest_row(ui, ingest);
                }
            }
            ui.add_space(6.0);
            if self.signatures.is_empty() {
                ui.colored_label(theme::weak_text(ui), "No fleet crash signatures loaded.");
                return;
            }
            if ui.available_width() < CRASH_SPLIT_BREAKPOINT {
                self.signature_list(ui);
                ui.add_space(8.0);
                self.signature_detail(ui);
            } else {
                ui.columns(2, |cols| {
                    self.signature_list(&mut cols[0]);
                    self.signature_detail(&mut cols[1]);
                });
            }
        });
    }

    fn crash_toolbar(&mut self, ui: &mut Ui, cmd_tx: &Sender<Cmd>) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(format!("{} Analyze dumps", icons::PLAY))
                .on_hover_text("Start detached WinDbg batch analysis of the 8 newest minidumps on the client (dump-decode run_analyze_batch).")
                .clicked()
            {
                Self::call_remote_tool(cmd_tx, DUMP_DECODE, "run_analyze_batch");
                self.status =
                    "Batch analysis started on client — fetch results in ~1-2 min.".to_string();
            }
            if ui
                .button(format!("{} Fetch results", icons::DOWNLOAD))
                .on_hover_text("Read the batch result. Signatures auto-ingest into fleet intel when done.")
                .clicked()
            {
                Self::call_remote_tool(cmd_tx, DUMP_DECODE, "read_batch");
            }
            if ui
                .button(format!("{} Live-kernel", icons::FLASK))
                .on_hover_text("Analyze the newest LiveKernelReports dump (GPU/DPC watchdog hangs that never blue-screen).")
                .clicked()
            {
                Self::call_remote_tool(cmd_tx, DUMP_DECODE, "run_analyze_livekernel");
            }
            ui.separator();
            ui.add(
                TextEdit::singleline(&mut self.search)
                    .hint_text("module / bugcheck")
                    .desired_width(160.0),
            );
            if ui.button(format!("{} Search", icons::SEARCH)).clicked() {
                self.load_signatures(self.search.clone());
            }
            if ui.button(format!("{} Recent", icons::REFRESH)).clicked() {
                self.search.clear();
                self.load_signatures(String::new());
            }
        });
        ui.separator();
    }

    fn crash_summary_strip(&self, ui: &mut Ui) {
        let sightings: u64 = self
            .signatures
            .iter()
            .map(|(sig, _)| u64::from(sig.sighting_count))
            .sum();
        let machines: HashSet<&str> = self
            .signatures
            .iter()
            .flat_map(|(sig, _)| sig.machines.iter().map(String::as_str))
            .collect();
        let first = self.signatures.iter().map(|(sig, _)| sig.first_seen).min();
        let last = self.signatures.iter().map(|(sig, _)| sig.last_seen).max();
        let span = match (first, last) {
            (Some(f), Some(l)) => {
                format!("{} {} {}", fmt_date(&f), icons::ARROW_RIGHT, fmt_date(&l))
            }
            _ => "no data".to_string(),
        };
        let tiles = [
            (icons::DIAGNOSTICS, "Signatures", self.signatures.len().to_string()),
            (icons::LIST, "Sightings", sightings.to_string()),
            (icons::DESKTOP, "Machines", machines.len().to_string()),
            (icons::STATUS_QUEUED, "Loaded span", span),
        ];
        let cols = if ui.available_width() < STAT_STRIP_BREAKPOINT {
            2
        } else {
            tiles.len()
        };
        ui.columns(cols, |columns| {
            for (idx, tile) in tiles.iter().enumerate() {
                crash_stat_tile(&mut columns[idx % cols], tile.0, tile.1, &tile.2);
            }
        });
    }

    fn signature_list(&mut self, ui: &mut Ui) {
        ui.label(
            RichText::new(format!("Signatures ({})", self.signatures.len()))
                .strong()
                .color(theme::strong_text(ui)),
        );
        let picked = ScrollArea::vertical()
            .id_salt("crash_sig_list")
            .max_height(CRASH_LIST_MAX_HEIGHT)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                let mut picked = None;
                for (idx, (sig, verdicts)) in self.signatures.iter().enumerate() {
                    if signature_row(ui, sig, verdicts, self.selected_signature == Some(idx)) {
                        picked = Some(idx);
                    }
                }
                picked
            })
            .inner;
        if let Some(idx) = picked {
            self.selected_signature = Some(idx);
            self.sightings.clear();
            if let Some((sig, _)) = self.signatures.get(idx) {
                self.load_sightings(sig);
            }
        }
    }

    fn signature_detail(&mut self, ui: &mut Ui) {
        let count = self.signatures.len();
        let Some(idx) = self.selected_signature.filter(|i| *i < count) else {
            let weak = theme::weak_text(ui);
            glass_card::group(ui, |ui| {
                ui.colored_label(
                    weak,
                    "Select a signature to inspect its sightings and verdicts.",
                );
                ui.allocate_space(Vec2::new(ui.available_width(), 0.0));
            });
            return;
        };
        let signature = self.signatures[idx].0.id.clone();
        let mut save = false;
        glass_card::group(ui, |ui| {
            let (sig, verdicts) = &self.signatures[idx];
            signature_overview(ui, sig);
            ui.add_space(6.0);
            sightings_block(ui, &self.sightings);
            ui.add_space(6.0);
            verdict_block(ui, verdicts);
            ui.add_space(6.0);
            save = verdict_form(
                ui,
                &mut self.verdict_text,
                &mut self.verdict_fix,
                &mut self.verdict_confidence,
            );
            ui.allocate_space(Vec2::new(ui.available_width(), 0.0));
        });
        if save {
            self.save_verdict(&signature);
        }
    }

    fn ingest_row(&self, ui: &mut Ui, ingest: &CrashIngest) {
        let weak = theme::weak_text(ui);
        let strong = theme::strong_text(ui);
        let accent2 = theme::accent_secondary(ui);
        let warn = theme::warn(ui);
        let (glyph, tint) = if ingest.previously_seen {
            (icons::STATUS_WARN, warn)
        } else {
            (icons::STATUS_ON, theme::success(ui))
        };
        glass_card::group(ui, |ui| {
            ui.horizontal(|ui| {
                ui.colored_label(tint, glyph);
                ui.label(
                    RichText::new(&ingest.signature.bugcheck_code)
                        .strong()
                        .monospace()
                        .color(strong),
                );
                ui.label(
                    RichText::new(truncate_chars(&ingest.signature.module, MODULE_LABEL_CHARS))
                        .monospace()
                        .small()
                        .color(accent2),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(fmt_date(&ingest.signature.last_seen))
                            .small()
                            .monospace()
                            .color(weak),
                    );
                    badge(
                        ui,
                        &format!(
                            "{} prior on {} machine(s)",
                            ingest.prior_sighting_count, ingest.prior_machine_count
                        ),
                        accent2,
                    );
                });
            });
            if let Some(v) = ingest.verdicts.first() {
                if verdict_is_malformed(v) {
                    badge(ui, &format!("{} malformed AI draft", icons::STATUS_WARN), warn);
                }
                expandable_text(
                    ui,
                    &format!("ingest_{}", v.id.key_string()),
                    &v.verdict,
                    PROSE_LINES,
                );
            }
            ui.allocate_space(Vec2::new(ui.available_width(), 0.0));
        });
    }

    fn driver_section(&mut self, ui: &mut Ui, client: &ConnectedClient, cmd_tx: &Sender<Cmd>) {
        self.time_machine_card(ui, client, cmd_tx);
        ui.add_space(10.0);
        self.blocklist_card(ui, client);
    }

    fn time_machine_card(&mut self, ui: &mut Ui, client: &ConnectedClient, cmd_tx: &Sender<Cmd>) {
        let mut take_snapshot = false;
        let mut refresh = false;
        section_card(ui, icons::HARD_DRIVE, "Driver Time Machine", None, |ui| {
            let weak = theme::weak_text(ui);
            let bad = theme::error(ui);
            ui.horizontal_wrapped(|ui| {
                ComboBox::from_id_salt("snapshot_label")
                    .width(COMBO_WIDTH)
                    .selected_text(self.snapshot_label.as_str())
                    .show_ui(ui, |ui| {
                        for label in SNAPSHOT_LABELS {
                            ui.selectable_value(&mut self.snapshot_label, label.to_string(), label);
                        }
                    });
                if ui
                    .button(format!("{} Snapshot now", icons::PLAY))
                    .on_hover_text("Capture the full DriverStore inventory (pnputil) and persist it as a snapshot.")
                    .clicked()
                {
                    take_snapshot = true;
                }
                if ui.button(format!("{} Refresh", icons::REFRESH)).clicked() {
                    refresh = true;
                }
                ui.label(
                    RichText::new(format!("{} snapshot(s) held", self.snapshots.len()))
                        .small()
                        .color(weak),
                );
            });
            ui.add_space(6.0);
            if self.snapshots.is_empty() {
                ui.colored_label(weak, "No driver snapshots recorded for this client yet.");
                return;
            }
            for (idx, snap) in self.snapshots.iter().enumerate() {
                render_snapshot_row(ui, snap, idx == 0);
            }
            if let (Some(newer), Some(older)) = (self.snapshots.first(), self.snapshots.get(1)) {
                ui.add_space(8.0);
                render_drift(ui, older, newer);
            }
            if let Some(latest) = self.snapshots.first() {
                let hits = KnownBadDriver::match_inventory(&self.blocklist, &latest.drivers);
                if !hits.is_empty() {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.colored_label(bad, icons::STATUS_WARN);
                        ui.label(
                            RichText::new(format!(
                                "{} known-bad driver(s) in the newest snapshot",
                                hits.len()
                            ))
                            .strong()
                            .color(bad),
                        );
                    });
                    for hit in &hits {
                        render_known_bad_hit(ui, hit);
                    }
                }
            }
        });
        if take_snapshot {
            crate::plugins::driver_intel_hooks::set_pending_label(
                &client.connection_string,
                &self.snapshot_label,
            );
            Self::call_remote_tool(cmd_tx, DRIVERSTORE, "snapshot");
            self.status = "Driver snapshot requested — refresh in a few seconds.".to_string();
        }
        if refresh {
            self.load_driver_data(client.connection_string.clone());
        }
    }

    fn blocklist_card(&mut self, ui: &mut Ui, client: &ConnectedClient) {
        let mut submit = false;
        let title = format!("Blocklist ({} active)", self.blocklist.len());
        section_card(ui, icons::STATUS_DISABLED, &title, None, |ui| {
            let weak = theme::weak_text(ui);
            let strong = theme::strong_text(ui);
            if self.blocklist.is_empty() {
                ui.colored_label(weak, "No active known-bad drivers in the fleet blocklist.");
            } else {
                for entry in &self.blocklist {
                    render_blocklist_card(ui, entry);
                }
            }
            ui.add_space(8.0);
            ui.separator();
            ui.label(
                RichText::new(format!("{} Add known-bad driver", icons::PLUS))
                    .strong()
                    .color(strong),
            );
            let full_width = ui.available_width();
            // Two equal fields minus the severity combo and their two spacing gaps.
            let field_width = ((full_width - COMBO_WIDTH - ui.spacing().item_spacing.x * 2.0) / 2.0)
                .clamp(FIELD_MIN_WIDTH, FIELD_MAX_WIDTH);
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    TextEdit::singleline(&mut self.kbd_module)
                        .hint_text("module (rtwlane)")
                        .desired_width(field_width),
                );
                ui.add(
                    TextEdit::singleline(&mut self.kbd_versions)
                        .hint_text("bad versions, comma-separated (blank = every version)")
                        .desired_width(field_width),
                );
                severity_combo(ui, &mut self.kbd_severity);
            });
            ui.add(
                TextEdit::multiline(&mut self.kbd_symptom)
                    .hint_text("symptom the tech sees on the bench")
                    .desired_rows(3)
                    .desired_width(full_width),
            );
            ui.add(
                TextEdit::multiline(&mut self.kbd_fix)
                    .hint_text("recommended fix")
                    .desired_rows(3)
                    .desired_width(full_width),
            );
            let can_add = !self.kbd_module.trim().is_empty();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(can_add, Button::new(format!("{} Add to blocklist", icons::PLUS)))
                    .on_hover_text("Persist a fleet-wide known-bad driver entry.")
                    .clicked()
                {
                    submit = true;
                }
                if !can_add {
                    ui.colored_label(weak, "module is required");
                }
            });
        });
        if submit {
            self.add_blocklist_entry(&client.connection_string);
        }
    }

    fn add_blocklist_entry(&mut self, connection_string: &str) {
        let entry = KnownBadDriver {
            id: database::schema::random_record_id(database::schema::KNOWN_BAD_DRIVER_TABLE),
            module: self.kbd_module.trim().to_string(),
            display_name: String::new(),
            vendor: String::new(),
            bad_versions: self
                .kbd_versions
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            fixed_version: None,
            symptom: self.kbd_symptom.trim().to_string(),
            fix: self.kbd_fix.trim().to_string(),
            severity: self.kbd_severity.clone(),
            signature_ref: None,
            active: true,
            created_at: chrono::Utc::now().into(),
            updated_at: chrono::Utc::now().into(),
        };
        let tx = self.tx.clone();
        PlatformSpawner::spawn(async move {
            let msg = match KnownBadDriver::create(&entry).await {
                Ok(_) => "Blocklist entry added".to_string(),
                Err(e) => format!("Blocklist add failed: {e}"),
            };
            let _ = tx.try_send(FleetIntelMsg::Status(msg));
        });
        self.kbd_module.clear();
        self.kbd_versions.clear();
        self.kbd_symptom.clear();
        self.kbd_fix.clear();
        self.load_driver_data(connection_string.to_string());
    }

    fn save_verdict(&mut self, signature: &RecordId) {
        let tx = self.tx.clone();
        let sig_id = signature.clone();
        let verdict = self.verdict_text.trim().to_string();
        let fix = self.verdict_fix.trim().to_string();
        let confidence = self.verdict_confidence.clone();
        let author = crate::get_current_user_from_auth()
            .map(|u| u.get_name().to_string())
            .unwrap_or_default();
        PlatformSpawner::spawn(async move {
            let msg = match CrashVerdict::create(
                &sig_id, &verdict, &fix, &confidence, &author, "tech", None,
            )
            .await
            {
                Ok(id) => format!("Verdict recorded ({})", id.key_string()),
                Err(e) => format!("Verdict save failed: {e}"),
            };
            let _ = tx.try_send(FleetIntelMsg::Status(msg));
        });
        self.verdict_text.clear();
        self.verdict_fix.clear();
        self.load_signatures(self.search.clone());
    }
}

fn crash_stat_tile(ui: &mut Ui, glyph: &str, label: &str, value: &str) {
    let weak = theme::weak_text(ui);
    let strong = theme::strong_text(ui);
    glass_card::group(ui, |ui| {
        ui.horizontal(|ui| {
            ui.colored_label(theme::accent(ui), glyph);
            ui.label(RichText::new(label).small().color(weak));
        });
        ui.label(RichText::new(value).strong().monospace().color(strong));
        ui.allocate_space(Vec2::new(ui.available_width(), 0.0));
    });
}

/// Returns true when the row was clicked.
fn signature_row(
    ui: &mut Ui,
    sig: &CrashSignature,
    verdicts: &[CrashVerdict],
    selected: bool,
) -> bool {
    let weak = theme::weak_text(ui);
    let strong = theme::strong_text(ui);
    let accent2 = theme::accent_secondary(ui);
    let good = theme::success(ui);
    let warn = theme::warn(ui);
    let title = if sig.bugcheck_name.is_empty() {
        sig.bugcheck_code.clone()
    } else {
        format!("{} {}", sig.bugcheck_code, sig.bugcheck_name)
    };
    let malformed = verdicts.iter().any(verdict_is_malformed);
    let mut clicked = false;
    glass_card::group(ui, |ui| {
        ui.horizontal(|ui| {
            clicked = ui
                .selectable_label(
                    selected,
                    RichText::new(truncate_chars(&title, SIGNATURE_TITLE_CHARS))
                        .strong()
                        .color(strong),
                )
                .on_hover_text(title.as_str())
                .clicked();
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if malformed {
                    badge(ui, &format!("{} malformed draft", icons::STATUS_WARN), warn);
                } else if let Some(v) = verdicts.first() {
                    let text = if v.confidence.is_empty() {
                        format!("{} verdict", icons::STATUS_ON)
                    } else {
                        format!("{} verdict: {}", icons::STATUS_ON, v.confidence)
                    };
                    badge(ui, &text, good);
                } else {
                    badge(ui, "no verdict", weak);
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(truncate_chars(&sig.module, MODULE_LABEL_CHARS))
                    .monospace()
                    .small()
                    .color(accent2),
            );
            ui.label(
                RichText::new(format!(
                    "{} sighting(s) / {} machine(s)",
                    sig.sighting_count,
                    sig.machines.len()
                ))
                .small()
                .color(weak),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(fmt_date(&sig.last_seen))
                        .small()
                        .monospace()
                        .color(weak),
                );
            });
        });
        ui.allocate_space(Vec2::new(ui.available_width(), 0.0));
    });
    clicked
}

fn signature_overview(ui: &mut Ui, sig: &CrashSignature) {
    let weak = theme::weak_text(ui);
    let strong = theme::strong_text(ui);
    let accent2 = theme::accent_secondary(ui);
    ui.horizontal(|ui| {
        ui.colored_label(theme::accent(ui), icons::CRITICAL);
        ui.label(RichText::new(&sig.bugcheck_code).strong().monospace().color(strong));
        if !sig.bugcheck_name.is_empty() {
            ui.label(RichText::new(&sig.bugcheck_name).strong().color(strong));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            badge(ui, &format!("{} machine(s)", sig.machines.len()), accent2);
            badge(ui, &format!("{} sighting(s)", sig.sighting_count), accent2);
        });
    });
    kv_row(ui, "Module", &sig.module);
    kv_row(ui, "First seen", &fmt_date(&sig.first_seen));
    kv_row(ui, "Last seen", &fmt_date(&sig.last_seen));
    if !sig.offsets.is_empty() {
        kv_row(ui, "Offsets", &sig.offsets.join(", "));
    }
    if !sig.module_versions.is_empty() {
        kv_row(ui, "Module versions", &sig.module_versions.join(", "));
    }
    if !sig.tags.is_empty() {
        kv_row(ui, "Tags", &sig.tags.join(", "));
    }
    if !sig.failure_buckets.is_empty() {
        ui.label(RichText::new("Failure buckets").small().color(weak));
        wrapped_text(ui, &sig.failure_buckets.join(", "), strong);
    }
    if !sig.machines.is_empty() {
        ui.label(RichText::new("Machines").small().color(weak));
        wrapped_text(ui, &sig.machines.join(", "), strong);
    }
}

fn sightings_block(ui: &mut Ui, sightings: &[CrashSighting]) {
    let weak = theme::weak_text(ui);
    let strong = theme::strong_text(ui);
    let accent2 = theme::accent_secondary(ui);
    if sightings.is_empty() {
        ui.colored_label(weak, "No sightings loaded for this signature.");
        return;
    }
    ui.label(
        RichText::new(format!("Sightings ({})", sightings.len()))
            .strong()
            .color(strong),
    );
    for s in sightings {
        glass_card::group(ui, |ui| {
            ui.horizontal(|ui| {
                ui.colored_label(theme::accent(ui), icons::FILE_TEXT);
                ui.label(
                    RichText::new(s.dump_name.as_deref().unwrap_or("-"))
                        .monospace()
                        .small()
                        .color(strong),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(fmt_date_time(&s.created_at))
                            .small()
                            .monospace()
                            .color(weak),
                    );
                    if !s.dump_kind.is_empty() {
                        badge(ui, &s.dump_kind, accent2);
                    }
                });
            });
            ui.label(
                RichText::new(s.connection_string.as_deref().unwrap_or("?"))
                    .small()
                    .monospace()
                    .color(weak),
            );
            if let Some(bucket) = s.failure_bucket.as_deref() {
                wrapped_text(ui, bucket, weak);
            }
            ui.allocate_space(Vec2::new(ui.available_width(), 0.0));
        });
    }
}

fn verdict_block(ui: &mut Ui, verdicts: &[CrashVerdict]) {
    let weak = theme::weak_text(ui);
    let strong = theme::strong_text(ui);
    let accent = theme::accent(ui);
    let accent2 = theme::accent_secondary(ui);
    let good = theme::success(ui);
    let warn = theme::warn(ui);
    if verdicts.is_empty() {
        ui.colored_label(weak, "No verdict recorded for this signature yet.");
        return;
    }
    ui.label(
        RichText::new(format!("Verdicts ({})", verdicts.len()))
            .strong()
            .color(strong),
    );
    for v in verdicts {
        let malformed = verdict_is_malformed(v);
        let key = v.id.key_string();
        glass_card::group(ui, |ui| {
            ui.horizontal(|ui| {
                let (glyph, tint) = if malformed {
                    (icons::STATUS_WARN, warn)
                } else {
                    (icons::STATUS_ON, good)
                };
                ui.colored_label(tint, glyph);
                let author = if v.author.is_empty() { "unattributed" } else { v.author.as_str() };
                ui.label(RichText::new(author).strong().color(strong));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(fmt_date(&v.created_at))
                            .small()
                            .monospace()
                            .color(weak),
                    );
                    if malformed {
                        badge(ui, &format!("{} malformed AI draft", icons::STATUS_WARN), warn);
                    }
                    if !v.source.is_empty() {
                        badge(ui, &v.source, accent2);
                    }
                    if !v.confidence.is_empty() {
                        badge(ui, &v.confidence, accent);
                    }
                });
            });
            if malformed {
                ui.label(
                    RichText::new("Stored text contains raw tool-call markup; shown verbatim.")
                        .small()
                        .color(warn),
                );
            }
            expandable_text(ui, &format!("verdict_{key}"), &v.verdict, PROSE_LINES);
            if !v.fix.is_empty() {
                ui.label(RichText::new("Fix").small().color(weak));
                expandable_text(ui, &format!("verdict_fix_{key}"), &v.fix, PROSE_LINES);
            }
            ui.allocate_space(Vec2::new(ui.available_width(), 0.0));
        });
    }
}

/// True when the stored verdict or fix text carries raw tool-call markup.
fn verdict_is_malformed(v: &CrashVerdict) -> bool {
    markup_leak(&v.verdict) || markup_leak(&v.fix)
}

/// Returns true when the operator clicked save.
fn verdict_form(ui: &mut Ui, text: &mut String, fix: &mut String, confidence: &mut String) -> bool {
    let strong = theme::strong_text(ui);
    ui.separator();
    ui.label(
        RichText::new(format!("{} Record verdict", icons::EDIT))
            .strong()
            .color(strong),
    );
    let width = ui.available_width();
    ui.add(
        TextEdit::multiline(text)
            .hint_text("diagnosis")
            .desired_rows(2)
            .desired_width(width),
    );
    ui.add(
        TextEdit::multiline(fix)
            .hint_text("fix that worked")
            .desired_rows(2)
            .desired_width(width),
    );
    let mut save = false;
    ui.horizontal(|ui| {
        ComboBox::from_id_salt("verdict_confidence")
            .width(COMBO_WIDTH)
            .selected_text(confidence.as_str())
            .show_ui(ui, |ui| {
                for c in ["low", "medium", "high", "confirmed"] {
                    ui.selectable_value(confidence, c.to_string(), c);
                }
            });
        let can_save = !text.trim().is_empty();
        if ui
            .add_enabled(can_save, Button::new(format!("{} Save verdict", icons::SAVE)))
            .clicked()
        {
            save = true;
        }
    });
    save
}

fn severity_combo(ui: &mut Ui, severity: &mut String) {
    ComboBox::from_id_salt("kbd_severity")
        .width(COMBO_WIDTH)
        .selected_text(severity.as_str())
        .show_ui(ui, |ui| {
            for level in SEVERITIES {
                ui.selectable_value(severity, level.to_string(), level);
            }
        });
}

fn severity_style(ui: &Ui, severity: &str) -> (&'static str, Color32) {
    match severity {
        "critical" => (icons::CRITICAL, theme::error(ui)),
        "warn" => (icons::STATUS_WARN, theme::warn(ui)),
        "info" => (icons::INFO, theme::info(ui)),
        _ => (icons::STATUS_IDLE, theme::weak_text(ui)),
    }
}

fn non_empty<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.trim().is_empty() { fallback } else { value }
}

/// pnputil `MM/DD/YYYY`, WMI `YYYY-MM-DD`, and CIM `YYYYMMDD…` driver dates to MM/DD/YYYY.
fn fmt_driver_date(raw: &str) -> String {
    let raw = raw.trim();
    for pattern in ["%Y-%m-%d", "%m/%d/%Y"] {
        if let Ok(date) = chrono::NaiveDate::parse_from_str(raw, pattern) {
            return date.format("%m/%d/%Y").to_string();
        }
    }
    if raw.len() >= 8 && raw.as_bytes()[..8].iter().all(u8::is_ascii_digit) {
        if let Ok(date) = chrono::NaiveDate::parse_from_str(&raw[..8], "%Y%m%d") {
            return date.format("%m/%d/%Y").to_string();
        }
    }
    raw.to_string()
}

fn render_snapshot_row(ui: &mut Ui, snap: &DriverSnapshot, newest: bool) {
    let weak = theme::weak_text(ui);
    let strong = theme::strong_text(ui);
    let accent2 = theme::accent_secondary(ui);
    let (glyph, glyph_color) = if newest {
        (icons::STATUS_ON, theme::accent(ui))
    } else {
        (icons::STATUS_DOT, weak)
    };
    ui.horizontal(|ui| {
        ui.colored_label(glyph_color, glyph);
        ui.label(RichText::new(fmt_date_time(&snap.taken_at)).monospace().color(strong));
        badge(ui, non_empty(&snap.label, "manual"), accent2);
        ui.label(
            RichText::new(format!("{} packages", snap.driver_count))
                .small()
                .color(weak),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if !snap.source.is_empty() {
                ui.label(RichText::new(&snap.source).small().monospace().color(weak));
            }
            if !snap.notes.is_empty() {
                ui.label(
                    RichText::new(truncate_chars(&snap.notes, INLINE_META_CHARS))
                        .small()
                        .color(weak),
                )
                .on_hover_text(snap.notes.as_str());
            }
        });
    });
}

fn render_drift(ui: &mut Ui, older: &DriverSnapshot, newer: &DriverSnapshot) {
    let diff = diff_driver_sets(&older.drivers, &newer.drivers);
    let weak = theme::weak_text(ui);
    let strong = theme::strong_text(ui);
    let accent2 = theme::accent_secondary(ui);
    let good = theme::success(ui);
    let warn = theme::warn(ui);
    let bad = theme::error(ui);
    let span = format!(
        "{} ({}) {} {} ({})",
        fmt_date(&older.taken_at),
        non_empty(&older.label, "manual"),
        icons::ARROW_RIGHT,
        fmt_date(&newer.taken_at),
        non_empty(&newer.label, "manual")
    );
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(accent2, icons::REFRESH);
        ui.label(RichText::new("Drift").strong().color(strong));
        ui.label(RichText::new(span).small().monospace().color(weak));
    });
    if diff.is_empty() {
        ui.colored_label(weak, "No driver drift between these two snapshots.");
        return;
    }
    ui.horizontal_wrapped(|ui| {
        badge(ui, &format!("{} added", diff.added.len()), good);
        badge(ui, &format!("{} removed", diff.removed.len()), bad);
        badge(ui, &format!("{} changed", diff.changed.len()), warn);
    });
    drift_group(ui, "drift_changed", icons::REFRESH, warn, "Version changed", diff.changed.len(), |ui| {
        for change in diff.changed.iter().take(DRIFT_ROWS_SHOWN) {
            render_driver_change_row(ui, change);
        }
        render_overflow(ui, diff.changed.len());
    });
    drift_group(ui, "drift_added", icons::PLUS, good, "Installed since", diff.added.len(), |ui| {
        for driver in diff.added.iter().take(DRIFT_ROWS_SHOWN) {
            render_driver_row(ui, driver, icons::PLUS, good);
        }
        render_overflow(ui, diff.added.len());
    });
    drift_group(ui, "drift_removed", icons::TRASH, bad, "Removed since", diff.removed.len(), |ui| {
        for driver in diff.removed.iter().take(DRIFT_ROWS_SHOWN) {
            render_driver_row(ui, driver, icons::TRASH, bad);
        }
        render_overflow(ui, diff.removed.len());
    });
}

fn drift_group(
    ui: &mut Ui,
    id_salt: &str,
    glyph: &str,
    color: Color32,
    title: &str,
    count: usize,
    add_rows: impl FnOnce(&mut Ui),
) {
    if count == 0 {
        return;
    }
    CollapsingHeader::new(RichText::new(format!("{glyph} {title} ({count})")).color(color))
        .id_salt(id_salt)
        .default_open(count <= DRIFT_ROWS_SHOWN)
        .show(ui, add_rows);
}

fn render_overflow(ui: &mut Ui, total: usize) {
    if total > DRIFT_ROWS_SHOWN {
        let weak = theme::weak_text(ui);
        ui.colored_label(weak, format!("{} more not shown", total - DRIFT_ROWS_SHOWN));
    }
}

fn render_driver_row(ui: &mut Ui, driver: &DriverRecord, glyph: &str, color: Color32) {
    let weak = theme::weak_text(ui);
    let strong = theme::strong_text(ui);
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(color, glyph);
        ui.label(RichText::new(driver.key()).monospace().color(strong));
        badge(ui, non_empty(&driver.driver_version, "unknown"), color);
        if !driver.driver_date.is_empty() {
            ui.label(
                RichText::new(fmt_driver_date(&driver.driver_date))
                    .small()
                    .monospace()
                    .color(weak),
            );
        }
        if !driver.provider.is_empty() {
            ui.label(
                RichText::new(truncate_chars(&driver.provider, INLINE_META_CHARS))
                    .small()
                    .color(weak),
            )
            .on_hover_text(driver.provider.as_str());
        }
    });
}

fn render_driver_change_row(ui: &mut Ui, change: &DriverChange) {
    let weak = theme::weak_text(ui);
    let strong = theme::strong_text(ui);
    let warn = theme::warn(ui);
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(warn, icons::REFRESH);
        ui.label(RichText::new(&change.key).monospace().color(strong));
        badge(ui, non_empty(&change.old_version, "unknown"), weak);
        ui.colored_label(weak, icons::ARROW_RIGHT);
        badge(ui, non_empty(&change.new_version, "unknown"), warn);
        ui.label(
            RichText::new(format!(
                "{} {} {}",
                fmt_driver_date(&change.old_date),
                icons::ARROW_RIGHT,
                fmt_driver_date(&change.new_date)
            ))
            .small()
            .monospace()
            .color(weak),
        );
        if !change.provider.is_empty() {
            ui.label(
                RichText::new(truncate_chars(&change.provider, INLINE_META_CHARS))
                    .small()
                    .color(weak),
            )
            .on_hover_text(change.provider.as_str());
        }
    });
}

fn render_known_bad_hit(ui: &mut Ui, hit: &KnownBadHit) {
    let (glyph, sev_color) = severity_style(ui, &hit.entry.severity);
    let weak = theme::weak_text(ui);
    let strong = theme::strong_text(ui);
    let good = theme::success(ui);
    let bad = theme::error(ui);
    let salt = format!("kbh_{}_{}", hit.entry.id.key_string(), hit.driver.key());
    glass_card::group(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(sev_color, glyph);
            ui.label(RichText::new(hit.driver.key()).strong().monospace().color(strong));
            badge(ui, non_empty(&hit.driver.driver_version, "unknown"), sev_color);
            badge(ui, "installed now", bad);
            if let Some(device) = hit.driver.device_name.as_deref().filter(|d| !d.is_empty()) {
                ui.label(
                    RichText::new(truncate_chars(device, INLINE_META_CHARS))
                        .small()
                        .color(weak),
                )
                .on_hover_text(device);
            }
        });
        expandable_text(ui, &format!("{salt}_symptom"), &hit.entry.symptom, PROSE_LINES);
        wrapped_text(
            ui,
            non_empty(&hit.entry.fix, "No fix recorded — see the blocklist entry."),
            good,
        );
        ui.allocate_space(Vec2::new(ui.available_width(), 0.0));
    });
}

fn render_blocklist_card(ui: &mut Ui, entry: &KnownBadDriver) {
    let (glyph, sev_color) = severity_style(ui, &entry.severity);
    let weak = theme::weak_text(ui);
    let strong = theme::strong_text(ui);
    let good = theme::success(ui);
    let warn = theme::warn(ui);
    let faint = theme::bg_faint(ui);
    let window_stroke = ui.visuals().window_stroke;
    let corner = ui.visuals().menu_corner_radius;
    let key = entry.id.key_string();
    let dates = format!(
        "added {} — updated {}",
        fmt_date(&entry.created_at),
        fmt_date(&entry.updated_at)
    );
    glass_card::group(ui, |ui| {
        ui.horizontal(|ui| {
            ui.colored_label(sev_color, glyph);
            ui.label(RichText::new(&entry.module).strong().monospace().color(strong));
            if !entry.display_name.is_empty() {
                ui.label(
                    RichText::new(truncate_chars(&entry.display_name, INLINE_META_CHARS))
                        .small()
                        .color(weak),
                );
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(fmt_date(&entry.updated_at))
                        .small()
                        .monospace()
                        .color(weak),
                )
                .on_hover_text(dates.as_str());
                badge(ui, non_empty(&entry.severity, "warn"), sev_color);
            });
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("versions").small().color(weak));
            if entry.bad_versions.is_empty() {
                badge(ui, "any", warn);
            } else {
                for version in &entry.bad_versions {
                    badge(ui, version, sev_color);
                }
            }
            if let Some(fixed) = entry.fixed_version.as_deref().filter(|f| !f.is_empty()) {
                ui.label(RichText::new("fixed in").small().color(weak));
                badge(ui, fixed, good);
            }
            if !entry.vendor.is_empty() {
                ui.label(
                    RichText::new(truncate_chars(&entry.vendor, INLINE_META_CHARS))
                        .small()
                        .color(weak),
                );
            }
        });
        if entry.symptom.is_empty() {
            ui.colored_label(weak, "No symptom recorded.");
        } else {
            expandable_text(ui, &format!("kbd_{key}_symptom"), &entry.symptom, PROSE_LINES);
        }
        if !entry.fix.is_empty() {
            Frame::new()
                .fill(faint)
                .stroke(window_stroke)
                .inner_margin(Margin::symmetric(8, 6))
                .corner_radius(corner)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.colored_label(good, icons::WRENCH);
                        ui.label(RichText::new("Recommended fix").small().strong().color(good));
                    });
                    expandable_text(ui, &format!("kbd_{key}_fix"), &entry.fix, PROSE_LINES);
                    ui.allocate_space(Vec2::new(ui.available_width(), 0.0));
                });
        }
        ui.allocate_space(Vec2::new(ui.available_width(), 0.0));
    });
}

#[cfg(test)]
mod tests {
    use super::fmt_driver_date;

    #[test]
    fn driver_dates_normalize_to_us_format() {
        assert_eq!(fmt_driver_date("2023-12-06"), "12/06/2023");
        assert_eq!(fmt_driver_date("12/06/2023"), "12/06/2023");
        assert_eq!(fmt_driver_date("20231206000000.000000-000"), "12/06/2023");
        assert_eq!(fmt_driver_date(" 2023-12-06 "), "12/06/2023");
        assert_eq!(fmt_driver_date(""), "");
        assert_eq!(fmt_driver_date("n/a"), "n/a");
    }
}
