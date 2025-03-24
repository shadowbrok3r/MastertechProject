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

#[derive(Debug, Default, Clone, PartialEq, Eq)]
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
    tag: TodoItemTag,
    category: Category, // Changed from tag to category
}

impl TodoItem {
    pub fn new(text: &str,category: Category) -> Self {
        // let tag = if let Some(tag) = tag { tag } else { TodoItemTag::default() };
        Self {
            text: text.to_owned(),
            status: Status::Todo,
            pass: String::new(),
            warn: String::new(),
            fail: String::new(),
            error: String::new(),
            tag: TodoItemTag::default(),
            category
        }
    }

    pub fn set_status(mut self, status: Status) -> Self {
        self.status = status;
        self
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

    pub fn category(&self) -> Category {
        self.category.clone()
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    #[default]
    Todo,
    Completed,
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub enum Category {
    #[default]
    Tuneup,
    Qc,
    Informational,
    JunkwareRemoval, // For "Junkware Removal" checklist
    WindowsUpdates,
    RunPrechecks,
    UserScripts(String), // For flexibility
}

impl<'a> ScriptsTab<'a> {
    pub fn update_checklist(&mut self, category: Category, item: &str, status: bool) {
        let category_str = match category {
            Category::Tuneup => "Tuneup",
            Category::Qc => "QC",
            Category::Informational => "Informational",
            Category::JunkwareRemoval => "Junkware Removal",
            Category::WindowsUpdates => "WindowsUpdates",
            Category::RunPrechecks => "RunPrechecks",
            Category::UserScripts(ref name) => name,
        };

        if let Some(todo_list) = self.checklists.get_mut(category_str) {
            if let Some(todo_item) = todo_list.items.iter_mut().find(|i| i.text == item) {
                todo_item.status = if status { Status::Completed } else { Status::Todo };
            } else {
                todo_list.items.push(TodoItem::new(item, category.clone())
                    .set_status(if status { Status::Completed } else { Status::Todo }));
            }
        } else {
            let new_list = TodoList {
                name: category_str.to_string(),
                items: vec![TodoItem::new(item, category.clone())
                    .set_status(if status { Status::Completed } else { Status::Todo })],
                state: ListState::default(),
            };
            self.checklists.insert(category_str.to_string(), new_list);
        }
    }
}
