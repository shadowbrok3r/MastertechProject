use chrono::{DateTime, SecondsFormat};
use log::{debug, info};
use tokio::spawn;

use crate::{app_state::MastertechContext, database::send_payload};



impl MastertechContext{
    pub fn submit_tur_mastertech(&mut self) {
        let due_date = Some(
            self.date.unwrap_or(
                DateTime::default()
            ).to_rfc3339_opts(SecondsFormat::Secs,  true)
        );
        let mut task_data = self.task_data.clone();
        let customer_data = self.customer_data.clone();
        let ticket_data = self.ticket_data.clone();
        let computer_data = self.computer_data.clone();
        let task_notes = self.task_notes.clone();
        
        task_data.due_date = due_date.unwrap_or_default();

        match self.database{
            Some(ref database) => {
                let database = database.clone();
                spawn(async move {
                    let x = send_payload(
                        ticket_data,
                        customer_data,
                        computer_data,
                        task_data,
                        task_notes,
                        database
                    ).await;
                    info!("output: {x:?}");
                });
            }, None => debug!("No database connection"),
        };
    }

}