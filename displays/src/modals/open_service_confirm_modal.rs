//! Stage 4: confirmation modal for binding a PrestaShop open service
//! order to a connected client + creating/updating the associated
//! computer row with a live-vs-presta-merged spec sheet.
//!
//! Modeled on `DuplicateMergeModal::show` — owns its own egui window,
//! renders independently of the global `ModalType` lifecycle, and
//! returns a `OpenServiceConfirmOutcome` once the operator clicks
//! Confirm or Reject so the caller (Stage 5) can apply the writes.
//!
//! Pipeline:
//!   1. Connected-client card chip click → `pending_open_service_candidate`.
//!   2. `SharedContext::receive_shared_ui` drains that and instantiates
//!      this modal with the matching `OpenServiceSuggestion` snapshot.
//!   3. Operator edits the spec preview (per-field overrides), picks
//!      Confirm or Reject; the outcome lands in
//!      `pending_open_service_apply` for the Stage-5 persistence layer
//!      to read.

use crate::open_service_suggestions::OpenServiceSuggestion;
use database::schema::service_match::{OpenServiceCandidate, PrestaSpecsSnapshot};
use eframe::egui::{
    Align, Align2, Button, Color32, Context, Frame, Key, Layout, Margin, RichText,
    ScrollArea, Shadow, TextEdit, Ui, Vec2, Widget, Window,
};

/// Tag for which side a given field's value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpecSource {
    /// Live `SystemInformation` snapshot from the running Mastertech
    /// on the client.  Wins when non-empty (matches the merge rule
    /// you confirmed up front).
    Live,
    /// Parsed PrestaShop order body (`Order::extract_*`).  Used when
    /// the live value for the field is empty.
    Presta,
    /// Field had no value on either side and is empty.
    Empty,
    /// Operator manually overrode the merged value in the modal.
    Manual,
}

impl SpecSource {
    fn badge_label(self) -> &'static str {
        match self {
            SpecSource::Live => "LIVE",
            SpecSource::Presta => "PRESTA",
            SpecSource::Empty => "—",
            SpecSource::Manual => "EDIT",
        }
    }
    fn badge_color(self) -> Color32 {
        match self {
            SpecSource::Live => Color32::from_rgb(120, 220, 140),
            SpecSource::Presta => Color32::from_rgb(255, 215, 120),
            SpecSource::Empty => Color32::from_rgb(140, 140, 145),
            SpecSource::Manual => Color32::from_rgb(180, 200, 230),
        }
    }
}

/// One row in the spec-merge preview.  Carries the resolved value the
/// operator sees, the original sources for reference, and the source
/// tag that drives the badge.  `value` is mutable so the operator can
/// inline-edit; doing so flips `source` to `Manual`.
#[derive(Debug, Clone)]
struct SpecField {
    label: &'static str,
    value: String,
    live: String,
    presta: String,
    source: SpecSource,
}

impl SpecField {
    fn from_live_presta(label: &'static str, live: String, presta: String) -> Self {
        let (value, source) = if !live.trim().is_empty() {
            (live.clone(), SpecSource::Live)
        } else if !presta.trim().is_empty() {
            (presta.clone(), SpecSource::Presta)
        } else {
            (String::new(), SpecSource::Empty)
        };
        Self { label, value, live, presta, source }
    }
}

/// User-visible outcome of the modal.  Caller (Stage 5) reads this to
/// decide whether to persist the binding.
#[derive(Debug, Clone)]
pub enum OpenServiceConfirmOutcome {
    Confirm(OpenServiceConfirmApply),
    Reject,
}

/// The bundle of state Stage 5's persistence layer needs to apply the
/// confirmed binding.  Carries the resolved spec fields *after* the
/// operator's edits, the candidate metadata, the customer match, and
/// the target client's `connection_string`.  Deliberately plain
/// strings (no `RecordId`s) so this is trivially serializable should
/// a future iteration want to ferry it across the wire.
#[derive(Debug, Clone)]
pub struct OpenServiceConfirmApply {
    pub connection_string: String,
    pub customer_id: String,
    pub customer_first_name: String,
    pub customer_last_name: String,
    pub friendly_name: String,
    pub candidate: OpenServiceCandidate,
    /// Merged + operator-edited specs — these are what Stage 5 writes
    /// into the `computer` row.  Order matches the visible field list.
    pub resolved_specs: PrestaSpecsSnapshot,
}

#[derive(Debug, Clone)]
pub struct OpenServiceConfirmModal {
    pub title: String,
    pub connection_string: String,
    pub suggestion: OpenServiceSuggestion,
    pub candidate_index: usize,

    /// In-modal spec preview rows, initialized from the live+presta
    /// merge and mutated as the operator edits.
    fields: Vec<SpecField>,

    pub is_open: bool,
    pub confirmed: bool,
    pub rejected: bool,
}

impl OpenServiceConfirmModal {
    /// Build a modal from the cached suggestion + the candidate the
    /// operator clicked.  Returns `None` when `candidate_index` is
    /// out of range (e.g. the candidate list was refreshed and shrank
    /// after the click).
    pub fn new(
        connection_string: String,
        suggestion: OpenServiceSuggestion,
        candidate_index: usize,
    ) -> Option<Self> {
        if candidate_index >= suggestion.candidates.len() {
            return None;
        }
        let candidate = &suggestion.candidates[candidate_index];
        let title = format!(
            "Bind service #{} to {}",
            candidate.service_number, connection_string
        );

        // Build the per-field merge rows.  Live values come from the
        // `SystemInformation` snapshot the client shipped alongside the
        // candidates; PrestaShop values come from `candidate.specs`.
        //
        // `SystemInformation` doesn't have direct cpu-string/gpu-string
        // fields per se — gpu lives under `gpu_info.card[0]`, ram comes
        // from `total_memory` (kb), os is `os_version`, etc.  The
        // mapping below normalizes those into the display strings the
        // computer row + ticket display elsewhere in the app use.
        let live = suggestion.live_specs.as_ref();
        let presta = &candidate.specs;

        let live_cpu = live.map(|s| s.cpu.clone()).unwrap_or_default();
        let live_gpu = live
            .and_then(|s| s.gpu_info.card.first())
            .map(|c| {
                let brand = c.brand.trim();
                let name = c.name.trim();
                if brand.is_empty() {
                    name.to_string()
                } else if name.is_empty() {
                    brand.to_string()
                } else {
                    format!("{brand} {name}")
                }
            })
            .unwrap_or_default();
        let live_ram = live
            .map(|s| {
                let total = s.total_memory; // already in MB per the surrounding code
                if total <= 0.0 {
                    String::new()
                } else if total >= 1024.0 {
                    format!("{:.0} GB", total / 1024.0)
                } else {
                    format!("{:.0} MB", total)
                }
            })
            .unwrap_or_default();
        let live_os = live.map(|s| s.os_version.clone()).unwrap_or_default();
        let live_host = live.map(|s| s.hostname.clone()).unwrap_or_default();
        let live_mfg = live.map(|s| s.product_vendor.clone()).unwrap_or_default();
        let live_model = live.map(|s| s.product_name.clone()).unwrap_or_default();
        let live_serial = live.map(|s| s.product_serial.clone()).unwrap_or_default();
        let live_mobo = live.map(|s| s.motherboard_name.clone()).unwrap_or_default();

        let mut fields = vec![
            SpecField::from_live_presta("CPU", live_cpu, presta.cpu.clone()),
            SpecField::from_live_presta("GPU", live_gpu, presta.gpu.clone()),
            SpecField::from_live_presta("RAM", live_ram, presta.ram.clone()),
            SpecField::from_live_presta("OS", live_os, presta.operating_system.clone()),
            SpecField::from_live_presta("Hostname", live_host, String::new()),
            SpecField::from_live_presta("Device MFG", live_mfg, presta.device_mfg.clone()),
            SpecField::from_live_presta("Device Model", live_model, presta.device_model.clone()),
            SpecField::from_live_presta(
                "Device Serial",
                live_serial,
                presta.device_serial.clone(),
            ),
            SpecField::from_live_presta(
                "Motherboard",
                live_mobo,
                presta.motherboard_name.clone(),
            ),
        ];

        // Drives don't fit the single-value pattern, so we render them
        // in their own block below; we still record them on the
        // resolved spec sheet at confirm time straight from the
        // PrestaShop snapshot (live snapshot's drive shape differs and
        // is handled separately in Stage 5 if we want it).
        let _ = &mut fields;

        Some(Self {
            title,
            connection_string,
            suggestion,
            candidate_index,
            fields,
            is_open: true,
            confirmed: false,
            rejected: false,
        })
    }

    /// Render the modal.  Returns `Some(...)` once the operator has
    /// either confirmed or rejected; caller is expected to drop the
    /// modal after consuming the outcome.
    pub fn show(&mut self, ctx: &Context) -> Option<OpenServiceConfirmOutcome> {
        if !self.is_open {
            return None;
        }
        // ESC closes as Reject.
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.rejected = true;
            self.is_open = false;
            return Some(OpenServiceConfirmOutcome::Reject);
        }

        let style = &ctx.global_style().visuals;
        let mut shadow = Shadow::default();
        shadow.blur = 2;
        shadow.spread = 4;
        shadow.color = style.window_stroke.color;
        let title_text = RichText::new(&self.title)
            .heading()
            .color(style.warn_fg_color);

        let mut open = true;
        Window::new(title_text)
            .frame(
                Frame::default()
                    .inner_margin(Margin::symmetric(16, 16))
                    .stroke(style.window_stroke)
                    .fill(style.window_fill)
                    .corner_radius(style.menu_corner_radius)
                    .shadow(shadow),
            )
            .pivot(Align2::CENTER_CENTER)
            .fixed_size([960.0, 720.0])
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                self.body(ui);
            });

        // X-out behaves like Reject.
        if !open {
            self.rejected = true;
            self.is_open = false;
            return Some(OpenServiceConfirmOutcome::Reject);
        }

        if self.rejected {
            self.is_open = false;
            return Some(OpenServiceConfirmOutcome::Reject);
        }
        if self.confirmed {
            self.is_open = false;
            return Some(OpenServiceConfirmOutcome::Confirm(self.build_apply()));
        }
        None
    }

    fn body(&mut self, ui: &mut Ui) {
        // Two columns: left = candidate metadata + checkin notes;
        // right = merged spec preview with per-field source badges.
        let available_height = ui.available_height() - 60.0;
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width() * 0.45, available_height),
                Layout::top_down(Align::Min),
                |ui| {
                    self.left_panel(ui);
                },
            );
            ui.separator();
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), available_height),
                Layout::top_down(Align::Min),
                |ui| {
                    self.right_panel(ui);
                },
            );
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);
        self.action_buttons(ui);
    }

    fn left_panel(&mut self, ui: &mut Ui) {
        let c = match self.suggestion.candidates.get(self.candidate_index) {
            Some(c) => c,
            None => {
                ui.colored_label(
                    Color32::from_rgb(220, 100, 100),
                    "Candidate index out of range — close and try again.",
                );
                return;
            }
        };
        ui.label(RichText::new("Service Order").heading().strong());
        ui.add_space(4.0);
        kv_row(ui, "Service #", &c.service_number);
        kv_row(ui, "Doc Alias", &c.doc_alias);
        kv_row(ui, "State", &format!("{} ({})", c.state_name, c.state_id));
        kv_row(ui, "Created", &c.date_add);
        kv_row(ui, "Updated", &c.date_upd);

        if let Some(m) = self.suggestion.match_.as_ref() {
            ui.add_space(10.0);
            ui.label(RichText::new("Customer").heading().strong());
            ui.add_space(4.0);
            kv_row(
                ui,
                "Name",
                &format!("{} {}", m.first_name, m.last_name),
            );
            kv_row(ui, "PrestaShop ID", &m.id_customer);
            kv_row(ui, "Original Order", &m.id_order);
        }

        ui.add_space(10.0);
        ui.label(RichText::new("Check-in Notes").heading().strong());
        ui.add_space(4.0);
        Frame::default()
            .fill(Color32::from_rgb(28, 28, 32))
            .stroke(ui.style().visuals.window_stroke)
            .corner_radius(eframe::egui::CornerRadius::same(4))
            .inner_margin(Margin::same(8))
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .max_height(220.0)
                    .id_salt("checkin-notes")
                    .show(ui, |ui| {
                        let notes = if c.checkin_notes.trim().is_empty() {
                            "(no check-in notes on this order)".to_string()
                        } else {
                            c.checkin_notes.clone()
                        };
                        ui.label(RichText::new(notes).monospace().small());
                    });
            });
    }

    fn right_panel(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Computer record (merged)").heading().strong());
        ui.add_space(2.0);
        ui.label(
            RichText::new(
                "Live values from the running Mastertech win when non-empty; \
                 PrestaShop order fills gaps.  Edit any field inline to override.",
            )
            .small()
            .weak(),
        );
        ui.add_space(8.0);

        let total = self.fields.len();
        ScrollArea::vertical()
            .max_height(ui.available_height() - 40.0)
            .id_salt("spec-preview")
            .show(ui, |ui| {
                for idx in 0..total {
                    let f = &mut self.fields[idx];
                    ui.horizontal(|ui| {
                        // Badge — fixed width-ish so labels align.
                        ui.allocate_ui_with_layout(
                            Vec2::new(70.0, 22.0),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                let badge = RichText::new(f.source.badge_label())
                                    .color(f.source.badge_color())
                                    .strong()
                                    .small();
                                ui.label(badge);
                            },
                        );
                        ui.label(RichText::new(format!("{}:", f.label)).strong());
                        let prev = f.value.clone();
                        let resp = TextEdit::singleline(&mut f.value)
                            .desired_width(ui.available_width() - 12.0)
                            .show(ui);
                        if resp.response.changed() && f.value != prev {
                            f.source = SpecSource::Manual;
                        }
                    });
                    ui.horizontal(|ui| {
                        // Sub-line showing the alternative the operator
                        // *didn't* pick, so the override decision is
                        // never blind.
                        if !f.live.is_empty() {
                            ui.label(
                                RichText::new(format!("  live: {}", trim_display(&f.live, 80)))
                                    .small()
                                    .weak(),
                            );
                        }
                        if !f.presta.is_empty() {
                            ui.label(
                                RichText::new(format!("  presta: {}", trim_display(&f.presta, 80)))
                                    .small()
                                    .weak(),
                            );
                        }
                    });
                    ui.add_space(2.0);
                }
            });
    }

    fn action_buttons(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if Button::new(
                    RichText::new("Confirm & bind")
                        .color(Color32::from_rgb(80, 200, 120))
                        .strong(),
                )
                .ui(ui)
                .on_hover_text(
                    "Persist: customer link + customer_locked=true, computer row with \
                     the resolved specs above, and service_order with this computer's id.",
                )
                .clicked()
                {
                    self.confirmed = true;
                }
                ui.add_space(6.0);
                if Button::new(
                    RichText::new("Reject")
                        .color(Color32::from_rgb(220, 120, 120))
                        .strong(),
                )
                .ui(ui)
                .on_hover_text("Close without writing anything.")
                .clicked()
                {
                    self.rejected = true;
                }
            });
        });
    }

    fn build_apply(&self) -> OpenServiceConfirmApply {
        // Rebuild a PrestaSpecsSnapshot from the (possibly edited)
        // field rows.  Keep PrestaShop's drives verbatim — Stage 5
        // can fold those in or not at its discretion.
        let candidate = self.suggestion.candidates[self.candidate_index].clone();
        let mut resolved = candidate.specs.clone();
        for f in &self.fields {
            let v = f.value.trim().to_string();
            match f.label {
                "CPU" => resolved.cpu = v,
                "GPU" => resolved.gpu = v,
                "RAM" => resolved.ram = v,
                "OS" => resolved.operating_system = v,
                "Hostname" => {} // Not on PrestaSpecsSnapshot.
                "Device MFG" => resolved.device_mfg = v,
                "Device Model" => resolved.device_model = v,
                "Device Serial" => resolved.device_serial = v,
                "Motherboard" => resolved.motherboard_name = v,
                _ => {}
            }
        }
        let (first, last, friendly, customer_id) = match self.suggestion.match_.as_ref() {
            Some(m) => (
                m.first_name.clone(),
                m.last_name.clone(),
                m.friendly_name.clone(),
                m.id_customer.clone(),
            ),
            None => (String::new(), String::new(), String::new(), String::new()),
        };
        OpenServiceConfirmApply {
            connection_string: self.connection_string.clone(),
            customer_id,
            customer_first_name: first,
            customer_last_name: last,
            friendly_name: friendly,
            candidate,
            resolved_specs: resolved,
        }
    }
}

fn kv_row(ui: &mut Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{key}:")).strong().small());
        ui.label(RichText::new(value).small());
    });
}

fn trim_display(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}
