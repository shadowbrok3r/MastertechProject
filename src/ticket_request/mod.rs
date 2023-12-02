use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod request;
pub mod scaffold;

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
    //pub transactions: Transactions,
    pub addresses: Addresses,
    pub items: Vec<Option<Value>>,
}

pub struct GetKeysResponse{
    pub webroot_key: String,
    pub superanti_key: String,
}

#[derive(Deserialize, Debug)]
pub struct Header {
    pub CUST_CODE: Option<String>,
    pub USER_ID: Option<String>, // "USER_ID": "BP3", //checkin rep
    pub TERMS: Option<String>, // "TERMS": "CC",
    pub DOC_ALIAS: Option<String>, // "DOC_ALIAS": "SERVICE ORDER",
    pub DEP: Option<String>, // "DEP": "LTN"
    pub JURISCODE: Option<String>, //"JURISCODE": "LTN",
    pub COG: Option<String>, // "COG": "7.1000", //Cost of goods?
    pub INV_AMOUNT: Option<String>, // "INV_AMOUNT": "53.6100",
}

#[derive(Deserialize, Debug)]
pub struct Customer {
    pub NAME: String, 
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
pub struct Transactions{
    pub TRANSAC_OBJ_ONE: TransacObjectOne,
}

#[derive(Deserialize, Debug)]
pub struct TransacObjectOne{
/*
    "TRANHIST_DATE": "2023-05-04 14:25:36.000",
    "USER_ID": "KMJ",
    "AMOUNT": "53.6100",
    "PAY_TYPE": "LTNVM",
    "DESCRIPT": "PAYMENT RECEIVED ON SALES ORDER",
 */
}

#[derive(Deserialize, Debug)]
pub struct Addresses {
    pub address_object: AddressObject,
/*
    "ACCT_NAME": "Timber Ridge Fireplace LLC",
    "NAME": "Timber Ridge Fireplace LLC",
    "LAST_NAME": "Hale",
    "FIRST_NAME": "Lisa",
    "MOBILE_PHONE": "8013501447",
    "ADDRESS_LINE1": "3080 N Fairfield Rd Suite #1",
 */
}

#[derive(Deserialize, Debug)]
pub struct AddressObject{
    // Phone Number 1 & 2
    pub TEL1: String, 
    pub TEL2: String, 
    pub EMAIL: String,
}