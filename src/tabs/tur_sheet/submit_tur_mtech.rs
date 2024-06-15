use chrono::{DateTime, SecondsFormat};
use log::{debug, info};
use tokio::spawn;

use crate::{app_state::MastertechContext, database::{schema::HardwareTests, send_payload, PreTicketData}};



impl MastertechContext{
    pub fn submit_tur_mastertech(&mut self) {
                                                
        let hdd_test = format!("{:?}", &self.hdd_test_cbox);
        let ram_test = format!("{:?}", &self.ram_test_cbox);
        let ssd_test = format!("{:?}", &self.ssd_test_cbox);

        let mut pre_ticket: PreTicketData = self.ticket_info.clone();

        pre_ticket.due_date = Some(
            self.date.unwrap_or(
                DateTime::default()
            ).to_rfc3339_opts(SecondsFormat::Secs,  true)
        );
        
        // let payload = TicketPayload::serialize_payload(
        //     &pre_ticket,
        //     &self.system_info,
        //     &self.so_number,
        //     &self.current_antivirus,
        //     &self.recommendations,
        //     self.technician.clone(),
        //     self.salesman.clone(), 
        //     HardwareTests{
        //         hdd_test,
        //         ssd_test,
        //         ram_test,
        //     } // example
        // );
        let system_info = self.system_info.clone();
        let so_number = self.so_number.clone();
        let current_antivirus = self.current_antivirus.clone();
        let recommendations = self.recommendations.clone();
        let technician = self.technician.clone();
        let salesman = self.salesman.clone();
        let hw_tests = HardwareTests{
            hdd_test,
            ssd_test,
            ram_test,
        };
        
        match self.database{
            Some(ref database) => {
                debug!("Sending reqwest");
                let database = database.clone();
                spawn(async move {
                    let x = send_payload(
                        pre_ticket.clone(), 
                        system_info,
                        so_number,
                        current_antivirus,
                        recommendations,
                        technician,
                        salesman,
                        hw_tests,
                        database
                    ).await;
                    info!("output: {:?}", x);
                });
            }, None => debug!("No database connection"),
        };
    }

}