use eframe::egui::Ui;
use egui::UiKind;

use super::FileBrowser;

impl FileBrowser{
    pub fn filebrowser_ctx_menu(ui: &mut Ui) {
        if ui.button("Open...").clicked() {
            ui.close_kind(UiKind::Menu);
        }
        ui.menu_button("SubMenu", |ui| {
            ui.menu_button("SubMenu", |ui| {
                if ui.button("Open...").clicked() {
                    ui.close_kind(UiKind::Menu);
                }
                let _ = ui.button("Item");
            });
            ui.menu_button("SubMenu", |ui| {
                if ui.button("Open...").clicked() {
                    ui.close_kind(UiKind::Menu);
                }
                let _ = ui.button("Item");
            });
            let _ = ui.button("Item");
            if ui.button("Open...").clicked() {
                ui.close_kind(UiKind::Menu);
            }
        });
        ui.menu_button("SubMenu", |ui| {
            let _ = ui.button("Item1");
            let _ = ui.button("Item2");
            let _ = ui.button("Item3");
            let _ = ui.button("Item4");
            if ui.button("Open...").clicked() {
                ui.close_kind(UiKind::Menu);
            }
        });
        let _ = ui.button("Very long text for this item");
    }
}