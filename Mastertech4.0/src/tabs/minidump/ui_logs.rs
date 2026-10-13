use crate::tabs::minidump::{MiniDumpApp, frame_signature_from_indices, threadname};
use displays::ui_tools::{
    controls::{self, BarKind, TextArea},
    icons::svg,
};
use eframe::egui;
use egui::Ui;

/// Width of the thread and frame pickers.
const PICKER_W: f32 = 400.0;

pub struct LogUiState {
    pub cur_thread: Option<usize>,
    pub cur_frame: Option<usize>,
}

/// Borrowed dropdown options from owned labels.
fn options(labels: &[(Option<usize>, String)]) -> Vec<(Option<usize>, &str)> {
    labels.iter().map(|(v, s)| (*v, s.as_str())).collect()
}

impl MiniDumpApp {
    pub fn ui_logs(&mut self, ui: &mut Ui, _ctx: &egui::Context) {
        let ui_state = &mut self.log_ui_state;
        if let Some(Ok(state)) = &self.processed {
            controls::bar(ui, BarKind::Tool, |ui| {
                let threads: Vec<(Option<usize>, String)> =
                    std::iter::once((None, "<no thread>".to_owned()))
                        .chain(
                            state
                                .threads
                                .iter()
                                .enumerate()
                                .map(|(idx, stack)| (Some(idx), threadname(stack))),
                        )
                        .collect();
                controls::icon_label(ui, svg::GIT_BRANCH, "Thread");
                if controls::dropdown(
                    ui,
                    "minidump.logs.thread",
                    &mut ui_state.cur_thread,
                    &options(&threads),
                    PICKER_W,
                )
                .changed()
                {
                    ui_state.cur_frame = None;
                }
                let thread = ui_state.cur_thread.and_then(|t| state.threads.get(t));
                if let Some(thread) = thread {
                    let frames: Vec<(Option<usize>, String)> = std::iter::once(None)
                        .chain((0..thread.frames.len()).map(Some))
                        .map(|idx| {
                            let name =
                                frame_signature_from_indices(state, ui_state.cur_thread, idx);
                            (idx, name)
                        })
                        .collect();
                    controls::icon_label(ui, svg::STACK, "Frame");
                    controls::dropdown(
                        ui,
                        "minidump.logs.frame",
                        &mut ui_state.cur_frame,
                        &options(&frames),
                        PICKER_W,
                    );
                }
            });
        }

        // Print the logs
        egui::ScrollArea::vertical().show(ui, |ui| {
            let text = match (ui_state.cur_thread, ui_state.cur_frame) {
                (Some(t), Some(f)) => self.logger.string_for_frame(t, f),
                (Some(t), None) => self.logger.string_for_thread(t),
                _ => self.logger.string_for_all(),
            };
            TextArea::new(&mut &**text).mono(true).show(ui);
        });
    }
}
