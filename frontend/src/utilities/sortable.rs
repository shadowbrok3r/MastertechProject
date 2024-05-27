use chrono::{DateTime, Timelike};
use database::schema::{Priority, TaskPayload};
use log::info;

use super::Sortable;

impl Sortable for Vec<TaskPayload>{
    fn sort_task_payloads(&mut self) -> &mut Vec<TaskPayload> {
        let priority_mapping = |priority: &Priority| -> i32 {
            match priority {
                Priority::Express => 2,
                Priority::Rfs => 3,
                Priority::CustomerFire => 4,
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
}


fn get_date_without_time(task: &TaskPayload) -> chrono::prelude::DateTime<chrono::prelude::Utc> {
    // info!("date: {:?}", &task.due_date);
    let date = DateTime::parse_from_rfc3339(&task.due_date).unwrap();
    date.with_hour(2).unwrap().with_minute(2).unwrap().with_second(2).unwrap().with_nanosecond(3).unwrap().into()
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