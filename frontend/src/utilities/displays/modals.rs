use database::{schema::{TaskPayload, TicketPayload}, Database};
use egui::{Align, Grid, Id, Layout, NumExt, RichText, ScrollArea, Ui};
use egui_extras::{Size, StripBuilder};
use log::info;
use serde::Serialize;

#[derive(Serialize, Default, Clone)]
pub enum ModalType{
    CreateTaskModal,
    TaskModal(String),
    #[default]
    Null,
}

#[derive(Default, Serialize)]
pub struct ModalHandler {
    modal: Option<Modal>,
    should_open: bool,
}

/// Response returned by [`Modal::ui`].
pub struct ModalResponse<R> {
    /// What the content closure returned, if it was actually run.
    pub inner: Option<R>,

    /// Whether the modal should remain open.
    pub open: bool,
}

impl ModalType{
    pub fn create_task_modal(&mut self, _ui: &mut Ui){
        info!("Creating a task!!");
    }
    pub fn task_modal(&mut self, ui: &mut Ui, database: Database, task: &TaskPayload, ticket_payload: &TicketPayload){

        task_modal(ui, database, task, ticket_payload);

    }
    pub fn other(&mut self, _ui: &mut Ui){
        info!("No modal...");
    }
}

/// Show a modal window with Rerun style.
///
/// The positioning of the modal is as follows:
///
/// ```text
/// ┌─rerun window─────▲─────────────────────┐
/// │                  │ 75px / 10%          │
/// │          ╔═modal═▼══════════╗  ▲       │
/// │          ║               ▲  ║  │       │
/// │          ║ actual height │  ║  │       │
/// │          ║      based on │  ║  │ max   │
/// │          ║       content │  ║  │ height│
/// │          ║               │  ║  │       │
/// │          ║               ▼  ║  │       │
/// │          ╚══════════════════╝  │       │
/// │          │                  │  │       │
/// │          └───────▲──────────┘  ▼       │
/// │                  │ 75px / 10%          │
/// └──────────────────▼─────────────────────┘
/// ```
///
/// The modal sets the clip rect such as to allow full-span highlighting behavior (e.g. with
/// [`crate::list_item::ListItem`]). Consider using [`crate::ReUi::full_span_separator`] to draw a
/// separator that spans the full width of the modal instead of the usual [`egui::Ui::separator`]
/// method.
///
/// Note that [`Modal`] are typically used via the [`ModalHandler`] helper object to reduce
/// boilerplate.
#[derive(Serialize)]
pub struct Modal {
    title: String,
    min_width: Option<f32>,
    min_height: Option<f32>,
    default_height: Option<f32>,
    full_span_content: bool,
}

fn task_modal(ui: &mut Ui, database: Database, task: &TaskPayload, ticket_payload: &TicketPayload){
    StripBuilder::new(ui)
        .cell_layout(Layout::top_down_justified(Align::Center))
        .size(Size::relative(0.01))
        .size(Size::relative(0.07))
        .size(Size::relative(0.92))
        .vertical(|mut strip| 
    {
        strip
            .strip(|strip| 
        {
            strip
                .size(Size::remainder())
                .horizontal( |mut strip| 
            {
                strip.cell(|ui|{
                    ui.horizontal(|ui|{

                        if ui.selectable_label(false, RichText::new("").heading()).clicked(){
                            
                        };
                        if ui.selectable_label(false, RichText::new("⛨").heading()).clicked(){
                            
                        };
                        if ui.selectable_label(false, RichText::new("").heading()).clicked(){
                            
                        };
                    });
                });
                
            });
        });
        strip.empty();
        strip
            .strip(|strip| 
        {
            strip
                .size(Size::remainder())
                .horizontal( |mut strip| 
            {
                strip.cell(|ui|
                {

                    ScrollArea::both()
                        .id_source("ticketScroll")
                        // .max_height(ui.available_height())
                        .show(ui, |ui| 
                    {
                
                        Grid::new(Id::new(format!("Grid")))// self.id.as_ref().unwrap().0.id.clone()
                            .num_columns(6)
                            .show(ui, |ui| 
                        {
                
                            let customer = ticket_payload.customer.as_ref();
                            let computer = ticket_payload.computer.as_ref();
                            
                            ui.label(format!("created_at: {:?}", ticket_payload.created_at));
                            ui.label(format!("id: {:?}", ticket_payload.id));
                            ui.label(format!("service_task: {:?}", ticket_payload.service_task));
                            ui.label(format!("service_number: {:?}", ticket_payload.service_number));
                            ui.label(format!("checkin_rep: {:?}", ticket_payload.checkin_rep));
                            ui.label(format!("sales_rep: {:?}", ticket_payload.sales_rep));
                            ui.end_row();
                            ui.label(format!("checkin_notes: {:?}", ticket_payload.checkin_notes));
                            ui.label(format!("recommendations: {:?}", ticket_payload.recommendations));
                            ui.label(format!("tech: {:?}", ticket_payload.tech));
                            ui.label(format!("salesman: {:?}", ticket_payload.salesman));
                            ui.label(format!("dep: {:?}", ticket_payload.dep));
                            ui.label(format!("terms: {:?}", ticket_payload.terms));
                            ui.end_row();
                            ui.label(format!("ticket_total: {:?}", ticket_payload.ticket_total));
                            ui.label(format!("doc_alias: {:?}", ticket_payload.doc_alias));
                            ui.label(format!("current_antivirus: {:?}", ticket_payload.current_antivirus));
                            // ui.label(format!("hardware_test_results: {:?}", ticket_payload.hardware_test_results));
                            ui.end_row();
                
                            if let Some(customer) = customer{
                
                                ui.label(format!("part_order_links: {:?}", customer.part_order_links));
                                ui.label(format!("services: {:?}", customer.services));
                                ui.label(format!("cust_code: {:?}", customer.cust_code));
                                ui.label(format!("name: {:?}", customer.name));
                                ui.label(format!("phone_number: {:?}", customer.phone_number));
                                ui.label(format!("phone_number_2: {:?}", customer.phone_number_2));
                                ui.end_row();
                                ui.label(format!("email: {:?}", customer.email));
                                ui.label(format!("li_doc: {:?}", customer.li_doc));
                                ui.label(format!("li_amnt: {:?}", customer.li_amnt));
                                ui.label(format!("num_inv: {:?}", customer.num_inv));
                                ui.end_row();
                            }
                            ui.end_row();
                            if let Some(computer) = computer{
                                let seb_info = computer.seb_info.as_ref();
                                ui.label(format!("hostname: {:?}", computer.hostname));
                                ui.label(format!("operating_system: {:?}", computer.operating_system));
                                ui.label(format!("cpu: {:?}", computer.cpu));
                                ui.label(format!("gpu: {:?}", computer.gpu));
                                ui.label(format!("ram: {:?}", computer.ram));
                                ui.label(format!("drives: {:?}", computer.drives));
                                ui.end_row();
                
                                if let Some(seb_info) = seb_info{
                                    ui.label(format!("InstalledDeviceId: {:?}", seb_info.InstalledDeviceId));
                                    ui.label(format!("InstallInstanceId: {:?}", seb_info.InstallInstanceId));
                                    ui.label(format!("HasIssues: {:?}", seb_info.HasIssues));
                                    ui.label(format!("InstallationStage: {:?}", seb_info.InstallationStage));
                                    ui.label(format!("ReasonCode: {:?}", seb_info.ReasonCode));
                                    ui.label(format!("ActivationCode: {:?}", seb_info.ActivationCode));
                                    ui.end_row();
                                    ui.label(format!("InstallVersion: {:?}", seb_info.InstallVersion));
                                    ui.label(format!("MachineName: {:?}", seb_info.MachineName));
                                    ui.end_row();
                
                                    if let Some(extended_seb) = seb_info.ExtendedSeb.as_ref(){
                                        ui.label(format!("email: {:?}", extended_seb.email));
                                        ui.label(format!("phone: {:?}", extended_seb.phone));
                                        ui.label(format!("userid: {:?}", extended_seb.userid));
                                        ui.label(format!("device_name: {:?}", extended_seb.device_name));
                                        ui.label(format!("device_id: {:?}", extended_seb.device_id));
                                        ui.label(format!("state: {:?}", extended_seb.state));
                                        ui.end_row();
                                        ui.label(format!("usage_gb: {:?}", extended_seb.usage_gb));
                                        ui.label(format!("date_device_created: {:?}", extended_seb.date_device_created));
                                        ui.label(format!("activated: {:?}", extended_seb.activated));
                                        ui.label(format!("activation_code: {:?}", extended_seb.activation_code));
                                        ui.label(format!("last_complete_backup: {:?}", extended_seb.last_complete_backup));
                                        ui.label(format!("last_client_status_update: {:?}", extended_seb.last_client_status_update));
                                        ui.end_row();
                                        ui.label(format!("id_recurly_account: {:?}", extended_seb.id_recurly_account));
                                        ui.label(format!("date_last_scan: {:?}", extended_seb.date_last_scan));
                                        ui.label(format!("date_email_sent: {:?}", extended_seb.date_email_sent));
                                        ui.label(format!("date_canceled_account: {:?}", extended_seb.date_canceled_account));
                                        ui.label(format!("date_deleted_account: {:?}", extended_seb.date_deleted_account));
                                        ui.end_row();
                                        ui.label(format!("current_period_ends_at: {:?}", extended_seb.current_period_ends_at));
                                        ui.label(format!("date_modified: {:?}", extended_seb.date_modified));
                                        ui.label(format!("date_created: {:?}", extended_seb.date_created));
                                        ui.end_row();
                                    }
                                }
                            }
                            
                        });
                    });
                });
            });
        });
    });
}


impl ModalHandler {
    /// Open the model next time the [`ModalHandler::ui`] method is called.
    pub fn open(&mut self) {
        self.should_open = true;
    }

    /// Draw the modal window, creating/destroying it as required.
    pub fn ui<R>(
        &mut self,
        ctx: &egui::Context,
        make_modal: impl FnOnce() -> Modal,
        content_ui: impl FnOnce(&mut egui::Ui, &mut bool) -> R,
    ) -> Option<R> {
        if self.modal.is_none() && self.should_open {
            self.modal = Some(make_modal());
            self.should_open = false;
        }
        if let Some(modal) = &mut self.modal {
            let ModalResponse { inner, open } = modal.ui(ctx, content_ui);
            if !open {
                self.modal = None;
            }

            inner
        } else {
            None
        }
    }
}


impl Modal {
    /// Create a new modal with the given title.
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_owned(),
            min_width: None,
            min_height: None,
            default_height: None,
            full_span_content: false,
        }
    }

    /// Set the minimum width of the modal window.
    #[inline]
    pub fn min_width(mut self, min_width: f32) -> Self {
        self.min_width = Some(min_width);
        self
    }

    /// Set the minimum height of the modal window.
    #[inline]
    pub fn min_height(mut self, min_height: f32) -> Self {
        self.min_height = Some(min_height);
        self
    }

    /// Set the default height of the modal window.
    #[inline]
    pub fn default_height(mut self, default_height: f32) -> Self {
        self.default_height = Some(default_height);
        self
    }

    /// Configure the content area of the modal for full span highlighting.
    ///
    /// This includes:
    /// - setting the vertical spacing to 0.0
    /// - removing any padding at the bottom of the area
    ///
    /// In this mode, the user code is responsible for adding spacing between items.
    #[inline]
    pub fn full_span_content(mut self, full_span_content: bool) -> Self {
        self.full_span_content = full_span_content;
        self
    }

    /// Show the modal window.
    ///
    /// Typically called by [`ModalHandler::ui`].
    pub fn ui<R>(
        &mut self,
        ctx: &egui::Context,
        content_ui: impl FnOnce(&mut egui::Ui, &mut bool) -> R,
    ) -> ModalResponse<R> {
        Self::dim_background(ctx);

        let mut open = ctx.input(|i| !i.key_pressed(egui::Key::Escape));

        let screen_height = ctx.screen_rect().height();
        let modal_vertical_margins = (75.0).at_most(screen_height * 0.1);

        let mut window = egui::Window::new(&self.title)
            .pivot(egui::Align2::CENTER_TOP)
            .fixed_pos(ctx.screen_rect().center_top() + egui::vec2(0.0, modal_vertical_margins))
            .constrain_to(ctx.screen_rect())
            .max_height(screen_height - 2.0 * modal_vertical_margins)
            .collapsible(false)
            .resizable(true)
            .frame(egui::Frame {
                // Note: inner margin are kept to zero so the clip rect is set to the same size as the modal itself,
                // which is needed for the full-span highlighting behavior.
                fill: ctx.style().visuals.panel_fill,
                ..Default::default()
            })
            .title_bar(false);

        if let Some(min_width) = self.min_width {
            window = window.min_width(min_width);
        }

        if let Some(min_height) = self.min_height {
            window = window.min_height(min_height);
        }

        if let Some(default_height) = self.default_height {
            window = window.default_height(default_height);
        }

        let response = window.show(ctx, |ui| {
            let item_spacing_y = ui.spacing().item_spacing.y;
            ui.spacing_mut().item_spacing.y = 0.0;

            egui::Frame {
                inner_margin: egui::Margin::symmetric(10.0, 0.0),
                ..Default::default()
            }
            .show(ui, |ui| {
                ui.add_space(10.0);
                Self::title_bar(ui, &self.title, &mut open);
                // ui.add_space(ReUi::view_padding());
                // crate::ReUi::full_span_separator(ui);
                // we must restore vertical spacing and add view padding at the bottom
                ui.add_space(item_spacing_y);

                egui::Frame {
                    inner_margin: egui::Margin {
                        bottom: 10.0,
                        ..Default::default()
                    },
                    ..Default::default()
                }
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = item_spacing_y;
                    content_ui(ui, &mut open)
                })
                .inner
            })
            .inner
        });

        // Any click outside causes the window to close.
        let cursor_was_over_window = response
            .as_ref()
            .and_then(|response| {
                ctx.input(|i| i.pointer.interact_pos())
                    .map(|interact_pos| response.response.rect.contains(interact_pos))
            })
            .unwrap_or(false);
        if !cursor_was_over_window && ctx.input(|i| i.pointer.any_pressed()) {
            open = false;
        }

        ModalResponse {
            inner: response.and_then(|response| response.inner),
            open,
        }
    }

    /// Dim the background to indicate that the window is modal.
    #[allow(clippy::needless_pass_by_ref_mut)]
    fn dim_background(ctx: &egui::Context) {
        let painter = egui::Painter::new(
            ctx.clone(),
            egui::LayerId::new(egui::Order::PanelResizeLine, egui::Id::new("DimLayer")),
            egui::Rect::EVERYTHING,
        );
        painter.add(egui::Shape::rect_filled(
            ctx.screen_rect(),
            egui::Rounding::ZERO,
            egui::Color32::from_black_alpha(128),
        ));
    }

    /// Display a title bar in our own style.
    fn title_bar(ui: &mut egui::Ui, title: &str, open: &mut bool) {
        ui.horizontal(|ui| {
            ui.strong(title);

            ui.add_space(16.0);

            let mut ui = ui.child_ui(
                ui.max_rect(),
                egui::Layout::right_to_left(egui::Align::Center),
            );
            if ui.button("X")
                .clicked()
            {
                *open = false;
            }
        });
    }
}