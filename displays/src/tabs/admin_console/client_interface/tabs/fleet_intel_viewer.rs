//! Fleet Intel view: crash-signature intelligence + driver time machine for one client.
//!
//! Crash side: browse/search fleet `crash_signature` rows, inspect sightings and
//! verdicts, record new verdicts, and drive dump-decode analysis on the client.
//! Driver side: take pnputil snapshots, review snapshot history and drift, and
//! maintain the `known_bad_driver` blocklist.

use crossbeam::channel::{unbounded, Receiver, Sender};
use database::schema::{
    crash_intel::{CrashIngest, CrashSighting, CrashSignature, CrashVerdict},
    driver_intel::{diff_driver_sets, DriverSnapshot, KnownBadDriver},
    ConnectedClient, RecordIdExt,
};
use eframe::egui::{Color32, ComboBox, Grid, RichText, ScrollArea, TextEdit, Ui};

use crate::ui_tools::icons;
use crate::{Cmd, PlatformSpawner, Spawner};

const DUMP_DECODE: &str = "com.mastertech.dump-decode";
const DRIVERSTORE: &str = "com.mastertech.driverstore";
const SNAPSHOT_LABELS: [&str; 4] = ["intake", "pre_service", "post_service", "manual"];

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
                ui.horizontal(|ui| {
                    let running = crate::plugins::intake_autopilot::triage_running();
                    let label = if running {
                        format!("{} Triage running…", icons::STATUS_WAIT)
                    } else {
                        format!("{} Run Intake Triage", icons::LIGHTBULB)
                    };
                    if ui
                        .add_enabled(!running, eframe::egui::Button::new(label))
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
                });
                if !self.status.is_empty() {
                    ui.label(RichText::new(&self.status).color(Color32::from_rgb(220, 180, 120)).small());
                }
                ui.add_space(6.0);
                self.crash_section(ui, client, cmd_tx);
                ui.add_space(12.0);
                self.driver_section(ui, client, cmd_tx);
            });
    }

    fn crash_section(&mut self, ui: &mut Ui, client: &ConnectedClient, cmd_tx: &Sender<Cmd>) {
        ui.heading(format!("{} Crash Intelligence", icons::DIAGNOSTICS));
        ui.horizontal(|ui| {
            if ui
                .button(format!("{} Analyze dumps", icons::PLAY))
                .on_hover_text("Start detached WinDbg batch analysis of the 8 newest minidumps on the client (dump-decode run_analyze_batch).")
                .clicked()
            {
                Self::call_remote_tool(cmd_tx, DUMP_DECODE, "run_analyze_batch");
                self.status = "Batch analysis started on client — fetch results in ~1-2 min.".to_string();
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
            ui.add(TextEdit::singleline(&mut self.search).hint_text("module / bugcheck").desired_width(160.0));
            if ui.button(format!("{} Search", icons::SEARCH)).clicked() {
                self.load_signatures(self.search.clone());
            }
            if ui.button(format!("{} Recent", icons::REFRESH)).clicked() {
                self.search.clear();
                self.load_signatures(String::new());
            }
        });

        let ingests = crate::plugins::crash_intel_hooks::latest_ingests(&client.connection_string);
        if !ingests.is_empty() {
            ui.add_space(4.0);
            ui.label(RichText::new("Latest analysis on this client:").strong());
            for ingest in &ingests {
                self.ingest_row(ui, ingest);
            }
        }

        ui.add_space(6.0);
        if self.signatures.is_empty() {
            ui.label(RichText::new("No fleet crash signatures loaded.").weak());
            return;
        }

        Grid::new("crash_sig_grid").striped(true).min_col_width(60.0).show(ui, |ui| {
            ui.label(RichText::new("Bugcheck").strong());
            ui.label(RichText::new("Module").strong());
            ui.label(RichText::new("Sightings").strong());
            ui.label(RichText::new("Machines").strong());
            ui.label(RichText::new("Last seen").strong());
            ui.label(RichText::new("Latest verdict").strong());
            ui.end_row();

            for idx in 0..self.signatures.len() {
                let (sig, verdicts) = &self.signatures[idx];
                let selected = self.selected_signature == Some(idx);
                let name = if sig.bugcheck_name.is_empty() {
                    sig.bugcheck_code.clone()
                } else {
                    format!("{} {}", sig.bugcheck_code, sig.bugcheck_name)
                };
                if ui.selectable_label(selected, name).clicked() {
                    self.selected_signature = Some(idx);
                    self.sightings.clear();
                    let sig = self.signatures[idx].0.clone();
                    self.load_sightings(&sig);
                }
                ui.label(&sig.module);
                ui.label(sig.sighting_count.to_string());
                ui.label(sig.machines.len().to_string());
                ui.label(format!("{}", sig.last_seen));
                match verdicts.first() {
                    Some(v) => ui.label(
                        RichText::new(&v.verdict).color(Color32::from_rgb(150, 220, 150)),
                    ),
                    None => ui.label(RichText::new("—").weak()),
                };
                ui.end_row();
            }
        });

        if let Some(idx) = self.selected_signature {
            if let Some((sig, verdicts)) = self.signatures.get(idx).cloned() {
                ui.add_space(6.0);
                ui.group(|ui| {
                    ui.label(RichText::new(format!(
                        "{} {} — {} sighting(s) on {} machine(s)",
                        sig.bugcheck_code,
                        sig.module,
                        sig.sighting_count,
                        sig.machines.len()
                    ))
                    .strong());
                    if !sig.failure_buckets.is_empty() {
                        ui.label(RichText::new(format!("Buckets: {}", sig.failure_buckets.join(", "))).small());
                    }
                    for v in &verdicts {
                        ui.label(format!(
                            "• [{} | {} | {}] {}{}",
                            v.confidence,
                            v.source,
                            v.author,
                            v.verdict,
                            if v.fix.is_empty() { String::new() } else { format!(" — Fix: {}", v.fix) }
                        ));
                    }
                    if !self.sightings.is_empty() {
                        ui.collapsing(format!("Sightings ({})", self.sightings.len()), |ui| {
                            for s in &self.sightings {
                                ui.label(format!(
                                    "{} | {} | {} {}",
                                    s.created_at,
                                    s.connection_string.as_deref().unwrap_or("?"),
                                    s.dump_name.as_deref().unwrap_or("-"),
                                    s.failure_bucket.as_deref().unwrap_or("")
                                ));
                            }
                        });
                    }
                    ui.separator();
                    ui.label(RichText::new("Record verdict").strong());
                    ui.add(TextEdit::singleline(&mut self.verdict_text).hint_text("diagnosis").desired_width(360.0));
                    ui.add(TextEdit::singleline(&mut self.verdict_fix).hint_text("fix that worked").desired_width(360.0));
                    ui.horizontal(|ui| {
                        ComboBox::from_id_salt("verdict_confidence")
                            .selected_text(&self.verdict_confidence)
                            .show_ui(ui, |ui| {
                                for c in ["low", "medium", "high", "confirmed"] {
                                    ui.selectable_value(&mut self.verdict_confidence, c.to_string(), c);
                                }
                            });
                        let can_save = !self.verdict_text.trim().is_empty();
                        if ui.add_enabled(can_save, eframe::egui::Button::new(format!("{} Save verdict", icons::SAVE))).clicked() {
                            let tx = self.tx.clone();
                            let sig_id = sig.id.clone();
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
                    });
                });
            }
        }
    }

    fn ingest_row(&self, ui: &mut Ui, ingest: &CrashIngest) {
        let known = ingest.previously_seen;
        let color = if known {
            Color32::from_rgb(240, 180, 100)
        } else {
            Color32::from_rgb(150, 200, 240)
        };
        let verdict = ingest
            .verdicts
            .first()
            .map(|v| format!(" — {}", v.verdict))
            .unwrap_or_default();
        ui.label(
            RichText::new(format!(
                "{} {} {} ({} prior on {} machine(s)){}",
                if known { icons::STATUS_WARN } else { icons::STATUS_ON },
                ingest.signature.bugcheck_code,
                ingest.signature.module,
                ingest.prior_sighting_count,
                ingest.prior_machine_count,
                verdict
            ))
            .color(color),
        );
    }

    fn driver_section(&mut self, ui: &mut Ui, client: &ConnectedClient, cmd_tx: &Sender<Cmd>) {
        ui.heading(format!("{} Driver Time Machine", icons::HARD_DRIVE));
        ui.horizontal(|ui| {
            ComboBox::from_id_salt("snapshot_label")
                .selected_text(&self.snapshot_label)
                .show_ui(ui, |ui| {
                    for l in SNAPSHOT_LABELS {
                        ui.selectable_value(&mut self.snapshot_label, l.to_string(), l);
                    }
                });
            if ui
                .button(format!("{} Snapshot now", icons::PLAY))
                .on_hover_text("Capture the full DriverStore inventory (pnputil) and persist it as a snapshot.")
                .clicked()
            {
                crate::plugins::driver_intel_hooks::set_pending_label(
                    &client.connection_string,
                    &self.snapshot_label,
                );
                Self::call_remote_tool(cmd_tx, DRIVERSTORE, "snapshot");
                self.status = "Driver snapshot requested — refresh in a few seconds.".to_string();
            }
            if ui.button(format!("{} Refresh", icons::REFRESH)).clicked() {
                self.load_driver_data(client.connection_string.clone());
            }
        });

        ui.add_space(4.0);
        if self.snapshots.is_empty() {
            ui.label(RichText::new("No driver snapshots recorded for this client yet.").weak());
        } else {
            Grid::new("driver_snap_grid").striped(true).show(ui, |ui| {
                ui.label(RichText::new("Taken").strong());
                ui.label(RichText::new("Label").strong());
                ui.label(RichText::new("Packages").strong());
                ui.end_row();
                for s in &self.snapshots {
                    ui.label(format!("{}", s.taken_at));
                    ui.label(&s.label);
                    ui.label(s.driver_count.to_string());
                    ui.end_row();
                }
            });

            if self.snapshots.len() >= 2 {
                let diff = diff_driver_sets(&self.snapshots[1].drivers, &self.snapshots[0].drivers);
                if diff.is_empty() {
                    ui.label(RichText::new("No driver drift between the two newest snapshots.").weak());
                } else {
                    ui.collapsing(
                        format!(
                            "Drift since previous snapshot: +{} / -{} / {} changed",
                            diff.added.len(),
                            diff.removed.len(),
                            diff.changed.len()
                        ),
                        |ui| {
                            for d in &diff.added {
                                ui.label(RichText::new(format!("+ {} {} ({})", d.key(), d.driver_version, d.provider)).color(Color32::from_rgb(150, 220, 150)));
                            }
                            for d in &diff.removed {
                                ui.label(RichText::new(format!("- {} {} ({})", d.key(), d.driver_version, d.provider)).color(Color32::from_rgb(230, 140, 140)));
                            }
                            for c in &diff.changed {
                                ui.label(format!("~ {} {} -> {}", c.key, c.old_version, c.new_version));
                            }
                        },
                    );
                }
            }

            if let Some(latest) = self.snapshots.first() {
                let hits = KnownBadDriver::match_inventory(&self.blocklist, &latest.drivers);
                if !hits.is_empty() {
                    ui.add_space(4.0);
                    for hit in &hits {
                        ui.label(
                            RichText::new(format!(
                                "{} KNOWN-BAD: {} {} — {} (fix: {})",
                                icons::STATUS_ERR,
                                hit.driver.key(),
                                hit.driver.driver_version,
                                hit.entry.symptom,
                                if hit.entry.fix.is_empty() { "see blocklist" } else { &hit.entry.fix }
                            ))
                            .color(Color32::from_rgb(240, 120, 120)),
                        );
                    }
                }
            }
        }

        ui.add_space(6.0);
        ui.collapsing(format!("Blocklist ({} active)", self.blocklist.len()), |ui| {
            for b in &self.blocklist {
                ui.label(format!(
                    "{} [{}] versions {} — {} (fix: {})",
                    b.module,
                    b.severity,
                    if b.bad_versions.is_empty() { "any".to_string() } else { b.bad_versions.join(", ") },
                    b.symptom,
                    b.fix
                ));
            }
            ui.separator();
            ui.label(RichText::new("Add known-bad driver").strong());
            ui.horizontal(|ui| {
                ui.add(TextEdit::singleline(&mut self.kbd_module).hint_text("module (rtwlane)").desired_width(120.0));
                ui.add(TextEdit::singleline(&mut self.kbd_versions).hint_text("bad versions, comma-sep (blank = all)").desired_width(200.0));
                ComboBox::from_id_salt("kbd_severity")
                    .selected_text(&self.kbd_severity)
                    .show_ui(ui, |ui| {
                        for s in ["info", "warn", "critical"] {
                            ui.selectable_value(&mut self.kbd_severity, s.to_string(), s);
                        }
                    });
            });
            ui.add(TextEdit::singleline(&mut self.kbd_symptom).hint_text("symptom").desired_width(400.0));
            ui.add(TextEdit::singleline(&mut self.kbd_fix).hint_text("recommended fix").desired_width(400.0));
            let can_add = !self.kbd_module.trim().is_empty();
            if ui.add_enabled(can_add, eframe::egui::Button::new(format!("{} Add to blocklist", icons::PLUS))).clicked() {
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
                let cs = client.connection_string.clone();
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
                self.load_driver_data(client.connection_string.clone());
                let _ = cs;
            }
        });
    }
}
