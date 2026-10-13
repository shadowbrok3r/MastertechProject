#![allow(clippy::too_many_arguments)]
use super::processor::ProcessingStatus;
use crate::tabs::minidump::{MiniDumpApp, Tab, frame_signature, frame_source, listing, threadname};
use displays::ui_tools::{
    controls::{self, Metrics, SpinnerSize},
    glass_card,
    icons::svg,
};
use eframe::egui;
use egui::{Color32, Context, FontId, Frame, ScrollArea, Ui};
use egui_extras::{Column, TableBody, TableBuilder};
use minidump_common::utils::basename;
use minidump_processor::ProcessState;
use minidump_unwind::{CallStack, StackFrame};

/// Width of the thread and frame pickers.
const PICKER_W: f32 = 400.0;

pub struct ProcessedUiState {
    pub cur_thread: usize,
    pub cur_frame: usize,
}

use inline_shim::*;

#[cfg(feature = "inline")]
mod inline_shim {
    pub use minidump_unwind::InlineFrame;
    use minidump_unwind::StackFrame;
    pub fn get_inline_frames(frame: &StackFrame) -> &[InlineFrame] {
        &frame.inlines
    }
}

#[cfg(not(feature = "inline"))]
mod inline_shim {
    use minidump_unwind::StackFrame;

    /// A stack frame in an inlined function.
    #[derive(Debug, Clone)]
    pub struct InlineFrame {
        /// The name of the function
        pub function_name: String,
        /// The file name of the stack frame
        pub source_file_name: Option<String>,
        /// The line number of the stack frame
        pub source_line: Option<u32>,
    }

    pub fn get_inline_frames(_frame: &StackFrame) -> &[InlineFrame] {
        &[]
    }
}

impl MiniDumpApp {
    pub fn ui_processed(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        if let Some(Err(e)) = &self.minidump {
            controls::empty_state(
                ui,
                svg::FILE_X,
                "Minidump couldn't be read!",
                &e.to_string(),
                &[],
            );
            return;
        }
        if let Some(state) = &self.processed {
            match state {
                Ok(state) => {
                    self.ui_processed_good(ui, ctx, &state.clone());
                }
                Err(e) => {
                    controls::empty_state(
                        ui,
                        svg::WARNING_OCTAGON,
                        "Minidump couldn't be processed!",
                        &e.to_string(),
                        &[],
                    );
                }
            }
        }
    }

    fn ui_processed_good(&mut self, ui: &mut Ui, ctx: &Context, state: &ProcessState) {
        // let is_symbolicated = self.cur_status == ProcessingStatus::Done;
        eframe::egui::Panel::top("info")
            .resizable(true)
            .default_size((ui.available_height() / 2.0).round())
            .frame(Frame::new())
            .show(ui, |ui| {
                self.ui_processed_data(ui, ctx, state);
            });
        eframe::egui::Panel::bottom("progress")
            .frame(Frame::new())
            .show(ui, |ui| {
                ui.add_space(Metrics::get(ui.ctx()).gap.y / 2.0);
                ui.horizontal(|ui| {
                    let stats = self.analysis_state.stats.lock().unwrap();
                    let symbols = stats.pending_symbols.lock().unwrap().clone();
                    let (t_done, t_todo) = stats.processor_stats.get_thread_count();
                    let frames_walked = stats.processor_stats.get_frame_count();

                    let estimated_frames_per_thread = 10.0;
                    let estimated_progress = if t_todo == 0 {
                        0.0
                    } else {
                        let ratio =
                            frames_walked as f32 / (t_todo as f32 * estimated_frames_per_thread);
                        ratio.min(0.9)
                    };
                    let in_progress = self.cur_status < ProcessingStatus::Done;
                    let progress = if in_progress { estimated_progress } else { 1.0 };

                    controls::icon_label(
                        ui,
                        svg::CLOUD_ARROW_DOWN,
                        &format!(
                            "fetching symbols {}/{}",
                            symbols.symbols_processed, symbols.symbols_requested
                        ),
                    );
                    controls::icon_label(
                        ui,
                        svg::GIT_BRANCH,
                        &format!("processing threads {t_done}/{t_todo}"),
                    );
                    controls::icon_label(ui, svg::STACK, &format!("frames walked {frames_walked}"));

                    if in_progress {
                        controls::spinner(ui, SpinnerSize::Sm);
                    }
                    controls::ProgressBar::new(progress)
                        .percentage(true)
                        .show(ui);
                });
            });
        egui::CentralPanel::default()
            .frame(Frame::new())
            .show(ui, |ui| {
                controls::hairline(ui);

                if let Some(stack) = state.threads.get(self.processed_ui_state.cur_thread) {
                    self.ui_processed_backtrace(ui, ctx, stack);
                }
            });
    }

    fn ui_processed_data(&mut self, ui: &mut Ui, ctx: &Context, state: &ProcessState) {
        let cur_threadname = state
            .threads
            .get(self.processed_ui_state.cur_thread)
            .map(threadname)
            .unwrap_or_default();
        eframe::egui::Panel::left("overall info")
            .default_size((ui.available_width() / 2.0).round())
            .frame(Frame::new())
            .show(ui, |ui| {
                ScrollArea::vertical().show(ui, |ui| {
                    glass_card::card_header(ui, svg::APP_WINDOW, "Process", None);

                    listing(
                        ui,
                        ctx,
                        1,
                        [
                            ("OS".to_owned(), state.system_info.os.to_string()),
                            (
                                "OS version".to_owned(),
                                state
                                    .system_info
                                    .format_os_version()
                                    .map(|s| s.into_owned())
                                    .unwrap_or_default(),
                            ),
                            ("CPU".to_owned(), state.system_info.cpu.to_string()),
                            (
                                "CPU info".to_owned(),
                                state.system_info.cpu_info.clone().unwrap_or_default(),
                            ),
                            // ("Process Create Time".to_owned(), state.process_create_time.map(|s| format!("{:?}", s)).unwrap_or_default()),
                            // ("Process Crash Time".to_owned(), format!("{:?}", state.time)),
                            (
                                "Crash Reason".to_owned(),
                                state
                                    .exception_info
                                    .as_ref()
                                    .map(|e| e.reason.to_string())
                                    .unwrap_or_default(),
                            ),
                            (
                                "Crash Assertion".to_owned(),
                                state.assertion.clone().unwrap_or_default(),
                            ),
                            (
                                "Crash Address".to_owned(),
                                state
                                    .exception_info
                                    .as_ref()
                                    .map(|e| self.format_addr(e.address.0))
                                    .unwrap_or_default(),
                            ),
                            ("Crashing Thread".to_owned(), cur_threadname.clone()),
                        ],
                    );
                });
            });
        egui::CentralPanel::default()
            .frame(Frame::new())
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    controls::icon_label(ui, svg::GIT_BRANCH, "Thread");
                    let names: Vec<(usize, String)> = state
                        .threads
                        .iter()
                        .enumerate()
                        .map(|(idx, stack)| (idx, threadname(stack)))
                        .collect();
                    let options: Vec<(usize, &str)> = names
                        .iter()
                        .map(|(idx, name)| (*idx, name.as_str()))
                        .collect();
                    if controls::dropdown(
                        ui,
                        "minidump.processed.thread",
                        &mut self.processed_ui_state.cur_thread,
                        &options,
                        PICKER_W,
                    )
                    .changed()
                    {
                        self.processed_ui_state.cur_frame = 0;
                    }
                });
                controls::hairline(ui);
                ScrollArea::vertical().show(ui, |ui| {
                    if let Some(thread) = state.threads.get(self.processed_ui_state.cur_thread) {
                        listing(
                            ui,
                            ctx,
                            2,
                            [(
                                "last_error_value".to_owned(),
                                thread
                                    .last_error_value
                                    .map(|e| e.to_string())
                                    .unwrap_or_default(),
                            )],
                        );
                        if let Some(frame) = thread.frames.get(self.processed_ui_state.cur_frame) {
                            ui.add_space(Metrics::get(ui.ctx()).gap.y * 4.0);
                            ui.horizontal(|ui| {
                                use std::fmt::Write;
                                controls::icon_label(ui, svg::STACK, "Frame");
                                let names: Vec<(usize, String)> = thread
                                    .frames
                                    .iter()
                                    .enumerate()
                                    .map(|(idx, frame)| {
                                        let mut label = String::new();
                                        write!(&mut label, "{idx:02} - ").unwrap();
                                        frame_signature(&mut label, frame).unwrap();
                                        (idx, label)
                                    })
                                    .collect();
                                let options: Vec<(usize, &str)> = names
                                    .iter()
                                    .map(|(idx, name)| (*idx, name.as_str()))
                                    .collect();
                                controls::dropdown(
                                    ui,
                                    "minidump.processed.frame",
                                    &mut self.processed_ui_state.cur_frame,
                                    &options,
                                    PICKER_W,
                                );
                            });

                            let regs = frame
                                .context
                                .valid_registers()
                                .map(|(name, val)| (name.to_owned(), self.format_addr(val)));
                            listing(ui, ctx, 3, regs);
                        }
                    }
                })
            });
    }

    fn ui_processed_backtrace(&mut self, ui: &mut Ui, ctx: &Context, stack: &CallStack) {
        let font = egui::style::TextStyle::Body.resolve(ui.style());
        TableBuilder::new(ui)
            .striped(true)
            .cell_layout(
                egui::Layout::left_to_right(egui::Align::Min).with_cross_align(egui::Align::Center),
            )
            .column(Column::initial(60.0).at_least(40.0))
            .column(Column::initial(80.0).at_least(40.0))
            .column(Column::initial(160.0).at_least(40.0))
            .column(Column::initial(160.0).at_least(40.0))
            .column(Column::remainder().at_least(60.0))
            .resizable(true)
            .header(Metrics::get(ctx).row_h, |mut header| {
                for title in ["Frame", "Trust", "Module", "Source", "Signature"] {
                    header.col(|ui| {
                        controls::section_label(ui, title);
                    });
                }
            })
            .body(|mut body| {
                let mut frame_count = 0;
                let mut widths = [0.0f32; 5];
                widths.clone_from_slice(body.widths());
                for (frame_idx, frame) in stack.frames.iter().enumerate() {
                    for inline in get_inline_frames(frame).iter().rev() {
                        let frame_num = frame_count;
                        frame_count += 1;
                        self.ui_inline_frame(
                            &mut body, ctx, &widths, &font, frame_num, frame, inline,
                        );
                    }

                    let frame_num = frame_count;
                    frame_count += 1;
                    self.ui_real_frame(&mut body, ctx, &widths, &font, frame_idx, frame_num, frame);
                }
            });
    }

    fn ui_real_frame(
        &mut self,
        body: &mut TableBody,
        ctx: &Context,
        widths: &[f32],
        font: &FontId,
        frame_idx: usize,
        frame_num: usize,
        frame: &StackFrame,
    ) {
        let col1_width = widths[0];
        let col2_width = widths[1];
        let col3_width = widths[2];
        let col4_width = widths[3];
        let col5_width = widths[4];
        let pad = Metrics::get(ctx).gap.y;

        let (col1, col2, col3, col4, col5, row_height) = ctx.fonts_mut(|fonts| {
            let col1 = {
                fonts.layout(
                    frame_num.to_string(),
                    font.clone(),
                    Color32::PLACEHOLDER,
                    col1_width,
                )
            };
            let col2 = {
                let trust = match frame.trust {
                    minidump_unwind::FrameTrust::None => "none",
                    minidump_unwind::FrameTrust::Scan => "scan",
                    minidump_unwind::FrameTrust::CfiScan => "cfi scan",
                    minidump_unwind::FrameTrust::FramePointer => "frame pointer",
                    minidump_unwind::FrameTrust::CallFrameInfo => "cfi",
                    minidump_unwind::FrameTrust::PreWalked => "prewalked",
                    minidump_unwind::FrameTrust::Context => "context",
                };
                fonts.layout(
                    trust.to_owned(),
                    font.clone(),
                    Color32::PLACEHOLDER,
                    col2_width,
                )
            };
            let col3 = {
                let label = if let Some(module) = &frame.module {
                    basename(&module.name).to_string()
                } else {
                    String::new()
                };
                fonts.layout(label, font.clone(), Color32::PLACEHOLDER, col3_width)
            };
            let col4 = {
                let mut label = String::new();
                frame_source(&mut label, frame).unwrap();
                fonts.layout(label, font.clone(), Color32::PLACEHOLDER, col4_width)
            };
            let col5 = {
                let mut label = String::new();
                frame_signature(&mut label, frame).unwrap();
                fonts.layout(label, font.clone(), Color32::PLACEHOLDER, col5_width)
            };

            let row_height = col1
                .rect
                .height()
                .max(col2.rect.height())
                .max(col3.rect.height())
                .max(col4.rect.height())
                .max(col5.rect.height())
                + pad;
            (col1, col2, col3, col4, col5, row_height)
        });

        body.row(row_height, |mut row| {
            row.col(|ui| {
                ui.centered_and_justified(|ui| {
                    if ui.link(col1).clicked() {
                        self.processed_ui_state.cur_frame = frame_idx;
                    }
                });
            });
            row.col(|ui| {
                ui.centered_and_justified(|ui| {
                    if ui.link(col2).clicked() {
                        self.tab = Tab::Logs;
                        self.log_ui_state.cur_thread = Some(self.processed_ui_state.cur_thread);
                        self.log_ui_state.cur_frame = Some(frame_idx);
                    }
                });
            });
            row.col(|ui| {
                ui.centered_and_justified(|ui| {
                    ui.label(col3);
                });
            });
            row.col(|ui| {
                ui.label(col4);
            });
            row.col(|ui| {
                ui.label(col5);
            });
        });
    }

    fn ui_inline_frame(
        &mut self,
        body: &mut TableBody,
        ctx: &Context,
        widths: &[f32],
        font: &FontId,
        frame_num: usize,
        real_frame: &StackFrame,
        frame: &InlineFrame,
    ) {
        let col1_width = widths[0];
        let col2_width = widths[1];
        let col3_width = widths[2];
        let col4_width = widths[3];
        let col5_width = widths[4];
        let pad = Metrics::get(ctx).gap.y;
        let (col1, col2, col3, col4, col5, row_height) = ctx.fonts_mut(|fonts| {
            let col1 = {
                fonts.layout(
                    frame_num.to_string(),
                    font.clone(),
                    Color32::PLACEHOLDER,
                    col1_width,
                )
            };
            let col2 = {
                let trust = "inlined";
                fonts.layout(
                    trust.to_owned(),
                    font.clone(),
                    Color32::PLACEHOLDER,
                    col2_width,
                )
            };
            let col3 = {
                let label = if let Some(module) = &real_frame.module {
                    basename(&module.name).to_string()
                } else {
                    String::new()
                };
                fonts.layout(label, font.clone(), Color32::PLACEHOLDER, col3_width)
            };
            let col4 = {
                let label = if let (Some(source_file), Some(line)) =
                    (frame.source_file_name.as_ref(), frame.source_line.as_ref())
                {
                    format!("{}: {}", basename(source_file).to_owned(), line)
                } else {
                    String::new()
                };
                fonts.layout(label, font.clone(), Color32::PLACEHOLDER, col4_width)
            };
            let col5 = {
                let label = frame.function_name.clone();
                fonts.layout(label, font.clone(), Color32::PLACEHOLDER, col5_width)
            };

            let row_height = col1
                .rect
                .height()
                .max(col2.rect.height())
                .max(col3.rect.height())
                .max(col4.rect.height())
                .max(col5.rect.height())
                + pad;
            (col1, col2, col3, col4, col5, row_height)
        });
        body.row(row_height, |mut row| {
            row.col(|ui| {
                ui.centered_and_justified(|ui| {
                    ui.label(col1);
                });
            });
            row.col(|ui| {
                ui.centered_and_justified(|ui| {
                    ui.label(col2);
                });
            });
            row.col(|ui| {
                ui.centered_and_justified(|ui| {
                    ui.label(col3);
                });
            });
            row.col(|ui| {
                ui.label(col4);
            });
            row.col(|ui| {
                ui.label(col5);
            });
        });
    }
}
