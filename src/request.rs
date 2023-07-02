use reqwest::header::{CONTENT_TYPE, ACCEPT};
use serde::{Deserialize, Serialize};
use serde_json::*;
use std::error::Error;
/*impl Default for post_request{
    fn default() -> Self {
        post_request {
            user_email_passw: [
                ("user_email".to_string(), "logan.lees@pclaptops.com".to_string()), 
                ("user_password".to_string(), "Poolparty1".to_string())
            ],

            call: "getOrder".to_string(),
            action: "everest_call".to_string(),
            application: "everest".to_string(),
            arg1: "".to_string(),
            arg2: "".to_string(),
            company: "pcl".to_string()
            
        }
        
    }
}*/

#[derive(Debug, Deserialize)]
pub struct GetTicketResponse {
    pub header: Header,
    pub customer: Customer,
    //pub transactions: Transactions,
    pub addresses: Addresses,
    pub items: Vec<Value>,
}

#[derive(Deserialize, Debug)]
pub struct Header {
    pub CUST_CODE: String,
    pub USER_ID: String, // "USER_ID": "BP3", //checkin rep
    pub TERMS: String, // "TERMS": "CC",
    pub DOC_ALIAS: String, // "DOC_ALIAS": "SERVICE ORDER",
    pub DEP: String, // "DEP": "LTN"
    pub JURISCODE: String, //"JURISCODE": "LTN",
    pub COG: String, // "COG": "7.1000", //Cost of goods?
    pub INV_AMOUNT: String, // "INV_AMOUNT": "53.6100",
}

#[derive(Deserialize, Debug)]
pub struct Customer {
    pub NAME: String, // "NAME": "Timber Ridge Fireplace LLC",
    //pub CUSTOMER_ADDRESS: String,
    pub LI_DOC: String, //"LI_DOC": "53745333",
    pub LI_AMT: String,  //"LI_AMT": "53.6100", //I COULD USE THIS TO CHECK LAST TUNEUP
    //pub LAST_TUNEUP_DATE: String, // <-- HERE
    pub DW_UPDATE_DATE: String, // "DW_UPDATE_DATE": "2023-06-27 13:38:50.440",
    pub NUM_INV: String, // "NUM_INV": "21",
/*		"LP_AMT": "-53.6100",
		"LP_DOC": "52883815",
		"LP_DOC_TYP": "8",
		"LP_DATE": "2023-05-04 00:00:00.000", */

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
    pub TEL1: String, // "TEL1": "8018376254",
    pub TEL2: String, // "TEL2": "",
    pub EMAIL: String,
}
#[derive(Deserialize, Debug)]
pub struct ItemsArray{ // Okay, so the number of items is the number of item codes you have on an order....  
    //so i may need to iterate through them to get all line items. especially if i check for a new build
    
    // object_one: Option<String>, // Item_code // I should pull the ITEM_CODE here too ("brand/pcl"),
    // this could also get srvc/etc
   pub item_objects: Vec<Value>,
   /*

////////////////////////////////////    ARRAY ONE

"ITEM_CODE": "BRAND-PCL",

"X_INVOICE_ID": "16994221",

"ITEM_QTY": "1.000000",

"QTY_SHIP": "1.000000",

"ITEM_PRICE": ".000000",

"serials": [] //each of these in the 'Items' array of objects store all serials attached

}, {



////////////////////////////////////    ARRAY TWO

"NOTE": "This service,





////////////////////////////////////    ARRAY THREE



"ITEM_CODE": "SRVC/TUNEUP/PCL",

"DESC_TYPE": "1",

"ITEM_QTY": "1.000000",

"QTY_SHIP": "1.000000",

"ITEM_PRICE": "159.990000",

"DISCOUNT_V": "159.990000",

"SALES_REP": "KMJ",

    "STK_ITEM_QTY": "1.000000",

    "STK_QTY_SHIP": "1.000000",



////////////////////////////////////    ARRAY FOUR





"ITEM_CODE": "SW/PCLCPS/O",

"DESC_TYPE": "1",

"ITEM_QTY": "1.000000",

"QTY_SHIP": "1.000000",



"COST": "7.100000",

"MISC_COST": "1.420000", 

"C_COST": "7.100000", //I CAN SEE COST

"ITEM_PRICE": "49.990000", // VS WHAT WE CHARGED

"FACTORED_COST_PER": "20.000000", // ????









#[derive(Deserialize, Debug)]

pub struct ItemsObjectTwo{

    checkin_notes: String, // NOTE <-- Bingo

    object_one: Option<String>, 

    object_two: String, 

}



#[derive(Deserialize, Debug)]

pub struct ItemsObjectThree{

    checkin_notes: String,

    object_one: Option<String>,// Item_code //I should pull the ITEM_CODE here too ("brand/pcl"), this could also get srvc/etc

    object_two: String, // NOTE

}

*/

}

//tx: watch::Sender<Option<Result<String, reqwest::Error>>>
pub async fn request_ticket_info(so_number: String)  -> core::result::Result<GetTicketResponse, Box<dyn Error>> {
    let params = serde_json::json!({
        "user_email": "logan.lees@pclaptops.com",
        "user_password": "Poolparty1", 
        "call": "getOrder", 
        "action": "everest_call",
        "application": "everest", 
        "arg1": so_number, 
        "arg2": "false", 
        "company": "pcl"
});    
    
let response = reqwest::Client::new().post("https://scaffold.pclaptops.com/api/index") //https://5dccaa60-8a54-47f1-8ff6-ce32034dd0f6.mock.pstmn.io
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(&params)
        .send()
        .await; //need to find a way for this to return the response not the result

        match response {
            Ok(res) => {
                //let raw_response = res.text().await?; //before parsing the data
                let json_response: GetTicketResponse = res.json().await?;// serde_json::from_str(&raw_response)?;
                //println!("heres the stuff: {:?}", json_response);
                Ok(json_response)
            },
            Err(e) => Err(Box::new(e)),
        }


    //Ok(())
}

// pub async fn request_seb_info(cust_id: String)  -> core::result::Result<GetTicketResponse, Box<dyn Error>> {}

// pub async fn request_keys(so_number: String)  -> core::result::Result<GetTicketResponse, Box<dyn Error>> {}

// pub async fn get_computer_purchases(cust_id: String)  -> core::result::Result<GetTicketResponse, Box<dyn Error>> {}






/*Bounded channel: If you need a bounded channel, you should use a bounded Tokio mpsc channel for both directions of communication. 
Instead of calling the async send or recv methods, in synchronous code you will need to use the blocking_send or blocking_recv methods.

Unbounded channel: You should use the kind of channel that matches where the receiver is. So for sending a message from async to sync, 
you should use the standard library unbounded channel or crossbeam. Similarly, for sending a message from sync to async, you should use an unbounded Tokio mpsc channel.

Please be aware that the above remarks were written with the mpsc channel in mind, but they can also be generalized to other kinds of channels. 
In general, any channel method that isn’t marked async can be called anywhere, including outside of the runtime. For example, sending a message on a 
oneshot channel from outside the runtime is perfectly fine. */


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