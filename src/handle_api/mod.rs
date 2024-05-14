#[allow(non_snake_case)]
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub mod api_request;
pub mod scaffold;
pub mod email_builder;

#[derive(Serialize, Deserialize, Debug)]
pub struct AsanaResponse{
    pub gid: Option<String>,
    //pub created_at: Option<String>,
    pub status: Option<usize>,
    //pub raw_resp: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GetTicketResponse {
    pub header: Header,
    pub customer: Customer,
    //pub transactions: Vec<Transactions>,
    pub addresses: Vec<Option<AddressObject>>,
    pub items: Vec<Option<Value>>,
}



#[derive(Deserialize, Debug)]
pub struct Header {
    pub CUST_CODE: Option<String>,
    pub USER_ID: Option<String>, // "USER_ID": "BP3", //checkin rep
    pub TERMS: Option<String>, // "TERMS": "CC",
    pub DOC_ALIAS: Option<String>, // "DOC_ALIAS": "SERVICE ORDER",
    pub DEP: Option<Store>, // "DEP": "LTN"
    pub JURISCODE: Option<String>, //"JURISCODE": "LTN",
    pub COG: Option<String>, // "COG": "7.1000", //Cost of goods?
    pub INV_AMOUNT: Option<String>, // "INV_AMOUNT": "53.6100",
    pub SALES_REP: Option<String>
}

#[derive(Deserialize, Debug)]
pub struct Customer {
    pub NAME: Option<String>, 
    //pub CUSTOMER_ADDRESS: String,
    //"LI_DOC": "53745333",
    pub LI_DOC: Option<String>, 
    // "LI_AMT": "53.6100", //I COULD USE THIS TO CHECK LAST TUNEUP
    pub LI_AMT: Option<String>,  
    // "DW_UPDATE_DATE": "2023-06-27 13:38:50.440",
    pub DW_UPDATE_DATE: Option<String>, 
    // "NUM_INV": "21",
    pub NUM_INV: Option<String>, 
/*		"LP_AMT": "-53.6100",
		"LP_DOC": "52883815",
		"LP_DOC_TYP": "8",
		"LP_DATE": "2023-05-04 00:00:00.000", 
*/

}

#[derive(Deserialize, Debug)]
pub struct AddressObject{
    // Phone Number 1 & 2
    pub TEL1: Option<String>, 
    pub TEL2: Option<String>,
    pub EMAIL: Option<String>
}

#[derive(Deserialize, Debug)]
pub struct Transactions{
/*
    "TRANHIST_DATE": "2023-05-04 14:25:36.000",
    "USER_ID": "KMJ",
    "AMOUNT": "53.6100",
    "PAY_TYPE": "LTNVM",
    "DESCRIPT": "PAYMENT RECEIVED ON SALES ORDER",
 */
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub enum Store{
    None,
    #[default]
    RIV,
    MUR,
    WJ,
    LTN,
    AF,
    SAN,
    ORE,
    SLC,
    SLC1,
    // EXEMPT
}

impl Store {
    pub fn store_email(&self) -> &'static str {
        match *self {
            Store::None => "",
            Store::RIV => "RIV",
            Store::MUR => "pclmur@pclaptops.com",
            Store::WJ => "pclwj@pclaptops.com",
            Store::LTN => "pclltn@pclaptops.com",
            Store::AF => "pclaf@pclaptops.com",
            Store::SAN => "pclsan@pclaptops.com",
            Store::ORE => "pclore@pclaptops.com",
            Store::SLC => "pclmur@pclaptops.com",
            Store::SLC1 => "pclmur@pclaptops.com",
        }
    }
    pub fn as_str(&self) -> &'static str {
        match *self {
            Store::None => "",
            Store::RIV => "RIV",
            Store::MUR => "MUR",
            Store::WJ => "WJ",
            Store::LTN => "LTN",
            Store::AF => "AF",
            Store::SAN => "SAN",
            Store::ORE => "ORE",
            Store::SLC => "SLC",
            Store::SLC1 => "SLC1"
        }  
    }

}