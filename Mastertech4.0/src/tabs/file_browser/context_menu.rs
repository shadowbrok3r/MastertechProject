use displays::ui_tools::{controls, icons::svg};
use eframe::egui::Ui;
use egui::UiKind;

use super::FileBrowser;

/// An "Open..." row that closes the menu when clicked.
fn open_item(ui: &mut Ui) {
    if controls::menu_item(ui, svg::FOLDER_OPEN, "Open...").clicked() {
        ui.close_kind(UiKind::Menu);
    }
}

impl FileBrowser {
    pub fn filebrowser_ctx_menu(ui: &mut Ui) {
        controls::menu_body(ui, |ui| {
            open_item(ui);
            controls::submenu(ui, svg::FOLDER, "SubMenu", |ui| {
                controls::submenu(ui, svg::FOLDER, "SubMenu", |ui| {
                    open_item(ui);
                    let _ = controls::menu_item(ui, svg::FILE, "Item");
                });
                controls::submenu(ui, svg::FOLDER, "SubMenu", |ui| {
                    open_item(ui);
                    let _ = controls::menu_item(ui, svg::FILE, "Item");
                });
                let _ = controls::menu_item(ui, svg::FILE, "Item");
                open_item(ui);
            });
            controls::submenu(ui, svg::FOLDER, "SubMenu", |ui| {
                let _ = controls::menu_item(ui, svg::FILE, "Item1");
                let _ = controls::menu_item(ui, svg::FILE, "Item2");
                let _ = controls::menu_item(ui, svg::FILE, "Item3");
                let _ = controls::menu_item(ui, svg::FILE, "Item4");
                open_item(ui);
            });
            let _ = controls::menu_item(ui, svg::FILE_TEXT, "Very long text for this item");
        });
    }
}
