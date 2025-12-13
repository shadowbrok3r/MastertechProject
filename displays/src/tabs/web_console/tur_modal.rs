//! TUR Sheet creation modal.
//!
//! Displays computer specs from a connected client and allows
//! editing before creating a TUR sheet / task.

use database::schema::{ComputerData, ConnectedClient, CustomerData};
use eframe::egui::{
    Align, Button, Color32, Context, Frame, Grid, Layout, Margin, RichText, Rounding, ScrollArea,
    TextEdit, Ui, Vec2, Window,
};
use serde::{Deserialize, Serialize};

/// State for the TUR creation modal
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TurModalState {
    /// The client we're creating a TUR for
    pub client: Option<ConnectedClient>,
    /// Computer data (fetched or entered)
    pub computer: Option<ComputerData>,
    /// Customer data (if linked)
    pub customer: Option<CustomerData>,
    /// Is loading data?
    pub loading: bool,
    /// Error message
    pub error: Option<String>,
    /// Editable fields
    pub hostname: String,
    pub operating_system: String,
    pub cpu: String,
    pub gpu: String,
    pub ram: String,
    pub drives: String,
    pub device_serial: String,
    pub product_name: String,
    pub notes: String,
    /// Service number (auto-generated or manual)
    pub service_number: String,
}

impl TurModalState {
    pub fn new(client: ConnectedClient) -> Self {
        Self {
            client: Some(client),
            computer: None,
            customer: None,
            loading: true,
            error: None,
            hostname: String::new(),
            operating_system: String::new(),
            cpu: String::new(),
            gpu: String::new(),
            ram: String::new(),
            drives: String::new(),
            device_serial: String::new(),
            product_name: String::new(),
            notes: String::new(),
            service_number: String::new(),
        }
    }

    /// Populate editable fields from computer data
    pub fn populate_from_computer(&mut self, computer: &ComputerData) {
        self.computer = Some(computer.clone());
        self.hostname = computer.hostname.clone();
        self.operating_system = computer.operating_system.clone();
        self.cpu = computer.cpu.clone();
        self.gpu = computer.gpu.clone();
        self.ram = computer.ram.clone();
        self.drives = computer
            .drives
            .iter()
            .map(|d| format!("{}: {}", d.name, d.total_capacity))
            .collect::<Vec<_>>()
            .join(", ");
        self.device_serial = computer.device_serial.clone().unwrap_or_default();
        self.product_name = computer.product_name.clone();
        self.loading = false;
    }

    /// Check if form is valid for submission
    pub fn is_valid(&self) -> bool {
        !self.hostname.is_empty() && !self.service_number.is_empty()
    }
}

/// Result of the TUR modal interaction
#[derive(Debug, Clone)]
pub enum TurModalResult {
    /// User cancelled
    Cancelled,
    /// User confirmed - create the TUR with these details
    Confirmed(TurCreationData),
    /// Still open, no action yet
    Open,
}

/// Data needed to create a TUR sheet
#[derive(Debug, Clone)]
pub struct TurCreationData {
    pub connection_string: String,
    pub hostname: String,
    pub operating_system: String,
    pub cpu: String,
    pub gpu: String,
    pub ram: String,
    pub drives: String,
    pub device_serial: String,
    pub product_name: String,
    pub notes: String,
    pub service_number: String,
    pub computer_id: Option<surrealdb::RecordId>,
    pub customer_id: Option<surrealdb::RecordId>,
}

/// Render the TUR creation modal
pub fn show_tur_modal(ctx: &Context, state: &mut TurModalState) -> TurModalResult {
    let mut result = TurModalResult::Open;
    let mut open = true;

    let client_name = state
        .client
        .as_ref()
        .and_then(|c| c.friendly_name.clone())
        .unwrap_or_else(|| {
            state
                .client
                .as_ref()
                .map(|c| c.connection_string.clone())
                .unwrap_or_else(|| "Unknown".to_string())
        });

    Window::new(RichText::new(format!("Create TUR Sheet - {}", client_name)).size(16.0))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(600.0)
        .min_width(500.0)
        .show(ctx, |ui| {
            if state.loading {
                ui.centered_and_justified(|ui| {
                    ui.spinner();
                    ui.label("Loading computer data...");
                });
                return;
            }

            if let Some(error) = &state.error {
                ui.colored_label(Color32::RED, error);
                ui.add_space(8.0);
            }

            ScrollArea::vertical().show(ui, |ui| {
                // Computer specs section
                ui.heading("Computer Specifications");
                ui.add_space(8.0);

                Frame::none()
                    .fill(Color32::from_rgb(25, 28, 35))
                    .inner_margin(Margin::same(12.0))
                    .rounding(Rounding::same(6.0))
                    .show(ui, |ui| {
                        Grid::new("tur_specs_grid")
                            .num_columns(2)
                            .spacing([12.0, 8.0])
                            .striped(true)
                            .show(ui, |ui| {
                                // Hostname
                                ui.label(
                                    RichText::new("Hostname:")
                                        .color(Color32::from_rgb(160, 165, 175)),
                                );
                                ui.add(
                                    TextEdit::singleline(&mut state.hostname)
                                        .desired_width(300.0),
                                );
                                ui.end_row();

                                // OS
                                ui.label(
                                    RichText::new("Operating System:")
                                        .color(Color32::from_rgb(160, 165, 175)),
                                );
                                ui.add(
                                    TextEdit::singleline(&mut state.operating_system)
                                        .desired_width(300.0),
                                );
                                ui.end_row();

                                // CPU
                                ui.label(
                                    RichText::new("CPU:").color(Color32::from_rgb(160, 165, 175)),
                                );
                                ui.add(
                                    TextEdit::singleline(&mut state.cpu).desired_width(300.0),
                                );
                                ui.end_row();

                                // GPU
                                ui.label(
                                    RichText::new("GPU:").color(Color32::from_rgb(160, 165, 175)),
                                );
                                ui.add(
                                    TextEdit::singleline(&mut state.gpu).desired_width(300.0),
                                );
                                ui.end_row();

                                // RAM
                                ui.label(
                                    RichText::new("RAM:").color(Color32::from_rgb(160, 165, 175)),
                                );
                                ui.add(
                                    TextEdit::singleline(&mut state.ram).desired_width(300.0),
                                );
                                ui.end_row();

                                // Drives
                                ui.label(
                                    RichText::new("Drives:")
                                        .color(Color32::from_rgb(160, 165, 175)),
                                );
                                ui.add(
                                    TextEdit::singleline(&mut state.drives).desired_width(300.0),
                                );
                                ui.end_row();

                                // Serial
                                ui.label(
                                    RichText::new("Serial Number:")
                                        .color(Color32::from_rgb(160, 165, 175)),
                                );
                                ui.add(
                                    TextEdit::singleline(&mut state.device_serial)
                                        .desired_width(300.0),
                                );
                                ui.end_row();

                                // Product Name
                                ui.label(
                                    RichText::new("Product Name:")
                                        .color(Color32::from_rgb(160, 165, 175)),
                                );
                                ui.add(
                                    TextEdit::singleline(&mut state.product_name)
                                        .desired_width(300.0),
                                );
                                ui.end_row();
                            });
                    });

                ui.add_space(16.0);

                // Service info section
                ui.heading("Service Information");
                ui.add_space(8.0);

                Frame::none()
                    .fill(Color32::from_rgb(25, 28, 35))
                    .inner_margin(Margin::same(12.0))
                    .rounding(Rounding::same(6.0))
                    .show(ui, |ui| {
                        Grid::new("tur_service_grid")
                            .num_columns(2)
                            .spacing([12.0, 8.0])
                            .show(ui, |ui| {
                                // Service Number
                                ui.label(
                                    RichText::new("Service Number: *")
                                        .color(Color32::from_rgb(255, 200, 100)),
                                );
                                ui.horizontal(|ui| {
                                    ui.add(
                                        TextEdit::singleline(&mut state.service_number)
                                            .desired_width(200.0),
                                    );
                                    if ui.small_button("Generate").clicked() {
                                        // Generate a service number based on date and random
                                        let now = chrono::Local::now();
                                        state.service_number =
                                            format!("{}-{:04}", now.format("%Y%m%d"), rand::random::<u16>() % 10000);
                                    }
                                });
                                ui.end_row();

                                // Notes
                                ui.label(
                                    RichText::new("Notes:")
                                        .color(Color32::from_rgb(160, 165, 175)),
                                );
                                ui.add(
                                    TextEdit::multiline(&mut state.notes)
                                        .desired_width(300.0)
                                        .desired_rows(3),
                                );
                                ui.end_row();
                            });
                    });

                ui.add_space(16.0);

                // Customer info (if available)
                if let Some(customer) = &state.customer {
                    ui.heading("Customer");
                    ui.add_space(8.0);

                    Frame::none()
                        .fill(Color32::from_rgb(25, 28, 35))
                        .inner_margin(Margin::same(12.0))
                        .rounding(Rounding::same(6.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Name:")
                                        .color(Color32::from_rgb(160, 165, 175)),
                                );
                                ui.label(
                                    RichText::new(&customer.name)
                                        .color(Color32::from_rgb(51, 255, 189)),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Phone:")
                                        .color(Color32::from_rgb(160, 165, 175)),
                                );
                                ui.label(&customer.phone_number);
                            });
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Email:")
                                        .color(Color32::from_rgb(160, 165, 175)),
                                );
                                ui.label(&customer.email);
                            });
                        });

                    ui.add_space(16.0);
                }
            });

            // Bottom action buttons
            ui.add_space(16.0);
            ui.separator();
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Create button
                    let can_create = state.is_valid();
                    let create_btn = Button::new(
                        RichText::new("Create TUR Sheet")
                            .size(14.0)
                            .color(if can_create {
                                Color32::WHITE
                            } else {
                                Color32::GRAY
                            }),
                    )
                    .min_size(Vec2::new(140.0, 32.0))
                    .fill(if can_create {
                        Color32::from_rgb(50, 150, 80)
                    } else {
                        Color32::from_rgb(60, 65, 75)
                    });

                    if ui.add_enabled(can_create, create_btn).clicked() {
                        result = TurModalResult::Confirmed(TurCreationData {
                            connection_string: state
                                .client
                                .as_ref()
                                .map(|c| c.connection_string.clone())
                                .unwrap_or_default(),
                            hostname: state.hostname.clone(),
                            operating_system: state.operating_system.clone(),
                            cpu: state.cpu.clone(),
                            gpu: state.gpu.clone(),
                            ram: state.ram.clone(),
                            drives: state.drives.clone(),
                            device_serial: state.device_serial.clone(),
                            product_name: state.product_name.clone(),
                            notes: state.notes.clone(),
                            service_number: state.service_number.clone(),
                            computer_id: state.computer.as_ref().map(|c| c.id.clone()),
                            customer_id: state.customer.as_ref().map(|c| c.id.clone()),
                        });
                    }

                    ui.add_space(8.0);

                    // Cancel button
                    let cancel_btn = Button::new(RichText::new("Cancel").size(14.0))
                        .min_size(Vec2::new(80.0, 32.0));

                    if ui.add(cancel_btn).clicked() {
                        result = TurModalResult::Cancelled;
                    }
                });
            });
        });

    if !open {
        result = TurModalResult::Cancelled;
    }

    result
}

