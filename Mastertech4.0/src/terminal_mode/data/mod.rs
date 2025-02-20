use database::schema::{prestashop_schema, utilities::get_prestashop_payload, ComputerData, CustomerData, LiveTaskPayload, TaskNotePayload, TicketData};
use crossbeam::channel::{Receiver, Sender};

pub struct ServiceData {
    pub task_data: LiveTaskPayload,
    pub ticket_data: TicketData,
    pub customer_data: CustomerData,
    pub computer_data: ComputerData,
    pub task_notes: Vec<TaskNotePayload>,
    pub prestashop_api_tx: Sender<prestashop_schema::PrestashopPayload>,
    prestashop_api_rx: Receiver<prestashop_schema::PrestashopPayload>,
}

impl Default for ServiceData {
    fn default() -> Self {
        let (prestashop_api_tx, prestashop_api_rx) = crossbeam::channel::unbounded();
        Self {
            task_data: Default::default(),
            ticket_data: Default::default(),
            customer_data: Default::default(),
            computer_data: Default::default(),
            task_notes: Default::default(),

            prestashop_api_tx,
            prestashop_api_rx,
        }
    }
}

impl ServiceData {
    pub fn receive_ticket(&self) -> anyhow::Result<(), anyhow::Error> {
        if let Ok(data) = self.prestashop_api_rx.try_recv() {
            log::info!("{:?}", &serde_json::to_string(&data)?);
            log::info!("{:?}", serde_json::to_value(&data)?);
        }
        Ok(())
    }
    
    pub fn get_ticket(&self, input: &str) {
        let service_num = self.ticket_data.service_number.clone();
        let input = self.ticket_data.service_number.clone();
        let tx = self.prestashop_api_tx.clone();
        if !input.is_empty() {
            tokio::spawn(async move {

                let prestashop_order = get_prestashop_payload(&input).await?;
                tx.try_send(prestashop_order)?;
                Ok::<(), anyhow::Error>(())
            });
        }
    }

    // fn test_fn<T, R>(&mut self, f: impl FnMut(&mut T) -> R) {
    //     // f(|t: &mut T| {});
    // }

    // fn another(&mut self) {
    //     let x = self.test_fn::<ServiceData, bool>(|x| {

    //         true
    //     });
    // }
}