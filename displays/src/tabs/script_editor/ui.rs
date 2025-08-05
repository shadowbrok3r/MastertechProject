use eframe::egui::{Button, CentralPanel, Color32, FontId, Frame, Id, Layout, Margin, RichText, SidePanel, Stroke, TextEdit, TopBottomPanel, Ui, Vec2, Widget};
use crate::{file_viewer::{ColorTheme, FileViewer, Syntax}, get_current_user_from_auth};
use log::info;


use super::ScriptEditor;

impl ScriptEditor {
    pub fn ui(&mut self, ui: &mut Ui) {
        self.filesystem.receive();
        
        TopBottomPanel::top("Script editor top panel")
            .exact_height(40.)
            .frame(
                Frame::default()
                .fill(ui.style().visuals.extreme_bg_color)
                .stroke(Stroke::new(1., Color32::from_additive_luminance(150)))
                .outer_margin(Margin::symmetric(0, 2))
            )
            .show_inside(ui, |ui| 
        {
            ui.add_space(2.);
            ui.horizontal_top(|ui| {
                ui.with_layout(Layout::left_to_right(eframe::egui::Align::Center), |ui| {
                    ui.add_space(10.);
                    ui.label(RichText::new("Script name: ").heading().font(FontId::monospace(12.)));
                    TextEdit::singleline(&mut self.script_name)
                        .desired_width(300.)
                        .show(ui);
                });

                ui.with_layout(Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                    ui.add_space(10.);
                    let button_size = Vec2::new(50.0, 10.0);

                    if Button::new("💾 Save Script")
                        .min_size(button_size)
                        .ui(ui)
                        .clicked() 
                    {
                        if self.script_name.len() > 0 && self.code.len() != 0 {
                            if !self.script_name.ends_with(".ps1") {
                                self.script_name.push_str(".ps1");
                            }
                            self.filesystem.upload_script(self.script_name.clone(), self.code.clone());
                            let _ = self.filesystem.request_contents("/");
                        }
                        else {
                            self.open_notification_modal = true;
                            self.notification_text = "You need to enter a file name first".to_string()
                        }
                    }

                    ui.add_space(5.);

                    if Button::new("➕ New Script")
                        .min_size(button_size)
                        .ui(ui)
                        .clicked() 
                    {
                        if self.script_name.len() == 0 && self.code.len() != 0 {
                            self.filesystem.upload_script(self.script_name.clone(), self.code.clone());
                            self.code.clear();
                            self.script_name.clear();
                        }
                        else {
                            self.open_notification_modal = true;
                            self.notification_text = "You need to a file name first".to_string()
                        }
                    }

                    ui.add_space(5.);

                    let txt = match self.open_file_browser {
                        false => "<- Show File Browser",
                        true => "Hide File Browser ->",
                    };
    
                    if ui.button(txt).clicked() {
                        self.open_file_browser = !self.open_file_browser;
                    }
                });
            });
        });

        SidePanel::right(Id::new("Script editor sidebar"))
        .default_width(160.)
        .frame(
            Frame::default()
            .inner_margin(Margin::symmetric(4, 2))
            .outer_margin(Margin::symmetric(2, 4))
        )
        .show_animated_inside(ui, self.open_file_browser, |ui| {
            ui.vertical_centered_justified(|ui| {
                if self.filesystem.get_current_folder().is_some() {
                    ui.add_space(10.);
                    ui.label(RichText::new("Toolbox").heading());
                    ui.separator();
                    ui.add_space(5.);

                    self.filesystem.display(ui);
                }

                if self.first_run {
                    self.first_run = false;
                    if !self.filesystem.user.get_name().is_empty() {
                        info!("We have a user, requesting contents");
                        info!("request: {:?}", self.filesystem.request_contents("/"));
                        info!("Contents: {:?}", self.filesystem.root);
                    } else {
                        info!("We need a user");
                        match get_current_user_from_auth() {
                            Some(usr) => {
                                let _ = self.filesystem.set_user(usr);
                                let _ = self.filesystem.request_contents("/");
                            },
                            None => log::info!("Could not retrieve user."),
                        };
                    }
                }

                if self.open_notification_modal {
                    eframe::egui::Modal::new(Id::new("Upload Script"))
                    .show(ui.ctx(), |ui| {
                        ui.with_layout(Layout::left_to_right(eframe::egui::Align::Min), |ui| {
                            ui.colored_label(Color32::LIGHT_RED, &self.notification_text);
                            ui.add_space(10.);
                            if ui.button("❌").clicked() {
                                self.open_notification_modal = false;
                            }
                        });
                    });
                }
            });
        });

        CentralPanel::default()
            .frame(
                Frame::default()
                .outer_margin(Margin::symmetric(2, 4))
            )
            .show_inside(ui, |ui| 
        {
            FileViewer::default()
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
