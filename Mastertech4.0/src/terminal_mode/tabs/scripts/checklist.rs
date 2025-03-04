use std::fmt::Display;

use ratatui::widgets::ListState;

use super::ScriptsTab;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum TodoItemTag {
    #[default]
    Tuneup,
    Qc,
    Informational,
    Custom(String)
}

pub struct TodoList {
    pub name: String,
    pub items: Vec<TodoItem>,
    pub state: ListState,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TodoItem {
    pub text: String,
    pub status: Status,
    pass: String,
    warn: String,
    fail: String,
    error: String,
    tag: TodoItemTag
}

impl TodoItem {
    pub fn new(text: &str, tag: TodoItemTag) -> Self {
        
        Self {
            text: text.to_owned(),
            status: Status::Todo,
            pass: String::new(),
            warn: String::new(),
            fail: String::new(),
            error: String::new(),
            tag
        }
    }

    pub fn set_pass_criteria(mut self, pass: impl Display) -> Self {
        self.pass = pass.to_string();
        self
    }

    pub fn set_warning_criteria(mut self, warn: impl Display) -> Self {
        self.warn = warn.to_string();
        self
    }

    pub fn set_error_criteria(mut self, error: impl Display) -> Self {
        self.error = error.to_string();
        self
    }

    pub fn set_fail_criteria(mut self, fail: impl Display) -> Self {
        self.fail = fail.to_string();
        self
    }

    pub fn get_pass_criteria(&self) -> String {
        self.pass.clone()
    }

    pub fn get_warning_criteria(&self) -> String {
        self.warn.clone()
    }
    
    pub fn get_error_criteria(&self) -> String {
        self.error.clone()
    }

    pub fn get_fail_criteria(&self) -> String {
        self.fail.clone()
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    #[default]
    Todo,
    Completed,
}

impl<'a> ScriptsTab<'a> {
    pub fn update_checklist(&mut self, category: &str, item: &str, status: bool) {
        if let Some(todo_list) = self.checklists.get_mut(category) {
            if let Some(todo_item) = todo_list.items.iter_mut().find(|i| i.text == item) {
                todo_item.status = if status { Status::Completed } else { Status::Todo };
            } else {
                // If the item doesn't exist, add it
                todo_list.items.push(TodoItem {
                    text: item.to_string(),
                    status: if status { Status::Completed } else { Status::Todo },
                    ..Default::default()
                });
            }
        } else {
            // If the category doesn't exist, create a new checklist
            let new_list = TodoList {
                name: category.to_string(),
                items: vec![TodoItem {
                    text: item.to_string(),
                    status: if status { Status::Completed } else { Status::Todo },
                    ..Default::default()
                }],
                state: ListState::default(),
            };
            self.checklists.insert(category.to_string(), new_list);
        }
    }
}
