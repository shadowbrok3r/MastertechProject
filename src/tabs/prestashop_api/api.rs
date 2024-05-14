use log::{debug, info};
use reqwest::{header::AUTHORIZATION, Client};
use serde::{Deserialize, Serialize};
use serde_json::{from_value, Value};
use std::fmt::Debug;
use quickxml_to_serde::{xml_string_to_json, Config as xmlConfig};

use super::resources::{Employees, Addresses, Orders};

#[derive(Serialize, Deserialize, Debug)]
pub enum PrestashopData {
    Orders(Orders),
    Employees(Employees),
    Addresses(Addresses),
}

pub struct Prestashop{
    client: Client,
    /// [field1,field2 …] or 'full'
    display: String,
    /// &schema=synopsis for tests
    schema: Option<String>, 
    /** 
    * [1|5]	    OR operator: list of possible values
    * [1,10]    Interval operator: define interval of possible values
    * [John]	Literal value (not case sensitive)
    * [Jo]%	    Begin operator: fields begins with the value (not case sensitive)
    * %[hn]	    End operator: fields ends with the value (not case sensitive)
    * %[oh]%	Contains operator: fields contains the value (not case sensitive)
    */ 
    filter: Option<String>,
    /// number, or starting index (limit from number to the index)
    limit: Option<i32>,
    // data_channel: PrestaDataChannel
}

impl Default for Prestashop{
    fn default() -> Self {
        Self {
            client: Client::new(),
            schema: None,
            display: "?display=full".to_string(),
            filter: None,
            limit: None,
        }
    }
}

impl Prestashop {

    pub fn new<T: for<'a> Deserialize<'a> + std::fmt::Debug>(
        client: Client, display: String, filter: Option<String>, limit: Option<i32>, schema: Option<String>,
    ) -> Self{
        Self { client, display, filter, limit, schema }
    }

    pub async fn request_resource<T: for<'a> Deserialize<'a> + std::fmt::Debug>(&self, resource: String, get_subresource: Option<String>) 
        -> anyhow::Result<T, anyhow::Error>
    {
        let response = self.client // 2063620
            .get(format!("https://pclaptops-dev.mojo11.com/api/{resource}{}", self.display)) // ?output_format=JSON
            .header(AUTHORIZATION, "Basic SVAxUlE2UkZSTUZXQjZCOFdIUVY4RFpQV1ZOTDIxWE06")
            .send()
            .await?
            .text()
            .await?;
        
        // let y: T = serde_xml_rs::from_str(response.as_str())?; // ::<PrestaResource::<T>>
        
        let xml = xml_string_to_json(response, &xmlConfig::new_with_defaults())?;
        info!("xml: {:#?}", xml.clone());
        
        let typed_value = from_value(xml.clone()); 

        if let Ok(typed_value) = typed_value{
            info!("OK: {typed_value:#?}");
            return Ok(typed_value);
        }else{
            info!("{:#?}", xml["prestashop"][resource.as_str()].clone());
            let x: T = from_value(xml["prestashop"][resource.as_str()].clone())?;
            return Ok(x);
        }
        // if let Some(subresource) = get_subresource{
        //     // for resources in &presta_info.prestashop.employees.employee[0..5]{
        //         // request_subresources(client.clone(), resources).await?;
        //     // }
        // }
    }
    
    pub async fn request_subresources(&self, resources: &Data) -> anyhow::Result<Value, anyhow::Error>{
        let response: Value = self.client // 2063620
            .get(resources.link.clone())   // .header(CONTENT_TYPE, "application/json") .header(ACCEPT, "application/json") // .json(&params)
            .header(AUTHORIZATION, "Basic SVAxUlE2UkZSTUZXQjZCOFdIUVY4RFpQV1ZOTDIxWE06")
            .send()
            .await?
            .json()
            .await?;
    
        

        debug!("RESOURCE: {:?}", response.clone());
        Ok(response)
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Data{
    #[serde(rename="@id")]
    id: Option<i32>,
    #[serde(rename="@xlink:href")]
    link: String
}