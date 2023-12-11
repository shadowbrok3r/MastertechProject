use serde::{Serialize, Deserialize};
use crate::{filesystem::system_info::DiskData, ticket_request::Store};

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct TicketData{
    pub cust_code: String,
    pub user_id: String, // "USER_ID": "BP3", //checkin rep
    pub terms: String, // "TERMS": "CC",
    pub doc_alias: String, // "DOC_ALIAS": "SERVICE ORDER",
    pub department: String, // "DEP": "LTN"
    pub jurisdiction: Store, //"JURISCODE": "LTN",
    pub invoice_amnt: String,

    pub customer_name: String, // "NAME": "Timber Ridge Fireplace LLC",
    pub customer_phone_1: String,
    pub customer_phone_2: String,
    pub customer_email: String,
    pub last_invoice_number: String, // "LI_DOC": "53745333",
    pub last_invoice_amount: String,  // "LI_AMT": "53.6100", //I COULD USE THIS TO CHECK LAST TUNEUP
    //last_tuneup_date: String, // <-- HERE
    //last_checkin_date: String, // "DW_UPDATE_DATE": "2023-06-27 13:38:50.440",
    pub total_invoice_count: String,

    pub checkin_notes: String,
    pub item_codes: String,

    // pub ssd_test_cbox: scaffold::HardwareTest,
    // pub antivirus_installed: String,
    // pub service_number: i32,
    // pub checkin_rep: String,
    // pub recommendations: String,
    // pub tech: String,
    // pub salesman: String,
    // pub dep: String, // Store
    // pub terms: String,
    // pub ticket_total: String,
    // pub doc_alias: String,
}

#[derive(Serialize, Deserialize)]
pub struct PulledKeys{
    pub webroot_key: String,
    pub superanti_key: String,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct ComputerData{
    pub hostname: String,
    // pub operating_system: String,
    pub cpu: String,
    pub gpu: Option<String>,
    pub ram: String,
    pub drives: DiskData,
}


#[derive(Serialize, Deserialize, Debug)]
pub struct TicketResponse{
    pub ticket_data: TicketData,
    pub customer_data: CustomerData,
    pub computer_data: ComputerData
}


// #[derive(Serialize, Deserialize, Debug)]
// pub struct TicketData{
//     pub service_number: i32,
//     pub checkin_rep: String,
//     pub checkin_notes: String,
//     pub recommendations: String,
//     pub tech: String,
//     pub salesman: String,
//     pub dep: String, // Store
//     pub terms: String,
//     pub ticket_total: String,
//     pub doc_alias: String,
// }

#[derive(Serialize, Deserialize, Debug)]
pub struct CustomerData{
    pub cust_code: i32,
    pub name: String,
    pub phone_number: String,
    pub phone_number_2: String,
    pub email: String, 
    pub address: String,
    pub li_doc: i32,
    pub li_amnt: String,
    pub num_inv: i32,
}


#[derive(Serialize, Deserialize, Debug)]
pub struct DriveData{
    pub drive_type: String,
    pub total_size: String,
    pub space_left: String,

}


impl TicketResponse{
    pub fn serialize_payload(&mut self) -> Self{
        self.ticket_data = TicketData{
            cust_code: todo!(),
            user_id: todo!(), // "USER_ID": "BP3", //checkin rep
            terms: todo!(), // "TERMS": "CC",
            doc_alias: todo!(), // "DOC_ALIAS": "SERVICE ORDER",
            department: todo!(), // "DEP": "LTN"
            jurisdiction: todo!(), //"JURISCODE": "LTN",
            invoice_amnt: todo!(),
            customer_name: todo!(), // "NAME": "Timber Ridge Fireplace LLC",
            customer_phone_1: todo!(),
            customer_phone_2: todo!(),
            customer_email: todo!(),
            last_invoice_number: todo!(), // "LI_DOC": "53745333",
            last_invoice_amount: todo!(),  // "LI_AMT": "53.6100", //I COULD USE THIS TO CHECK LAST TUNEUP
            total_invoice_count: todo!(),
            checkin_notes: todo!(),
            item_codes: todo!(),
        };

        self.computer_data = ComputerData{
            hostname: todo!(),
            // operating_system: todo!(),
            cpu: todo!(),
            gpu: todo!(),
            ram: todo!(),
            drives: todo!(),
        };

        self.customer_data = CustomerData{
            cust_code: todo!(),
            name: todo!(),
            phone_number: todo!(),
            phone_number_2: todo!(),
            email: todo!(),
            address: todo!(),
            li_doc: todo!(),
            li_amnt: todo!(),
            num_inv: todo!(),
        };

        TicketResponse { 
            ticket_data: self.ticket_data, 
            customer_data: self.customer_data, 
            computer_data: self.computer_data 
        }

    }
}