use eframe::egui::{Align, CentralPanel, Frame, Id, Label, Layout, Margin, RichText, ScrollArea, Ui};
use crate::{file_viewer::{ColorTheme, FileViewer, Syntax}, get_current_user_from_auth};
use crate::ui_tools::controls::{
    self, BarKind, ButtonKind, Metrics, ModalWidth, Role, SpinnerSize, TextArea, TextButton,
    TextField,
};
use crate::ui_tools::icons::svg;
use crate::ui_tools::theme;
use log::info;

use super::ScriptEditor;

impl ScriptEditor {
    pub fn ui(&mut self, ui: &mut Ui) {
        self.filesystem.receive();
        self.poll_ai_result();
        if self.ai_generating {
            ui.ctx().request_repaint();
        }

        let m = Metrics::get(ui.ctx());
        eframe::egui::Panel::top("Script editor top panel")
            .exact_size(m.bar_h)
            .frame(Frame::NONE.fill(ui.style().visuals.extreme_bg_color))
            .show(ui, |ui|
        {
            controls::bar(ui, BarKind::Tool, |ui| {
                ui.add(svg::TERMINAL_WINDOW.image(m.icon_md).tint(theme::primary(ui)));
                ui.label("Script name");
                TextField::new(&mut self.script_name)
                    .icon(svg::FILE_CODE)
                    .hint("script.ps1")
                    .mono(true)
                    .width(300.)
                    .show(ui);

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if TextButton::new("Save Script")
                        .kind(ButtonKind::Primary)
                        .icon(svg::FLOPPY_DISK)
                        .show(ui)
                        .clicked()
                    {
                        if !self.script_name.is_empty() && !self.code.is_empty() {
                            if !self.script_name.ends_with(".ps1") {
                                self.script_name.push_str(".ps1");
                            }
                            self.filesystem.upload_script(self.script_name.clone(), self.code.clone());
                            let _ = self.filesystem.request_contents("/");
                        } else {
                            self.open_notification_modal = true;
                            self.notification_text = "You need to enter a file name first".to_string()
                        }
                    }

                    if controls::icon_text_button(ui, svg::FILE_PLUS, "New Script").clicked() {
                        self.code.clear();
                        self.script_name.clear();
                    }

                    if self.ai_generating {
                        controls::spinner(ui, SpinnerSize::Sm);
                        ui.label("Generating...");
                    } else if controls::icon_text_button(ui, svg::SPARKLE, "AI Generate").clicked() {
                        self.show_ai_popup = true;
                    }

                    let (browser_icon, browser_label) = match self.open_file_browser {
                        false => (svg::SIDEBAR_SIMPLE, "Show File Browser"),
                        true => (svg::SIDEBAR, "Hide File Browser"),
                    };

                    if controls::icon_text_button(ui, browser_icon, browser_label).clicked() {
                        self.open_file_browser = !self.open_file_browser;
                    }
                });
            });
        });

        // AI generation modal
        if self.show_ai_popup {
            let outcome = controls::modal(
                ui.ctx(),
                Id::new("AI Script Generator"),
                svg::SPARKLE,
                "AI Script Generator",
                ModalWidth::Regular,
                &[(Role::Primary, "Generate"), (Role::Cancel, "Cancel")],
                |ui| {
                    ui.label("Describe the script you want to generate:");
                    ui.add_space(m.pad.y);

                    ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                        TextArea::new(&mut self.ai_prompt)
                            .rows(6)
                            .hint("e.g. 'Write a script that cleans temp files older than 30 days and logs the results'")
                            .show(ui);
                    });
                },
            );
            let can_generate = !self.ai_prompt.trim().is_empty();
            if outcome.clicked == Some(0) && can_generate {
                self.generate_script_from_prompt();
                self.show_ai_popup = false;
            }
            if outcome.should_close {
                self.show_ai_popup = false;
            }
        }

        let mut file_browser_open = self.open_file_browser;
        eframe::egui::Panel::right(Id::new("Script editor sidebar"))
        .default_size(160.)
        .frame(
            Frame::default()
            .inner_margin(Margin::symmetric(4, 2))
            .outer_margin(Margin::symmetric(2, 4))
        )
        .show_collapsible(ui, &mut file_browser_open, |ui| {
            ui.vertical_centered_justified(|ui| {
                controls::panel_header(ui, svg::TOOLBOX, "Toolbox", |_| {});
                ui.add_space(m.pad.y);

                self.filesystem.display(ui);

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
                    let outcome = controls::modal(
                        ui.ctx(),
                        Id::new("Upload Script"),
                        svg::WARNING_CIRCLE,
                        "Upload Script",
                        ModalWidth::Narrow,
                        &[(Role::Cancel, "Close")],
                        |ui| {
                            let color = theme::error(ui);
                            ui.add(Label::new(RichText::new(&self.notification_text).color(color)).wrap());
                        },
                    );
                    if outcome.should_close {
                        self.open_notification_modal = false;
                    }
                }
            });
        });

        CentralPanel::default()
            .frame(
                Frame::default()
                .outer_margin(Margin::symmetric(2, 4))
            )
            .show(ui, |ui|
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
