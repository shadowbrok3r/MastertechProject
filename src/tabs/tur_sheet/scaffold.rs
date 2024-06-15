#[allow(non_snake_case)]
use std::error::Error;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::database::schema::Store;

#[async_trait]
pub trait _SendReq<T>{
    async fn retrieve_data(&self, so_number: &str, client: reqwest::Client) -> Result<T, Box<dyn Error>>;
}


#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub enum HardwareTest{
    RamPass,
    RamFail,
    #[default] 
    RamNotTested,
    HddPass,
    HddFail,
    HddNotTested,
    SsdPass,
    SsdFail,
    SsdNotTested,
}

impl HardwareTest{
    pub fn as_str(&self) -> &'static str {
        match *self {
            HardwareTest::RamPass => "RAM Pass",
            HardwareTest::RamFail => "RAM Fail",
            HardwareTest::HddPass => "HDD Pass",
            HardwareTest::HddFail => "HDD Fail",
            HardwareTest::SsdPass => "SSD Pass",
            HardwareTest::SsdFail => "SSD Fail",
            HardwareTest::RamNotTested => "RAM not tested",
            HardwareTest::HddNotTested => "HDD not tested",
            HardwareTest::SsdNotTested => "SSD not tested",
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[allow(non_snake_case)]
#[serde(rename_all(serialize = "PascalCase", deserialize = "snake_case"))]
pub enum ScaffoldApps{
    Everest,
    SoftwareLicenseFetch,
    CustomerRequestOrder,
}

#[derive(Serialize, Deserialize, Debug)]
#[allow(non_snake_case)]
#[serde(rename_all="snake_case")]
pub enum ScaffoldActions {
    Create,
    Read,
    Update,
    Delete,
    Search,
    GetList,
    GetStatus,
    EverestCall,
    FetchKeys
}


#[derive(Serialize, Deserialize, Debug)]
#[allow(non_snake_case)]
#[serde(rename_all(serialize = "PascalCase", deserialize = "snake_case"))]
pub enum ScaffoldCalls{
    None,
    CheckStock,
    GetOrderDetailSerials,                                    // ( $id_order, $id_item )
    GetOrderDetails,                                          // ( $id_order )
    GetOrderList,                                             // ( $limit, $pcl_only, $override )
    GetOpenSerialsByPaging,                                   // ( $start, $limit )
    GetActiveItems,                                           // ( $limit )
    ItemCodeSearch,                                           // ( $itemCode, $detailed )
    DisplayStock,                                             // ( $data )
    DisplayAutocomplete,                                      // ( $data )
    GetCog,                                                   // ( $order_no )
    GetItemCategory,                                          // ( $item_code )
    GetDocAlias,                                              // ( $order_no )
    GetCogMovementByDate,                                     // ( $date )
    GetItemSellThroughByRepByDateRange,                       // ( $item_code, $sales_rep, $date1, $date2 )
    GetItemSellThroughByMonth,                                // ( $item_code, $date )
    GetItemSellThroughByDateRange,                            // ( $item_code, $date1, $date2 )
    GetComputerServicesByDateRange,                           // ( $date1, $date2 )
    GetOpenServiceOrders,
    GetOpenServiceOrdersWithCallNotes,                        // ( $date )
    CountInvoicePaymentMethodsByDateRange,                    // ( $date1, $date2 )
    GetAllServiceOrdersWithCallNotesByDateRange,              // ( $date1, $date2 )
    GetOpenComputerServiceOrdersWithCallNotes,
    GetInvoicedComputerServiceOrdersWithCallNotesByDateRange, // ( $date1, $date2 )
    GetInvoicedOrdersWithCallNotesByDateRange,                // ( $date1, $date2 )
    GetItemDetailBySerial,                                    // ( $serial )
    GetItemDetailBySerialString,                              // ( $str, $is_numeric )
    GetEmployeeDetailsByName,                                 // ( $data )
    GetSalesInvoicesForLocationByDateRange,                   // ( $location, $date1, $date2 )
    GetSalesOrdersWithSebAhsForLocationsByDateRange,          // ( $location, $date1, $date2 )
    GetCustomerNameByIdOrder,                                 // ( $id_order )
    GetSerialNumbersByDocnum,                                 // ( $id_order )
    GetDocnumBySerialNumber,                                  // ( $id_order )
    GetSerialNumbersByReference,                              // ( $id_order )
    GetSerialNumbersByOrderID,                                // ( $id_order )
    IsOrderValid,                                             // ( $id_order )
    GetNameByOrderId,                                         // ( $id_order )
    CompareOrderCustomer,                                     // ( $id_order, $id_order_2 )
    GetMonthlySales,                                          // ( $date1, $date2, $type )
    GetCustomers,                                             // ( $start, $limit )
    GetCustomer,                                              // ( $cust_code )
    GetAddressByOrderId,                                      // ( $order_id )
    GetTransactionHistory,                                    // ( $id_order )
    GetAddressesByCustomerCode,                               // ( $cust_code, $status )
    GetCustomerByPhone,                                       // ( $data )
    GetOrdersByCustomerId,                                    // ( $data, $limit )
    GetOrder,                                                 // ( $id_order, $full )
    ListFunctions,
}



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
// #[serde(rename_all(serialize = "PascalCase", deserialize = "snake_case"))]
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
