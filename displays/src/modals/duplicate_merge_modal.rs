//! Duplicate Merge Modal
//! 
//! Displays a diff view when duplicate records are detected during task creation.
//! Allows users to keep existing, use new, or merge fields from both versions.

use eframe::egui::{
    Align, Align2, Button, Color32, Context, Frame, Key, Layout, Margin, 
    RichText, ScrollArea, Shadow, Stroke, Ui, Vec2, Widget, Window
};
use database::schema::{
    ComputerData, CustomerData, DuplicateCheckResult, DuplicatePair, 
    DuplicateResolution, FieldDisplay, FieldSelections, LiveTaskPayload, 
    MergeResolution, TicketData
};
use serde::Serialize;

use super::task_modal::ModalAction;

/// The current page/tab being displayed in the merge modal
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub enum MergeModalPage {
    #[default]
    Task,
    ServiceOrder,
    Customer,
    Computer,
    Summary,
}

/// Duplicate Merge Modal for resolving conflicts between existing and new records
#[derive(Debug, Clone, Serialize)]
pub struct DuplicateMergeModal {
    pub title: String,
    /// The duplicate check result containing all potential conflicts
    #[serde(skip)]
    pub check_result: DuplicateCheckResult,
    /// User's resolution choices
    pub resolution: DuplicateResolution,
    /// Current page being viewed
    pub current_page: MergeModalPage,
    /// Whether the modal is open
    pub is_open: bool,
    /// Whether the user has confirmed their choices
    pub confirmed: bool,
    /// Whether the user cancelled
    pub cancelled: bool,
}

impl Default for DuplicateMergeModal {
    fn default() -> Self {
        Self {
            title: "Resolve Duplicate Records".to_string(),
            check_result: DuplicateCheckResult::default(),
            resolution: DuplicateResolution::default(),
            current_page: MergeModalPage::default(),
            is_open: false,
            confirmed: false,
            cancelled: false,
        }
    }
}

impl DuplicateMergeModal {
    pub fn new(check_result: DuplicateCheckResult) -> Self {
        let title = format!("Duplicate Records Found - SO#{}", check_result.service_number);
        Self {
            title,
            check_result,
            resolution: DuplicateResolution::default(),
            current_page: MergeModalPage::Task,
            is_open: true,
            confirmed: false,
            cancelled: false,
        }
    }

    pub fn open(&mut self) {
        self.is_open = true;
        self.confirmed = false;
        self.cancelled = false;
    }

    pub fn close(&mut self) {
        self.is_open = false;
    }

    /// Returns true if user confirmed their resolution
    pub fn is_confirmed(&self) -> bool {
        self.confirmed
    }

    /// Returns true if user cancelled
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    /// Display the modal
    pub fn show(&mut self, ctx: &Context) -> Option<ModalAction> {
        if !self.is_open {
            return None;
        }

        let mut open = ctx.input(|i| !i.key_pressed(Key::Escape));
        let style = &ctx.style().visuals;
        let mut shadow = Shadow::default();
        shadow.blur = 2;
        shadow.spread = 4;
        shadow.color = style.window_stroke.color;

        let title_text = RichText::new(&self.title)
            .heading()
            .color(style.warn_fg_color);

        Window::new(title_text)
            .frame(
                Frame::default()
                    .inner_margin(Margin::symmetric(12, 12))
                    .stroke(style.window_stroke)
                    .fill(style.window_fill)
                    .corner_radius(style.menu_corner_radius)
                    .shadow(shadow)
            )
            .pivot(Align2::CENTER_CENTER)
            .default_size([800.0, 600.0])
            .min_width(750.0)
            .max_width(900.0)
            .min_height(500.0)
            .max_height(700.0)
            .collapsible(false)
            .resizable(true)
            .open(&mut open)
            .show(ctx, |ui| {
                self.ui_content(ui);
            });

        if !open || self.cancelled {
            self.is_open = false;
            self.cancelled = true;
            return Some(ModalAction::Close);
        }

        if self.confirmed {
            self.is_open = false;
            return Some(ModalAction::Close);
        }

        None
    }

    fn ui_content(&mut self, ui: &mut Ui) {
        // Navigation tabs
        self.render_tabs(ui);
        
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(10.0);

        // Main content area
        ScrollArea::vertical()
            .max_height(400.0)
            .show(ui, |ui| {
                match self.current_page {
                    MergeModalPage::Task => self.render_task_diff(ui),
                    MergeModalPage::ServiceOrder => self.render_service_order_diff(ui),
                    MergeModalPage::Customer => self.render_customer_diff(ui),
                    MergeModalPage::Computer => self.render_computer_diff(ui),
                    MergeModalPage::Summary => self.render_summary(ui),
                }
            });

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(10.0);

        // Action buttons
        self.render_action_buttons(ui);
    }

    fn render_tabs(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let has_task = self.check_result.task.is_some();
            let has_service = self.check_result.service_order.is_some();
            let has_customer = self.check_result.customer.is_some();
            let has_computer = self.check_result.computer.is_some();

            if has_task {
                let selected = self.current_page == MergeModalPage::Task;
                let task_dup = self.check_result.task.as_ref().unwrap();
                let label = if task_dup.is_identical { "Task ✓" } else { "Task ⚠" };
                if ui.selectable_label(selected, RichText::new(label).strong()).clicked() {
                    self.current_page = MergeModalPage::Task;
                }
            }

            if has_service {
                let selected = self.current_page == MergeModalPage::ServiceOrder;
                let svc_dup = self.check_result.service_order.as_ref().unwrap();
                let label = if svc_dup.is_identical { "Service ✓" } else { "Service ⚠" };
                if ui.selectable_label(selected, RichText::new(label).strong()).clicked() {
                    self.current_page = MergeModalPage::ServiceOrder;
                }
            }

            if has_customer {
                let selected = self.current_page == MergeModalPage::Customer;
                let cust_dup = self.check_result.customer.as_ref().unwrap();
                let label = if cust_dup.is_identical { "Customer ✓" } else { "Customer ⚠" };
                if ui.selectable_label(selected, RichText::new(label).strong()).clicked() {
                    self.current_page = MergeModalPage::Customer;
                }
            }

            if has_computer {
                let selected = self.current_page == MergeModalPage::Computer;
                let comp_dup = self.check_result.computer.as_ref().unwrap();
                let label = if comp_dup.is_identical { "Computer ✓" } else { "Computer ⚠" };
                if ui.selectable_label(selected, RichText::new(label).strong()).clicked() {
                    self.current_page = MergeModalPage::Computer;
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.selectable_label(
                    self.current_page == MergeModalPage::Summary,
                    RichText::new("Summary").strong()
                ).clicked() {
                    self.current_page = MergeModalPage::Summary;
                }
            });
        });
    }

    fn render_task_diff(&mut self, ui: &mut Ui) {
        self.render_task_page(ui);
    }

    fn render_service_order_diff(&mut self, ui: &mut Ui) {
        self.render_service_page(ui);
    }

    fn render_customer_diff(&mut self, ui: &mut Ui) {
        self.render_customer_page(ui);
    }

    fn render_computer_diff(&mut self, ui: &mut Ui) {
        self.render_computer_page(ui);
    }

    fn render_task_page(&mut self, ui: &mut Ui) {
        if let Some(ref dup) = self.check_result.task.clone() {
            if dup.is_identical {
                ui.colored_label(Color32::GREEN, "✓ Task records are identical - no action needed");
                return;
            }
            render_task_field_diff(
                ui,
                &dup.existing,
                &dup.new,
                &mut self.resolution.task_resolution,
                &mut self.resolution.task_fields,
            );
        } else {
            ui.label("No duplicate task found.");
        }
    }

    fn render_service_page(&mut self, ui: &mut Ui) {
        if let Some(ref dup) = self.check_result.service_order.clone() {
            if dup.is_identical {
                ui.colored_label(Color32::GREEN, "✓ Service Order records are identical - no action needed");
                return;
            }
            render_ticket_field_diff(
                ui,
                &dup.existing,
                &dup.new,
                &mut self.resolution.service_order_resolution,
                &mut self.resolution.service_order_fields,
            );
        } else {
            ui.label("No duplicate service order found.");
        }
    }

    fn render_customer_page(&mut self, ui: &mut Ui) {
        if let Some(ref dup) = self.check_result.customer.clone() {
            if dup.is_identical {
                ui.colored_label(Color32::GREEN, "✓ Customer records are identical - no action needed");
                return;
            }
            render_customer_field_diff(
                ui,
                &dup.existing,
                &dup.new,
                &mut self.resolution.customer_resolution,
                &mut self.resolution.customer_fields,
            );
        } else {
            ui.label("No duplicate customer found.");
        }
    }

    fn render_computer_page(&mut self, ui: &mut Ui) {
        if let Some(ref dup) = self.check_result.computer.clone() {
            if dup.is_identical {
                ui.colored_label(Color32::GREEN, "✓ Computer records are identical - no action needed");
                return;
            }
            render_computer_field_diff(
                ui,
                &dup.existing,
                &dup.new,
                &mut self.resolution.computer_resolution,
                &mut self.resolution.computer_fields,
            );
        } else {
            ui.label("No duplicate computer found.");
        }
    }

    fn render_summary(&mut self, ui: &mut Ui) {
        ui.heading("Resolution Summary");
        ui.add_space(10.0);

        let resolution_text = |res: &MergeResolution| -> &str {
            match res {
                MergeResolution::KeepExisting => "Keep Existing",
                MergeResolution::UseNew => "Use New",
                MergeResolution::Merge => "Merge Fields",
                MergeResolution::Cancel => "Cancel",
            }
        };

        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .column(egui_extras::Column::exact(150.0))
            .column(egui_extras::Column::exact(150.0))
            .column(egui_extras::Column::remainder())
            .header(25.0, |mut header| {
                header.col(|ui| { ui.strong("Entity"); });
                header.col(|ui| { ui.strong("Status"); });
                header.col(|ui| { ui.strong("Resolution"); });
            })
            .body(|mut body| {
                if let Some(ref dup) = self.check_result.task {
                    body.row(25.0, |mut row| {
                        row.col(|ui| { ui.label("Task"); });
                        row.col(|ui| {
                            if dup.is_identical {
                                ui.colored_label(Color32::GREEN, "Identical");
                            } else {
                                ui.colored_label(Color32::YELLOW, "Conflict");
                            }
                        });
                        row.col(|ui| { ui.label(resolution_text(&self.resolution.task_resolution)); });
                    });
                }

                if let Some(ref dup) = self.check_result.service_order {
                    body.row(25.0, |mut row| {
                        row.col(|ui| { ui.label("Service Order"); });
                        row.col(|ui| {
                            if dup.is_identical {
                                ui.colored_label(Color32::GREEN, "Identical");
                            } else {
                                ui.colored_label(Color32::YELLOW, "Conflict");
                            }
                        });
                        row.col(|ui| { ui.label(resolution_text(&self.resolution.service_order_resolution)); });
                    });
                }

                if let Some(ref dup) = self.check_result.customer {
                    body.row(25.0, |mut row| {
                        row.col(|ui| { ui.label("Customer"); });
                        row.col(|ui| {
                            if dup.is_identical {
                                ui.colored_label(Color32::GREEN, "Identical");
                            } else {
                                ui.colored_label(Color32::YELLOW, "Conflict");
                            }
                        });
                        row.col(|ui| { ui.label(resolution_text(&self.resolution.customer_resolution)); });
                    });
                }

                if let Some(ref dup) = self.check_result.computer {
                    body.row(25.0, |mut row| {
                        row.col(|ui| { ui.label("Computer"); });
                        row.col(|ui| {
                            if dup.is_identical {
                                ui.colored_label(Color32::GREEN, "Identical");
                            } else {
                                ui.colored_label(Color32::YELLOW, "Conflict");
                            }
                        });
                        row.col(|ui| { ui.label(resolution_text(&self.resolution.computer_resolution)); });
                    });
                }
            });
    }

    fn render_action_buttons(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui.add(
                Button::new(RichText::new("Cancel").color(Color32::LIGHT_RED))
                    .min_size(Vec2::new(100.0, 30.0))
            ).clicked() {
                self.cancelled = true;
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(
                    Button::new(RichText::new("Confirm & Submit").color(Color32::LIGHT_GREEN))
                        .min_size(Vec2::new(150.0, 30.0))
                ).clicked() {
                    self.confirmed = true;
                }

                if ui.add(
                    Button::new("Use All New")
                        .min_size(Vec2::new(100.0, 30.0))
                ).clicked() {
                    self.resolution.task_resolution = MergeResolution::UseNew;
                    self.resolution.service_order_resolution = MergeResolution::UseNew;
                    self.resolution.customer_resolution = MergeResolution::UseNew;
                    self.resolution.computer_resolution = MergeResolution::UseNew;
                }

                if ui.add(
                    Button::new("Keep All Existing")
                        .min_size(Vec2::new(120.0, 30.0))
                ).clicked() {
                    self.resolution.task_resolution = MergeResolution::KeepExisting;
                    self.resolution.service_order_resolution = MergeResolution::KeepExisting;
                    self.resolution.customer_resolution = MergeResolution::KeepExisting;
                    self.resolution.computer_resolution = MergeResolution::KeepExisting;
                }
            });
        });
    }

    /// Get the final resolution after user confirms
    pub fn get_resolution(&self) -> &DuplicateResolution {
        &self.resolution
    }
}

/// Helper function to render a field diff row
fn render_field_row(
    ui: &mut Ui,
    field_name: &str,
    existing_value: &str,
    new_value: &str,
    use_new: &mut bool,
    show_merge_option: bool,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(field_name).strong());
    });

    ui.horizontal(|ui| {
        // Existing value column
        Frame::default()
            .fill(Color32::from_rgb(60, 30, 30))
            .corner_radius(4.0)
            .inner_margin(Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.set_min_width(250.0);
                let text = if existing_value.is_empty() { "(empty)" } else { existing_value };
                ui.label(RichText::new(text).color(Color32::from_rgb(255, 150, 150)));
            });

        ui.label("→");

        // New value column
        Frame::default()
            .fill(Color32::from_rgb(30, 60, 30))
            .corner_radius(4.0)
            .inner_margin(Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.set_min_width(250.0);
                let text = if new_value.is_empty() { "(empty)" } else { new_value };
                ui.label(RichText::new(text).color(Color32::from_rgb(150, 255, 150)));
            });

        if show_merge_option {
            ui.checkbox(use_new, "Use New");
        }
    });

    ui.add_space(5.0);
}

/// Render diff for LiveTaskPayload
pub fn render_task_field_diff(
    ui: &mut Ui,
    existing: &LiveTaskPayload,
    new: &LiveTaskPayload,
    resolution: &mut MergeResolution,
    selections: &mut FieldSelections,
) {
    ui.heading("Task Differences");
    ui.add_space(10.0);

    // Resolution selector
    ui.horizontal(|ui| {
        ui.label("Resolution:");
        ui.radio_value(resolution, MergeResolution::KeepExisting, "Keep Existing");
        ui.radio_value(resolution, MergeResolution::UseNew, "Use New");
        ui.radio_value(resolution, MergeResolution::Merge, "Merge Fields");
    });

    ui.add_space(10.0);

    let show_merge = *resolution == MergeResolution::Merge;
    let fields = existing.get_differing_fields(new);

    if fields.is_empty() {
        ui.colored_label(Color32::GREEN, "No differences found.");
        return;
    }

    for (field_name, existing_val, new_val) in fields {
        let mut use_new = selections.use_new(&field_name);
        render_field_row(ui, &field_name, &existing_val, &new_val, &mut use_new, show_merge);
        if show_merge {
            selections.set_use_new(&field_name, use_new);
        }
    }
}

/// Render diff for TicketData
pub fn render_ticket_field_diff(
    ui: &mut Ui,
    existing: &TicketData,
    new: &TicketData,
    resolution: &mut MergeResolution,
    selections: &mut FieldSelections,
) {
    ui.heading("Service Order Differences");
    ui.add_space(10.0);

    ui.horizontal(|ui| {
        ui.label("Resolution:");
        ui.radio_value(resolution, MergeResolution::KeepExisting, "Keep Existing");
        ui.radio_value(resolution, MergeResolution::UseNew, "Use New");
        ui.radio_value(resolution, MergeResolution::Merge, "Merge Fields");
    });

    ui.add_space(10.0);

    let show_merge = *resolution == MergeResolution::Merge;
    let fields = existing.get_differing_fields(new);

    if fields.is_empty() {
        ui.colored_label(Color32::GREEN, "No differences found.");
        return;
    }

    for (field_name, existing_val, new_val) in fields {
        let mut use_new = selections.use_new(&field_name);
        render_field_row(ui, &field_name, &existing_val, &new_val, &mut use_new, show_merge);
        if show_merge {
            selections.set_use_new(&field_name, use_new);
        }
    }
}

/// Render diff for CustomerData
pub fn render_customer_field_diff(
    ui: &mut Ui,
    existing: &CustomerData,
    new: &CustomerData,
    resolution: &mut MergeResolution,
    selections: &mut FieldSelections,
) {
    ui.heading("Customer Differences");
    ui.add_space(10.0);

    ui.horizontal(|ui| {
        ui.label("Resolution:");
        ui.radio_value(resolution, MergeResolution::KeepExisting, "Keep Existing");
        ui.radio_value(resolution, MergeResolution::UseNew, "Use New");
        ui.radio_value(resolution, MergeResolution::Merge, "Merge Fields");
    });

    ui.add_space(10.0);

    let show_merge = *resolution == MergeResolution::Merge;
    let fields = existing.get_differing_fields(new);

    if fields.is_empty() {
        ui.colored_label(Color32::GREEN, "No differences found.");
        return;
    }

    for (field_name, existing_val, new_val) in fields {
        let mut use_new = selections.use_new(&field_name);
        render_field_row(ui, &field_name, &existing_val, &new_val, &mut use_new, show_merge);
        if show_merge {
            selections.set_use_new(&field_name, use_new);
        }
    }
}

/// Render diff for ComputerData
pub fn render_computer_field_diff(
    ui: &mut Ui,
    existing: &ComputerData,
    new: &ComputerData,
    resolution: &mut MergeResolution,
    selections: &mut FieldSelections,
) {
    ui.heading("Computer Differences");
    ui.add_space(10.0);

    ui.horizontal(|ui| {
        ui.label("Resolution:");
        ui.radio_value(resolution, MergeResolution::KeepExisting, "Keep Existing");
        ui.radio_value(resolution, MergeResolution::UseNew, "Use New");
        ui.radio_value(resolution, MergeResolution::Merge, "Merge Fields");
    });

    ui.add_space(10.0);

    let show_merge = *resolution == MergeResolution::Merge;
    let fields = existing.get_differing_fields(new);

    if fields.is_empty() {
        ui.colored_label(Color32::GREEN, "No differences found.");
        return;
    }

    for (field_name, existing_val, new_val) in fields {
        let mut use_new = selections.use_new(&field_name);
        render_field_row(ui, &field_name, &existing_val, &new_val, &mut use_new, show_merge);
        if show_merge {
            selections.set_use_new(&field_name, use_new);
        }
    }
}

