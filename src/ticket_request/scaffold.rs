use log::debug;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use dotenv::var;

use super::Store;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Salesman {
    Jake,
    Danny
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Techs{
    Logan,
    Bread,
    Taco
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



pub struct ScaffoldRequestBuilder{
    pub call: Option<ScaffoldCalls>,
    pub action: ScaffoldActions,
    pub app: ScaffoldApps,
    pub arguments: Option<Vec<Value>>,
}

#[derive(Serialize, Deserialize, Debug)]
#[allow(non_snake_case)]
#[serde(rename_all(serialize = "PascalCase", deserialize = "snake_case"))]
pub enum ScaffoldApps{
    Everest,
    SoftwareLicenseFetch,
    CustomerRequestOrder,
}

// impl ScaffoldApps{
//     fn as_str(&self) -> &'static str {
//         match *self {
//             ScaffoldApps::Everest => "everest",
//             ScaffoldApps::SoftwareLicenseFetch => "software_license_fetch",
//             ScaffoldApps::CustomerRequestOrder => "customer_request_order",
//         }
//     }
// }

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

// impl ScaffoldActions {
//     fn as_str(&self) -> &'static str {
//         match *self {
//             ScaffoldActions::Create => "create",
//             ScaffoldActions::Read => "read",
//             ScaffoldActions::Update => "update",
//             ScaffoldActions::Delete => "delete",
//             ScaffoldActions::Search => "search",
//             ScaffoldActions::GetList => "get_list",
//             ScaffoldActions::GetStatus => "get_status",
//             ScaffoldActions::EverestCall => "everest_call",
//             ScaffoldActions::FetchKeys => "fetch_keys",
//         }
//     }
// }

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

// impl ScaffoldCalls {
//     fn as_str(&self) -> &'static str {
//         match *self {
//             ScaffoldCalls::None => "",
//             ScaffoldCalls::CheckStock => "check_stock",
//             ScaffoldCalls::GetOrderDetailSerials => "get_order_detail_serials",
//             ScaffoldCalls::GetOrderDetails => "get_order_details",
//             ScaffoldCalls::GetOrderList => "get_order_list",
//             ScaffoldCalls::GetOpenSerialsByPaging => "get_open_serials_by_paging",
//             ScaffoldCalls::GetActiveItems => "get_active_items",
//             ScaffoldCalls::ItemCodeSearch => "item_code_search",
//             ScaffoldCalls::DisplayStock => "display_stock",
//             ScaffoldCalls::DisplayAutocomplete => "display_autocomplete",
//             ScaffoldCalls::GetCog => "get_cog",
//             ScaffoldCalls::GetItemCategory => "get_item_category",
//             ScaffoldCalls::GetDocAlias => "get_doc_alias",
//             ScaffoldCalls::GetCogMovementByDate => "get_cog_movement_by_date",
//             ScaffoldCalls::GetItemSellThroughByRepByDateRange => "get_item_sell_through_by_rep_by_date_range",
//             ScaffoldCalls::GetItemSellThroughByMonth => "get_item_sell_through_by_month",
//             ScaffoldCalls::GetItemSellThroughByDateRange => "get_item_sell_through_by_date_range",
//             ScaffoldCalls::GetComputerServicesByDateRange => "get_computer_services_by_date_range",
//             ScaffoldCalls::GetOpenServiceOrders => "get_open_service_orders",
//             ScaffoldCalls::GetOpenServiceOrdersWithCallNotes => "get_open_service_orders_with_call_notes",
//             ScaffoldCalls::CountInvoicePaymentMethodsByDateRange => "count_invoice_payment_methods_by_date_range",
//             ScaffoldCalls::GetAllServiceOrdersWithCallNotesByDateRange => "get_all_service_orders_with_call_notes_by_date_range",
//             ScaffoldCalls::GetOpenComputerServiceOrdersWithCallNotes => "get_open_computer_service_orders_with_call_notes",
//             ScaffoldCalls::GetInvoicedComputerServiceOrdersWithCallNotesByDateRange => "get_invoiced_computer_service_orders_with_call_notes_by_date_range",
//             ScaffoldCalls::GetInvoicedOrdersWithCallNotesByDateRange => "get_invoiced_orders_with_call_notes_by_date_range",
//             ScaffoldCalls::GetItemDetailBySerial => "get_item_detail_by_serial",
//             ScaffoldCalls::GetItemDetailBySerialString => "get_item_detail_by_serial_string",
//             ScaffoldCalls::GetEmployeeDetailsByName => "get_employee_details_by_name",
//             ScaffoldCalls::GetSalesInvoicesForLocationByDateRange => "get_sales_invoices_for_location_by_date_range",
//             ScaffoldCalls::GetSalesOrdersWithSebAhsForLocationsByDateRange => "get_sales_orders_with_seb_ahs_for_locations_by_date_range",
//             ScaffoldCalls::GetCustomerNameByIdOrder => "get_customer_name_by_id_order",
//             ScaffoldCalls::GetSerialNumbersByDocnum => "get_serial_numbers_by_docnum",
//             ScaffoldCalls::GetDocnumBySerialNumber => "get_docnum_by_serial_number",
//             ScaffoldCalls::GetSerialNumbersByReference => "get_serial_numbers_by_reference",
//             ScaffoldCalls::GetSerialNumbersByOrderID => "get_serial_numbers_by_order_id",
//             ScaffoldCalls::IsOrderValid => "is_order_valid",
//             ScaffoldCalls::GetNameByOrderId => "get_name_by_order_id",
//             ScaffoldCalls::CompareOrderCustomer => "compare_order_customer",
//             ScaffoldCalls::GetMonthlySales => "get_monthly_sales",
//             ScaffoldCalls::GetCustomers => "get_customers",
//             ScaffoldCalls::GetCustomer => "get_customer",
//             ScaffoldCalls::GetAddressByOrderId => "get_address_by_order_id",
//             ScaffoldCalls::GetTransactionHistory => "get_transaction_history",
//             ScaffoldCalls::GetAddressesByCustomerCode => "get_addresses_by_customer_code",
//             ScaffoldCalls::GetCustomerByPhone => "get_customer_by_phone",
//             ScaffoldCalls::GetOrdersByCustomerId => "get_orders_by_customer_id",
//             ScaffoldCalls::GetOrder => "getOrder",
//             ScaffoldCalls::ListFunctions => "list_functions",
//         }
//     }
// }

impl ScaffoldRequestBuilder {
    pub fn build_scaffold_call(&mut self) -> Value {
        debug!("build_scaffold_call");
        let company = "pcl".to_string();
        // dotenv::var("SCAFFOLD_USER").unwrap()

        let mut scaffold_call = serde_json::json!({
            "user_email": "logan.lees@pclaptops.com", 
            "user_password": "Poolparty1",
            "action": self.action,
            //"call": self.call.to_string(), 
            "application": self.app, 
            "company": company.to_string()
        });

        if let Some(args_vec) = &self.arguments {
            match (args_vec.get(0), args_vec.get(1), args_vec.get(2)) {
                (Some(arg1), Some(arg2), Some(arg3)) => {
                    scaffold_call.as_object_mut().unwrap().insert("arg1".to_string(), arg1.clone());
                    scaffold_call.as_object_mut().unwrap().insert("arg2".to_string(), arg2.clone());
                    scaffold_call.as_object_mut().unwrap().insert("arg3".to_string(), arg3.clone());
                },
                (Some(arg1), Some(arg2), None) => {
                    if let Some(call) = &self.call{
                        let call_value = serde_json::to_string(call).unwrap();
                        scaffold_call.as_object_mut().unwrap().insert("call".to_string(), Value::String(call_value));
                        scaffold_call.as_object_mut().unwrap().insert("arg1".to_string(), arg1.clone());
                        scaffold_call.as_object_mut().unwrap().insert("arg2".to_string(), arg2.clone());
                    }
                    scaffold_call.as_object_mut().unwrap().insert("arg1".to_string(), arg1.clone());
                    scaffold_call.as_object_mut().unwrap().insert("arg2".to_string(), arg2.clone());
                },
                (Some(arg1), None, None) => {
                    match &self.call{
                        Some(_) => {
                            scaffold_call.as_object_mut().unwrap().insert("id_order".to_string(), arg1.clone());
                        }
                        None => {
                            let call_value = serde_json::to_string(&self.call).unwrap();
                            scaffold_call.as_object_mut().unwrap().insert("call".to_string(), serde_json::Value::String(call_value));
                            scaffold_call.as_object_mut().unwrap().insert("arg1".to_string(), arg1.clone());
                        }
                    }
                },
                _ => {},
            }
                
        }
        scaffold_call
    }
       
}



/*
Request Array(   <--  Use this method to search for a single key->term pair
    'user_email' => 'user@domain.com',
    'user_password' => 'S3cuRe!Pas5',
    'action' => 'search',
    'search' => 'HSL', // the term you want to search
    'target' => 'initials', // the column you're searching against, if you leave this empty, they all get searched!
    'application' => 'users', // the chosen application to search, this will grab the parent app. and search siblings as well,
    'search_siblings' => true, // if you pass this variable AT ALL, it'll search the application's siblings as well. (ie. part requests, spo, etc. all at once!)
    'field_only' => true, // Add this if you want your search to return ONLY the target field mentioned above (useful for translation of text to ID)
    'start' => 0, // The starting position for the result set (used for pagination)
    'limit' => 20 // the amount of records you'd like to return (max: 1000)
);
Request Array(   <--  Use this method to search for a multiple key->term pairs, as well as specifying search operators (one term can still be used this way)
    'user_email' => 'user@domain.com',
    'user_password' => 'S3cuRe!Pas5',
    'action' => 'search',
    'search' => json_encode(array(
        0 => array(
            'field' => 'item_sku',
            'value' => 'GPU/RTX3080,
            'operator' => ' = '
        ),
        1 => array(
            'field' => 'item_quantity',
            'value' => 1,
            'operator' => ' > '
        )
	)),
    'application' => 'users' // the chosen application to search, this will grab the parent app. and search siblings as well,
    'search_siblings' => true, // if you pass this variable AT ALL, it'll search the application's siblings as well. (ie. part requests, spo, etc. all at once!)
    'start' => 0, // The starting position for the result set (used for pagination)
    'limit' => 20 // the amount of records you'd like to return (max: 1000)
);
 */

 