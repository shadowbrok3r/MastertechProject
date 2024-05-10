use log::{debug, info, error};
use reqwest::{header::AUTHORIZATION, Client};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::Debug;


async fn request_resource(client: Client) -> anyhow::Result<Value, anyhow::Error>{

    let response = client // 2063620
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

async fn request_subresources(client: Client, resources: &Data) -> anyhow::Result<Value, anyhow::Error>{

    let response: Value = client // 2063620
        .get(resources.link.clone())   // .header(CONTENT_TYPE, "application/json") .header(ACCEPT, "application/json") // .json(&params)
        .header(AUTHORIZATION, "Basic SVAxUlE2UkZSTUZXQjZCOFdIUVY4RFpQV1ZOTDIxWE06")
        .send()
        .await?
        .json()
        .await?;


    debug!("RESOURCE: {:?}", response.clone());
    Ok(response)
}

#[derive(Serialize, Deserialize, Debug)]
struct Prestashop{
    prestashop: Employees,
}

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