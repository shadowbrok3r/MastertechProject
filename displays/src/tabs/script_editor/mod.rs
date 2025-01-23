use eframe::egui::{Button, CentralPanel, Id, SidePanel, TextEdit, Ui, Vec2, Widget};
use serde::Serialize;

use crate::{code_editor::{CodeEditor, ColorTheme, Syntax}, virtual_filesystem::FileSystem};



#[derive(Serialize)]
pub struct ScriptEditor {
    code: String,
    script_name: String,
    open_save_modal: bool,
    #[serde(skip)]
    filesystem: FileSystem
}


impl ScriptEditor {
    pub fn new() -> Self {
        Self { 
            code: Default::default(),
            script_name: Default::default(),
            open_save_modal: false,
            filesystem: FileSystem::new()
         }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        SidePanel::right(Id::new("Script editor sidebar"))
        .default_width(125.)
        .show_inside(ui, |ui| {
            ui.vertical_centered_justified(|ui| {
                let button_size = Vec2::new(50.0, 15.0);
                if Button::new("Save Script")
                    .min_size(button_size)
                    .ui(ui)
                    .clicked() 
                {
                    self.open_save_modal = true;
                }
                ui.add_space(5.);
                if Button::new("New +")
                    .min_size(button_size)
                    .ui(ui)
                    .clicked() {
                    
                }

                if self.open_save_modal {
                    eframe::egui::Modal::new(Id::new("Upload Script"))
                    .show(ui.ctx(), |ui| {
                        ui.vertical_centered(|ui| {
                            ui.label("Script Name");
                            
                            let res = TextEdit::singleline(&mut self.script_name).ui(ui);
                            if res.lost_focus() && self.script_name.len() > 0 {
                                self.filesystem.upload_script(
                                    self.script_name.clone(), 
                                    self.code.clone()
                                );
                                self.open_save_modal = false;
                            }
                        });
                    });
                }
            });
        });

        CentralPanel::default()
            .show_inside(ui, |ui| 
        {
            CodeEditor::default()
                .id_source("Script Editor")
                .with_rows(48)
                .vscroll(true)
                .auto_shrink(false)
                .with_fontsize(14.0)
                .with_theme(ColorTheme::TOKYO_DARK)
                .with_syntax(Syntax::powershell())
                .with_numlines(true)
                .show(ui, &mut self.code);
        });
    }
}