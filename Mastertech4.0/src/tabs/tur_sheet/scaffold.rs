#[allow(non_snake_case)]
use serde::{Deserialize, Serialize};
use async_trait::async_trait;
use std::error::Error;
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
