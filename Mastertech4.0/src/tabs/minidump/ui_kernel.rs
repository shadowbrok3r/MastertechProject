//! Kernel-crash (BSOD) triage view: bugcheck breakdown, driver blame, and
//! fleet crash-intel (prior sightings/verdicts, known-bad-driver hits,
//! sighting ingest + verdict recording).

use crossbeam::channel::{unbounded, Receiver, Sender};
use database::schema::{
    module_stem, CrashIngest, CrashSignature, CrashVerdict, KnownBadDriver, ParsedCrash,
    SightingContext,
};
use dump_triage::KernelDumpTriage;
use eframe::egui::{self, Color32, Grid, RichText, TextEdit, Ui, Widget};
use egui_extras::{Column, TableBuilder};

use super::{listing, MiniDumpApp};

/// Sort key for the module table.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum ModuleSort {
    #[default]
    Base,
    Name,
    Size,
}

/// Fleet lookups for the loaded dump's signature.
pub struct IntelLookup {
    pub signature: Option<CrashSignature>,
    pub verdicts: Vec<CrashVerdict>,
    /// Known-bad rows matched against the dump's driver list, keyed by the
    /// driver name that hit.
    pub known_bad: Vec<(String, KnownBadDriver)>,
    pub error: Option<String>,
}

pub struct KernelIntelState {
    lookup_started: bool,
    lookup: Option<IntelLookup>,
    lookup_tx: Sender<IntelLookup>,
    lookup_rx: Receiver<IntelLookup>,

    ingest_running: bool,
    ingest_status: Option<Result<String, String>>,
    ingest_tx: Sender<Result<CrashIngest, String>>,
    ingest_rx: Receiver<Result<CrashIngest, String>>,

    verdict_text: String,
    fix_text: String,
    confidence: String,
    verdict_running: bool,
    verdict_status: Option<Result<String, String>>,
    verdict_tx: Sender<Result<String, String>>,
    verdict_rx: Receiver<Result<String, String>>,

    // Module-table view state.
    module_filter: String,
    module_sort: ModuleSort,
    module_sort_desc: bool,
}

impl Default for KernelIntelState {
    fn default() -> Self {
        let (lookup_tx, lookup_rx) = unbounded();
        let (ingest_tx, ingest_rx) = unbounded();
        let (verdict_tx, verdict_rx) = unbounded();
        Self {
            lookup_started: false,
            lookup: None,
            lookup_tx,
            lookup_rx,
            ingest_running: false,
            ingest_status: None,
            ingest_tx,
            ingest_rx,
            verdict_text: String::new(),
            fix_text: String::new(),
            confidence: "medium".to_string(),
            verdict_running: false,
            verdict_status: None,
            verdict_tx,
            verdict_rx,
            module_filter: String::new(),
            module_sort: ModuleSort::default(),
            module_sort_desc: false,
        }
    }
}

/// Signature module used for crash-intel keys: best blame, else RIP module,
/// else "unknown".
fn signature_module(triage: &KernelDumpTriage) -> String {
    triage
        .blamed_module
        .clone()
        .or_else(|| triage.rip_module.clone())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Map a triage result into crash-intel's ParsedCrash shape.
fn to_parsed_crash(triage: &KernelDumpTriage, dump_name: Option<String>) -> ParsedCrash {
    let offset = match (&triage.rip, &triage.rip_module) {
        (Some(rip), Some(module)) => {
            let rip = u64::from_str_radix(rip.trim_start_matches("0x"), 16).unwrap_or(0);
            triage
                .drivers
                .iter()
                .find(|d| d.name == *module)
                .map(|d| format!("+{:#x}", rip.saturating_sub(d.base)))
        }
        _ => None,
    };
    let raw_excerpt = format!(
        "{} ({}) params: {} | rip: {} | blame: {} [dump-triage local parse]",
        triage.bugcheck_name,
        triage.bugcheck_code,
        triage.bugcheck_parameters.join(", "),
        triage.rip.as_deref().unwrap_or("-"),
        triage.blamed_module.as_deref().unwrap_or("-"),
    );
    // Normalized crash-time module names — the fleet co-occurrence asset.
    let loaded_modules: Vec<String> = {
        let mut v: Vec<String> = triage.drivers.iter().map(|d| module_stem(&d.name)).collect();
        v.sort();
        v.dedup();
        v
    };
    // Structured forensic detail persisted verbatim onto the sighting.
    let triage_blob = serde_json::json!({
        "params": triage.bugcheck_parameters,
        "rip": triage.rip,
        "rip_module": triage.rip_module,
        "blamed_module": triage.blamed_module,
        "dump_type": triage.dump_type_name,
        "uptime_secs": triage.uptime_secs,
        "loaded_count": triage.drivers.len(),
        "drivers": triage.drivers.iter().map(|d| serde_json::json!({
            "name": d.name, "base": d.base, "size": d.size,
        })).collect::<Vec<_>>(),
    });
    ParsedCrash {
        bugcheck_code: triage.bugcheck_code.clone(),
        bugcheck_name: triage.bugcheck_name.clone(),
        module: signature_module(triage),
        offset,
        process_name: None,
        failure_bucket: None,
        caused_by: triage.blamed_module.clone(),
        dump_name,
        dump_time: triage.system_time_unix.map(unix_to_string),
        raw_excerpt,
        loaded_modules,
        triage: Some(triage_blob),
    }
}

fn unix_to_string(secs: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(secs, 0)
        .map(|d| d.format("%m/%d/%Y %H:%M UTC").to_string())
        .unwrap_or_else(|| secs.to_string())
}

impl MiniDumpApp {
    pub fn ui_kernel(&mut self, ui: &mut Ui, _ctx: &egui::Context) {
        let triage = match &self.kernel_triage {
            Some(Ok(t)) => t.clone(),
            Some(Err(e)) => {
                ui.colored_label(
                    ui.style().visuals.error_fg_color,
                    format!("Kernel dump triage failed: {e}"),
                );
                return;
            }
            None => {
                ui.label("No kernel dump loaded.");
                return;
            }
        };
        let dump_name = self
            .settings
            .picked_path
            .as_deref()
            .and_then(|p| p.rsplit(['/', '\\']).next())
            .map(str::to_string);

        self.kernel_intel_receive();
        self.kickoff_intel_lookup(&triage);
        let ctx = ui.ctx().clone();

        // Title row.
        ui.horizontal(|ui| {
            ui.heading(
                RichText::new(format!("{} ({})", triage.bugcheck_name, triage.bugcheck_code))
                    .color(ui.style().visuals.error_fg_color),
            );
            ui.label(RichText::new(&triage.dump_type_name).weak());
            if let Some(name) = &dump_name {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(name).weak().monospace());
                });
            }
        });
        ui.separator();

        // Top summary: bugcheck+params on the left, blame+fleet on the right.
        ui.columns(2, |cols| {
            render_bugcheck_panel(&mut cols[0], &ctx, &triage);
            render_blame(&mut cols[1], &triage);
            cols[1].add_space(6.0);
            self.render_fleet_intel(&mut cols[1], &triage, dump_name);
        });

        ui.add_space(4.0);
        render_registers(ui, &ctx, &triage);
        render_stack(ui, &triage);
        self.render_modules(ui, &triage);
        render_memory(ui, &triage);
    }

    fn kernel_intel_receive(&mut self) {
        if let Ok(lookup) = self.kernel_intel.lookup_rx.try_recv() {
            self.kernel_intel.lookup = Some(lookup);
        }
        if let Ok(result) = self.kernel_intel.ingest_rx.try_recv() {
            self.kernel_intel.ingest_running = false;
            self.kernel_intel.ingest_status = Some(result.map(|ing| {
                format!(
                    "Recorded. Fleet has now seen this signature {} time(s).",
                    ing.signature.sighting_count
                )
            }));
            // Sighting counts changed; refresh the fleet lookup.
            self.kernel_intel.lookup_started = false;
        }
        if let Ok(result) = self.kernel_intel.verdict_rx.try_recv() {
            self.kernel_intel.verdict_running = false;
            self.kernel_intel.verdict_status = Some(result);
            self.kernel_intel.lookup_started = false;
        }
    }

    fn kickoff_intel_lookup(&mut self, triage: &KernelDumpTriage) {
        if self.kernel_intel.lookup_started {
            return;
        }
        self.kernel_intel.lookup_started = true;
        let code = triage.bugcheck_code.clone();
        let module = signature_module(triage);
        let driver_names: Vec<String> = triage.drivers.iter().map(|d| d.name.clone()).collect();
        let tx = self.kernel_intel.lookup_tx.clone();
        tokio::spawn(async move {
            let mut out = IntelLookup {
                signature: None,
                verdicts: Vec::new(),
                known_bad: Vec::new(),
                error: None,
            };
            match CrashSignature::find(&code, &module).await {
                Ok(sig) => {
                    if let Some(sig) = &sig {
                        out.verdicts = CrashSignature::verdicts(&sig.id, 10)
                            .await
                            .unwrap_or_default();
                    }
                    out.signature = sig;
                }
                Err(e) => out.error = Some(format!("signature lookup: {e}")),
            }
            match KnownBadDriver::active().await {
                Ok(kbd) => {
                    for name in &driver_names {
                        let stem = module_stem(name);
                        for row in &kbd {
                            if row.module == stem {
                                out.known_bad.push((name.clone(), row.clone()));
                            }
                        }
                    }
                }
                Err(e) => {
                    let msg = format!("known-bad lookup: {e}");
                    out.error = Some(match out.error.take() {
                        Some(prev) => format!("{prev}; {msg}"),
                        None => msg,
                    });
                }
            }
            let _ = tx.try_send(out);
        });
    }

    fn render_fleet_intel(
        &mut self,
        ui: &mut Ui,
        triage: &KernelDumpTriage,
        dump_name: Option<String>,
    ) {
        ui.label(RichText::new("Fleet Crash Intel").strong().size(14.0));

        let Some(lookup) = &self.kernel_intel.lookup else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Checking fleet history…");
            });
            return;
        };

        if let Some(err) = &lookup.error {
            ui.colored_label(ui.style().visuals.warn_fg_color, err);
        }

        match &lookup.signature {
            Some(sig) => {
                ui.label(format!(
                    "Seen {} time(s) on {} machine(s), last {}",
                    sig.sighting_count,
                    sig.machines.len(),
                    sig.last_seen,
                ));
                if lookup.verdicts.is_empty() {
                    ui.colored_label(
                        ui.style().visuals.weak_text_color(),
                        "No verdicts recorded for this signature yet.",
                    );
                }
                for v in &lookup.verdicts {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&v.confidence).small().strong());
                            ui.label(RichText::new(format!("{} ({})", v.author, v.source)).small());
                            ui.label(RichText::new(v.created_at.to_string()).weak().small());
                        });
                        ui.label(&v.verdict);
                        if !v.fix.trim().is_empty() {
                            ui.label(RichText::new(format!("Fix: {}", v.fix)).italics());
                        }
                    });
                }
            }
            None => {
                ui.colored_label(
                    ui.style().visuals.weak_text_color(),
                    "New signature — not seen in the fleet before.",
                );
            }
        }

        if !lookup.known_bad.is_empty() {
            ui.add_space(4.0);
            for (driver, row) in &lookup.known_bad {
                ui.colored_label(
                    ui.style().visuals.error_fg_color,
                    format!(
                        "⚠ Known-bad driver loaded: {} — {} (fix: {})",
                        driver,
                        if row.symptom.is_empty() { "see fleet notes" } else { &row.symptom },
                        if row.fix.is_empty() { "n/a" } else { &row.fix },
                    ),
                );
            }
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let can_ingest = !self.kernel_intel.ingest_running
                && !matches!(self.kernel_intel.ingest_status, Some(Ok(_)));
            if ui
                .add_enabled(can_ingest, egui::Button::new("Record sighting to fleet intel"))
                .on_hover_text(
                    "Upserts the crash signature and adds a sighting row for this dump, \
                     so fleet history reflects bench analysis too.",
                )
                .clicked()
            {
                self.kernel_intel.ingest_running = true;
                self.kernel_intel.ingest_status = None;
                let parsed = to_parsed_crash(triage, dump_name.clone());
                let ctx = SightingContext {
                    dump_kind: "minidump".to_string(),
                    ..Default::default()
                };
                let tx = self.kernel_intel.ingest_tx.clone();
                tokio::spawn(async move {
                    let result = CrashSignature::ingest(&parsed, &ctx)
                        .await
                        .map_err(|e| e.to_string());
                    let _ = tx.try_send(result);
                });
            }
            if self.kernel_intel.ingest_running {
                ui.spinner();
            }
            match &self.kernel_intel.ingest_status {
                Some(Ok(msg)) => {
                    ui.colored_label(Color32::from_rgb(100, 200, 100), msg);
                }
                Some(Err(e)) => {
                    ui.colored_label(ui.style().visuals.error_fg_color, e);
                }
                None => {}
            }
        });

        ui.add_space(6.0);
        ui.collapsing("Record verdict", |ui| {
            Grid::new("kernel_verdict_grid").num_columns(2).show(ui, |ui| {
                ui.label("Verdict");
                TextEdit::singleline(&mut self.kernel_intel.verdict_text)
                    .hint_text("Root cause, e.g. 'unstable XMP corrupting memory'")
                    .desired_width(360.0)
                    .ui(ui);
                ui.end_row();
                ui.label("Fix");
                TextEdit::singleline(&mut self.kernel_intel.fix_text)
                    .hint_text("What resolved it")
                    .desired_width(360.0)
                    .ui(ui);
                ui.end_row();
                ui.label("Confidence");
                egui::ComboBox::from_id_salt("kernel_verdict_confidence")
                    .selected_text(&self.kernel_intel.confidence)
                    .show_ui(ui, |ui| {
                        for c in ["low", "medium", "high", "confirmed"] {
                            ui.selectable_value(
                                &mut self.kernel_intel.confidence,
                                c.to_string(),
                                c,
                            );
                        }
                    });
                ui.end_row();
            });
            ui.horizontal(|ui| {
                let can_record = !self.kernel_intel.verdict_running
                    && !self.kernel_intel.verdict_text.trim().is_empty();
                if ui
                    .add_enabled(can_record, egui::Button::new("Record verdict"))
                    .clicked()
                {
                    self.kernel_intel.verdict_running = true;
                    self.kernel_intel.verdict_status = None;
                    let code = triage.bugcheck_code.clone();
                    let module = signature_module(triage);
                    let verdict = self.kernel_intel.verdict_text.clone();
                    let fix = self.kernel_intel.fix_text.clone();
                    let confidence = self.kernel_intel.confidence.clone();
                    let author = std::env::var("USERNAME")
                        .or_else(|_| std::env::var("USER"))
                        .unwrap_or_else(|_| "tech".to_string());
                    let tx = self.kernel_intel.verdict_tx.clone();
                    tokio::spawn(async move {
                        let result = async {
                            let sig = CrashSignature::ensure(&code, &module).await?;
                            CrashVerdict::create(
                                &sig.id, &verdict, &fix, &confidence, &author, "tech", None,
                            )
                            .await?;
                            anyhow::Ok("Verdict recorded.".to_string())
                        }
                        .await
                        .map_err(|e| e.to_string());
                        let _ = tx.try_send(result);
                    });
                }
                if self.kernel_intel.verdict_running {
                    ui.spinner();
                }
                match &self.kernel_intel.verdict_status {
                    Some(Ok(msg)) => {
                        ui.colored_label(Color32::from_rgb(100, 200, 100), msg);
                    }
                    Some(Err(e)) => {
                        ui.colored_label(ui.style().visuals.error_fg_color, e);
                    }
                    None => {}
                }
            });
        });
    }
}

/// Left summary column: crash meta + bugcheck parameters.
fn render_bugcheck_panel(ui: &mut Ui, ctx: &egui::Context, triage: &KernelDumpTriage) {
    let mut meta: Vec<(String, String)> = Vec::new();
    if let Some(t) = triage.system_time_unix {
        meta.push(("Crash time".into(), unix_to_string(t)));
    }
    if let Some(up) = triage.uptime_secs {
        meta.push(("Uptime".into(), format!("{}h {}m", up / 3600, (up % 3600) / 60)));
    }
    meta.push(("Processors".into(), triage.number_processors.to_string()));
    if let Some(rip) = &triage.rip {
        meta.push(("RIP".into(), rip.clone()));
    }
    if let Some(rsp) = &triage.rsp {
        meta.push(("RSP".into(), rsp.clone()));
    }
    if let Some(exc) = &triage.exception_code {
        meta.push(("Exception".into(), exc.clone()));
    }
    listing(ui, ctx, 0xB50C_0001, meta);

    if !triage.bugcheck_parameters.is_empty() {
        ui.add_space(4.0);
        ui.label(RichText::new("Bugcheck parameters").strong());
        let params = triage
            .bugcheck_parameters
            .iter()
            .enumerate()
            .map(|(i, p)| (format!("P{}", i + 1), p.clone()));
        listing(ui, ctx, 0xB50C_0002, params);
        for note in &triage.parameter_notes {
            ui.label(RichText::new(note).italics().weak());
        }
    }
}

fn render_blame(ui: &mut Ui, triage: &KernelDumpTriage) {
    ui.label(RichText::new("Blame").strong().size(14.0));
    match &triage.blamed_module {
        Some(m) => {
            ui.horizontal(|ui| {
                ui.label("Probable module:");
                ui.label(
                    RichText::new(m)
                        .monospace()
                        .strong()
                        .color(ui.style().visuals.warn_fg_color),
                );
            });
        }
        None => {
            ui.colored_label(
                ui.style().visuals.weak_text_color(),
                "No module attribution available (RIP outside known module ranges).",
            );
        }
    }
    if triage.rip_in_kernel_image {
        ui.label(
            RichText::new(
                "RIP is inside the kernel image itself. Repeated dumps faulting in nt with \
                 varied processes and no recurring third-party driver usually indicate \
                 memory-subsystem instability (bad RAM / XMP / FCLK), not a driver bug.",
            )
            .italics()
            .weak(),
        );
    }
}

/// Full register file from the crash CONTEXT, in a striped two-column table.
fn render_registers(ui: &mut Ui, ctx: &egui::Context, triage: &KernelDumpTriage) {
    if triage.registers.is_empty() {
        return;
    }
    ui.collapsing(RichText::new("Registers").strong(), |ui| {
        listing(
            ui,
            ctx,
            0xB50C_0003,
            triage.registers.iter().cloned(),
        );
    });
}

/// Scanned-stack backtrace — mirrors the app-crash 5-column table.
fn render_stack(ui: &mut Ui, triage: &KernelDumpTriage) {
    let n = triage.scanned_stack.len();
    egui::CollapsingHeader::new(RichText::new(format!("Call stack ({n})")).strong())
        .default_open(true)
        .show(ui, |ui| {
            if triage.scanned_stack.is_empty() {
                ui.colored_label(
                    ui.style().visuals.weak_text_color(),
                    "No stack available for this dump type. Full/BMP/live dumps and \
                     triage minidumps with an embedded call stack populate this.",
                );
                return;
            }
            ui.label(
                RichText::new(
                    "Scanned frames are heuristic (stack values that land in a module \
                     range); they can include stale addresses.",
                )
                .weak()
                .small(),
            );
            TableBuilder::new(ui)
                .id_salt("kernel_stack_table")
                .striped(true)
                .vscroll(false)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::initial(44.0).at_least(36.0))
                .column(Column::initial(60.0).at_least(48.0))
                .column(Column::initial(160.0).at_least(80.0))
                .column(Column::remainder().at_least(120.0))
                .resizable(true)
                .header(18.0, |mut h| {
                    h.col(|ui| { ui.strong("#"); });
                    h.col(|ui| { ui.strong("Trust"); });
                    h.col(|ui| { ui.strong("Module"); });
                    h.col(|ui| { ui.strong("Address / offset"); });
                })
                .body(|mut body| {
                    for (i, f) in triage.scanned_stack.iter().enumerate() {
                        body.row(18.0, |mut row| {
                            row.col(|ui| { ui.monospace(i.to_string()); });
                            row.col(|ui| {
                                let color = if f.trust == "context" {
                                    ui.style().visuals.warn_fg_color
                                } else {
                                    ui.style().visuals.weak_text_color()
                                };
                                ui.colored_label(color, &f.trust);
                            });
                            row.col(|ui| { ui.monospace(&f.module); });
                            row.col(|ui| {
                                ui.monospace(format!("{}+{:#x}", f.module, f.offset));
                            });
                        });
                    }
                });
        });
}

/// Loaded-module table with a name filter and click-to-sort headers.
impl MiniDumpApp {
    fn render_modules(&mut self, ui: &mut Ui, triage: &KernelDumpTriage) {
        let known_bad: Vec<String> = self
            .kernel_intel
            .lookup
            .as_ref()
            .map(|l| l.known_bad.iter().map(|(name, _)| name.clone()).collect())
            .unwrap_or_default();
        let blamed = triage.blamed_module.clone();

        egui::CollapsingHeader::new(
            RichText::new(format!("Loaded drivers ({})", triage.drivers.len())).strong(),
        )
        .show(ui, |ui| {
            if triage.drivers.is_empty() {
                ui.colored_label(
                    ui.style().visuals.weak_text_color(),
                    "No driver list available for this dump type.",
                );
                return;
            }

            ui.horizontal(|ui| {
                ui.label("Filter:");
                TextEdit::singleline(&mut self.kernel_intel.module_filter)
                    .hint_text("driver name…")
                    .desired_width(220.0)
                    .ui(ui);
                if ui.button("✕").on_hover_text("Clear filter").clicked() {
                    self.kernel_intel.module_filter.clear();
                }
            });

            let filter = self.kernel_intel.module_filter.to_ascii_lowercase();
            let mut rows: Vec<&dump_triage::DriverEntry> = triage
                .drivers
                .iter()
                .filter(|d| filter.is_empty() || d.name.contains(&filter))
                .collect();
            let sort = self.kernel_intel.module_sort;
            rows.sort_by(|a, b| match sort {
                ModuleSort::Base => a.base.cmp(&b.base),
                ModuleSort::Name => a.name.cmp(&b.name),
                ModuleSort::Size => a.size.cmp(&b.size),
            });
            if self.kernel_intel.module_sort_desc {
                rows.reverse();
            }

            let mut sort_header = |ui: &mut Ui, label: &str, key: ModuleSort| {
                let active = self.kernel_intel.module_sort == key;
                let arrow = if active {
                    if self.kernel_intel.module_sort_desc { " ▼" } else { " ▲" }
                } else {
                    ""
                };
                if ui
                    .selectable_label(active, RichText::new(format!("{label}{arrow}")).strong())
                    .clicked()
                {
                    if active {
                        self.kernel_intel.module_sort_desc = !self.kernel_intel.module_sort_desc;
                    } else {
                        self.kernel_intel.module_sort = key;
                        self.kernel_intel.module_sort_desc = false;
                    }
                }
            };

            TableBuilder::new(ui)
                .id_salt("kernel_module_table")
                .striped(true)
                .vscroll(false)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::remainder().at_least(160.0))
                .column(Column::initial(150.0).at_least(100.0))
                .column(Column::initial(90.0).at_least(60.0))
                .resizable(true)
                .header(20.0, |mut h| {
                    h.col(|ui| sort_header(ui, "Driver", ModuleSort::Name));
                    h.col(|ui| sort_header(ui, "Base", ModuleSort::Base));
                    h.col(|ui| sort_header(ui, "Size", ModuleSort::Size));
                })
                .body(|mut body| {
                    for d in rows {
                        body.row(18.0, |mut row| {
                            row.col(|ui| {
                                let text = if known_bad.contains(&d.name) {
                                    RichText::new(&d.name)
                                        .monospace()
                                        .color(ui.style().visuals.error_fg_color)
                                } else if blamed.as_deref() == Some(d.name.as_str()) {
                                    RichText::new(&d.name)
                                        .monospace()
                                        .color(ui.style().visuals.warn_fg_color)
                                } else {
                                    RichText::new(&d.name).monospace()
                                };
                                ui.label(text);
                            });
                            row.col(|ui| { ui.monospace(format!("{:#018x}", d.base)); });
                            row.col(|ui| { ui.monospace(format!("{} KB", d.size / 1024)); });
                        });
                    }
                });
        });
    }
}

/// Hex+ASCII windows around RIP and at RSP (populated for full/BMP/live dumps).
fn render_memory(ui: &mut Ui, triage: &KernelDumpTriage) {
    if triage.rip_region.is_none() && triage.rsp_region.is_none() {
        return;
    }
    ui.collapsing(RichText::new("Memory").strong(), |ui| {
        if let Some(r) = &triage.rip_region {
            ui.label(RichText::new("Around RIP").weak());
            hex_dump(ui, r);
            ui.add_space(4.0);
        }
        if let Some(r) = &triage.rsp_region {
            ui.label(RichText::new("At RSP").weak());
            hex_dump(ui, r);
        }
    });
}

fn hex_dump(ui: &mut Ui, region: &dump_triage::HexRegion) {
    let mut out = String::new();
    for (i, chunk) in region.bytes.chunks(16).enumerate() {
        let addr = region.base + (i * 16) as u64;
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        let ascii: String = chunk
            .iter()
            .map(|b| if b.is_ascii_graphic() { *b as char } else { '.' })
            .collect();
        out.push_str(&format!("{addr:016x}  {:<47}  {ascii}\n", hex.join(" ")));
    }
    ui.add(egui::Label::new(RichText::new(out).monospace().size(11.0)).wrap_mode(egui::TextWrapMode::Extend));
}
