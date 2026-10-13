//! Duplicate Merge Modal
//!
//! Displays a diff view when duplicate records are detected during task creation.
//! Allows users to keep existing, use new, or merge fields from both versions.

use eframe::egui::{Align2, Context, Frame, Grid, Key, Margin, RichText, ScrollArea, Ui};
use crate::ui_tools::{
    controls::{self, BarKind, Icon, IconLabel, Metrics, Role, Section, Tokens},
    icons::svg,
    theme,
    tokens::composite,
};
use database::schema::{
    ComputerData, CustomerData, DuplicateCheckResult, RecordId, RecordIdExt,
    DuplicateResolution, FieldDisplay, FieldSelections, LiveTaskPayload,
    MergeResolution, TicketData, merge_task, merge_ticket, merge_customer, merge_computer
};
use serde::Serialize;
use std::collections::HashMap;

use super::task_modal::ModalAction;

/// Inner margin of the merge window.
const WINDOW_MARGIN: i8 = 16;
/// Share of a status hue in the old/new value wash.
const VALUE_WASH: f32 = 0.16;
/// Footer buttons, in the order `render_action_buttons` matches on.
const ACTIONS: [(Role, &str); 4] = [
    (Role::Primary, "Confirm & Submit"),
    (Role::Secondary, "Use All New"),
    (Role::Secondary, "Keep All Existing"),
    (Role::Cancel, "Cancel"),
];

/// The current page/tab being displayed in the merge modal
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub enum MergeModalPage {
    #[default]
    Task,
    ServiceOrder,
    Customer,
    Computer,
    Preview,
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
    /// Cache of user RecordId -> username for display
    #[serde(skip)]
    pub user_cache: HashMap<String, String>,
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
            user_cache: HashMap::new(),
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
            user_cache: HashMap::new(),
        }
    }

    /// Add a user to the cache for display purposes
    pub fn cache_user(&mut self, id: &RecordId, username: &str) {
        self.user_cache.insert(id.key_string(), username.to_string());
    }

    /// Get a formatted string for an assignee: "Username (RecordId)" or just the ID if not cached
    pub fn format_assignee(&self, id: &RecordId) -> String {
        let id_str = id.key_string();
        if let Some(username) = self.user_cache.get(&id_str) {
            format!("{} ({})", username, id_str)
        } else {
            id_str
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
        let title = self.title.clone();

        let warn = ctx.global_style().visuals.warn_fg_color;
        controls::icon_window_tone(ctx, svg::WARNING, &title, warn)
            .frame(Frame::window(&ctx.global_style()).inner_margin(Margin::same(WINDOW_MARGIN)))
            .pivot(Align2::CENTER_CENTER)
            .fixed_size([800., 700.0])
            .collapsible(false)
            .resizable(false)
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
        let m = Metrics::get(ui.ctx());
        // Navigation tabs
        self.render_tabs(ui);
        ui.add_space(m.gap.y);

        // Main content area - use available height minus space for buttons
        let available_height = ui.available_height() - 60.0;
        ScrollArea::vertical()
            .min_scrolled_height(available_height)
            .max_width(available_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                match self.current_page {
                    MergeModalPage::Task => self.render_task_diff(ui),
                    MergeModalPage::ServiceOrder => self.render_service_order_diff(ui),
                    MergeModalPage::Customer => self.render_customer_diff(ui),
                    MergeModalPage::Computer => self.render_computer_diff(ui),
                    MergeModalPage::Preview => self.render_preview(ui),
                    MergeModalPage::Summary => self.render_summary(ui),
                }
            });

        ui.add_space(m.gap.y);

        // Action buttons
        self.render_action_buttons(ui);
    }

    fn render_tabs(&mut self, ui: &mut Ui) {
        let status_icon = |identical: bool| if identical { svg::CHECK_CIRCLE } else { svg::WARNING };
        controls::bar(ui, BarKind::Tool, |ui| {
            if let Some(task_dup) = self.check_result.task.as_ref() {
                let selected = self.current_page == MergeModalPage::Task;
                if controls::pill(ui, status_icon(task_dup.is_identical), "Task", selected).clicked() {
                    self.current_page = MergeModalPage::Task;
                }
            }

            if let Some(svc_dup) = self.check_result.service_order.as_ref() {
                let selected = self.current_page == MergeModalPage::ServiceOrder;
                if controls::pill(ui, status_icon(svc_dup.is_identical), "Service", selected).clicked() {
                    self.current_page = MergeModalPage::ServiceOrder;
                }
            }

            if let Some(cust_dup) = self.check_result.customer.as_ref() {
                let selected = self.current_page == MergeModalPage::Customer;
                if controls::pill(ui, status_icon(cust_dup.is_identical), "Customer", selected).clicked() {
                    self.current_page = MergeModalPage::Customer;
                }
            }

            if let Some(comp_dup) = self.check_result.computer.as_ref() {
                let selected = self.current_page == MergeModalPage::Computer;
                if controls::pill(ui, status_icon(comp_dup.is_identical), "Computer", selected).clicked() {
                    self.current_page = MergeModalPage::Computer;
                }
            }

            ui.with_layout(eframe::egui::Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                let summary = self.current_page == MergeModalPage::Summary;
                if controls::pill(ui, svg::LIST_CHECKS, "Summary", summary).clicked() {
                    self.current_page = MergeModalPage::Summary;
                }

                let preview = self.current_page == MergeModalPage::Preview;
                if controls::pill(ui, svg::CLIPBOARD_TEXT, "Preview", preview).clicked() {
                    self.current_page = MergeModalPage::Preview;
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
                identical_state(ui, "Task records are identical");
                return;
            }
            let user_cache = self.user_cache.clone();
            render_task_field_diff(
                ui,
                &dup.existing,
                &dup.new,
                &mut self.resolution.task_resolution,
                &mut self.resolution.task_fields,
                &user_cache,
            );
        } else {
            controls::empty_state(ui, svg::CLIPBOARD_TEXT, "No duplicate task found.", "", &[]);
        }
    }

    fn render_service_page(&mut self, ui: &mut Ui) {
        if let Some(ref dup) = self.check_result.service_order.clone() {
            if dup.is_identical {
                identical_state(ui, "Service Order records are identical");
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
            controls::empty_state(ui, svg::TICKET, "No duplicate service order found.", "", &[]);
        }
    }

    fn render_customer_page(&mut self, ui: &mut Ui) {
        if let Some(ref dup) = self.check_result.customer.clone() {
            if dup.is_identical {
                identical_state(ui, "Customer records are identical");
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
            controls::empty_state(ui, svg::USER, "No duplicate customer found.", "", &[]);
        }
    }

    fn render_computer_page(&mut self, ui: &mut Ui) {
        if let Some(ref dup) = self.check_result.computer.clone() {
            if dup.is_identical {
                identical_state(ui, "Computer records are identical");
                return;
            }
            render_computer_field_diff(
                ui,
                &dup.existing,
                &dup.new,
                &mut self.resolution.computer_resolution,
                &mut self.resolution.computer_fields,
            );

            ui.add_space(Metrics::get(ui.ctx()).gap.y);
            controls::hairline(ui);
            controls::toggle(ui, &mut self.resolution.add_second_computer, "Add as Second Computer")
                .on_hover_text(
                    "Keep the existing one; save this device as a new computer for the same customer",
                );
            if self.resolution.add_second_computer {
                IconLabel::new(
                    svg::INFO,
                    "The existing computer is preserved. This device is saved as a separate computer record under the same customer (the resolution choice above is ignored).",
                )
                .tone(theme::weak_text(ui))
                .wrap(true)
                .show(ui);
            }
        } else {
            controls::empty_state(ui, svg::DESKTOP_TOWER, "No duplicate computer found.", "", &[]);
        }
    }

    fn render_summary(&mut self, ui: &mut Ui) {
        controls::bar(ui, BarKind::Header, |ui| {
            controls::icon_label(ui, svg::LIST_CHECKS, "Resolution Summary");
        });
        ui.add_space(Metrics::get(ui.ctx()).gap.y);

        let resolution_text = |res: &MergeResolution| -> &str {
            match res {
                MergeResolution::KeepExisting => "Keep Existing",
                MergeResolution::UseNew => "Use New",
                MergeResolution::Merge => "Merge Fields",
                MergeResolution::Cancel => "Cancel",
            }
        };
        let status = |ui: &mut Ui, identical: bool| {
            if identical {
                IconLabel::new(svg::CHECK_CIRCLE, "Identical")
                    .tone(theme::success(ui))
                    .show(ui);
            } else {
                IconLabel::new(svg::WARNING, "Conflict")
                    .tone(theme::warn(ui))
                    .show(ui);
            }
        };
        let row_h = Metrics::get(ui.ctx()).row_h;

        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .column(egui_extras::Column::exact(150.0))
            .column(egui_extras::Column::exact(150.0))
            .column(egui_extras::Column::remainder())
            .header(row_h, |mut header| {
                header.col(|ui| { ui.strong("Entity"); });
                header.col(|ui| { ui.strong("Status"); });
                header.col(|ui| { ui.strong("Resolution"); });
            })
            .body(|mut body| {
                if let Some(ref dup) = self.check_result.task {
                    body.row(row_h, |mut row| {
                        row.col(|ui| { controls::icon_label(ui, svg::CLIPBOARD_TEXT, "Task"); });
                        row.col(|ui| status(ui, dup.is_identical));
                        row.col(|ui| { ui.label(resolution_text(&self.resolution.task_resolution)); });
                    });
                }

                if let Some(ref dup) = self.check_result.service_order {
                    body.row(row_h, |mut row| {
                        row.col(|ui| { controls::icon_label(ui, svg::TICKET, "Service Order"); });
                        row.col(|ui| status(ui, dup.is_identical));
                        row.col(|ui| { ui.label(resolution_text(&self.resolution.service_order_resolution)); });
                    });
                }

                if let Some(ref dup) = self.check_result.customer {
                    body.row(row_h, |mut row| {
                        row.col(|ui| { controls::icon_label(ui, svg::USER, "Customer"); });
                        row.col(|ui| status(ui, dup.is_identical));
                        row.col(|ui| { ui.label(resolution_text(&self.resolution.customer_resolution)); });
                    });
                }

                if let Some(ref dup) = self.check_result.computer {
                    body.row(row_h, |mut row| {
                        row.col(|ui| { controls::icon_label(ui, svg::DESKTOP_TOWER, "Computer"); });
                        row.col(|ui| status(ui, dup.is_identical));
                        row.col(|ui| {
                            if self.resolution.add_second_computer {
                                ui.label("Add as Second Computer");
                            } else {
                                ui.label(resolution_text(&self.resolution.computer_resolution));
                            }
                        });
                    });
                }
            });
    }

    fn render_preview(&mut self, ui: &mut Ui) {
        let m = Metrics::get(ui.ctx());
        controls::bar(ui, BarKind::Header, |ui| {
            controls::icon_label(ui, svg::CLIPBOARD_TEXT, "Preview - Final Result");
        });
        ui.add_space(m.pad.y);
        IconLabel::new(
            svg::INFO,
            "This shows what will be created/updated based on your resolution choices.",
        )
        .tone(theme::weak_text(ui))
        .show(ui);
        ui.add_space(2.0 * m.gap.y);

        // Task Preview
        if let Some(ref dup) = self.check_result.task.clone() {
            let final_task = match self.resolution.task_resolution {
                MergeResolution::KeepExisting => dup.existing.clone(),
                MergeResolution::UseNew => dup.new.clone(),
                MergeResolution::Merge => merge_task(&dup.existing, &dup.new, &self.resolution.task_fields),
                MergeResolution::Cancel => dup.existing.clone(),
            };

            let assignee_display = self.format_assignee(&final_task.assignee);

            preview_section(ui, "merge_preview_task", svg::CLIPBOARD_TEXT, "Task", |ui| {
                render_preview_grid(ui, &[
                    ("Task Name", &final_task.task_name),
                    ("Description", &final_task.task_description),
                    ("Service Number", &final_task.service_number.clone().unwrap_or_default()),
                    ("Assignee", &assignee_display),
                    ("Status", final_task.status.as_str()),
                    ("Priority", final_task.priority.as_str()),
                    ("Completed", &final_task.completed.to_string()),
                ]);
            });
            ui.add_space(m.gap.y);
        }

        // Service Order Preview
        if let Some(ref dup) = self.check_result.service_order.clone() {
            let final_ticket = match self.resolution.service_order_resolution {
                MergeResolution::KeepExisting => dup.existing.clone(),
                MergeResolution::UseNew => dup.new.clone(),
                MergeResolution::Merge => merge_ticket(&dup.existing, &dup.new, &self.resolution.service_order_fields),
                MergeResolution::Cancel => dup.existing.clone(),
            };

            preview_section(ui, "merge_preview_service", svg::TICKET, "Service Order", |ui| {
                render_preview_grid(ui, &[
                    ("Service Number", &final_ticket.service_number),
                    ("Check-in Rep", &final_ticket.checkin_rep),
                    ("Sales Rep", &final_ticket.sales_rep),
                    ("Tech", &final_ticket.tech),
                    ("Salesman", &final_ticket.salesman),
                    ("Terms", &final_ticket.terms),
                    ("Total", &final_ticket.ticket_total),
                    ("Check-in Notes", &final_ticket.checkin_notes),
                ]);
            });
            ui.add_space(m.gap.y);
        }

        // Customer Preview
        if let Some(ref dup) = self.check_result.customer.clone() {
            let final_customer = match self.resolution.customer_resolution {
                MergeResolution::KeepExisting => dup.existing.clone(),
                MergeResolution::UseNew => dup.new.clone(),
                MergeResolution::Merge => merge_customer(&dup.existing, &dup.new, &self.resolution.customer_fields),
                MergeResolution::Cancel => dup.existing.clone(),
            };

            preview_section(ui, "merge_preview_customer", svg::USER, "Customer", |ui| {
                render_preview_grid(ui, &[
                    ("Name", &final_customer.name),
                    ("Email", &final_customer.email),
                    ("Phone", &final_customer.phone_number),
                    ("Phone 2", &final_customer.phone_number_2),
                    ("Customer Code", &final_customer.cust_code),
                ]);
            });
            ui.add_space(m.gap.y);
        }

        // Computer Preview
        if let Some(ref dup) = self.check_result.computer.clone() {
            let final_computer = if self.resolution.add_second_computer {
                dup.new.clone()
            } else {
                match self.resolution.computer_resolution {
                    MergeResolution::KeepExisting => dup.existing.clone(),
                    MergeResolution::UseNew => dup.new.clone(),
                    MergeResolution::Merge => merge_computer(&dup.existing, &dup.new, &self.resolution.computer_fields),
                    MergeResolution::Cancel => dup.existing.clone(),
                }
            };

            let computer_header = if self.resolution.add_second_computer {
                "Computer (new — added as second)"
            } else {
                "Computer"
            };
            preview_section(ui, "merge_preview_computer", svg::DESKTOP_TOWER, computer_header, |ui| {
                render_preview_grid(ui, &[
                    ("Hostname", &final_computer.hostname),
                    ("OS", &final_computer.operating_system),
                    ("CPU", &final_computer.cpu),
                    ("GPU", &final_computer.gpu),
                    ("RAM", &final_computer.ram),
                    ("Motherboard", &final_computer.motherboard_name),
                    ("Product Name", &final_computer.product_name),
                    ("Product Serial", &final_computer.product_serial),
                ]);
            });
        }
    }

    fn render_action_buttons(&mut self, ui: &mut Ui) {
        let clicked = controls::bar(ui, BarKind::Footer, |ui| controls::dialog_buttons(ui, &ACTIONS));
        match clicked {
            Some(0) => self.confirmed = true,
            Some(1) => {
                self.resolution.task_resolution = MergeResolution::UseNew;
                self.resolution.service_order_resolution = MergeResolution::UseNew;
                self.resolution.customer_resolution = MergeResolution::UseNew;
                self.resolution.computer_resolution = MergeResolution::UseNew;
                self.resolution.add_second_computer = false;
            }
            Some(2) => {
                self.resolution.task_resolution = MergeResolution::KeepExisting;
                self.resolution.service_order_resolution = MergeResolution::KeepExisting;
                self.resolution.customer_resolution = MergeResolution::KeepExisting;
                self.resolution.computer_resolution = MergeResolution::KeepExisting;
                self.resolution.add_second_computer = false;
            }
            Some(_) => self.cancelled = true,
            None => {}
        }
    }

    /// Get the final resolution after user confirms
    pub fn get_resolution(&self) -> &DuplicateResolution {
        &self.resolution
    }
}

/// Placeholder shown when both versions of a record match.
fn identical_state(ui: &mut Ui, title: &str) {
    controls::empty_state(ui, svg::SEAL_CHECK, title, "No action needed.", &[]);
}

/// Collapsible preview block, closed by default.
fn preview_section(ui: &mut Ui, id_salt: &str, icon: Icon, title: &str, add: impl FnOnce(&mut Ui)) {
    Section::new(id_salt, title)
        .icon(icon)
        .default_open(false)
        .show(ui, add);
}

/// Section header plus the Keep / Use New / Merge picker every diff page opens with.
fn resolution_header(ui: &mut Ui, icon: Icon, title: &str, resolution: &mut MergeResolution) {
    let m = Metrics::get(ui.ctx());
    controls::bar(ui, BarKind::Header, |ui| {
        controls::icon_label(ui, icon, title);
    });
    ui.add_space(m.gap.y);

    ui.horizontal(|ui| {
        ui.label("Resolution:");
        let choices = [
            (MergeResolution::KeepExisting, svg::ARCHIVE, "Keep Existing"),
            (MergeResolution::UseNew, svg::SWAP, "Use New"),
            (MergeResolution::Merge, svg::GIT_MERGE, "Merge Fields"),
        ];
        for (choice, icon, label) in choices {
            let selected = *resolution == choice;
            if controls::pill(ui, icon, label, selected).clicked() {
                *resolution = choice;
            }
        }
    });

    ui.add_space(m.gap.y);
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
    let t = Tokens::get(ui.ctx());
    let m = Metrics::get(ui.ctx());
    let available_width = 780.;
    // Calculate column width: total width minus arrow (30px), checkbox area (80px if shown), and spacing
    let checkbox_width = if show_merge_option { 90.0 } else { 0.0 };
    let column_width = (available_width - 40.0 - checkbox_width) / 2.0;
    let value_margin = Margin::symmetric(m.gap.x as i8, m.gap.y as i8);

    IconLabel::new(svg::ARROWS_LEFT_RIGHT, field_name)
        .tone(theme::warn(ui))
        .show(ui);

    // Use a grid for proper column alignment
    Grid::new(format!("field_row_{}", field_name))
        .num_columns(if show_merge_option { 4 } else { 3 })
        .spacing([m.gap.x, m.pad.y])
        .show(ui, |ui| {
            // Existing value column
            Frame::default()
                .fill(composite(t.error.gamma_multiply(VALUE_WASH), t.field))
                .corner_radius(t.radius_sm)
                .inner_margin(value_margin)
                .show(ui, |ui| {
                    ui.set_min_width(column_width);
                    ui.set_max_width(column_width);
                    let text = if existing_value.is_empty() { "(empty)" } else { existing_value };
                    ui.add(eframe::egui::Label::new(
                        RichText::new(text).color(t.error)
                    ).wrap());
                });

            svg::ARROW_RIGHT.show(ui, m.icon_md);

            // New value column
            Frame::default()
                .fill(composite(t.success.gamma_multiply(VALUE_WASH), t.field))
                .corner_radius(t.radius_sm)
                .inner_margin(value_margin)
                .show(ui, |ui| {
                    ui.set_min_width(column_width);
                    ui.set_max_width(column_width);
                    let text = if new_value.is_empty() { "(empty)" } else { new_value };
                    ui.add(eframe::egui::Label::new(
                        RichText::new(text).color(t.success)
                    ).wrap());
                });

            if show_merge_option {
                controls::toggle(ui, use_new, "Use New");
            }

            ui.end_row();
        });

    ui.add_space(m.gap.y);
}

/// Render diff for LiveTaskPayload
pub fn render_task_field_diff(
    ui: &mut Ui,
    existing: &LiveTaskPayload,
    new: &LiveTaskPayload,
    resolution: &mut MergeResolution,
    selections: &mut FieldSelections,
    user_cache: &HashMap<String, String>,
) {
    resolution_header(ui, svg::CLIPBOARD_TEXT, "Task Differences", resolution);

    let show_merge = *resolution == MergeResolution::Merge;
    let fields = existing.get_differing_fields(new);

    if fields.is_empty() {
        identical_state(ui, "No differences found.");
        return;
    }

    for (field_name, existing_val, new_val) in fields {
        let mut use_new = selections.use_new(&field_name);

        // Format assignee field with username if available
        let (display_existing, display_new) = if field_name == "assignee" {
            let existing_display = user_cache.get(&existing_val)
                .map(|name| format!("{} ({})", name, existing_val))
                .unwrap_or_else(|| existing_val.clone());
            let new_display = user_cache.get(&new_val)
                .map(|name| format!("{} ({})", name, new_val))
                .unwrap_or_else(|| new_val.clone());
            (existing_display, new_display)
        } else {
            (existing_val.clone(), new_val.clone())
        };

        render_field_row(ui, &field_name, &display_existing, &display_new, &mut use_new, show_merge);
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
    resolution_header(ui, svg::TICKET, "Service Order Differences", resolution);

    let show_merge = *resolution == MergeResolution::Merge;
    let fields = existing.get_differing_fields(new);

    if fields.is_empty() {
        identical_state(ui, "No differences found.");
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
    resolution_header(ui, svg::USER, "Customer Differences", resolution);

    let show_merge = *resolution == MergeResolution::Merge;
    let fields = existing.get_differing_fields(new);

    if fields.is_empty() {
        identical_state(ui, "No differences found.");
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
    resolution_header(ui, svg::DESKTOP_TOWER, "Computer Differences", resolution);

    let show_merge = *resolution == MergeResolution::Merge;
    let fields = existing.get_differing_fields(new);

    if fields.is_empty() {
        identical_state(ui, "No differences found.");
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

/// Helper function to render a preview grid showing field values
fn render_preview_grid(ui: &mut Ui, fields: &[(&str, &str)]) {
    let t = Tokens::get(ui.ctx());
    let m = Metrics::get(ui.ctx());
    Grid::new(ui.next_auto_id())
        .num_columns(2)
        .spacing([3.0 * m.gap.x, m.gap.y])
        .striped(true)
        .show(ui, |ui| {
            for (label, value) in fields {
                ui.colored_label(t.text_dim, *label);
                let display_value = if value.is_empty() { "(empty)" } else { *value };
                ui.add(eframe::egui::Label::new(display_value).wrap());
                ui.end_row();
            }
        });
}
