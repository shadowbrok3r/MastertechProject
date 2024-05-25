use database::schema::{Priority, Status, TaskPayload};

use super::{get_tasks::{CompletedTasks, MyTasks, StoreTasks}, FilterTasks};


impl FilterTasks for Vec<TaskPayload>{
    type Wrapper = TaskPayload;
    fn filter_by_assignee(self, assignee: &String) -> Vec<Self::Wrapper> {
        self.into_iter()
            .filter(|task| task.assignee_initials.as_deref() == Some(&assignee))
            .collect()
    }

    fn filter_by_completed(self, completed: bool) -> Vec<Self::Wrapper> {
        self.into_iter()
            .filter(|task| task.completed == completed)
            .collect()
    }

    fn filter_by_status(self, status: &Status) -> Vec<Self::Wrapper> {
        self.into_iter()
            .filter(|task| task.status == *status)
            .collect()
    }

    fn filter_by_priority(self, priority: &Priority) -> Vec<Self::Wrapper> {
        self.into_iter()
            .filter(|task| task.priority == *priority)
            .collect()
    }

    fn get_tasks(self) -> Vec<TaskPayload> {
        self.into_iter().collect()
    }
}
pub struct TaskRef<'a, T: 'a>(&'a mut T);

pub struct TaskRefs<'a, T: 'a>(Vec<TaskRef<'a, T>>);

impl<'a, T> From<&'a mut Vec<T>> for TaskRefs<'a, T> {
    fn from(tasks: &'a mut Vec<T>) -> Self {
        TaskRefs(tasks.iter_mut().map(TaskRef).collect())
    }
}
impl<'a, T> FilterTasks for TaskRefs<'a, T>
where
    T: AsMut<TaskPayload>,
{
    type Wrapper = TaskRef<'a, T>;

    fn filter_by_assignee(mut self, assignee: &String) -> Vec<Self::Wrapper> {
        let tasks: Vec<_> = self.0.into_iter().filter(|task| task.0.as_mut().assignee_initials.as_deref() == Some(assignee)).collect();
        tasks
    }

    fn filter_by_completed(mut self, completed: bool) -> Vec<Self::Wrapper> {
        let tasks: Vec<_> = self.0.into_iter().filter(|task| task.0.as_mut().completed == completed).collect();
        tasks
    }

    fn filter_by_status(mut self, status: &Status) -> Vec<Self::Wrapper> {
        let tasks: Vec<_> = self.0.into_iter().filter(|task| task.0.as_mut().status == *status).collect();
        tasks
    }

    fn filter_by_priority(mut self, priority: &Priority) -> Vec<Self::Wrapper> {
        let tasks: Vec<_> = self.0.into_iter().filter(|task| task.0.as_mut().priority == *priority).collect();
        tasks
    }


    fn get_tasks(self) -> Vec<TaskPayload> {
        self.0.into_iter().map(|task| std::mem::take(task.0.as_mut())).collect()
    }
}


impl AsMut<TaskPayload> for MyTasks {
    fn as_mut(&mut self) -> &mut TaskPayload {
        &mut self.0
    }
}

impl AsMut<TaskPayload> for StoreTasks {
    fn as_mut(&mut self) -> &mut TaskPayload {
        &mut self.0
    }
}

impl AsMut<TaskPayload> for CompletedTasks {
    fn as_mut(&mut self) -> &mut TaskPayload {
        &mut self.0
    }
}
