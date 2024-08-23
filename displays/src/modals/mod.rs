
use eframe::egui::{vec2, Align, Align2, Button, Color32, Context, Frame, Id, Key, LayerId, Layout, Margin, NumExt, Order, Painter, Pos2, Rect, Response, RichText, Rounding, Shape, Stroke, Ui, Vec2, Widget, Window};
use database::schema::{CustomerData, Priority, SpecialPartOrder, TaskId, TaskNotePayload, TaskPayload, TicketData, User};
use modal_types::ModalTypes;
use crate::markdown_editor::EasyMarkEditor;
use surrealdb::sql::Id as SurrealId;
use serde::Serialize;
use chrono::NaiveDate;

pub mod modal_types;
// pub mod create_task_modal;
// pub mod task_modal;
// pub mod ai_chat; 


pub trait DisplayModal{
    fn display(&mut self, ui: &mut Ui, current_page_state: ModalAction) -> Option<ModalAction>;
}


#[derive(Debug, Clone)]
pub enum TaskUiActions{
    OpenTaskModal(TaskPayload),
    CreateTaskModal,
    OpenChatModal((TaskId, Vec<TaskNotePayload>)),
    Response(Response),
    Editing(SurrealId),
    CommitChanges(SurrealId),
    None
}

#[derive(Serialize, Default, Clone, Debug)]
pub enum ModalType{
    CreateTaskModal(CreateTaskModal),
    TaskModal(TaskModal),
    ChatView(ChatView),
    #[default]
    Null,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatView{
    pub state: ModalState,
    pub title: String,
    pub messages: Vec<TaskNotePayload>,
    pub current_user: Option<User>,
    pub task_id: Option<TaskId>,
    #[serde(skip)]
    pub markdown_editor: EasyMarkEditor,
}

impl Default for ChatView{
    fn default() -> Self {
        Self { 
            state: ModalState::default(), 
            title: "Chat".to_string(), 
            messages: Vec::new(), 
            current_user: None, 
            markdown_editor: EasyMarkEditor::default(),
            task_id: None
        }
    }
}

#[derive(Serialize, Default, Debug, Clone)]
pub struct CreateTaskModal{
    pub title: String,
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub default_height: Option<f32>,
    pub full_span_content: bool,  
    pub store_users: Vec<User>,

    pub ticket_data: TicketData,
    pub task_data: TaskPayload,
    pub customer_data: CustomerData,
    pub task_notes: TaskNotePayload,

    pub task_name: String,
    pub task_priority: Priority,
    pub due_date: NaiveDate,
    pub description: String,
    pub assignee: Option<User>,
    #[serde(skip)]
    pub state: ModalState
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct TaskModal{
    pub title: String,
    pub task: TaskPayload,
    #[serde(skip)]
    pub chat_view: ChatView,
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub default_height: Option<f32>,
    pub full_span_content: bool,

    pub state: ModalState,
    pub spo: SpecialPartOrder,
}

impl TaskModal{
    pub fn new(chat_view: ChatView, task: TaskPayload) -> Self {
        Self {
            title: "Task Details".to_string(),
            task,
            min_width: Some(600.0),
            min_height: Some(600.0),
            default_height: Some(800.0),
            full_span_content: false,
            state: ModalState::default(),
            chat_view,
            spo: SpecialPartOrder::default()
        }
    }
}

impl ModalTypes for TaskModal{
    fn modal_state(&mut self) -> &mut ModalState {
        &mut self.state
    }
    fn title(mut self, title: String) -> Self {
        self.modal_state().title = Some(title);
        self
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub enum ModalAction{
    TicketInfoPage,
    PartOrderPage,
    ComputerInfoPage,
    TaskNotePage,
    ImportTask,
    Close,
    #[default]
    None
}

#[derive(Default, Serialize)]
pub struct ModalHandler<M: ModalTypes>{
    modal: Option<M>,
    should_open: bool,
    page_state: ModalAction,
}

#[derive(Serialize, Clone, Debug)]
pub struct ModalState {
    pub title: Option<String>,
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub default_height: Option<f32>,
    pub full_span_content: bool,
    #[serde(skip)]
    pub page_state: ModalAction
}

/// Response returned by [`Modal::ui`].
pub struct ModalResponse<R> {
    /// What the content closure returned, if it was actually run.
    pub inner: Option<R>,
    /// Whether the modal should remain open.
    pub open: bool,
    pub page_state: ModalAction,
}

impl Default for ModalState {
    fn default() -> Self {
        Self { title: Some("Create Task".to_string()), min_width: None, min_height: None, default_height: None, full_span_content: false, page_state: ModalAction::default()}
    }
}

impl <M: ModalTypes>ModalHandler<M> {
    /// Open the model next time the [`ModalHandler::ui`] method is called.
    pub fn open(&mut self) {
        self.should_open = true;
    }

    /// Draw the modal window, creating/destroying it as required.
    pub fn ui<R>(
        &mut self,
        ctx: &Context,
        make_modal: impl FnOnce() -> M,
        content_ui: impl FnMut(&mut Ui, &mut bool, &mut ModalAction) -> R,
    ) -> Option<R> {
        if self.modal.is_none() && self.should_open {
            self.modal = Some(make_modal());
            self.should_open = false;
        }
        if let Some(modal) = &mut self.modal {
            let ModalResponse { inner, open , page_state} = modal.ui(ctx, content_ui);
            if !open {
                self.modal = None;
            }
            self.page_state = page_state;

            inner
        } else {
            None
        }
    }
}


pub struct ChatModalResponse<R> {
    /// What the content closure returned, if it was actually run.
    pub inner: Option<R>,
    /// Whether the modal should remain open.
    pub open: bool,
    pub page_state: ModalAction
}
#[derive(Default)]
pub struct ChatModalHandler{
    modal: Option<Modal>,
    should_open: bool,
    #[allow(dead_code)]
    page_state: ModalAction
}

#[derive(Default)]
pub struct TaskModalHandler{
    modal: Option<Modal>,
    should_open: bool,
    #[allow(dead_code)]
    page_state: ModalAction
}

pub struct Modal {
    title: String,
    min_width: Option<f32>,
    min_height: Option<f32>,
    default_height: Option<f32>,
    full_span_content: bool,
    page_state: ModalAction
}

impl TaskModalHandler{
    /// Open the model next time the [`ModalHandler::ui`] method is called.
    pub fn open(&mut self) {
        self.should_open = true;
    }

    /// Draw the modal window, creating/destroying it as required.
    pub fn ui<R>(
        &mut self,
        ctx: &Context,
        make_modal: impl FnOnce() -> Modal,
        content_ui: impl FnMut(&mut Ui, &mut bool, &mut ModalAction) -> R,
    ) -> Option<R> {
        if self.modal.is_none() && self.should_open {
            self.modal = Some(make_modal());
            self.should_open = false;
        }
        if let Some(modal) = &mut self.modal {
            let ChatModalResponse { inner, open, page_state: _ } = modal.ui_modal(ctx, content_ui);
            if !open {
                self.modal = None;
            }

            inner
        } else {
            None
        }
    }
}

impl ChatModalHandler {
    /// Open the model next time the [`ModalHandler::ui`] method is called.
    pub fn open(&mut self) {
        self.should_open = true;
    }

    /// Draw the modal window, creating/destroying it as required.
    pub fn ui<R>(
        &mut self,
        ctx: &Context,
        make_modal: impl FnOnce() -> Modal,
        content_ui: impl FnMut(&mut Ui, &mut bool, &mut ModalAction) -> R,
    ) -> Option<R> {
        if self.modal.is_none() && self.should_open {
            self.modal = Some(make_modal());
            self.should_open = false;
        }
        if let Some(modal) = &mut self.modal {
            let ChatModalResponse { inner, open, page_state: _ } = modal.ui_modal(ctx, content_ui);
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
            page_state: ModalAction::None
        }
    }

    pub fn title(mut self, title: String) -> Self {
        self.title = title;
        self
    }

    /// Set the minimum width of the modal window.
    pub fn min_width(mut self, min_width: f32) -> Self {
        self.min_width = Some(min_width);
        self
    }

    /// Set the minimum height of the modal window.
    pub fn min_height(mut self, min_height: f32) -> Self {
        self.min_height = Some(min_height);
        self
    }

    /// Set the default height of the modal window.
    pub fn default_height(mut self, default_height: f32) -> Self {
        self.default_height = Some(default_height);
        self
    }

    /// Configure the content area of the modal for full span highlighting.
    /// This includes:
    /// - setting the vertical spacing to 0.0
    /// - removing any padding at the bottom of the area
    /// In this mode, the user code is responsible for adding spacing between items.
    pub fn full_span_content(mut self, full_span_content: bool) -> Self {
        self.full_span_content = full_span_content;
        self
    }

    /// Show the modal window.
    /// Typically called by [`ModalHandler::ui`].
    fn ui_modal<R>(&mut self, ctx: &Context, content_ui: impl FnOnce(&mut Ui, &mut bool, &mut ModalAction) -> R) -> ChatModalResponse<R> {

        // Implementation for showing the modal
        Self::dim_background(ctx);

        let mut open = ctx.input(|i| !i.key_pressed(Key::Escape));

        let screen_height = ctx.screen_rect().height();
        let _screen_width = ctx.screen_rect().width();
        let modal_vertical_margins = (75.0).at_most(screen_height * 0.1);

        let mut window = Window::new(self.title.clone())
            .frame(
                Frame::default()
                .inner_margin(Margin::symmetric(0.0, 0.0))
                .outer_margin(Margin::same(30.0))
                .stroke(Stroke::new(2.0, Color32::from_additive_luminance(150)))
                .rounding(Rounding::same(15.0))
            )
            .pivot(Align2::CENTER_TOP)
            .fixed_pos(ctx.screen_rect().center_top() + vec2(0.0, modal_vertical_margins))
            .constrain_to(ctx.screen_rect())
            .max_height(600.0)
            .max_width(680.0)
            .default_width(680.0)
            .collapsible(false)
            .resizable(true)
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

            Frame {
                inner_margin: Margin::same(0.0),
                ..Default::default()
            }
            .show(ui, |ui| {
                Self::title_bar(ui, &self.title, &mut open);
                ui.add_space(item_spacing_y);

                Frame {
                    inner_margin: Margin::same(0.0),
                    ..Default::default()
                }
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = item_spacing_y;
                    content_ui(ui, &mut open, &mut self.page_state)
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

        ChatModalResponse {
            inner: response.and_then(|response| response.inner),
            open,
            page_state: self.page_state.clone()
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
            Color32::from_black_alpha(240),
        ));
    }

    fn title_bar(ui: &mut Ui, title: &str, open: &mut bool) {
        let t: RichText = RichText::new(title).heading().strong();
        Frame::default()
            .fill(Color32::from_rgb(20, 20, 25))
            .rounding(Rounding{nw: 15.0,ne: 15.0,sw: 0.0,se: 0.0})
            .inner_margin(Margin::same(0.0))
            .outer_margin(Margin::same(0.0))
            .show(ui, |ui| 
        {
            ui
            .with_layout(
                Layout::top_down(Align::Max), 
            |ui|{
                if Button::new(" X ").min_size(Vec2::new(15.0, 15.0)).rounding(Rounding::same(f32::INFINITY))
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

