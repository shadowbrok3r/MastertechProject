//! Viewer for the client's own MasterTech log ring.
//!
//! Distinct from `event_log_viewer`, which reads Windows' event logs. The
//! client keeps its records in memory whether or not it was launched with
//! `--log-to-file`, so this works against any connected client.

use crossbeam::channel::Sender;
use eframe::egui::{self, Color32, ComboBox, RichText, ScrollArea, TextEdit, Ui};

use crate::ui_tools::icons;
use crate::Cmd;

/// How much of the client's ring to pull.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogFetchMode {
    /// The last `tail_lines` lines.
    Tail,
    /// Everything the client still holds.
    Full,
}

pub struct ClientLogViewer {
    pub mode: LogFetchMode,
    pub tail_lines: u32,
    pub filter: String,
    pub loading: bool,
    pub text: String,
    /// Lines in `text`.
    pub lines: u32,
    /// Depth of the client's ring at fetch time.
    pub total_lines: u32,
    pub fetched_at: Option<chrono::DateTime<chrono::Local>>,
    pub status: Option<String>,
}

impl Default for ClientLogViewer {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientLogViewer {
    pub fn new() -> Self {
        Self {
            mode: LogFetchMode::Tail,
            tail_lines: 500,
            filter: String::new(),
            loading: false,
            text: String::new(),
            lines: 0,
            total_lines: 0,
            fetched_at: None,
            status: None,
        }
    }

    /// The `Cmd` matching the current mode.
    pub fn fetch_cmd(&self) -> Cmd {
        Cmd::ReadClientLog {
            max_lines: match self.mode {
                LogFetchMode::Tail => Some(self.tail_lines.max(1)),
                LogFetchMode::Full => None,
            },
        }
    }

    pub fn set_response(&mut self, text: String, lines: u32, total_lines: u32) {
        self.text = text;
        self.lines = lines;
        self.total_lines = total_lines;
        self.fetched_at = Some(chrono::Local::now());
        self.loading = false;
        self.status = None;
    }

    /// Lines matching `filter`, or the whole body when it is empty.
    fn visible_text(&self) -> std::borrow::Cow<'_, str> {
        let needle = self.filter.trim().to_lowercase();
        if needle.is_empty() {
            return std::borrow::Cow::Borrowed(&self.text);
        }
        let kept: Vec<&str> = self
            .text
            .lines()
            .filter(|l| l.to_lowercase().contains(&needle))
            .collect();
        std::borrow::Cow::Owned(kept.join("\n"))
    }

    pub fn display(&mut self, ui: &mut Ui, cmd_tx: &Sender<Cmd>) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Fetch:").strong());
            ComboBox::from_id_salt("client_log_mode")
                .selected_text(match self.mode {
                    LogFetchMode::Tail => "Last N lines",
                    LogFetchMode::Full => "Full log",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.mode, LogFetchMode::Tail, "Last N lines");
                    ui.selectable_value(&mut self.mode, LogFetchMode::Full, "Full log");
                });

            if self.mode == LogFetchMode::Tail {
                ui.add(
                    egui::DragValue::new(&mut self.tail_lines)
                        .range(10..=10_000)
                        .speed(10.0)
                        .suffix(" lines"),
                );
            }

            if ui
                .add_enabled(!self.loading, egui::Button::new(format!("{} Fetch", icons::REFRESH)))
                .clicked()
            {
                self.loading = true;
                self.status = None;
                if cmd_tx.try_send(self.fetch_cmd()).is_err() {
                    self.loading = false;
                    self.status = Some("Could not queue the request — client not connected.".into());
                }
            }

            ui.separator();

            let have_text = !self.text.is_empty();
            if ui
                .add_enabled(have_text, egui::Button::new(format!("{} Copy", icons::COPY)))
                .on_hover_text("Copy what is shown (filter applied) to the clipboard")
                .clicked()
            {
                let text = self.visible_text().into_owned();
                let copied = text.lines().count();
                ui.ctx().copy_text(text);
                self.status = Some(format!("Copied {copied} lines."));
            }

            #[cfg(not(target_arch = "wasm32"))]
            if ui
                .add_enabled(have_text, egui::Button::new(format!("{} Save", icons::SAVE)))
                .clicked()
            {
                self.save_to_file();
            }

            ui.separator();
            ui.label("Filter:");
            TextEdit::singleline(&mut self.filter)
                .hint_text("Substring, case-insensitive")
                .desired_width(180.0)
                .show(ui);
        });

        ui.add_space(4.0);

        if let Some(status) = &self.status {
            ui.label(RichText::new(status).color(Color32::from_rgb(160, 190, 160)).small());
        }

        if self.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Fetching client log...");
            });
            return;
        }

        if self.text.is_empty() {
            ui.label(
                RichText::new("No log fetched yet. Click Fetch to pull the client's log ring.")
                    .italics()
                    .color(Color32::GRAY),
            );
            return;
        }

        let body = self.visible_text().into_owned();
        let shown = body.lines().count();
        ui.horizontal(|ui| {
            let mut summary = format!("{} of {} lines held", self.lines, self.total_lines);
            if !self.filter.trim().is_empty() {
                summary.push_str(&format!("  |  {shown} match the filter"));
            }
            if let Some(at) = self.fetched_at {
                summary.push_str(&format!("  |  fetched {}", at.format("%I:%M:%S %p")));
            }
            ui.label(RichText::new(summary).small().color(Color32::GRAY));
        });

        ui.add_space(2.0);

        ScrollArea::both()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.add(
                    TextEdit::multiline(&mut body.as_str())
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .code_editor(),
                );
            });
    }

    /// Writes what is shown (filter applied) to a file the operator picks.
    #[cfg(not(target_arch = "wasm32"))]
    fn save_to_file(&mut self) {
        let default_name = format!(
            "mastertech-client-log-{}.txt",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        );
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("Text", &["txt", "log"])
            .save_file()
        else {
            return;
        };
        let body = self.visible_text().into_owned();
        self.status = match std::fs::write(&path, body) {
            Ok(()) => Some(format!("Saved to {}", path.display())),
            Err(e) => Some(format!("Save failed: {e}")),
        };
    }
}
