
use chrono::{DateTime, Utc};

use crate::schema::{LiveTaskPayload, Priority, TaskPayload};

#[derive(Default, PartialEq, Clone, serde::Serialize, Debug, serde::Deserialize)]
pub enum SortDirection{
    #[default]
    Asc,
    Desc
}

pub trait Sortable <T> {
    fn default_sort(&mut self, sort_direction: SortDirection) -> &mut Vec<T>;
    fn sort_by_date(&mut self, sort_direction: SortDirection) -> &mut Vec<T>;
    fn sort_by_name(&mut self, sort_direction: SortDirection) -> &mut Vec<T>;
}


impl Sortable<LiveTaskPayload> for Vec<LiveTaskPayload> {
    fn default_sort(&mut self,  sort_direction: SortDirection) -> &mut Vec<LiveTaskPayload> {
        let priority_mapping = |priority: &Priority| -> i32 {
            match priority {
                Priority::Express => 2,
                Priority::Rfs => 3,
                Priority::Fire => 4,
                Priority::Qc => 1,
                Priority::Normal => 0,
            }
        };

        self.sort_by(|a, b| {
            let date_a: DateTime<Utc> = a.due_date.clone().into();
            let date_b: DateTime<Utc> = b.due_date.clone().into();

            if date_a < date_b {
                match sort_direction {
                    SortDirection::Asc => return std::cmp::Ordering::Less,
                    SortDirection::Desc => return std::cmp::Ordering::Less.reverse(),
                }
            } else if date_a > date_b {
                match sort_direction {
                    SortDirection::Asc => return std::cmp::Ordering::Greater,
                    SortDirection::Desc => return std::cmp::Ordering::Greater.reverse(),
                }
            } else {
                let priority_a = priority_mapping(&a.priority);
                let priority_b = priority_mapping(&b.priority);
                let ordering = priority_b.cmp(&priority_a);
                match sort_direction {
                    SortDirection::Asc => return ordering,
                    SortDirection::Desc => return ordering.reverse(),
                }
            }
        });

        self
    }
    fn sort_by_date(&mut self, sort_direction: SortDirection) -> &mut Vec<LiveTaskPayload>{
        self.sort_by(|a: &LiveTaskPayload, b: &LiveTaskPayload| {
            let date_a: DateTime<Utc> = a.due_date.clone().into();
            let date_b: DateTime<Utc> = b.due_date.clone().into();
            
            let ordering = date_a.cmp(&date_b);
            
            match sort_direction {
                SortDirection::Asc => ordering,               // Use default ordering for ascending
                SortDirection::Desc => ordering.reverse(),    // Reverse ordering for descending
            }
        });
    
        self
    }
    fn sort_by_name(&mut self, sort_direction: SortDirection) -> &mut Vec<LiveTaskPayload> {
        self.sort_by(|a, b| {
            let name_a = &a.task_name.to_lowercase();
            let name_b = &b.task_name.to_lowercase();
            
            let ordering = name_a.cmp(name_b);
    
            match sort_direction {
                SortDirection::Asc => ordering,              // Default alphabetical ordering (A-Z)
                SortDirection::Desc => ordering.reverse(),   // Reverse alphabetical ordering (Z-A)
            }
        });
    
        self
    }
}


impl Sortable<TaskPayload> for Vec<TaskPayload> {
    fn default_sort(&mut self,  sort_direction: SortDirection) -> &mut Vec<TaskPayload> {
        let priority_mapping = |priority: &Priority| -> i32 {
            match priority {
                Priority::Express => 2,
                Priority::Rfs => 3,
                Priority::Fire => 4,
                Priority::Qc => 1,
                Priority::Normal => 0,
            }
        };

        self.sort_by(|a, b| {
            let date_a: DateTime<Utc> = a.due_date.clone().into();
            let date_b: DateTime<Utc> = b.due_date.clone().into();

            if date_a < date_b {
                match sort_direction {
                    SortDirection::Asc => return std::cmp::Ordering::Less,
                    SortDirection::Desc => return std::cmp::Ordering::Less.reverse(),
                }
            } else if date_a > date_b {
                match sort_direction {
                    SortDirection::Asc => return std::cmp::Ordering::Greater,
                    SortDirection::Desc => return std::cmp::Ordering::Greater.reverse(),
                }
            } else {
                let priority_a = priority_mapping(&a.priority);
                let priority_b = priority_mapping(&b.priority);
                let ordering = priority_b.cmp(&priority_a);
                match sort_direction {
                    SortDirection::Asc => return ordering,
                    SortDirection::Desc => return ordering.reverse(),
                }
            }
        });

        self
    }
    fn sort_by_date(&mut self, sort_direction: SortDirection) -> &mut Vec<TaskPayload>{
        self.sort_by(|a: &TaskPayload, b: &TaskPayload| {
            let date_a: DateTime<Utc> = a.due_date.clone().into();
            let date_b: DateTime<Utc> = b.due_date.clone().into();
            
            let ordering = date_a.cmp(&date_b);
            
            match sort_direction {
                SortDirection::Asc => ordering,               // Use default ordering for ascending
                SortDirection::Desc => ordering.reverse(),    // Reverse ordering for descending
            }
        });
    
        self
    }
    fn sort_by_name(&mut self, sort_direction: SortDirection) -> &mut Vec<TaskPayload> {
        self.sort_by(|a, b| {
            let name_a = &a.task_name.to_lowercase();
            let name_b = &b.task_name.to_lowercase();
            
            let ordering = name_a.cmp(name_b);
    
            match sort_direction {
                SortDirection::Asc => ordering,              // Default alphabetical ordering (A-Z)
                SortDirection::Desc => ordering.reverse(),   // Reverse alphabetical ordering (Z-A)
            }
        });
    
        self
    }
}