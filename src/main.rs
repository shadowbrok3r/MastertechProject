#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide output_console window on Windows in release
//#![allow(unused_imports)] //it thinks im not using catppuccin_egui

mod system_info;
mod request;
mod file_browser;
mod scaffold;
mod context;

use context::MasterTechApp;

use eframe::egui;
use egui::*;
use egui_dock::{DockArea, Style};
use catppuccin_egui::MOCHA;


#[tokio::main]
async fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(925.0, 740.0)),
        ..Default::default()
    };
    eframe::run_native(
        "Mastertech",
        options,
        Box::new(|_cc| Box::<MasterTechApp>::default()),
    )
}


impl eframe::App for MasterTechApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        catppuccin_egui::set_theme(ctx, MOCHA);

        let receiver = self.context.rx.as_ref().unwrap();
        while let Ok(message) = receiver.try_recv() {
            println!("{message:?}");
            // Try to parse the JSON string into a TicketInformation
            if let Ok(info) = serde_json::from_str::<scaffold::TicketInformation>(&message) {
                println!("{info:#?}");
                self.context.output_text.clear();
                let checkin_rep = info.user_id;
                self.context.ticket_info.user_id = checkin_rep.clone();
                if checkin_rep == "DMK"{self.context.salesman_cbox = scaffold::Salesman::Danny;}
                else if checkin_rep == "JDH2"{self.context.salesman_cbox = scaffold::Salesman::Jake}

                // Handle TicketInformation
                self.context.ticket_info.customer_name = info.customer_name;
                self.context.ticket_info.customer_phone_1 = info.customer_phone_1;
                self.context.ticket_info.customer_phone_2 = info.customer_phone_2;
                self.context.ticket_info.checkin_notes = info.checkin_notes;
                self.context.output_text += format!(
                    "Customer Code: {:?}
Customer Email: {:?}
Last Invoice Number: {:?}
Last Invoice Amount: {:?}
Department: {:?}
Jurisdiction: {:?}
Type of Order: {:?}
Item Codes: {:?}\n",
                &info.cust_code, &info.customer_email, &info.last_invoice_number, &info.last_invoice_amount,
                &info.department, &info.jurisdiction, &info.doc_alias, &info.item_codes).as_str();
                self.context.spinner = false;

            }             
            else if let Ok(info) = serde_json::from_str::<scaffold::PulledKeys>(&message) {
                // Handle PulledKeys
                self.context.keys.webroot_key = info.webroot_key;
                self.context.keys.superanti_key = info.superanti_key;
                self.context.spinner = false;
            }
            // If neither parse was successful, consider it an error
            else if let Ok(info) = serde_json::from_str::<system_info::SystemInformation>(&message) {
                self.context.system_name = info.system_name;
                self.context.cpu_name = info.cpu_name;
                self.context.total_ram = info.total_ram;
                for disk in info.disks.disks{
                    
                    self.context.disk_num += 1;

                    if let Some(disks_arr) = self.context.disks.as_array_mut() {
                        // Convert `disk` to a serde_json::Value
                        let disk_json = serde_json::to_value(&disk).unwrap();
                
                        disks_arr.push(disk_json);
                    } else {
                        eprintln!("Expected self.context.disks to be an Array");
                    }
                    
                }
                
            }
            else{
                // Handle error
                self.context.output_text = format!("Error parsing JSON: {}", message);
                self.context.spinner = false;
            }
        }



        TopBottomPanel::top("egui_dock::MenuBar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("View", |ui| {
                    // allow certain tabs to be toggled
                    for tab in &[&self.context.tur_sheet_tab, &self.context.scripts_tab, &self.context.output_console_tab, &self.context.system_info_tab, &self.context.file_browser_tab] {
                        if ui
                            .selectable_label(self.context.open_tabs.contains(*tab), *tab)
                            .clicked()
                        {
                            if let Some(index) = self.tree.find_tab(&tab.to_string()) {
                                self.tree.remove_tab(index);
                                self.context.open_tabs.remove(*tab);
                            } else {
                                self.tree.push_to_focused_leaf(tab.to_string());
                            }
                            ui.close_menu();
                        }
                    }
                });
            })
        });
        
        CentralPanel::default()// When displaying a DockArea in another UI, it looks better
            .frame(Frame::central_panel(&ctx.style()).inner_margin(4.))// to set inner margins to 0.
            .show(ctx, |ui| {
                let mut style = self.context.style.get_or_insert(Style::from_egui(ui.style())).clone();
                style.selection_color = Color32::from_rgb(92,0,87);
                style.separator.color_hovered = Color32::from_rgba_premultiplied(50,93,80,77);
                style.separator.color_idle = Color32::from_rgba_premultiplied(17,17,33,5);
                style.separator.color_dragged = Color32::from_rgba_premultiplied(189,189,189,130);
                style.buttons.add_tab_align = egui_dock::TabAddAlign::Left;
                style.tabs.rounding.nw = 15.0;
                style.tabs.rounding.ne = 15.0;
                style.tabs.text_color_active_focused = Color32::from_rgba_premultiplied(0, 254, 158, 255);
                style.tabs.text_color_active_unfocused = Color32::from_rgba_premultiplied(0, 255, 255, 255);
                style.tabs.text_color_unfocused = Color32::from_rgba_premultiplied(230, 230, 230, 100);
                style.buttons.close_tab_color = Color32::from_rgba_premultiplied(118, 0, 129, 58);

                DockArea::new(&mut self.tree)
                    .style(style)
                    .show_close_buttons(self.context.show_close_buttons)
                    .show_add_buttons(self.context.show_add_buttons)
                    .show_add_popup(true)
                    .draggable_tabs(self.context.draggable_tabs)
                    .show_tab_name_on_hover(self.context.show_tab_name_on_hover)
                    .show_inside(ui, &mut self.context);
            });
    }
}