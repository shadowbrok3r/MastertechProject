use std::path::PathBuf;

use super::{FileDialogModal, ModalAction, ModalState};
use crate::tabs::egui_file_dialog::config::{FileDialogConfig, FileDialogKeyBindings};
use displays::ui_tools::{
    controls::{self, Metrics, Role, Tokens},
    icons::svg,
};

/// Size of the warning icon.
const WARN_ICON: f32 = 40.0;
/// Opacity of the warning icon's duotone back layer.
const DUO_ALPHA: f32 = 0.3;

/// The modal that is used to ask the user if the selected path should be
/// overwritten.
pub struct OverwriteFileModal {
    /// The current state of the modal.
    state: ModalState,
    /// The path selected for overwriting.
    path: PathBuf,
}

impl OverwriteFileModal {
    /// Creates a new modal object.
    ///
    /// # Arguments
    ///
    /// * `path` - The path selected for overwriting.
    pub const fn new(path: PathBuf) -> Self {
        Self {
            state: ModalState::Pending,
            path,
        }
    }
}

impl OverwriteFileModal {
    /// Submits the modal and triggers the action to save the file.
    fn submit(&mut self) {
        self.state = ModalState::Close(ModalAction::SaveFile(self.path.clone()));
    }

    /// Closes the modal without overwriting the file.
    fn cancel(&mut self) {
        self.state = ModalState::Close(ModalAction::None);
    }
}

impl FileDialogModal for OverwriteFileModal {
    fn update(&mut self, config: &FileDialogConfig, ui: &mut egui::Ui) -> ModalState {
        let section_spacing = Metrics::get(ui.ctx()).gap.y * 2.0;

        ui.vertical_centered(|ui| {
            let warn = Tokens::get(ui.ctx()).warn;

            ui.add_space(section_spacing);

            let (slot, _) =
                ui.allocate_exact_size(egui::Vec2::splat(WARN_ICON), egui::Sense::hover());
            svg::WARNING.paint_duo(ui, slot, WARN_ICON, warn, warn.gamma_multiply(DUO_ALPHA));

            ui.add_space(section_spacing);

            // Used to wrap the path on a single line.
            let mut job = egui::text::LayoutJob::single_section(
                format!("'{}'", self.path.to_str().unwrap_or_default()),
                egui::TextFormat::default(),
            );

            job.wrap = egui::text::TextWrapping {
                max_rows: 1,
                ..Default::default()
            };

            ui.label(job);
            ui.label(&config.labels.overwrite_file_modal_text);

            ui.add_space(section_spacing);

            ui.horizontal(|ui| {
                let buttons = [
                    (Role::Danger, config.labels.overwrite.as_str()),
                    (Role::Cancel, config.labels.cancel.as_str()),
                ];
                match controls::dialog_buttons(ui, &buttons) {
                    Some(0) => self.submit(),
                    Some(_) => self.cancel(),
                    None => {}
                }
            });
        });

        self.state.clone()
    }

    fn update_keybindings(&mut self, config: &FileDialogConfig, ctx: &egui::Context) {
        if FileDialogKeyBindings::any_pressed(ctx, &config.keybindings.submit, true) {
            self.submit();
        }

        if FileDialogKeyBindings::any_pressed(ctx, &config.keybindings.cancel, true) {
            self.cancel();
        }
    }
}
