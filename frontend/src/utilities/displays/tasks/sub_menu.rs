use egui::Ui;

use super::task_layout::TaskLayout;

pub fn sub_menu(ui: &mut Ui) {
    if ui.button("Open...").clicked() {
        ui.close_menu();
    }
    
        ui.menu_button("SubMenu", |ui| {
            if ui.button("Open...").clicked() {
                ui.close_menu();
            }
            let _ = ui.button("Item");
        });

        ui.menu_button("SubMenu", |ui| {
            if ui.button("Open...").hovered() {
                // ui.close_menu();
            }
            let _ = ui.button("Item");
        });
        let _ = ui.button("Item");
        if ui.button("Open...").clicked() {
            ui.close_menu();
        }
        
    ui.menu_button("SubMenu", |ui| {
        let _ = ui.button("Item1");
        let _ = ui.button("Item2");
        let _ = ui.button("Item3");
        let _ = ui.button("Item4");
        if ui.button("Open...").clicked() {
            ui.close_menu();
        }
    });

    let _ = ui.button("Very long text for this item");
}
