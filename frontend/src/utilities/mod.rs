use crossbeam::channel::Sender;
use displays::{modals::create_task_modal::CreateTaskModal, modals::{ModalResponse, ModalState}, modals::task_modal::{ModalAction, TaskModal}};
use egui::{vec2, Align, Align2, Button, Color32, Context, Id, LayerId, Layout, Margin, NumExt, Order, Painter, Pos2, Rect, Response, RichText, Rounding, Shape, Ui, Widget, Window};
use database::{schema::{Priority, Status, Store, TaskNotePayload, TaskPayload, User}, Database};
use egui_extras::Strip;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
pub mod displays;
pub mod update_tasks;
pub mod get_tasks;
pub mod interact_tasks;
pub mod get_other;
pub mod task_crud;
pub mod filter;
pub mod handle_live_data;
pub mod sortable;

#[derive(Debug)]
pub enum TaskUiActions{
    OpenTaskModal(TaskPayload),
    CreateTaskModal,
    Response(Response)
}

// pub type Xy = 
#[derive(Serialize, Default, Clone, Debug)]
pub enum ModalType{
    CreateTaskModal(CreateTaskModal),
    TaskModal(TaskModal),
    ChatView(Vec<TaskNotePayload>),
    #[default]
    Null,
}

pub trait Displayable{ 
    fn display_cards(&mut self, ui: &mut Ui, database: Database, store_users: &Vec<User>) -> Option<TaskUiActions>;
}

pub trait DisplayCards{ 
    fn display_cards(&mut self, ui: &mut Ui, name: String);
}

pub trait ColumnLayout{
    fn layout_cols(&mut self, ui: &mut Ui);
    fn columns(&mut self,s: &mut Strip);
    fn headers(&mut self, s: Strip);
    // fn card_layout(&mut self, ui: &mut Ui) -> Option<TaskUiActions>;
}

pub trait Updatable { // This is correctly implemented
    fn update_completed(&self, completed: bool, db: Database);
    fn update_due_date(&self, due_date: String, db: Database);
    fn update_assignee_initials(&self, initials: String, db: Database);
    fn update_task_name(&self, name: String, db: Database);
    fn update_status(&self, status: Status, db: Database);
    fn update_dep(&self, store: Store, db: Database);
    fn update_priority(&self, priority: Option<Priority>, db: Database);
    fn update_task_description(&self, description: Option<String>, db: Database);
    fn update_recommendations(&self, recommendations: Option<String>, db: Database);
    fn update_checkin_notes(&self, checkin_notes: Option<String>, db: Database);
}

pub trait Interaction{ // This is correctly implemented
    fn interact_task_name(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_task_description(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_checkin_notes(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_recommendations(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_due_date(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_completed(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_status(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_dep(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_priority(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_assignee_initials(&mut self, ui: &mut Ui, database: Database, store_users: &Vec<User>) -> Option<Response>;
}

pub trait FilterTasks{ 
    fn filter_by_assignee(&self, assignee: &User) -> Vec<TaskPayload>;
    fn filter_by_completion(&self, completed: bool) -> Vec<TaskPayload>;
    fn filter_by_status(&self, status: &Status) -> Vec<TaskPayload>;
    fn filter_by_priority(&self, priority: &Priority) -> Vec<TaskPayload>;
    fn filter_by_date(&self, date: &String) -> Vec<TaskPayload>;
    /// Filters a list of tasks by their name based on a fuzzy search input.
    /// # Parameters
    /// - `search`: An iterator over items of type `S` where `S` can be referenced as a string slice.
    /// - `search_input`: A string representing the search input to filter tasks by.
    ///
    /// # Returns
    /// A vector of `TaskPayload` containing the filtered tasks.
    fn filter_by_task_name<T: IntoIterator<Item = S>, S: AsRef<str> + std::fmt::Debug>(&self, name: T, search_input: String) -> Vec<TaskPayload>;
}

pub trait Sortable{
    fn sort_task_payloads(&mut self) -> &mut Vec<TaskPayload>;
}

pub trait LiveUpdate{
    fn handle_live_create(self, existing_tasks: &mut Vec<TaskPayload>) -> anyhow::Result<(), anyhow::Error>; // <T: Serialize + for<'a> Deserialize<'a>>
    fn handle_live_update(self, existing_tasks: &mut Vec<TaskPayload>) -> anyhow::Result<(), anyhow::Error>; // <T: Serialize + for<'a> Deserialize<'a>>
    fn handle_live_delete(self, existing_tasks: &mut Vec<TaskPayload>) -> anyhow::Result<(), anyhow::Error>; // <T: Serialize + for<'a> Deserialize<'a>>
}

pub trait Task{ // <T: Serialize + for<'a> Deserialize<'a> + Debug>
    fn get_computer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, db: Database, tx: Sender<Option<T>>);
    fn get_customer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, db: Database, tx: Sender<Option<T>>);
    // fn get_service_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, db: Database, tx: Sender<Option<T>>);
    fn get_task_notes<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, db: Database, tx: Sender<Option<T>>);
    fn get_ticket_payload<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, db: Database, tx: Sender<Option<T>>);
    // fn create_data(&mut self, database: Database, data: T) -> anyhow::Result<Vec<Record>, anyhow::Error>;
    // fn get_data(&mut self, database: Database, data: T)    -> anyhow::Result<Vec<Record>, anyhow::Error>;
    // fn modify_data(&mut self, database: Database, data: T) -> anyhow::Result<Vec<Record>, anyhow::Error>;
    // fn delete_data(&mut self, database: Database, data: T) -> anyhow::Result<Vec<Record>, anyhow::Error>;
}

pub trait DisplayModal{
    fn display(&mut self, ui: &mut Ui, current_page_state: ModalAction) -> Option<ModalAction>;
    // fn set_state(self, action: ModalAction);
}

pub trait ModalTypes: Default{
    fn modal_state(&mut self) -> &mut ModalState;

    fn title(mut self, title: String) -> Self where Self: Sized {
        self.modal_state().title = Some(title);
        self
    }

    /// Set the minimum width of the modal window.
    fn min_width(mut self, min_width: f32) -> Self where Self: Sized {
        self.modal_state().min_width = Some(min_width);
        self
    }

    /// Set the minimum height of the modal window.
    fn min_height(mut self, min_height: f32) -> Self where Self: Sized {
        self.modal_state().min_height = Some(min_height);
        self
    }

    /// Set the default height of the modal window.
    fn default_height(mut self, default_height: f32) -> Self where Self: Sized {
        self.modal_state().default_height = Some(default_height);
        self
    }

    /// Configure the content area of the modal for full span highlighting.
    /// This includes:
    /// - setting the vertical spacing to 0.0
    /// - removing any padding at the bottom of the area
    /// In this mode, the user code is responsible for adding spacing between items.
    fn full_span_content(mut self, full_span_content: bool) -> Self where Self: Sized {
        self.modal_state().full_span_content = full_span_content;
        self
    }

    /// Show the modal window.
    /// Typically called by [`ModalHandler::ui`].
    fn ui<R>(&mut self, ctx: &Context, content_ui: impl FnOnce(&mut Ui, &mut bool, &mut ModalAction) -> R) -> ModalResponse<R> {
        // Implementation for showing the modal
        Self::dim_background(ctx);

        let mut open = ctx.input(|i| !i.key_pressed(egui::Key::Escape));
        // let mut page_state = &;

        let screen_height = ctx.screen_rect().height();
        let _screen_width = ctx.screen_rect().width();
        let modal_vertical_margins = (75.0).at_most(screen_height * 0.1);

        let mut window = Window::new(&*self.modal_state().title.as_ref().unwrap())
            .pivot(Align2::CENTER_TOP)
            .fixed_pos(ctx.screen_rect().center_top() + vec2(0.0, modal_vertical_margins))
            .constrain_to(ctx.screen_rect())
            .max_height(570.0)
            .max_width(650.0)
            .default_width(600.0)
            .collapsible(false)
            .resizable(false)
            .title_bar(false);

        if let Some(min_width) = self.modal_state().min_width {
            window = window.min_width(min_width);
        }

        if let Some(min_height) = self.modal_state().min_height {
            window = window.min_height(min_height);
        }

        if let Some(default_height) = self.modal_state().default_height {
            window = window.default_height(default_height);
        }

        let response = window.show(ctx, |ui| {
            
            let item_spacing_y = ui.spacing().item_spacing.y;
            ui.spacing_mut().item_spacing.y = 0.0;

            egui::Frame {
                inner_margin: egui::Margin::symmetric(5.0, 0.0),
                ..Default::default()
            }
            .show(ui, |ui| {
                Self::title_bar(ui, &self.modal_state().title.as_ref().unwrap_or(&"Modal".to_string()), &mut open);
                ui.add_space(item_spacing_y);

                egui::Frame {
                    inner_margin: egui::Margin {
                        top: 0.0,
                        bottom: 10.0,
                        ..Default::default()
                    },
                    ..Default::default()
                }
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = item_spacing_y;
                    content_ui(ui, &mut open, &mut self.modal_state().page_state)
                })
                .inner
            })
            .inner
        });

        let cursor_was_over_window = response
            .as_ref()
            .and_then(|response| {
                ctx.input(|i| i.pointer.interact_pos())
                    .map(|interact_pos| {
                        let pos_x = interact_pos.x;
                        let pos_y = interact_pos.y;
                        let final_pos = Pos2::new(pos_x - 10.0, pos_y - 10.0);
                        response.response.rect.contains(final_pos)
                    })
            })
            .unwrap_or(false);
        if !cursor_was_over_window && ctx.input(|i| i.pointer.any_pressed()) {
            open = false;
        }

        ModalResponse {
            inner: response.and_then(|response| response.inner),
            open,
            page_state: self.modal_state().page_state.clone()
        }
    }

    fn dim_background(ctx: &Context) {
        let painter = Painter::new(
            ctx.clone(),
            LayerId::new(Order::PanelResizeLine, Id::new("DimLayer")),
            Rect::EVERYTHING,
        );
        painter.add(Shape::rect_filled(
            ctx.screen_rect(),
            Rounding::ZERO,
            Color32::from_black_alpha(210),
        ));
    }

    fn title_bar(ui: &mut Ui, title: &str, open: &mut bool) {
        let t: RichText = RichText::new(title).heading().strong();
        egui::Frame::default()
            .fill(Color32::from_rgb(20, 20, 25))
            .rounding(Rounding{nw: 15.0,ne: 15.0,sw: 0.0,se: 0.0})
            .inner_margin(Margin::same(7.0))
            .outer_margin(Margin::same(0.0))
            .show(ui, |ui| 
        {
            ui
            .with_layout(
                Layout::top_down(Align::Max), 
            |ui|{
                if Button::new(" X ").rounding(Rounding::same(10.0))
                .fill(Color32::BLACK)
                    .ui(ui)
                    .clicked(){
                        *open = false;
                    }
            });

            ui
            .with_layout(
                Layout::top_down(Align::Center), 
            |ui|ui.heading(t));
        });
        ui.separator();
    }
}

