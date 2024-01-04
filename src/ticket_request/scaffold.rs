use std::error::Error;

use async_trait::async_trait;
use log::debug;
use reqwest::header::{CONTENT_TYPE, ACCEPT};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use dotenv::var;

use crate::ticket_request::GetTicketResponse;


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

#[async_trait]
pub trait SendReq<T>{
    async fn retrieve_data(&self, so_number: &str, client: reqwest::Client) -> Result<T, Box<dyn Error>>;
}

/* #[async_trait]
impl SendReq<GetTicketResponse> for SendRequest{
    async fn retrieve_data(&self, so_number: &str, client: reqwest::Client) -> Result<GetTicketResponse, Box<dyn Error>> {
        debug!("request_ticket_info");  
        let params: Value = serde_json::json!({
            "user_email": "logan.lees@pclaptops.com", 
            "user_password": "Poolparty1",
            "action": ScaffoldActions::EverestCall,
            "call": ScaffoldCalls::GetOrder,
            "application": ScaffoldApps::Everest, 
            "company": "pcl",
            "arg1": so_number,
            "arg2": "false"
        }); // scaffold_builder.build_scaffold_call();
        let response = client
            .post("https://scaffold.pclaptops.com/api/index")
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .json(&params)
            .send()
            .await;
        match response {
            Ok(res) => {
                let json_response: GetTicketResponse  = res.json().await?;
                Ok(json_response)
            },
            Err(e) => {
                debug!("Boxed error: {e:?}");
                Err(Box::new(e))
            },
        }
    }
} */

/* #[async_trait]
impl <GetKeysResponse>SendReq<GetKeysResponse> for SendRequest{
    async fn retrieve_data(&self, so_number: &str, client: reqwest::Client) -> Result<GetKeysResponse, Box<dyn Error>> {
        let params: Value = serde_json::json!({
            "user_email": "logan.lees@pclaptops.com", 
            "user_password": "Poolparty1",
            "action": ScaffoldActions::FetchKeys,
            "application": ScaffoldApps::SoftwareLicenseFetch, 
            "company": "pcl",
            "id_order": so_number,
        }); // scaffold_builder.build_scaffold_call();
        
        let join_handle = tokio::spawn(async move{
            let response = client.post("https://scaffold.pclaptops.com/api/index") 
                .header(CONTENT_TYPE, "application/json")
                .header(ACCEPT, "application/json")
                .json(&params)
                .send()
                .await;
        
            match response {
                Ok(res) => {
                    let mut response_text = res.text().await?;// serde_json::from_str(&raw_response)?;
                    debug!("response: {:?}", response_text);
        
                    let mut webroot_key: &str = "";
                    let mut superanti_key: &str = "";

                    if !response_text.contains("hasError"){
                        let wrav_offset = response_text.find("WRAV: ").unwrap_or(response_text.len());

                        let _: String = response_text.drain(..wrav_offset).collect(); 
    
                        let split_lines: Vec<&str> = response_text.split("\nSAS: ").collect();
    
                        let split_wrav: Vec<&str> = split_lines[0].split("WRAV: ").collect();

                        webroot_key = split_wrav[1].trim();
                        superanti_key = split_lines[1].trim();
                    }
                    else{
                        println!("SW\\/PCLCPS\\/O not on ticket");
                        webroot_key = "Error";
                        superanti_key = "Check console";
                    }
                    

                    let response_keys: GetKeysResponse = GetKeysResponse {
                        webroot_key: webroot_key.to_string(),
                        superanti_key: superanti_key.to_string(),
                    };
                    
                    Ok(response_keys)
                },
                Err(e) => Err(Box::new(e)),
            }
        });

        Ok(join_handle.await??)
        
    }
} */

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

