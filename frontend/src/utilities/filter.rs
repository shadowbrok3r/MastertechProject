use database::schema::{Priority, ReturnedStoreUsers, Status, TaskPayload};
use log::info;

use super::FilterTasks;


impl FilterTasks for Vec<TaskPayload>{
    fn filter_by_assignee(&self, assignees: &String) -> Vec<TaskPayload> {
        self.into_iter()
            .filter(|task| 
                // let mut res = false;
                // for user in assignees{
                    task.assignee_initials.as_deref() == Some(&assignees)
                // }
                // res
            )
            .cloned()
            .collect()
    }

    fn filter_by_completed(&self, completed: bool) -> Vec<TaskPayload> {
        self.into_iter()
            .filter(|task| task.completed == completed)
            .cloned()
            .collect()
    }

    fn filter_by_status(self, status: &Status) -> Vec<TaskPayload> {
        self.into_iter()
            .filter(|task| task.status == *status)
            .collect()
    }

    fn filter_by_priority(&self, priority: &Priority) -> Vec<TaskPayload> {
        self.into_iter()
            .filter(|task| task.priority == *priority)
            .cloned()
            .collect()
    }
}