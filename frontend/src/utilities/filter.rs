use database::schema::{Priority, Status, TaskPayload};

use super::FilterTasks;


impl FilterTasks for Vec<TaskPayload>{
    type Wrapper = TaskPayload;
    fn filter_by_assignee(&mut self, assignee: &String) -> Vec<&mut Self::Wrapper> {
        self.iter_mut()
            .filter(|task| task.assignee_initials.as_deref() == Some(&assignee))
            .collect()
    }

    fn filter_by_completed(&mut self, completed: bool) -> Vec<&mut Self::Wrapper> {
        self.iter_mut()
            .filter(|task| task.completed == completed)
            .collect()
    }

    fn filter_by_status(&mut self, status: &Status) -> Vec<&mut Self::Wrapper> {
        self.iter_mut()
            .filter(|task| task.status == *status)
            .collect()
    }

    fn filter_by_priority(&mut self, priority: &Priority) -> Vec<&mut Self::Wrapper> {
        self.iter_mut()
            .filter(|task| task.priority == *priority)
            .collect()
    }

    fn get_tasks(self) -> Vec<TaskPayload> {
        self.into_iter().collect()
    }
}