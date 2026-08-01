//! Machine-centric crash history for the connected client.
//!
//! Fleet Intel answers "who else in the fleet crashes like this"; this tab
//! answers "what has THIS box been doing": every `crash_sighting` recorded
//! against the client's computer record or connection string, the parent
//! signature and its verdicts, and the GPU detail carried on
//! `dump_kind = 'gpu_aftermath'` sightings.
//!
//! Text decoded out of a dump is not trusted: a misresolved `DUMP_STRING`
//! yields bytes no font can render, so every parser-sourced value goes through
//! `dump_text` before it reaches a label.

use crossbeam::channel::{unbounded, Receiver, Sender};
use database::schema::{
    crash_intel::{
        machine_crash_history, CrashSighting, CrashSignature, CrashVerdict, MachineCrashHistory,
        GPU_DUMP_KIND,
    },
    ConnectedClient, RecordIdExt,
};
use dump_triage::gpu::GpuCrashDump;
use eframe::egui::{Color32, ComboBox, RichText, ScrollArea, TextEdit, Ui};
use std::time::Duration;

use crate::ui_tools::info_card::{
    badge, expandable_text, fmt_date_time, kv_row, section_card, truncate_chars,
};
use crate::ui_tools::{dump_text, hex_json, icons, theme};
use crate::{PlatformSpawner, Spawner};

const KIND_FILTERS: [&str; 4] = ["all", "minidump", "livekernel", GPU_DUMP_KIND];
/// Stack the columns below this width; two columns below the next.
const ONE_COL_MAX: f32 = 780.0;
const TWO_COL_MAX: f32 = 1180.0;
const MODULE_CHIP_CHARS: usize = 28;

enum CrashDumpMsg {
    History(Box<MachineCrashHistory>),
    Status(String),
}

/// Sanitized view of the selected sighting, rebuilt only when it changes.
struct DetailView {
    key: String,
    modules: dump_text::ModuleSplit,
    excerpt: String,
    payload: Option<serde_json::Value>,
}

pub struct CrashDumpViewer {
    tx: Sender<CrashDumpMsg>,
    rx: Receiver<CrashDumpMsg>,
    /// connection_string the loaded history belongs to; gates the one-shot load.
    loaded_for: Option<String>,
    loading: bool,
    status: String,
    history: MachineCrashHistory,
    /// Index into `history.sightings`, so filtering never invalidates it.
    selected: Option<usize>,
    filter: String,
    kind_filter: String,
    limit: u32,
    detail: Option<DetailView>,
}

impl Default for CrashDumpViewer {
    fn default() -> Self {
        Self::new()
    }
}

impl CrashDumpViewer {
    pub fn new() -> Self {
        let (tx, rx) = unbounded();
        Self {
            tx,
            rx,
            loaded_for: None,
            loading: false,
            status: String::new(),
            history: MachineCrashHistory::default(),
            selected: None,
            filter: String::new(),
            kind_filter: "all".to_string(),
            limit: 200,
            detail: None,
        }
    }

    fn reload(&mut self, client: &ConnectedClient) {
        self.loaded_for = Some(client.connection_string.clone());
        self.loading = true;
        self.status.clear();
        self.selected = None;
        self.detail = None;
        self.history = MachineCrashHistory::default();

        let tx = self.tx.clone();
        let computer = client.computer.clone();
        let connection_string = client.connection_string.clone();
        let limit = self.limit;
        PlatformSpawner::spawn(async move {
            let msg = match machine_crash_history(computer.as_ref(), &connection_string, limit).await
            {
                Ok(h) => CrashDumpMsg::History(Box::new(h)),
                Err(e) => CrashDumpMsg::Status(format!("Crash history load failed: {e}")),
            };
            let _ = tx.try_send(msg);
        });
    }

    /// Drain every queued result and repaint so a load that finished while the
    /// UI was idle is shown without further input.
    fn drain(&mut self, ctx: &eframe::egui::Context) {
        let mut received = false;
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                CrashDumpMsg::History(h) => {
                    self.history = *h;
                    self.detail = None;
                    if self.history.sightings.is_empty() {
                        self.status = "No crash sightings recorded for this machine yet.".to_string();
                    }
                }
                CrashDumpMsg::Status(s) => self.status = s,
            }
            self.loading = false;
            received = true;
        }
        if received {
            ctx.request_repaint();
        }
    }

    /// Sanitize the selected sighting once, not per frame.
    fn ensure_detail(&mut self, idx: usize) {
        let Some(s) = self.history.sightings.get(idx) else {
            self.detail = None;
            return;
        };
        let key = s.id.key_string();
        if self.detail.as_ref().is_some_and(|d| d.key == key) {
            return;
        }
        self.detail = Some(DetailView {
            key,
            modules: dump_text::split_modules(&s.loaded_modules),
            excerpt: dump_text::sanitize_dump_text(&s.raw_excerpt),
            payload: s.triage.as_ref().map(dump_text::sanitize_json_strings),
        });
    }

    pub fn display(&mut self, ui: &mut Ui, client: &ConnectedClient) {
        self.drain(ui.ctx());
        ui.ctx().request_repaint_after(Duration::from_secs(1));
        if self.loaded_for.as_deref() != Some(client.connection_string.as_str()) {
            self.reload(client);
        }

        let mut reload = false;
        ui.horizontal(|ui| {
            ui.heading(format!("{} Crash Dumps — this machine", icons::CRITICAL));
            if self.loading {
                ui.spinner();
            }
            ui.separator();
            ComboBox::from_id_salt("crash_dump_kind")
                .selected_text(kind_label(&self.kind_filter))
                .show_ui(ui, |ui| {
                    for k in KIND_FILTERS {
                        ui.selectable_value(&mut self.kind_filter, k.to_string(), kind_label(k));
                    }
                });
            ui.add(
                TextEdit::singleline(&mut self.filter)
                    .hint_text("bugcheck / module / dump / process")
                    .desired_width(220.0),
            );
            if ui
                .button(format!("{} Refresh", icons::REFRESH))
                .on_hover_text("Re-read this machine's sightings, signatures, and verdicts.")
                .clicked()
            {
                reload = true;
            }
        });

        if !self.status.is_empty() {
            ui.label(RichText::new(&self.status).color(theme::warn(ui)).small());
        }

        ui.add_space(4.0);
        self.summary_strip(ui);
        ui.add_space(4.0);

        if let Some(idx) = self.selected.filter(|i| *i < self.history.sightings.len()) {
            self.ensure_detail(idx);
        }

        let avail_h = ui.available_height();
        let width = ui.available_width();
        let cols_n = if width < ONE_COL_MAX {
            1
        } else if width < TWO_COL_MAX {
            2
        } else {
            3
        };
        ui.columns(cols_n, |cols| {
            self.records_column(&mut cols[0], avail_h);
            self.detail_column(&mut cols[1.min(cols_n - 1)], avail_h);
            self.payload_column(&mut cols[2.min(cols_n - 1)], avail_h);
        });

        if reload {
            self.reload(client);
        }
    }

    /// Counts by dump kind and by bugcheck/DXGI code, repeats highlighted.
    fn summary_strip(&self, ui: &mut Ui) {
        if self.history.sightings.is_empty() {
            return;
        }
        ui.group(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!("{} crash record(s)", self.history.sightings.len()))
                        .strong(),
                );
                ui.separator();
                for (kind, n) in self.history.counts_by_kind() {
                    let (glyph, color) = kind_badge(ui, &kind);
                    ui.label(
                        RichText::new(format!("{glyph} {} x{n}", kind_label(&kind))).color(color),
                    );
                }
                ui.separator();
                for (code, n) in self.history.counts_by_code() {
                    let color = if n >= 2 {
                        theme::warn(ui)
                    } else {
                        theme::weak_text(ui)
                    };
                    ui.label(RichText::new(format!("{code} x{n}")).monospace().color(color));
                }
            });
        });
    }

    /// Newest-first rows. Four columns only, so the list survives a third of
    /// the width; module/process/dump live in the detail column.
    fn records_column(&mut self, ui: &mut Ui, avail_h: f32) {
        if self.history.sightings.is_empty() {
            ui.label(
                RichText::new("No crash sightings recorded for this machine.")
                    .color(theme::weak_text(ui)),
            );
            return;
        }
        let needle = self.filter.trim().to_ascii_lowercase();
        let kind = self.kind_filter.clone();
        let selected = self.selected;
        let history = &self.history;
        let mut clicked: Option<usize> = None;
        let mut shown = 0usize;

        let picked = ScrollArea::vertical()
            .id_salt("crash_records")
            .max_height(avail_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (idx, s) in history.sightings.iter().enumerate() {
                    if kind != "all" && s.dump_kind != kind {
                        continue;
                    }
                    let sig = history.signature_for(s);
                    if !needle.is_empty() && !sighting_matches(s, sig, &needle) {
                        continue;
                    }
                    shown += 1;

                    let when = fmt_date_time(&s.created_at);
                    let (glyph, kcolor) = kind_badge(ui, &s.dump_kind);
                    let code = match sig {
                        Some(g) => code_label(g),
                        None => s.signature.key_string(),
                    };
                    let (vglyph, vcolor) = if history.has_verdict(&s.signature) {
                        (icons::STATUS_ON, theme::success(ui))
                    } else {
                        (icons::STATUS_DOT, theme::weak_text(ui))
                    };
                    let row = ui.selectable_label(
                        selected == Some(idx),
                        RichText::new(format!("{when}  {glyph}  {code}")).monospace(),
                    );
                    if row.clicked() {
                        clicked = Some(idx);
                    }
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(kind_label(&s.dump_kind))
                                .color(kcolor)
                                .small(),
                        );
                        ui.label(RichText::new(vglyph).color(vcolor).small());
                    });
                    ui.separator();
                }
                if shown == 0 {
                    ui.label(
                        RichText::new("No sightings match the current filter.")
                            .color(theme::weak_text(ui))
                            .small(),
                    );
                }
                clicked
            })
            .inner;

        if let Some(idx) = picked {
            self.selected = Some(idx);
        }
    }

    /// Everything stored for the selected sighting, its signature, verdicts.
    fn detail_column(&mut self, ui: &mut Ui, avail_h: f32) {
        let Some(idx) = self.selected.filter(|i| *i < self.history.sightings.len()) else {
            ui.label(
                RichText::new("Select a crash to see everything we hold for it.")
                    .color(theme::weak_text(ui))
                    .small(),
            );
            return;
        };
        let history = &self.history;
        let s = &history.sightings[idx];
        let sig = history.signature_for(s);
        let verdicts = history.verdicts_for(&s.signature);
        let Some(view) = self.detail.as_ref() else { return };

        ScrollArea::vertical()
            .id_salt("crash_detail")
            .max_height(avail_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                Self::detail_body(ui, s, sig, &verdicts, view);
            });
    }

    fn detail_body(
        ui: &mut Ui,
        s: &CrashSighting,
        sig: Option<&CrashSignature>,
        verdicts: &[&CrashVerdict],
        view: &DetailView,
    ) {
        let gpu = gpu_detail(s);
        let (glyph, color) = kind_badge(ui, &s.dump_kind);

        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!("{glyph} {}", kind_label(&s.dump_kind)))
                    .color(color)
                    .strong(),
            );
            let code = match sig {
                Some(g) => code_label(g),
                None => s.signature.key_string(),
            };
            ui.label(RichText::new(code).monospace().strong());
            if ui
                .button(format!("{} Copy for ticket", icons::COPY))
                .on_hover_text("Copy this crash, its signature, and its verdicts as plain text.")
                .clicked()
            {
                ui.ctx()
                    .copy_text(clipboard_text(s, sig, verdicts, gpu.as_ref()));
            }
            if let Some(name) = s.dump_name.as_deref() {
                if ui
                    .button(format!("{} Copy dump path", icons::CLIPBOARD))
                    .on_hover_text("Copy the dump's recorded name/path on the client.")
                    .clicked()
                {
                    ui.ctx().copy_text(name.to_string());
                }
            }
        });

        if let Some(g) = gpu.as_ref() {
            ui.add_space(4.0);
            gpu_section(ui, g);
        }

        ui.add_space(4.0);
        section_card(ui, icons::FILE_TEXT, "Sighting", None, |ui| {
            kv_row(ui, "record", &s.id.key_string());
            dirty_kv_row(ui, "dump", s.dump_name.as_deref());
            kv_row(ui, "dump kind", &s.dump_kind);
            opt_kv_row(ui, "dump time", s.dump_time.as_deref());
            kv_row(ui, "ingested", &fmt_date_time(&s.created_at));
            opt_kv_row(ui, "offset", s.offset.as_deref());
            opt_kv_row(ui, "module version", s.module_version.as_deref());
            dirty_kv_row(ui, "failure bucket", s.failure_bucket.as_deref());
            dirty_kv_row(ui, "process", s.process_name.as_deref());
            dirty_kv_row(ui, "probable cause", s.caused_by.as_deref());
            opt_kv_row(ui, "connection", s.connection_string.as_deref());
            kv_row(ui, "computer", &opt_key(&s.computer));
            kv_row(ui, "session", &opt_key(&s.session_ref));
            kv_row(ui, "task", &opt_key(&s.task_ref));
        });

        if view.modules.total() > 0 {
            ui.add_space(4.0);
            modules_section(ui, s, view);
        }

        if !view.excerpt.is_empty() {
            ui.add_space(4.0);
            section_card(ui, icons::LIST, "Analyzer excerpt", None, |ui| {
                expandable_text(ui, &format!("excerpt-{}", view.key), &view.excerpt, 8);
            });
        }

        if let Some(g) = sig {
            ui.add_space(4.0);
            let subtitle = format!("{} machine(s)", g.machines.len());
            section_card(
                ui,
                icons::DIAGNOSTICS,
                "Fleet signature",
                Some(&subtitle),
                |ui| {
                    kv_row(ui, "code", &code_label(g));
                    dirty_kv_row(ui, "module", Some(&g.module));
                    kv_row(ui, "sightings", &g.sighting_count.to_string());
                    kv_row(ui, "first seen", &fmt_date_time(&g.first_seen));
                    kv_row(ui, "last seen", &fmt_date_time(&g.last_seen));
                    list_row(ui, "offsets", &g.offsets);
                    list_row(ui, "module versions", &g.module_versions);
                    list_row(ui, "failure buckets", &g.failure_buckets);
                    list_row(ui, "tags", &g.tags);
                },
            );
        }

        ui.add_space(4.0);
        let vsub = format!("{}", verdicts.len());
        section_card(ui, icons::LIGHTBULB, "Verdicts", Some(&vsub), |ui| {
            if verdicts.is_empty() {
                ui.label(
                    RichText::new("No verdict recorded for this signature yet.")
                        .small()
                        .color(theme::weak_text(ui)),
                );
            }
            for (i, v) in verdicts.iter().enumerate() {
                let meta = format!("[{} | {} | {}]", v.confidence, v.source, v.author);
                ui.label(RichText::new(meta).small().color(theme::weak_text(ui)));
                expandable_text(ui, &format!("verdict-{}-{i}", view.key), &v.verdict, 6);
                if !v.fix.is_empty() {
                    ui.label(RichText::new("Fix").small().color(theme::success(ui)));
                    expandable_text(ui, &format!("fix-{}-{i}", view.key), &v.fix, 6);
                }
                ui.separator();
            }
        });
    }

    /// The stored payload, addresses as hex and marked strings tinted.
    fn payload_column(&mut self, ui: &mut Ui, avail_h: f32) {
        let Some(view) = self.detail.as_ref() else { return };
        let Some(blob) = view.payload.as_ref() else { return };
        let salt = format!("crash-triage-{}", view.key);
        ScrollArea::vertical()
            .id_salt("crash_payload")
            .max_height(avail_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                section_card(ui, icons::LIST, "Full JSON payload", None, |ui| {
                    hex_json::dump_json_tree(ui, &salt, blob);
                });
            });
    }
}

/// Module chips, with unrenderable entries behind one disclosure so a parser
/// fault stays visible instead of vanishing.
fn modules_section(ui: &mut Ui, s: &CrashSighting, view: &DetailView) {
    let split = &view.modules;
    let suspect_n = split.readable.iter().filter(|m| m.suspect).count();
    let subtitle = match (suspect_n, split.unreadable.len()) {
        (0, 0) => String::new(),
        (sus, 0) => format!("{sus} suspect"),
        (0, bad) => format!("{bad} unreadable"),
        (sus, bad) => format!("{sus} suspect, {bad} unreadable"),
    };
    let title = format!("Modules loaded at crash time ({})", split.total());
    let sub = (!subtitle.is_empty()).then_some(subtitle.as_str());
    section_card(ui, icons::PACKAGE, &title, sub, |ui| {
        let clean = theme::info(ui);
        let odd = theme::warn(ui);
        ui.horizontal_wrapped(|ui| {
            for m in &split.readable {
                let color = if m.suspect { odd } else { clean };
                let chip = truncate_chars(&m.text, MODULE_CHIP_CHARS);
                let hint = if m.suspect {
                    format!("{}\n\nNot a valid module name — the parser truncated it.", m.text)
                } else {
                    m.text.clone()
                };
                badge(ui, &chip, color).on_hover_text(hint);
            }
        });
        if split.unreadable.is_empty() {
            return;
        }
        let header = format!(
            "{} {} unreadable entries",
            icons::STATUS_WARN,
            split.unreadable.len()
        );
        ui.collapsing(header, |ui| {
            ui.label(
                RichText::new(
                    "These are not text. The dump parser resolved a name outside the string pool, so these bytes came from unrelated dump data.",
                )
                .small()
                .color(theme::weak_text(ui)),
            );
            for entry in &split.unreadable {
                ui.label(RichText::new(&entry.text).monospace().small());
            }
            ui.horizontal(|ui| {
                if ui.small_button("Copy sanitized").clicked() {
                    let text = split
                        .unreadable
                        .iter()
                        .map(|e| e.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    ui.ctx().copy_text(text);
                }
                if ui
                    .small_button("Copy raw (escaped)")
                    .on_hover_text("Lossless escaped bytes, for a parser bug report.")
                    .clicked()
                {
                    let text = s
                        .loaded_modules
                        .iter()
                        .map(|m| dump_text::escaped_bytes(m))
                        .collect::<Vec<_>>()
                        .join("\n");
                    ui.ctx().copy_text(text);
                }
            });
        });
    });
}

/// GPU blob on a `gpu_aftermath` sighting. Gated on `dump_kind` because every
/// field of `GpuCrashDump` is `#[serde(default)]`, so a kernel triage blob
/// would otherwise deserialize into an all-empty GPU record.
fn gpu_detail(s: &CrashSighting) -> Option<GpuCrashDump> {
    if s.dump_kind != GPU_DUMP_KIND {
        return None;
    }
    let blob = s.triage.as_ref()?;
    let gpu: GpuCrashDump = serde_json::from_value(blob.clone()).ok()?;
    gpu.is_gpu_crash().then_some(gpu)
}

/// GPU detail with the ACTIVE breadcrumb path first; the deepest node names the
/// render scope the GPU stopped inside.
fn gpu_section(ui: &mut Ui, g: &GpuCrashDump) {
    section_card(
        ui,
        icons::MONITOR,
        "GPU crash — D3D12 / NVIDIA Aftermath",
        None,
        |ui| {
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    g.dxgi_reason.as_deref().unwrap_or("-"),
                    g.dxgi_reason_name.as_deref().unwrap_or("unknown reason")
                ))
                .monospace()
                .size(15.0)
                .color(theme::error(ui)),
            );

            if !g.breadcrumb_active.is_empty() {
                ui.label(
                    RichText::new("Active render path when the GPU stopped:")
                        .small()
                        .color(theme::weak_text(ui)),
                );
                let depth = g.breadcrumb_active.len();
                for (i, node) in g.breadcrumb_active.iter().enumerate() {
                    let indent = "  ".repeat(i);
                    if i + 1 == depth {
                        ui.label(
                            RichText::new(format!(
                                "{indent}{} {node}   <- stopped here",
                                icons::ARROW_RIGHT
                            ))
                            .monospace()
                            .strong()
                            .color(theme::error(ui)),
                        );
                    } else {
                        ui.label(
                            RichText::new(format!("{indent}{} {node}", icons::CARET_DOWN))
                                .monospace()
                                .color(theme::info(ui)),
                        );
                    }
                }
            } else if let Some(raw) = g.breadcrumbs_raw.as_deref() {
                ui.collapsing("Raw breadcrumb tree", |ui| {
                    ui.label(RichText::new(raw).monospace().small());
                });
            }

            opt_kv_row(ui, "adapter", g.gpu_adapter_name.as_deref());
            opt_kv_row(ui, "device id", g.gpu_device_id.as_deref());
            opt_kv_row(ui, "user driver", g.gpu_driver_version.as_deref());
            opt_kv_row(ui, "internal driver", g.gpu_driver_internal_version.as_deref());
            opt_kv_row(ui, "driver date", g.gpu_driver_date.as_deref());
            opt_kv_row(ui, "RHI", g.rhi_name.as_deref());
            opt_kv_row(ui, "crash type", g.crash_type.as_deref());
            opt_kv_row(ui, "error", g.error_message.as_deref());
            opt_kv_row(ui, "engine", g.engine_version.as_deref());
            opt_kv_row(ui, "map", g.map_name.as_deref());
            opt_kv_row(ui, "GI quality", g.gi_quality.as_deref());
            opt_kv_row(ui, "Nanite", g.use_nanite.as_deref());
            opt_kv_row(ui, "crash folder", g.crash_folder.as_deref());
            if let Some(stuck) = g.is_stuck {
                let value = match (stuck, g.stuck_thread_id) {
                    (true, Some(id)) => format!("yes (tid {id})"),
                    (true, None) => "yes".to_string(),
                    (false, _) => "no".to_string(),
                };
                kv_row(ui, "hung thread", &value);
            }
            if let Some(secs) = g.seconds_since_start {
                kv_row(
                    ui,
                    "uptime at crash",
                    &format!("{}h {}m", secs / 3600, (secs % 3600) / 60),
                );
            }
            if let Some(rev) = g.cpu_microcode_revision {
                kv_row(ui, "CPU microcode", &format!("{rev} ({rev:#x})"));
            }
        },
    );
}

/// `kv_row` that renders nothing when the value is absent or blank.
fn opt_kv_row(ui: &mut Ui, name: &str, value: Option<&str>) {
    if let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) {
        kv_row(ui, name, v);
    }
}

/// `opt_kv_row` for parser-sourced text: sanitized, and tinted when it was not
/// renderable to begin with.
fn dirty_kv_row(ui: &mut Ui, name: &str, value: Option<&str>) {
    let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return;
    };
    let report = dump_text::sanitize_dump_text_report(v);
    if report.is_clean() {
        kv_row(ui, name, &report.text);
        return;
    }
    let weak = theme::weak_text(ui);
    let warn = theme::warn(ui);
    ui.horizontal_top(|ui| {
        ui.label(RichText::new(name).small().color(weak));
        ui.label(RichText::new(&report.text).monospace().color(warn))
            .on_hover_text(format!(
                "{} codepoint(s) no bundled font can render — the parser produced bytes that are not text.",
                report.replaced
            ));
    });
}

fn list_row(ui: &mut Ui, label: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    let joined = dump_text::sanitize_dump_text(&items.join(", "));
    let short = truncate_chars(&joined, 120);
    kv_row(ui, label, &short);
}

fn opt_key(id: &Option<database::schema::RecordId>) -> String {
    match id {
        Some(r) => r.key_string(),
        None => "-".to_string(),
    }
}

fn kind_label(kind: &str) -> &str {
    match kind {
        "all" => "all kinds",
        "minidump" => "minidump",
        "livekernel" => "live kernel",
        GPU_DUMP_KIND => "GPU aftermath",
        other => other,
    }
}

fn kind_badge(ui: &Ui, kind: &str) -> (&'static str, Color32) {
    match kind {
        GPU_DUMP_KIND => (icons::MONITOR, theme::accent_secondary(ui)),
        "livekernel" => (icons::FLASK, theme::warn(ui)),
        "minidump" => (icons::FILE_TEXT, theme::info(ui)),
        _ => (icons::STATUS_DOT, theme::weak_text(ui)),
    }
}

/// `0x116 VIDEO_TDR_FAILURE`, or the bare code when no name is recorded.
fn code_label(sig: &CrashSignature) -> String {
    if sig.bugcheck_name.is_empty() {
        sig.bugcheck_code.clone()
    } else {
        format!("{} {}", sig.bugcheck_code, sig.bugcheck_name)
    }
}

fn sighting_matches(s: &CrashSighting, sig: Option<&CrashSignature>, needle: &str) -> bool {
    let mut hay = format!(
        "{} {} {} {} {} {}",
        s.dump_kind,
        s.dump_name.as_deref().unwrap_or(""),
        s.process_name.as_deref().unwrap_or(""),
        s.caused_by.as_deref().unwrap_or(""),
        s.failure_bucket.as_deref().unwrap_or(""),
        s.module_version.as_deref().unwrap_or(""),
    );
    if let Some(g) = sig {
        hay.push_str(&format!(" {} {} {}", g.bugcheck_code, g.bugcheck_name, g.module));
    }
    hay.to_ascii_lowercase().contains(needle)
}

/// Ticket-pasteable plain text for one crash.
fn clipboard_text(
    s: &CrashSighting,
    sig: Option<&CrashSignature>,
    verdicts: &[&CrashVerdict],
    gpu: Option<&GpuCrashDump>,
) -> String {
    let head = match sig {
        Some(g) => code_label(g),
        None => s.signature.key_string(),
    };
    let mut out = format!("=== {head} - {} ===\n", kind_label(&s.dump_kind));
    out.push_str(&format!("dump:      {}\n", s.dump_name.as_deref().unwrap_or("-")));
    out.push_str(&format!("dump time: {}\n", s.dump_time.as_deref().unwrap_or("-")));
    out.push_str(&format!("ingested:  {}\n", fmt_date_time(&s.created_at)));
    if let Some(g) = sig {
        out.push_str(&format!(
            "module:    {} {}\n",
            g.module,
            s.module_version.as_deref().unwrap_or("")
        ));
        out.push_str(&format!(
            "fleet:     {} sighting(s) on {} machine(s)\n",
            g.sighting_count,
            g.machines.len()
        ));
    }
    if let Some(p) = s.process_name.as_deref() {
        out.push_str(&format!("process:   {p}\n"));
    }
    if !s.loaded_modules.is_empty() {
        let split = dump_text::split_modules(&s.loaded_modules);
        let named = split
            .readable
            .iter()
            .map(|m| m.text.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("modules:   {named}\n"));
        if !split.unreadable.is_empty() {
            out.push_str(&format!(
                "           (+{} unreadable entries — parser resolved a name outside the string pool)\n",
                split.unreadable.len()
            ));
        }
    }
    if let Some(g) = gpu {
        out.push_str(&format!(
            "gpu:       {} {}\n",
            g.dxgi_reason.as_deref().unwrap_or("-"),
            g.dxgi_reason_name.as_deref().unwrap_or("-")
        ));
        if let Some(a) = g.gpu_adapter_name.as_deref() {
            out.push_str(&format!("adapter:   {a}\n"));
        }
        if let Some(d) = g.gpu_driver_version.as_deref() {
            out.push_str(&format!("driver:    {d}\n"));
        }
        if !g.breadcrumb_active.is_empty() {
            out.push_str(&format!("stopped:   {}\n", g.breadcrumb_active.join(" > ")));
        }
    }
    for v in verdicts {
        out.push_str(&format!("verdict:   [{}] {}\n", v.confidence, v.verdict));
        if !v.fix.is_empty() {
            out.push_str(&format!("fix:       {}\n", v.fix));
        }
    }
    if !s.raw_excerpt.is_empty() {
        out.push_str("---\n");
        out.push_str(&dump_text::sanitize_dump_text(&s.raw_excerpt));
        out.push('\n');
    }
    out
}
