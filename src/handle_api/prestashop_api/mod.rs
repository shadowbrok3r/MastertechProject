use log::debug;
use reqwest::{header::AUTHORIZATION, Client};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::Debug;

use self::resources::Resources;
mod resources;

struct Prestashop{
    client: Client,
    /// [field1,field2 …] or 'full'
    display: String,
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
            display: "full".to_string(),
            filter: None,
            limit: None
        }
    }
}

impl Prestashop {
    pub async fn request_resource(&self, resource: Resources) -> anyhow::Result<Value, anyhow::Error>{
        let response = self.client // 2063620
            .get("https://pclaptops-dev.mojo11.com/api/orders?output_format=JSON&schema=synopsis")
            .header(AUTHORIZATION, "Basic SVAxUlE2UkZSTUZXQjZCOFdIUVY4RFpQV1ZOTDIxWE06")
            .send()
            .await?
            .json::<Value>()
            .await?;
    
        // for resources in &presta_info.prestashop.employees.employee[0..5]{
        //     request_subresources(client.clone(), resources).await?;
        // }
        Ok(response)
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
//     prestashop: Employees,
// }

#[derive(Serialize, Deserialize, Debug)]
struct Employees{
    employees: Employee
}

#[derive(Serialize, Deserialize, Debug)]
struct Employee{
    employee: Vec<Data>
}

#[derive(Serialize, Deserialize, Debug)]
struct Data{
    #[serde(rename="@id")]
    id: Option<i32>,
    #[serde(rename="@xlink:href")]
    link: String
}