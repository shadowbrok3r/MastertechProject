use database::schema::{ConnectedClient, Priority, TaskPayload};
use chrono::{DateTime, Timelike, Utc};
use crate::{SortDirection, Sortable};

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

impl Sortable<ConnectedClient> for Vec<ConnectedClient> {
    fn default_sort(&mut self,  sort_direction: SortDirection) -> &mut Vec<ConnectedClient> {
        self.sort_by_date(sort_direction)
    }

    fn sort_by_date(&mut self, sort_direction: SortDirection) -> &mut Vec<ConnectedClient> {
        self.sort_by(|a: &ConnectedClient, b: &ConnectedClient| {
            let date_a = &a.last_update.as_ref().cloned().unwrap_or_default();
            let date_b = &b.last_update.as_ref().cloned().unwrap_or_default();
            
            let ordering = date_b.cmp(&date_a);
            
            match sort_direction {
                SortDirection::Asc => ordering,               // Use default ordering for ascending
                SortDirection::Desc => ordering.reverse(),    // Reverse ordering for descending
            }
        });
    
        self
    }

    fn sort_by_name(&mut self, sort_direction: SortDirection) -> &mut Vec<ConnectedClient> {
        self.sort_by(|a, b| {
            let name_a = &a.connection_string.to_lowercase();
            let name_b = &b.connection_string.to_lowercase();
            
            let ordering = name_a.cmp(name_b);
    
            match sort_direction {
                SortDirection::Asc => ordering,              // Default alphabetical ordering (A-Z)
                SortDirection::Desc => ordering.reverse(),   // Reverse alphabetical ordering (Z-A)
            }
        });
    
        self
    }
}

pub fn get_date_without_time(date_string: &String) -> chrono::prelude::DateTime<chrono::prelude::Utc> {
    // info!("date: {:?}", &task.due_date);
    let date = DateTime::parse_from_rfc3339(date_string).unwrap();
    date.with_hour(2)
        .unwrap()
        .with_minute(2)
        .unwrap()
        .with_second(2)
        .unwrap()
        .with_nanosecond(3)
        .unwrap()
        .into()
}

