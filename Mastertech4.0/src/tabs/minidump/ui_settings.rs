use displays::ui_tools::{
    controls::{self, ButtonKind, IconBox, Metrics, SpinnerSize, TextButton, TextField, Tokens},
    glass_card,
    icons::svg,
};
use eframe::egui;
use egui::{Grid, Ui};

use crate::tabs::minidump::{MiniDumpApp, processor::ProcessingStatus};

/// Width of the HTTP timeout field.
const TIMEOUT_W: f32 = 120.0;
/// Opacity of the drop-target scrim.
const SCRIM: f32 = 0.75;
/// Editable symbol sources: on/off, location and remove; returns the rows to remove.
fn source_rows(ui: &mut Ui, salt: &str, items: &mut [(String, bool)], hint: &str) -> Vec<usize> {
    let m = Metrics::get(ui.ctx());
    let mut to_remove = vec![];
    for (idx, (item, enabled)) in items.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            controls::toggle(ui, enabled, "");
            let width = ui.available_width() - IconBox::Md.side(&m) - ui.spacing().item_spacing.x;
            TextField::new(item)
                .hint(hint)
                .mono(true)
                .width(width)
                .id_salt((salt, idx))
                .show(ui);
            if controls::icon_button(ui, svg::TRASH, IconBox::Md, false, "Remove").clicked() {
                to_remove.push(idx);
            };
        });
    }
    to_remove
}

impl MiniDumpApp {
    pub fn ui_settings(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let m = Metrics::get(ui.ctx());
        let t = Tokens::get(ui.ctx());
        let (status_icon, message) = match self.cur_status {
            ProcessingStatus::NoDump => (Some(svg::TRAY_ARROW_DOWN), "Select or drop a minidump!"),
            ProcessingStatus::ReadingDump => (None, "Reading minidump..."),
            ProcessingStatus::RawProcessing => (None, "Processing minidump..."),
            ProcessingStatus::Symbolicating => (Some(svg::CHECK_CIRCLE), "Minidump processed!"),
            ProcessingStatus::Done => (Some(svg::CHECK_CIRCLE), "Minidump processed!"),
        };

        glass_card::titled_group(
            ui,
            svg::FILE_MAGNIFYING_GLASS,
            "Choose minidump",
            None,
            |ui| {
                // Show a listing of currently known minidumps to inspect
                let mut do_set_path = None;
                ui.horizontal_wrapped(|ui| {
                    for (i, path) in self.settings.available_paths.iter().enumerate() {
                        if controls::icon_text_button(
                            ui,
                            svg::FILE,
                            &path.file_name().unwrap().to_string_lossy(),
                        )
                        .clicked()
                        {
                            do_set_path = Some(i);
                        }
                    }
                });
                if let Some(i) = do_set_path {
                    self.set_path(i);
                }

                if let Some(Err(e)) = &self.minidump {
                    ui.horizontal(|ui| {
                        ui.add(svg::WARNING_CIRCLE.image(m.icon_md).tint(t.error));
                        ui.colored_label(t.error, format!("Failed to read dump: {e}"));
                    });
                }
                if let Some(Err(e)) = &self.kernel_triage {
                    ui.horizontal(|ui| {
                        ui.add(svg::WARNING_CIRCLE.image(m.icon_md).tint(t.error));
                        ui.colored_label(t.error, format!("Failed to triage kernel dump: {e}"));
                    });
                }

                ui.horizontal(|ui| {
                    match status_icon {
                        Some(icon) => {
                            let tint = if icon == svg::CHECK_CIRCLE {
                                t.success
                            } else {
                                t.icon
                            };
                            ui.add(icon.image(m.icon_md).tint(tint));
                        }
                        None => {
                            controls::spinner(ui, SpinnerSize::Sm);
                        }
                    }
                    ui.label(message);
                });
                ui.add_space(m.gap.y);

                ui.horizontal(|ui| {
                    if TextButton::new("Open file...")
                        .icon(svg::FOLDER_OPEN)
                        .kind(ButtonKind::Primary)
                        .show(ui)
                        .clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("minidump", &["dmp"])
                            .pick_file()
                    {
                        self.settings.available_paths.push(path);
                        self.set_path(self.settings.available_paths.len() - 1);
                    }

                    let cancellable = match self.cur_status {
                        ProcessingStatus::NoDump | ProcessingStatus::Done => false,
                        ProcessingStatus::ReadingDump
                        | ProcessingStatus::RawProcessing
                        | ProcessingStatus::Symbolicating => true,
                    };

                    if TextButton::new("Cancel")
                        .icon(svg::X_CIRCLE)
                        .enabled(cancellable)
                        .show(ui)
                        .clicked()
                    {
                        self.cancel_processing();
                    }
                    let reprocessable = matches!(&self.minidump, Some(Ok(_)));
                    if TextButton::new("Reprocess")
                        .icon(svg::ARROWS_CLOCKWISE)
                        .enabled(reprocessable)
                        .show(ui)
                        .clicked()
                    {
                        self.process_dump(
                            self.minidump.as_ref().unwrap().as_ref().unwrap().clone(),
                        );
                    }
                });
            },
        );

        glass_card::titled_group(ui, svg::CLOUD, "Symbol servers", None, |ui| {
            let to_remove = source_rows(
                ui,
                "minidump.symbol_url",
                &mut self.settings.symbol_urls,
                "https://symbols.example/",
            );
            for idx in to_remove.into_iter().rev() {
                self.settings.symbol_urls.remove(idx);
            }
            if controls::icon_text_button(ui, svg::PLUS, "Add server").clicked() {
                self.settings.symbol_urls.push((String::new(), true));
            }
        });

        let to_remove = glass_card::titled_group(ui, svg::FOLDERS, "Local symbols", None, |ui| {
            let to_remove = source_rows(
                ui,
                "minidump.symbol_path",
                &mut self.settings.symbol_paths,
                r"C:\ProgramData\dbg\sym",
            );
            if controls::icon_text_button(ui, svg::PLUS, "Add folder").clicked() {
                self.settings.symbol_paths.push((String::new(), true));
            }
            to_remove
        });

        glass_card::titled_group(ui, svg::SLIDERS_HORIZONTAL, "Misc settings", None, |ui| {
            Grid::new("minidump.misc_settings")
                .num_columns(2)
                .spacing(m.gap)
                .show(ui, |ui| {
                    controls::icon_label(ui, svg::DATABASE, "Symbol cache");
                    ui.horizontal(|ui| {
                        controls::toggle(ui, &mut self.settings.symbol_cache.1, "");
                        let width = ui.available_width();
                        TextField::new(&mut self.settings.symbol_cache.0)
                            .mono(true)
                            .width(width)
                            .id_salt("minidump.symbol_cache")
                            .show(ui);
                    });
                    ui.end_row();

                    controls::icon_label(ui, svg::TIMER, "HTTP timeout secs");
                    TextField::new(&mut self.settings.http_timeout_secs)
                        .mono(true)
                        .width(TIMEOUT_W)
                        .id_salt("minidump.http_timeout")
                        .show(ui);
                    ui.end_row();
                });
            for idx in to_remove.into_iter().rev() {
                self.settings.symbol_paths.remove(idx);
            }
            controls::toggle(
                ui,
                &mut self.settings.raw_dump_brief,
                "Hide memory dumps in raw mode",
            );
        });

        preview_files_being_dropped(ctx);

        // Collect dropped files:
        let mut pushed_path = false;
        for file in &ctx.input_mut(|i| i.raw.take().dropped_files) {
            pushed_path = true;
            self.settings
                .available_paths
                .push(file.path().to_path_buf());
        }
        if pushed_path {
            self.set_path(self.settings.available_paths.len() - 1);
        }
    }
}

/// Preview hovering files:
fn preview_files_being_dropped(ctx: &egui::Context) {
    use egui::*;
    use std::fmt::Write as _;

    if !ctx.input(|i: &InputState| i.raw.hovered_files.is_empty()) {
        let mut text = format!("{} Dropping files:\n", svg::TRAY_ARROW_DOWN.glyph());
        for file in &ctx.input_mut(|i| i.raw.take().hovered_files) {
            if let Some(path) = &file.path {
                write!(text, "\n{}", path.display()).ok();
            } else if !file.mime.is_empty() {
                write!(text, "\n{}", file.mime).ok();
            } else {
                text += "\n???";
            }
        }

        let t = Tokens::get(ctx);
        let painter =
            ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("file_drop_target")));

        let screen_rect = ctx.input(|i| i.content_rect());
        painter.rect_filled(screen_rect, 0.0, t.base.gamma_multiply(SCRIM));
        painter.text(
            screen_rect.center(),
            Align2::CENTER_CENTER,
            text,
            TextStyle::Heading.resolve(&ctx.global_style()),
            t.text,
        );
    }
}
