use log::{debug, info};
use reqwest::{header::AUTHORIZATION, Client};
use serde::{Deserialize, Serialize};
use serde_json::{from_value, Value};
use std::fmt::Debug;
use quickxml_to_serde::{xml_string_to_json, Config as xmlConfig};

use crate::tabs::prestashop_api::resources::Employee;

use super::resources::{Addresses, Employees, Orders, SubResource};

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

    pub fn new<T: for<'a> Deserialize<'a> + std::fmt::Debug + SubResource>(
        client: Client, display: String, filter: Option<String>, limit: Option<i32>, schema: Option<String>,
    ) -> Self { Self { client, display, filter, limit, schema } }

    pub async fn request_resource<T: for<'a> Deserialize<'a> + std::fmt::Debug + SubResource>(&self, resource: String, name: String, get_subresource: Option<String>) 
        -> anyhow::Result<Vec<T>, anyhow::Error>
    {
        let response = self.client // 2063620
            .get(format!("https://pclaptops-dev.mojo11.com/api/{resource}{}", self.display)) // ?output_format=JSON
            .header(AUTHORIZATION, "Basic SVAxUlE2UkZSTUZXQjZCOFdIUVY4RFpQV1ZOTDIxWE06")
            .send()
            .await?
            .text()
            .await?;
        
        let xml = xml_string_to_json(response, &xmlConfig::new_with_defaults()).unwrap();
        info!("XML: {xml:#?}");
        let x: Vec<T> = from_value(xml["prestashop"][resource.as_str()][name.clone()].clone()).unwrap();

        if let Some(subresource) = get_subresource{
            for item in x.iter().take(10){
                if let Some(field_value) = item.get_subresource(&subresource) {
                    let _ = self.request_subresources_by_name::<T>(&resource, &name, &field_value).await;
                } else {
                    info!("field {} not found in item: {:#?}", subresource, item);
                }
            }
            return Ok(x);
        }else{ return Ok(x); }

    }

    pub async fn request_subresources_by_name<T: for<'a> Deserialize<'a> + std::fmt::Debug + SubResource>(&self, resource: &String, name: &String, subresource: &String) 
        -> anyhow::Result<T, anyhow::Error>
    {
        let response: String = self.client 
            .get(format!("https://pclaptops-dev.mojo11.com/api/{resource}/{subresource}"))   // .header(CONTENT_TYPE, "application/json") .header(ACCEPT, "application/json") // .json(&params)
            .header(AUTHORIZATION, "Basic SVAxUlE2UkZSTUZXQjZCOFdIUVY4RFpQV1ZOTDIxWE06")
            .send()
            .await?
            .text()
            .await?;

        let xml_val = xml_string_to_json(response, &xmlConfig::new_with_defaults()).unwrap();
        let new: T = from_value(xml_val["prestashop"][name].clone()).unwrap();
        info!("new: {new:#?}");
        Ok(new)
    }

    pub async fn request_subresources_by_link(&self, resources: &Data) 
        -> anyhow::Result<Value, anyhow::Error>
    {

        // 2063620

        let response: Value = self.client 
            .get(resources.link.clone())   // .header(CONTENT_TYPE, "application/json") .header(ACCEPT, "application/json") // .json(&params)
            .header(AUTHORIZATION, "Basic SVAxUlE2UkZSTUZXQjZCOFdIUVY4RFpQV1ZOTDIxWE06")
            .send()
            .await?
            .json()
            .await?;
    
        

        info!("RESOURCE: {:?}", response.clone());
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