use eframe::egui::{Button, CentralPanel, Id, SidePanel, TextEdit, Ui, Vec2, Widget};
use database::schema::User;
use log::info;

use crate::code_editor::{CodeEditor, ColorTheme, Syntax};

use super::ScriptEditor;

impl ScriptEditor {
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
                
                if let Some(node) = self.filesystem.get_current_folder() {
                    info!("NODE: {node:?}");
                } else {
                    info!("ROOT FOR SCRIPT EDITOR: {:?}", self.filesystem.root);
                    self.filesystem.display(ui);
                }

                if self.first_run {
                    self.first_run = false;
                    if self.filesystem.user != User::default() {
                        info!("We have a user, requesting contents");
                        info!("request: {:?}", self.filesystem.request_contents(""));
                        info!("Contents: {:?}", self.filesystem.root);
                    } else {
                        info!("We need a user");
                    }
                }

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