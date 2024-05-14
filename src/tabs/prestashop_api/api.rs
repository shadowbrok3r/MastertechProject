use log::{debug, info};
use reqwest::{header::AUTHORIZATION, Client};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::Debug;

use super::resources::Resources;

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
}

impl Default for Prestashop{
    fn default() -> Self {
        Self {
            client: Client::new(),
            schema: None,
            display: "full".to_string(),
            filter: None,
            limit: None
        }
    }
}

impl Prestashop {

    pub fn new(client: Client, display: String, filter: Option<String>, limit: Option<i32>, schema: Option<String>) -> Self{
        Self { client, display, filter, limit, schema }
    }

    // <T: for<'a> Deserialize<'a> + std::fmt::Debug> // i cannot use this for xml..
    pub async fn request_resource<T: for<'a> Deserialize<'a> + std::fmt::Debug>(&self, resource: String, get_subresource: Option<String>) 
        -> anyhow::Result<T, anyhow::Error>
    {
        let response = self.client // 2063620
            .get(format!("https://pclaptops-dev.mojo11.com/api/{}", resource)) // ?output_format=JSON
            .header(AUTHORIZATION, "Basic SVAxUlE2UkZSTUZXQjZCOFdIUVY4RFpQV1ZOTDIxWE06")
            .send()
            .await?
            .text()
            .await?;


        let y: T = serde_xml_rs::from_str(response.as_str()).unwrap();
        
        info!("Resource: {y:#?}");

        // if let Some(subresource) = get_subresource{
            // for resources in &presta_info.prestashop.employees.employee[0..5]{
                // request_subresources(client.clone(), resources).await?;
            // }
        // }

            

        Ok(y)
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


// struct Prestashop{
//     ,
// }

#[derive(Serialize, Deserialize, Debug)]
pub struct Employees{
    #[serde(flatten)]
    employees: Vec<Data>
    // employees: Employee
}

// #[derive(Serialize, Deserialize, Debug)]
// pub struct Employee{
//     employee: 
// }

#[derive(Serialize, Deserialize, Debug)]
pub struct Data{
    // #[serde(rename="@id")]
    id: Option<i32>,
    // #[serde(rename="@xlink:href")]
    link: String
}