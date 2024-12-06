use crate::{SortDirection, Sortable};
use chrono::{DateTime, Timelike};
use database::schema::{Priority, TaskPayload};

impl Sortable for Vec<TaskPayload> {
    fn default_sort(&mut self) -> &mut Vec<TaskPayload> {
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
            let date_a = get_date_without_time(a);
            let date_b = get_date_without_time(b);

            if date_a < date_b {
                return std::cmp::Ordering::Less;
            } else if date_a > date_b {
                return std::cmp::Ordering::Greater;
            } else {
                let priority_a = priority_mapping(&a.priority);
                let priority_b = priority_mapping(&b.priority);
                return priority_b.cmp(&priority_a);
            }
        });

        self
    }
    fn sort_by_date(&mut self, sort_direction: SortDirection) -> &mut Vec<TaskPayload>{
        self.sort_by(|a: &TaskPayload, b: &TaskPayload| {
            let date_a = get_date_without_time(a);
            let date_b = get_date_without_time(b);
            
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

fn get_date_without_time(task: &TaskPayload) -> chrono::prelude::DateTime<chrono::prelude::Utc> {
    // info!("date: {:?}", &task.due_date);
    let date = DateTime::parse_from_rfc3339(&task.due_date).unwrap();
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

/*
fn get_date_without_time(task: &TaskPayload) -> anyhow::Result<DateTime<Utc>, anyhow::Error> {
    info!("date: {:?}", &task.due_date);
    let date = DateTime::parse_from_rfc3339(&task.due_date)?;
    let final_date = date
        .with_hour(2).unwrap()
        .with_minute(2).unwrap()
        .with_second(2).unwrap()
        .with_nanosecond(3).unwrap().into();
    Ok(final_date)
}
*/

