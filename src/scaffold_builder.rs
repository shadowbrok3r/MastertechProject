use serde::{Deserialize, Serialize};
use serde_json::*;

struct ScaffoldAuth{
    user: String,
    password: String
}

impl Default for ScaffoldAuth{
    fn default() -> Self {
        let user =  "logan.lees@pclaptops.com".to_string();
        let password = "Poolparty1".to_string();
        
        Self { user, password }
    } 
}

pub struct ScaffoldRequestBuilder{
    pub call: Option<ScaffoldCalls>,
    pub action: ScaffoldActions,
    pub app: ScaffoldApps,
    pub arguments: Option<Vec<Value>>,
}

pub enum ScaffoldApps{
    Everest,
    SoftwareLicenseFetch,
    CustomerRequestOrder,
}

impl ScaffoldApps{
    fn as_str(&self) -> &'static str {
        match *self {
            ScaffoldApps::Everest => "everest",
            ScaffoldApps::SoftwareLicenseFetch => "software_license_fetch",
            ScaffoldApps::CustomerRequestOrder => "customer_request_order",
        }
    }
}

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

impl ScaffoldActions {
    fn as_str(&self) -> &'static str {
        match *self {
            ScaffoldActions::Create => "create",
            ScaffoldActions::Read => "read",
            ScaffoldActions::Update => "update",
            ScaffoldActions::Delete => "delete",
            ScaffoldActions::Search => "search",
            ScaffoldActions::GetList => "get_list",
            ScaffoldActions::GetStatus => "get_status",
            ScaffoldActions::EverestCall => "everest_call",
            ScaffoldActions::FetchKeys => "fetch_keys",
        }
    }
}

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

impl ScaffoldCalls {
    fn as_str(&self) -> &'static str {
        match *self {
            ScaffoldCalls::None => "",
            ScaffoldCalls::CheckStock => "check_stock",
            ScaffoldCalls::GetOrderDetailSerials => "get_order_detail_serials",
            ScaffoldCalls::GetOrderDetails => "get_order_details",
            ScaffoldCalls::GetOrderList => "get_order_list",
            ScaffoldCalls::GetOpenSerialsByPaging => "get_open_serials_by_paging",
            ScaffoldCalls::GetActiveItems => "get_active_items",
            ScaffoldCalls::ItemCodeSearch => "item_code_search",
            ScaffoldCalls::DisplayStock => "display_stock",
            ScaffoldCalls::DisplayAutocomplete => "display_autocomplete",
            ScaffoldCalls::GetCog => "get_cog",
            ScaffoldCalls::GetItemCategory => "get_item_category",
            ScaffoldCalls::GetDocAlias => "get_doc_alias",
            ScaffoldCalls::GetCogMovementByDate => "get_cog_movement_by_date",
            ScaffoldCalls::GetItemSellThroughByRepByDateRange => "get_item_sell_through_by_rep_by_date_range",
            ScaffoldCalls::GetItemSellThroughByMonth => "get_item_sell_through_by_month",
            ScaffoldCalls::GetItemSellThroughByDateRange => "get_item_sell_through_by_date_range",
            ScaffoldCalls::GetComputerServicesByDateRange => "get_computer_services_by_date_range",
            ScaffoldCalls::GetOpenServiceOrders => "get_open_service_orders",
            ScaffoldCalls::GetOpenServiceOrdersWithCallNotes => "get_open_service_orders_with_call_notes",
            ScaffoldCalls::CountInvoicePaymentMethodsByDateRange => "count_invoice_payment_methods_by_date_range",
            ScaffoldCalls::GetAllServiceOrdersWithCallNotesByDateRange => "get_all_service_orders_with_call_notes_by_date_range",
            ScaffoldCalls::GetOpenComputerServiceOrdersWithCallNotes => "get_open_computer_service_orders_with_call_notes",
            ScaffoldCalls::GetInvoicedComputerServiceOrdersWithCallNotesByDateRange => "get_invoiced_computer_service_orders_with_call_notes_by_date_range",
            ScaffoldCalls::GetInvoicedOrdersWithCallNotesByDateRange => "get_invoiced_orders_with_call_notes_by_date_range",
            ScaffoldCalls::GetItemDetailBySerial => "get_item_detail_by_serial",
            ScaffoldCalls::GetItemDetailBySerialString => "get_item_detail_by_serial_string",
            ScaffoldCalls::GetEmployeeDetailsByName => "get_employee_details_by_name",
            ScaffoldCalls::GetSalesInvoicesForLocationByDateRange => "get_sales_invoices_for_location_by_date_range",
            ScaffoldCalls::GetSalesOrdersWithSebAhsForLocationsByDateRange => "get_sales_orders_with_seb_ahs_for_locations_by_date_range",
            ScaffoldCalls::GetCustomerNameByIdOrder => "get_customer_name_by_id_order",
            ScaffoldCalls::GetSerialNumbersByDocnum => "get_serial_numbers_by_docnum",
            ScaffoldCalls::GetDocnumBySerialNumber => "get_docnum_by_serial_number",
            ScaffoldCalls::GetSerialNumbersByReference => "get_serial_numbers_by_reference",
            ScaffoldCalls::GetSerialNumbersByOrderID => "get_serial_numbers_by_order_id",
            ScaffoldCalls::IsOrderValid => "is_order_valid",
            ScaffoldCalls::GetNameByOrderId => "get_name_by_order_id",
            ScaffoldCalls::CompareOrderCustomer => "compare_order_customer",
            ScaffoldCalls::GetMonthlySales => "get_monthly_sales",
            ScaffoldCalls::GetCustomers => "get_customers",
            ScaffoldCalls::GetCustomer => "get_customer",
            ScaffoldCalls::GetAddressByOrderId => "get_address_by_order_id",
            ScaffoldCalls::GetTransactionHistory => "get_transaction_history",
            ScaffoldCalls::GetAddressesByCustomerCode => "get_addresses_by_customer_code",
            ScaffoldCalls::GetCustomerByPhone => "get_customer_by_phone",
            ScaffoldCalls::GetOrdersByCustomerId => "get_orders_by_customer_id",
            ScaffoldCalls::GetOrder => "getOrder",
            ScaffoldCalls::ListFunctions => "list_functions",
        }
    }
}
pub struct TicketInformation{
    pub cust_code: String,
    pub user_id: String, // "USER_ID": "BP3", //checkin rep
    pub terms: String, // "TERMS": "CC",
    pub doc_alias: String, // "DOC_ALIAS": "SERVICE ORDER",
    pub department: String, // "DEP": "LTN"
    pub jurisdiction: String, //"JURISCODE": "LTN",
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
}

pub struct PulledKeys{
    pub webroot_key: String,
    pub superanti_key: String,
}

impl ScaffoldRequestBuilder {
    pub fn build_scaffold_call(&mut self) -> Value {

        let credentials = ScaffoldAuth::default();
        let user = credentials.user;
        let pass = credentials.password;
        let company = "pcl".to_string();
        
        let mut scaffold_call = serde_json::json!({
            "user_email": "logan.lees@pclaptops.com".to_string(),
            "user_password": "Poolparty1".to_string(),
            "action": self.action.as_str().to_string(),
            //"call": self.call.as_str().to_string(), 
            "application": self.app.as_str().to_string(), 
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
                    scaffold_call.as_object_mut().unwrap().insert("arg1".to_string(), arg1.clone());
                    scaffold_call.as_object_mut().unwrap().insert("arg2".to_string(), arg2.clone());
                },
                (Some(arg1), None, None) => {
                    if arg1 == "id_order"{
                        scaffold_call.as_object_mut().unwrap().insert("id_order".to_string(), arg1.clone());
                    }else{
                        scaffold_call.as_object_mut().unwrap().insert("arg1".to_string(), arg1.clone());
                    }
                    
                },
                _ => {},
            }
        }
        if let Some(call) = &self.call{
            match call.as_str().is_empty() {
            true => {
                
            }
            false => {
                let call_value = Value::String(call.as_str().to_string());
                scaffold_call.as_object_mut().unwrap().insert("call".to_string(), call_value);
            }

            }
        }


        return scaffold_call;
    }
}

/*
__construct
checkStock( $itemCode, $display )
getOrderDetailSerials( $id_order, $id_item )
getOrderDetails( $id_order )
getOrderList( $limit, $pcl_only, $override )
getOpenSerialsByPaging( $start, $limit )
getQuantityOnOrderBySku( $sku )
getActiveItems( $limit )
itemCodeSearch( $itemCode, $detailed )
displayStock( $data )
displayAutocomplete( $data )
getCog( $order_no )
getCogDev( $order_no )
getAgingLineItems
getAgingLineItemsDev
getItemCategory( $item_code )
getDocAlias( $order_no )
getCogMovementByDate( $date )
getItemSellThroughByRepByDateRange( $item_code, $sales_rep, $date1, $date2 )
getItemSellThroughByMonth( $item_code, $date )
getItemSellThroughByDateRange( $item_code, $date1, $date2 )
getMobileSalesByDateRange( $item_code, $date1, $date2 )
getComputerServicesByDateRange( $date1, $date2 )
getMobileServicesByDateRange( $date1, $date2 )
getOpenServiceOrders
getOpenServiceOrdersWithCallNotes( $date )
getCorporateProfitTracking( $date1, $date2 )
countInvoicePaymentMethodsByDateRange( $date1, $date2 )
getAllServiceOrdersWithCallNotesByDateRange( $date1, $date2 )
getOpenComputerServiceOrdersWithCallNotes
getInvoicedComputerServiceOrdersWithCallNotesByDateRange( $date1, $date2 )
getInvoicedOrdersWithCallNotesByDateRange( $date1, $date2 )
getItemDetailBySerial( $serial )
getItemDetailBySerialString( $str, $is_numeric )
getEmployeeDetailsByName( $data )
getSalesInvoicesForLocationByDateRange( $location, $date1, $date2 )
getSalesOrdersWithSebAhsForLocationsByDateRange( $location, $date1, $date2 )
getOutdatedOperatingSystemSales( $location, $date1, $date2 )
getOutdatedOperatingSystemSalesDev( $location, $date1, $date2 )
getCustomerNameByIdOrder( $id_order )
getSerialNumbersByDocnum( $id_order )
getDocnumBySerialNumber( $id_order )
getSerialNumbersByReference( $id_order )
getSerialNumbersByOrderID( $id_order )
getOrderIdByXidaxIDOrder( $id_order )
isOrderValid( $id_order )
getNameByOrderId( $id_order )
compareOrderCustomer( $id_order, $id_order_2 )
getMonthlySales( $date1, $date2, $type )
getCustomers( $start, $limit )
getCustomer( $cust_code )
getAddressByOrderId( $order_id )
getTransactionHistory( $id_order )
getAddressesByCustomerCode( $cust_code, $status )
getXidaxOrders( $order_date )
getPurchaseOrderList( $start, $limit, $status )
getPurchaseOrder( $po_num )
getPurchaseOrderByID( $po_id )
getPurchaseOrderLines( $po_num )
getPurchaseOrderSerialsByLineId( $po_id )
getVendorByCode( $vend_code )
getCustomerByPhone( $data )
getOrdersByCustomerId( $data, $limit )
getOrder( $id_order, $full )
curl( $url )
cacheResult( $query, $result )
readCache( $query )
list_functions
get_request_counts( $type, $date_from, $date_to )
__get( $key )


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