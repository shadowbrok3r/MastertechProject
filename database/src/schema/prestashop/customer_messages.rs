use crate::schema::{helper_traits::EmployeeHelper, prestashop::deserialize_to_string};
use crate::schema::{parse_msg_date, TaskNotePayload, User, TASK_NOTE_TABLE};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

use super::Employee;

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct CustomerMessage {
    #[serde(deserialize_with = "deserialize_to_string")]
    pub id: String,
    pub id_employee: String,
    pub id_customer_thread: String,
    pub message: String,
    pub file_name: String,
    pub private: String,
    pub date_add: String,
}

impl CustomerMessage {
    pub async fn into_task_note(&self, service_number: &str) -> anyhow::Result<TaskNotePayload, anyhow::Error> {
        match Employee::default().get_employee_from_id(&self.id_employee).await {
            Ok(employee) => {
                match User::query_user_or_employee_from_email(employee.email.clone()).await {
                    Ok(user) => { 
                        log::warn!("Pulled user: {}", user.get_name());
                        return Ok(TaskNotePayload {
                            note: self.message.clone(),
                            created_at: if let Ok(date) = DateTime::parse_from_rfc3339(&self.date_add) {
                                date.with_timezone(&Utc).into()
                            } else {
                                parse_msg_date(&self.date_add).unwrap_or(Utc::now().into())
                            },
                            id: crate::schema::RecordId::new(TASK_NOTE_TABLE, self.id.clone()),
                            username: user.get_username().to_string(),
                            user: user.get_id(),
                            id_customer_thread: Some(self.id_customer_thread.clone()),
                            id_customer_message: Some(self.id.clone()),
                            id_employee: Some(self.id_employee.clone()),
                            service_number: Some(service_number.to_string()),
                            task_id: None,
                            private: false,
                        });
                    },
                    Err(e) => Err(anyhow::anyhow!("Error querying user from email: {e:?}")),
                }
            },
            Err(e) => Err(anyhow::anyhow!("Error querying employee via id_employee: {e:?}")),
        }
    }
}