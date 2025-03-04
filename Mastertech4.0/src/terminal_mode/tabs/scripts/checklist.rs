use ratatui::widgets::ListState;

use super::ScriptsTab;



pub struct TodoList {
    pub name: String,
    pub items: Vec<TodoItem>,
    pub state: ListState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoItem {
    pub text: String,
    pub status: Status,
}

impl TodoItem {
    pub fn new(text: &str) -> Self {
        
        Self {
            text: text.to_owned(),
            status: Status::Todo,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
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
                });
            }
        } else {
            // If the category doesn't exist, create a new checklist
            let new_list = TodoList {
                name: category.to_string(),
                items: vec![TodoItem {
                    text: item.to_string(),
                    status: if status { Status::Completed } else { Status::Todo },
                }],
                state: ListState::default(),
            };
            self.checklists.insert(category.to_string(), new_list);
        }
    }
}
