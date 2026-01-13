use eframe::egui::{Button, Color32, Grid, RichText, ScrollArea, TextEdit, Ui, Vec2, Vec2b, Widget};
use database::schema::{random_record_id, ComputerData, RecordId, RecordIdExt, TicketData, COMPUTER_TABLE};
use std::collections::BTreeSet;

use crate::{ui_tools::autocomplete::AutoCompleteTextEdit, PlatformSpawner, Spawner};

use super::return_colors;

/// Computer selection data for switching computers in task modal
pub struct ComputerSearchData<'a> {
    pub search_query: &'a mut String,
    pub search_inputs: &'a BTreeSet<String>,
    pub customer_computers: &'a Vec<ComputerData>,
    pub selected_computer: &'a mut Option<ComputerData>,
}

pub fn display_computer_page(
    ui: &mut Ui, 
    service_ticket: Option<&mut TicketData>, 
    computer: Option<&mut ComputerData>,
    avail_size: Vec2
) {
    display_computer_page_with_search(ui, service_ticket, computer, avail_size, None);
}

pub fn display_computer_page_with_search(
    ui: &mut Ui, 
    service_ticket: Option<&mut TicketData>, 
    computer: Option<&mut ComputerData>,
    avail_size: Vec2,
    search_data: Option<ComputerSearchData>,
) {
    let ticket = if let Some(ticket) = service_ticket {
        ticket
    } else { &mut TicketData::default() };

    let computer = if let Some(computer) = computer { 
        computer 
    } else { &mut ComputerData{ id: random_record_id(COMPUTER_TABLE), ..Default::default() } };


    ScrollArea::vertical()
        .max_height(f32::INFINITY)
        .max_width(avail_size.x)
        .auto_shrink(Vec2b::new(false, false))
        .show(ui, |ui|
    {
        ui.vertical_centered(|ui| {
            // Computer search/selection section (if search data is provided)
            if let Some(mut search) = search_data {
                if !search.search_inputs.is_empty() {
                    ui.group(|ui| {
                        ui.label(RichText::new("Customer's Computers").strong().color(Color32::LIGHT_BLUE));
                        ui.horizontal(|ui| {
                            let autocomplete = AutoCompleteTextEdit::new(
                                search.search_query,
                                search.search_inputs.clone(),
                            );
                            autocomplete.ui(ui);
                            
                            // Find matching computer from search query
                            if ui.add_enabled(!search.search_query.is_empty(), Button::new("Select")).clicked() {
                                // Find the computer that matches the search query
                                for comp in search.customer_computers.iter() {
                                    let search_str = if !comp.hostname.is_empty() && !comp.cpu.is_empty() {
                                        format!("{} - {}", comp.hostname, comp.cpu)
                                    } else if !comp.hostname.is_empty() {
                                        comp.hostname.clone()
                                    } else if !comp.cpu.is_empty() {
                                        comp.cpu.clone()
                                    } else {
                                        comp.id.key_string()
                                    };
                                    
                                    if search_str == *search.search_query {
                                        // Copy this computer's data to the current computer
                                        computer.id = comp.id.clone();
                                        computer.hostname = comp.hostname.clone();
                                        computer.cpu = comp.cpu.clone();
                                        computer.gpu = comp.gpu.clone();
                                        computer.ram = comp.ram.clone();
                                        computer.operating_system = comp.operating_system.clone();
                                        computer.drives = comp.drives.clone();
                                        computer.motherboard_name = comp.motherboard_name.clone();
                                        computer.device_name = comp.device_name.clone();
                                        computer.device_mfg = comp.device_mfg.clone();
                                        computer.device_model = comp.device_model.clone();
                                        computer.device_serial = comp.device_serial.clone();
                                        computer.customer = comp.customer.clone();
                                        computer.seb_info = comp.seb_info.clone();
                                        computer.windows_active = comp.windows_active.clone();
                                        
                                        // Update ticket's computer reference
                                        ticket.computer = Some(computer.id.clone());
                                        
                                        log::info!("Selected computer: {} ({})", comp.hostname, comp.id.key_string());
                                        break;
                                    }
                                }
                                search.search_query.clear();
                            }
                        });
                    });
                    ui.add_space(10.0);
                }
            }
            
            if ui.button("Update").clicked() {
                let computer_data: ComputerData = computer.clone();
                let ticket_computer = ticket.computer.clone();
                PlatformSpawner::spawn(async move {
                    match computer_data.update_computer().await {
                        Ok(pc) => log::info!("Updated computer: {pc:?}"),
                        Err(e) => log::error!("Error creating computer: {e:?}"),
                    }
                    // Also update the ticket if computer was selected
                    if ticket_computer.is_some() {
                        // The ticket update will be handled separately
                    }
                });
            }
            ui.group(|ui| {
                Grid::new(format!("{} grid", computer.cpu))
                .max_col_width(avail_size.x / 2.14)
                .min_col_width(avail_size.x / 2.14)
                .with_row_color(|num, style| return_colors(num, style))
                .show(ui, |ui| {
                    ui.colored_label(Color32::LIGHT_RED, "ID");
                    ui.label(computer.id.key_string());
                    ui.end_row();
                    
                    if let Some(cust) = &computer.customer {
                        ui.colored_label(Color32::LIGHT_RED, "Linked Customer");
                        ui.label(cust.key_string());
                        ui.end_row();
                    }

                    ui.colored_label(Color32::LIGHT_RED, "Hostname");
                    TextEdit::singleline(&mut computer.hostname).desired_width(avail_size.x / 2.14).ui(ui);
                    ui.end_row();
                    ui.colored_label(Color32::LIGHT_RED, "Operating System");
                    TextEdit::singleline(&mut computer.operating_system).desired_width(avail_size.x / 2.14).ui(ui);
                    ui.end_row();
                    if let Some(active) = &computer.windows_active {
                        ui.colored_label(Color32::LIGHT_RED, "Windows Active");
                        ui.label(format!("{active}"));
                        ui.end_row();
                    }
                    ui.colored_label(Color32::LIGHT_RED, "CPU");
                    TextEdit::singleline(&mut computer.cpu).desired_width(avail_size.x / 2.14).ui(ui);
                    ui.end_row();
                    ui.colored_label(Color32::LIGHT_RED, "GPU");
                    TextEdit::singleline(&mut computer.gpu).desired_width(avail_size.x / 2.14).ui(ui);
                    ui.end_row();
                    ui.colored_label(Color32::LIGHT_RED, "RAM");
                    TextEdit::singleline(&mut computer.ram).desired_width(avail_size.x / 2.14).ui(ui);
                    ui.end_row();
                    ui.colored_label(Color32::LIGHT_RED, "Device Name");
                    if let Some(device_name) = computer.device_name.as_mut() {
                        TextEdit::singleline(device_name).desired_width(avail_size.x / 2.14).ui(ui);
                    } else {
                        ui.label(&format!(" - "));
                    }
                    ui.end_row();

                    ui.colored_label(Color32::LIGHT_RED, "Device Mfg");
                    if let Some(device_mfg) = computer.device_mfg.as_mut() {
                        TextEdit::singleline(device_mfg).desired_width(avail_size.x / 2.14).ui(ui);
                    } else {
                        ui.label(&format!(" - "));
                    }
                    ui.end_row();
                    ui.colored_label(Color32::LIGHT_RED, "Device Model");
                    if let Some(device_model) = computer.device_model.as_mut() {
                        TextEdit::singleline(device_model).desired_width(avail_size.x / 2.14).ui(ui);
                    } else {
                        ui.label(&format!(" - "));
                    }
                    ui.end_row();
                    ui.colored_label(Color32::LIGHT_RED, "Device Serial");
                    if let Some(device_serial) = computer.device_serial.as_mut() {
                        TextEdit::singleline(device_serial).desired_width(avail_size.x / 2.14).ui(ui);
                    } else {
                        ui.label(&format!(" - "));
                    }
                    
                    ui.end_row();
                    ui.end_row();

                    ui.colored_label(Color32::LIGHT_RED, "HDD Test:");
                    ui.label(&ticket.hardware_test_results.hdd_test);
                    ui.end_row();
                    ui.colored_label(Color32::LIGHT_RED, "SSD Test:");
                    ui.label(&ticket.hardware_test_results.ssd_test);
                    ui.end_row();
                    ui.colored_label(Color32::LIGHT_RED, "RAM Test:");
                    ui.label(&ticket.hardware_test_results.ram_test);
                    ui.end_row();

                });
            });

            ui.add_space(15.);

            ui.group(|ui| {
                Grid::new("group1").max_col_width(avail_size.x / 3.2 - 2.).min_col_width(avail_size.x / 3.2-2.).with_row_color(|num, style| return_colors(num, style))
                .show(ui, |ui| {
                    ui.colored_label(Color32::LIGHT_RED, "Letter");
                    ui.colored_label(Color32::LIGHT_RED, "Type");
                    ui.colored_label(Color32::LIGHT_RED, "Space Left / Total Size");
                    ui.end_row();

                    for drive_data in &computer.drives{
                        ui.colored_label(Color32::LIGHT_RED, &drive_data.drive_letter);
                        ui.label(&drive_data.drive_type);
                        ui.label(format!("{} Gb / {} Gb", &drive_data.space_left, &drive_data.total_size));
                        ui.end_row();
                    }
                });
            });

            ui.add_space(15.);
        });
    });
    
}
