//! Kernel-crash (BSOD) triage view: bugcheck breakdown, driver blame, and
//! fleet crash-intel (prior sightings/verdicts, known-bad-driver hits,
//! sighting ingest + verdict recording).

use crossbeam::channel::{Receiver, Sender, unbounded};
use database::schema::{
    CrashIngest, CrashSignature, CrashVerdict, KnownBadDriver, ParsedCrash, SightingContext,
    module_stem,
};
use database::shape_walk::{self, Row};
use displays::ui_tools::{
    controls::{self, ButtonKind, Metrics, SpinnerSize, TextButton, TextField, Tokens},
    fonts, glass_card,
    icons::svg,
};
use dump_triage::KernelDumpTriage;
use eframe::egui::{self, Color32, Grid, RichText, Ui};
use egui_extras::{Column, TableBuilder};

use super::{MiniDumpApp, listing, listing_rows};

/// Bugcheck title size.
const TITLE_PT: f32 = 18.0;
/// Hex-dump text size.
const HEX_PT: f32 = 11.0;
/// Width of the client-link field.
const LINK_W: f32 = 200.0;
/// Width of the verdict and fix fields.
const VERDICT_W: f32 = 360.0;
/// Width of the confidence dropdown.
const CONFIDENCE_W: f32 = 140.0;
/// Width of the driver filter.
const FILTER_W: f32 = 220.0;
/// Verdict confidence values and their labels.
const CONFIDENCE: [(&str, &str); 4] = [
    ("low", "Low"),
    ("medium", "Medium"),
    ("high", "High"),
    ("confirmed", "Confirmed"),
];

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
    /// Optional connection_string to attribute the sighting to a live client
    /// (links session/task/computer and enables dedup). Empty = unlinked.
    ingest_link_cs: String,

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
            ingest_link_cs: String::new(),
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
        let mut v: Vec<String> = triage
            .drivers
            .iter()
            .map(|d| module_stem(&d.name))
            .collect();
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
        module_version: None,
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

/// Success or error line for a finished background request.
fn result_line(ui: &mut Ui, result: &Result<String, String>) {
    let t = Tokens::get(ui.ctx());
    let (icon, text, tone) = match result {
        Ok(msg) => (svg::CHECK_CIRCLE, msg, t.success),
        Err(e) => (svg::X_CIRCLE, e, t.error),
    };
    controls::status_label(ui, icon, text, tone);
}

impl MiniDumpApp {
    pub fn ui_kernel(&mut self, ui: &mut Ui, _ctx: &egui::Context) {
        let t = Tokens::get(ui.ctx());
        let m = Metrics::get(ui.ctx());
        let triage = match &self.kernel_triage {
            Some(Ok(t)) => t.clone(),
            Some(Err(e)) => {
                controls::status_label(
                    ui,
                    svg::WARNING_OCTAGON,
                    &format!("Kernel dump triage failed: {e}"),
                    t.error,
                );
                return;
            }
            None => {
                controls::empty_state(
                    ui,
                    svg::FILE_X,
                    "No kernel dump loaded.",
                    "Open a kernel dump from the Settings tab.",
                    &[],
                );
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
            ui.add(svg::WARNING_OCTAGON.image(m.icon_lg).tint(t.error));
            ui.label(
                RichText::new(format!(
                    "{} ({})",
                    triage.bugcheck_name, triage.bugcheck_code
                ))
                .font(fonts::semibold(TITLE_PT))
                .color(t.error),
            );
            controls::badge(ui, &triage.dump_type_name, t.text_dim);
            if let Some(name) = &dump_name {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(name).color(t.text_dim).monospace());
                    ui.add(svg::FILE.image(m.icon_sm).tint(t.text_dim));
                });
            }
        });
        controls::hairline(ui);

        // Top summary: bugcheck+params on the left, blame+fleet on the right.
        ui.columns(2, |cols| {
            render_bugcheck_panel(&mut cols[0], &ctx, &triage);
            render_blame(&mut cols[1], &triage);
            cols[1].add_space(m.gap.y);
            self.render_fleet_intel(&mut cols[1], &triage, dump_name);
        });

        ui.add_space(m.gap.y);
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
        let t = Tokens::get(ui.ctx());
        let m = Metrics::get(ui.ctx());
        glass_card::card_header(ui, svg::SHARE_NETWORK, "Fleet Crash Intel", None);

        let Some(lookup) = &self.kernel_intel.lookup else {
            controls::loading_row(ui, "Checking fleet history…");
            return;
        };

        if let Some(err) = &lookup.error {
            controls::status_label(ui, svg::WARNING, err, t.warn);
        }

        match &lookup.signature {
            Some(sig) => {
                controls::status_label(
                    ui,
                    svg::CLOCK_COUNTER_CLOCKWISE,
                    &format!(
                        "Seen {} time(s) on {} machine(s), last {}",
                        sig.sighting_count,
                        sig.machines.len(),
                        sig.last_seen,
                    ),
                    t.text,
                );
                if lookup.verdicts.is_empty() {
                    controls::status_label(
                        ui,
                        svg::NOTE,
                        "No verdicts recorded for this signature yet.",
                        t.text_dim,
                    );
                }
                for v in &lookup.verdicts {
                    glass_card::group(ui, |ui| {
                        ui.horizontal(|ui| {
                            controls::badge(ui, &v.confidence, confidence_tone(&t, &v.confidence));
                            ui.add(svg::USER.image(m.icon_sm).tint(t.text_dim));
                            ui.label(RichText::new(format!("{} ({})", v.author, v.source)).small());
                            ui.label(
                                RichText::new(v.created_at.to_string())
                                    .color(t.text_faint)
                                    .small(),
                            );
                        });
                        ui.label(&v.verdict);
                        if !v.fix.trim().is_empty() {
                            ui.horizontal(|ui| {
                                ui.add(svg::WRENCH.image(m.icon_sm).tint(t.success));
                                ui.label(RichText::new(format!("Fix: {}", v.fix)).italics());
                            });
                        }
                    });
                }
            }
            None => {
                controls::status_label(
                    ui,
                    svg::SPARKLE,
                    "New signature — not seen in the fleet before.",
                    t.text_dim,
                );
            }
        }

        if !lookup.known_bad.is_empty() {
            ui.add_space(m.gap.y / 2.0);
            for (driver, row) in &lookup.known_bad {
                controls::status_label(
                    ui,
                    svg::WARNING,
                    &format!(
                        "Known-bad driver loaded: {} — {} (fix: {})",
                        driver,
                        if row.symptom.is_empty() {
                            "see fleet notes"
                        } else {
                            &row.symptom
                        },
                        if row.fix.is_empty() { "n/a" } else { &row.fix },
                    ),
                    t.error,
                );
            }
        }

        ui.add_space(m.gap.y);
        ui.horizontal(|ui| {
            controls::icon_label(ui, svg::LINK, "Link to client");
            TextField::new(&mut self.kernel_intel.ingest_link_cs)
                .hint("connection_string (optional)")
                .mono(true)
                .width(LINK_W)
                .id_salt("kernel_intel.link_cs")
                .show(ui)
                .on_hover_text(
                    "Optional. When set to a connected client's connection_string, the sighting \
                     links to that client's open session/task/computer and dedups on re-record. \
                     Leave empty to record an unlinked bench sighting.",
                );
        });
        ui.horizontal(|ui| {
            let can_ingest = !self.kernel_intel.ingest_running
                && !matches!(self.kernel_intel.ingest_status, Some(Ok(_)));
            let ingest = TextButton::new("Record sighting to fleet intel")
                .icon(svg::CLOUD_ARROW_UP)
                .enabled(can_ingest)
                .show(ui);
            if ingest
                .on_hover_text(
                    "Upserts the crash signature and adds a sighting row for this dump, \
                     so fleet history reflects bench analysis too.",
                )
                .clicked()
            {
                self.kernel_intel.ingest_running = true;
                self.kernel_intel.ingest_status = None;
                let parsed = to_parsed_crash(triage, dump_name.clone());
                let link_cs = self.kernel_intel.ingest_link_cs.trim().to_string();
                let tx = self.kernel_intel.ingest_tx.clone();
                tokio::spawn(async move {
                    let ctx = if link_cs.is_empty() {
                        SightingContext {
                            dump_kind: "minidump".to_string(),
                            ..Default::default()
                        }
                    } else {
                        database::schema::crash_intel::sighting_context_for_connection(
                            &link_cs, "minidump",
                        )
                        .await
                    };
                    let result = CrashSignature::ingest(&parsed, &ctx)
                        .await
                        .map_err(|e| e.to_string());
                    let _ = tx.try_send(result);
                });
            }
            if self.kernel_intel.ingest_running {
                controls::spinner(ui, SpinnerSize::Sm);
            }
        });
        if let Some(status) = &self.kernel_intel.ingest_status {
            result_line(ui, status);
        }

        ui.add_space(m.gap.y);
        if controls::Section::new("kernel_record_verdict", "Record verdict")
            .icon(svg::GAVEL)
            .default_open(false)
            .header(ui)
        {
            Grid::new("kernel_verdict_grid")
                .num_columns(2)
                .spacing(m.gap)
                .show(ui, |ui| {
                    controls::icon_label(ui, svg::NOTE_PENCIL, "Verdict");
                    TextField::new(&mut self.kernel_intel.verdict_text)
                        .hint("Root cause, e.g. 'unstable XMP corrupting memory'")
                        .width(VERDICT_W)
                        .id_salt("kernel_intel.verdict")
                        .show(ui);
                    ui.end_row();
                    controls::icon_label(ui, svg::WRENCH, "Fix");
                    TextField::new(&mut self.kernel_intel.fix_text)
                        .hint("What resolved it")
                        .width(VERDICT_W)
                        .id_salt("kernel_intel.fix")
                        .show(ui);
                    ui.end_row();
                    controls::icon_label(ui, svg::GAUGE, "Confidence");
                    let options: Vec<(String, &str)> = CONFIDENCE
                        .iter()
                        .map(|(value, label)| (value.to_string(), *label))
                        .collect();
                    controls::dropdown(
                        ui,
                        "kernel_verdict_confidence",
                        &mut self.kernel_intel.confidence,
                        &options,
                        CONFIDENCE_W,
                    );
                    ui.end_row();
                });
            ui.horizontal(|ui| {
                let can_record = !self.kernel_intel.verdict_running
                    && !self.kernel_intel.verdict_text.trim().is_empty();
                let record = TextButton::new("Record verdict")
                    .icon(svg::FLOPPY_DISK)
                    .kind(ButtonKind::Primary)
                    .enabled(can_record)
                    .show(ui);
                if record.clicked() {
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
                                &sig.id,
                                &verdict,
                                &fix,
                                &confidence,
                                &author,
                                "tech",
                                None,
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
                    controls::spinner(ui, SpinnerSize::Sm);
                }
            });
            if let Some(status) = &self.kernel_intel.verdict_status {
                result_line(ui, status);
            }
        }
    }
}

/// Badge colour for a verdict's confidence.
fn confidence_tone(t: &Tokens, confidence: &str) -> Color32 {
    match confidence {
        "confirmed" => t.success,
        "high" => t.info,
        "low" => t.warn,
        _ => t.secondary,
    }
}

/// Left summary column: crash meta + bugcheck parameters.
fn render_bugcheck_panel(ui: &mut Ui, ctx: &egui::Context, triage: &KernelDumpTriage) {
    // Field doc comments keyed by their walker label, attached to meta rows.
    let hovers: std::collections::HashMap<String, String> = shape_walk::rows(triage)
        .into_iter()
        .filter_map(|r| r.hover.map(|h| (r.label, h)))
        .collect();
    let hover = |walker_label: &str| hovers.get(walker_label).cloned();

    let mut meta: Vec<Row> = Vec::new();
    if let Some(t) = triage.system_time_unix {
        meta.push(Row {
            label: "Crash time".into(),
            value: unix_to_string(t),
            hover: hover("System Time Unix"),
        });
    }
    if let Some(up) = triage.uptime_secs {
        meta.push(Row {
            label: "Uptime".into(),
            value: format!("{}h {}m", up / 3600, (up % 3600) / 60),
            hover: hover("Uptime Secs"),
        });
    }
    meta.push(Row {
        label: "Processors".into(),
        value: triage.number_processors.to_string(),
        hover: hover("Number Processors"),
    });
    if let Some(rip) = &triage.rip {
        meta.push(Row {
            label: "RIP".into(),
            value: rip.clone(),
            hover: hover("Rip"),
        });
    }
    if let Some(rsp) = &triage.rsp {
        meta.push(Row {
            label: "RSP".into(),
            value: rsp.clone(),
            hover: hover("Rsp"),
        });
    }
    if let Some(exc) = &triage.exception_code {
        meta.push(Row {
            label: "Exception".into(),
            value: exc.clone(),
            hover: hover("Exception Code"),
        });
    }
    glass_card::card_header(ui, svg::INFO, "Crash details", None);
    listing_rows(ui, ctx, 0xB50C_0001, meta);

    if !triage.bugcheck_parameters.is_empty() {
        let m = Metrics::get(ui.ctx());
        ui.add_space(m.gap.y);
        glass_card::card_header(ui, svg::LIST_NUMBERS, "Bugcheck parameters", None);
        let params = triage
            .bugcheck_parameters
            .iter()
            .enumerate()
            .map(|(i, p)| (format!("P{}", i + 1), p.clone()));
        listing(ui, ctx, 0xB50C_0002, params);
        let dim = Tokens::get(ui.ctx()).text_dim;
        for note in &triage.parameter_notes {
            ui.label(RichText::new(note).italics().color(dim));
        }
    }
}

fn render_blame(ui: &mut Ui, triage: &KernelDumpTriage) {
    let t = Tokens::get(ui.ctx());
    glass_card::card_header(ui, svg::CROSSHAIR, "Blame", None);
    match &triage.blamed_module {
        Some(m) => {
            ui.horizontal(|ui| {
                controls::icon_label(ui, svg::PUZZLE_PIECE, "Probable module:");
                ui.label(RichText::new(m).monospace().strong().color(t.warn));
            });
        }
        None if !triage.warnings.is_empty() => {
            controls::status_label(
                ui,
                svg::X_CIRCLE,
                "Blame unavailable — the dump's driver list could not be parsed.",
                t.error,
            );
        }
        None => {
            controls::status_label(
                ui,
                svg::QUESTION,
                "No module attribution available (RIP outside known module ranges).",
                t.text_dim,
            );
        }
    }
    for warning in &triage.warnings {
        controls::status_label(ui, svg::WARNING, warning, t.warn);
    }
    if triage.rip_in_kernel_image {
        ui.horizontal_top(|ui| {
            let size = Metrics::get(ui.ctx()).icon_sm;
            ui.add(svg::MEMORY.image(size).tint(t.text_dim));
            ui.label(
                RichText::new(
                    "RIP is inside the kernel image itself. Repeated dumps faulting in nt with \
                     varied processes and no recurring third-party driver usually indicate \
                     memory-subsystem instability (bad RAM / XMP / FCLK), not a driver bug.",
                )
                .italics()
                .color(t.text_dim),
            );
        });
    }
}

/// Full register file from the crash CONTEXT, in a striped two-column table.
fn render_registers(ui: &mut Ui, ctx: &egui::Context, triage: &KernelDumpTriage) {
    if triage.registers.is_empty() {
        return;
    }
    if controls::Section::new("kernel_registers", "Registers")
        .icon(svg::CPU)
        .default_open(false)
        .header(ui)
    {
        listing_rows(
            ui,
            ctx,
            0xB50C_0003,
            triage.registers.iter().cloned().map(|(label, value)| Row {
                label,
                value,
                hover: None,
            }),
        );
    }
}

/// Scanned-stack backtrace — mirrors the app-crash 5-column table.
fn render_stack(ui: &mut Ui, triage: &KernelDumpTriage) {
    let t = Tokens::get(ui.ctx());
    let m = Metrics::get(ui.ctx());
    let n = triage.scanned_stack.len();
    if !controls::section_header(ui, "kernel_stack", svg::STACK, &format!("Call stack ({n})")) {
        return;
    }
    if triage.scanned_stack.is_empty() {
        controls::status_label(
            ui,
            svg::STACK,
            "No stack available for this dump type. Full/BMP/live dumps and \
             triage minidumps with an embedded call stack populate this.",
            t.text_dim,
        );
        return;
    }
    controls::status_label(
        ui,
        svg::INFO,
        "Scanned frames are heuristic (stack values that land in a module \
         range); they can include stale addresses.",
        t.text_faint,
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
        .header(m.row_h, |mut h| {
            for title in ["#", "Trust", "Module", "Address / offset"] {
                h.col(|ui| {
                    controls::section_label(ui, title);
                });
            }
        })
        .body(|mut body| {
            for (i, f) in triage.scanned_stack.iter().enumerate() {
                body.row(m.row_h, |mut row| {
                    row.col(|ui| {
                        ui.monospace(i.to_string());
                    });
                    row.col(|ui| {
                        let color = if f.trust == "context" {
                            t.warn
                        } else {
                            t.text_dim
                        };
                        ui.colored_label(color, &f.trust);
                    });
                    row.col(|ui| {
                        ui.monospace(&f.module);
                    });
                    row.col(|ui| {
                        ui.monospace(format!("{}+{:#x}", f.module, f.offset));
                    });
                });
            }
        });
}

/// Loaded-module table with a name filter and click-to-sort headers.
impl MiniDumpApp {
    fn render_modules(&mut self, ui: &mut Ui, triage: &KernelDumpTriage) {
        let t = Tokens::get(ui.ctx());
        let m = Metrics::get(ui.ctx());
        let known_bad: Vec<String> = self
            .kernel_intel
            .lookup
            .as_ref()
            .map(|l| l.known_bad.iter().map(|(name, _)| name.clone()).collect())
            .unwrap_or_default();
        let blamed = triage.blamed_module.clone();

        let title = format!("Loaded drivers ({})", triage.drivers.len());
        if !controls::Section::new("kernel_modules", &title)
            .icon(svg::PUZZLE_PIECE)
            .default_open(false)
            .header(ui)
        {
            return;
        }
        if triage.drivers.is_empty() {
            controls::status_label(
                ui,
                svg::PUZZLE_PIECE,
                "No driver list available for this dump type.",
                t.text_dim,
            );
            return;
        }

        controls::search_field(
            ui,
            &mut self.kernel_intel.module_filter,
            "driver name…",
            FILTER_W,
        );

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
            let (icon, kind) = match (active, self.kernel_intel.module_sort_desc) {
                (true, true) => (svg::SORT_DESCENDING, ButtonKind::Neutral),
                (true, false) => (svg::SORT_ASCENDING, ButtonKind::Neutral),
                (false, _) => (svg::CARET_UP_DOWN, ButtonKind::Ghost),
            };
            if TextButton::new(label)
                .icon(icon)
                .kind(kind)
                .show(ui)
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
            .header(m.row_h, |mut h| {
                h.col(|ui| sort_header(ui, "Driver", ModuleSort::Name));
                h.col(|ui| sort_header(ui, "Base", ModuleSort::Base));
                h.col(|ui| sort_header(ui, "Size", ModuleSort::Size));
            })
            .body(|mut body| {
                for d in rows {
                    body.row(m.row_h, |mut row| {
                        row.col(|ui| {
                            let flag = if known_bad.contains(&d.name) {
                                Some((svg::WARNING_CIRCLE, t.error))
                            } else if blamed.as_deref() == Some(d.name.as_str()) {
                                Some((svg::CROSSHAIR, t.warn))
                            } else {
                                None
                            };
                            match flag {
                                Some((icon, color)) => {
                                    ui.add(icon.image(m.icon_sm).tint(color));
                                    ui.label(RichText::new(&d.name).monospace().color(color));
                                }
                                None => {
                                    ui.label(RichText::new(&d.name).monospace());
                                }
                            }
                        });
                        row.col(|ui| {
                            ui.monospace(format!("{:#018x}", d.base));
                        });
                        row.col(|ui| {
                            ui.monospace(format!("{} KB", d.size / 1024));
                        });
                    });
                }
            });
    }
}

/// Hex+ASCII windows around RIP and at RSP (populated for full/BMP/live dumps).
fn render_memory(ui: &mut Ui, triage: &KernelDumpTriage) {
    if triage.rip_region.is_none() && triage.rsp_region.is_none() {
        return;
    }
    if !controls::Section::new("kernel_memory", "Memory")
        .icon(svg::BINARY)
        .default_open(false)
        .header(ui)
    {
        return;
    }
    if let Some(r) = &triage.rip_region {
        controls::section_label(ui, "Around RIP");
        hex_dump(ui, r);
        ui.add_space(Metrics::get(ui.ctx()).gap.y);
    }
    if let Some(r) = &triage.rsp_region {
        controls::section_label(ui, "At RSP");
        hex_dump(ui, r);
    }
}

fn hex_dump(ui: &mut Ui, region: &dump_triage::HexRegion) {
    let mut out = String::new();
    for (i, chunk) in region.bytes.chunks(16).enumerate() {
        let addr = region.base + (i * 16) as u64;
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        let ascii: String = chunk
            .iter()
            .map(|b| {
                if b.is_ascii_graphic() {
                    *b as char
                } else {
                    '.'
                }
            })
            .collect();
        out.push_str(&format!("{addr:016x}  {:<47}  {ascii}\n", hex.join(" ")));
    }
    ui.add(
        egui::Label::new(RichText::new(out).font(fonts::mono(HEX_PT)))
            .wrap_mode(egui::TextWrapMode::Extend),
    );
}
