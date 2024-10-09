use crate::modals::{ChatView, CreateTaskModal, TaskModal};
use crossbeam::channel::Sender;
use database::schema::{TaskId, TaskNotePayload, TaskPayload, User};
use eframe::egui::{text::LayoutJob, Color32, FontId, Response, TextFormat, Ui};
use egui_extras::Strip;
use serde::Serialize;
use surrealdb::sql::Id as SurrealId;

pub mod autocomplete;
pub mod carl_dark;
pub mod mention_handler;
pub mod toasts;
pub mod tokyo_dark;

#[derive(Debug, Clone)]
pub enum TaskUiActions {
    OpenTaskModal(TaskPayload),
    CreateTaskModal,
    OpenChatModal((TaskId, Vec<TaskNotePayload>)),
    Response(Response),
    Editing(SurrealId),
    CommitChanges(SurrealId),
    None,
}

// pub type Xy =
#[derive(Serialize, Default, Clone, Debug)]
pub enum ModalType {
    CreateTaskModal(CreateTaskModal),
    TaskModal(TaskModal),
    ChatView(ChatView),
    #[default]
    Null,
}

pub trait Displayable {
    fn display_cards(&mut self, ui: &mut Ui, store_users: &Vec<User>, tx: Sender<TaskUiActions>);
}

pub trait DisplayCards {
    fn display_cards(&mut self, ui: &mut Ui, name: String);
}

pub trait ColumnLayout {
    fn layout_cols(&mut self, ui: &mut Ui);
    fn columns(&mut self, s: &mut Strip);
    fn headers(&mut self, s: Strip);
    // fn card_layout(&mut self, ui: &mut Ui) -> Option<TaskUiActions>;
}

pub trait Interaction {
    // This is correctly implemented
    fn interact_task_name(&mut self, ui: &mut Ui) -> Response; // , task: Rc<RefCell<TaskPayload>>
    fn interact_task_description(&mut self, ui: &mut Ui) -> Response; // , task: Rc<RefCell<TaskPayload>>
    fn interact_checkin_notes(&mut self, ui: &mut Ui) -> Response; // , task: Rc<RefCell<TaskPayload>>
    fn interact_due_date(&mut self, ui: &mut Ui) -> Response; // , task: Rc<RefCell<TaskPayload>>
    fn interact_completed(&mut self, ui: &mut Ui) -> Response; // , task: Rc<RefCell<TaskPayload>>
    fn interact_status(&mut self, ui: &mut Ui) -> Response; // , task: Rc<RefCell<TaskPayload>>
    fn interact_priority(&mut self, ui: &mut Ui) -> Response; // , task: Rc<RefCell<TaskPayload>>
    fn interact_assignee_initials(&mut self, ui: &mut Ui, store_users: &Vec<User>) -> Response; // , task: Rc<RefCell<TaskPayload>>
}

/// Function to color text between two delimiters
pub fn color_between_delimiters(
    layout_job: &mut LayoutJob,
    text: &str,
    delimiters: (&str, &str),
    color: Color32,
) -> String {
    let mut remaining_text = String::from(text);

    while let Some(start_idx) = remaining_text.find(delimiters.0) {
        let after_start = start_idx + delimiters.0.len();
        if let Some(end_idx) = remaining_text[after_start..].find(delimiters.1) {
            let end_idx = after_start + end_idx;

            // Append the text before the first delimiter
            layout_job.append(&remaining_text[..start_idx], 0.0, TextFormat::default());

            // Append the first delimiter itself
            layout_job.append(delimiters.0, 0.0, TextFormat::default());

            // Append the text between the delimiters with the given color
            layout_job.append(
                &remaining_text[after_start..end_idx],
                0.0,
                TextFormat::simple(FontId::default(), color),
            );

            // Append the second delimiter
            layout_job.append(delimiters.1, 0.0, TextFormat::default());

            // Update remaining_text to the part after the second delimiter
            remaining_text = remaining_text[end_idx + delimiters.1.len()..].to_string();
        } else {
            break;
        }
    }

    remaining_text
}

/// Function to color text that matches a specific substring
pub fn color_matching_text(
    layout_job: &mut LayoutJob,
    text: &str,
    pattern: &str,
    color: Color32,
) -> String {
    let mut remaining_text = String::from(text);

    while let Some(start_idx) = remaining_text.find(pattern) {
        let end_idx = start_idx + pattern.len();

        // Append the text before the pattern
        layout_job.append(&remaining_text[..start_idx], 0.0, TextFormat::default());

        // Append the pattern with the given color
        layout_job.append(pattern, 0.0, TextFormat::simple(FontId::default(), color));

        // Update remaining_text to the part after the pattern
        remaining_text = remaining_text[end_idx..].to_string();
    }

    remaining_text
}
