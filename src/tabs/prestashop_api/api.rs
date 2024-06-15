use log::info;
use reqwest::{header::AUTHORIZATION, Client};
use serde::{Deserialize, Serialize};
use serde_json::from_value;
use std::{collections::HashMap, fmt::Debug};
use quickxml_to_serde::{xml_string_to_json, Config as xmlConfig};

use crate::database::prestashop_schema::SubResource;

const AUTH_TOKEN: &str = "Basic SVAxUlE2UkZSTUZXQjZCOFdIUVY4RFpQV1ZOTDIxWE06";

pub struct Prestashop<'a>{
    client: Client,
    /// [field1,field2 …] or 'full'
    display: &'a str,
    /// &schema=synopsis for tests
    schema: Option<&'a str>, 
    /** 
    * [1|5]	    OR operator: list of possible values
    * [1,10]    Interval operator: define interval of possible values
    * [John]	Literal value (not case sensitive)
    * [Jo]%	    Begin operator: fields begins with the value (not case sensitive)
    * %[hn]	    End operator: fields ends with the value (not case sensitive)
    * %[oh]%	Contains operator: fields contains the value (not case sensitive)
    */ 
    filter: Option<&'a str>,
    /// number, or starting index (limit from number to the index)
    limit: Option<(i32, i32)>,
    // data_channel: PrestaDataChannel
}

impl <'a> Default for Prestashop<'a>{
    fn default() -> Self {
        Self {
            client: Client::new(),
            schema: None,
            display: "full",
            filter: None,
            limit: None,
        }
    }
}

impl <'a>Prestashop<'a> {
    pub fn new<T: for <> Deserialize<'a> + std::fmt::Debug + SubResource>(
        client: Client, display: &'a str, filter: Option<&'a str>, limit: Option<(i32, i32)>, schema: Option<&'a str>,
    ) -> Self { Self { client, display, filter, limit, schema } }

    pub fn query_args(&self, resource_name: &str, url_params: HashMap<&str, &str>) -> String {
        let base_url = format!("https://pclaptops-dev.mojo11.com/api/{}", resource_name);
        
        let mut query_params = vec![];

        // Adding `display` parameter
        if !self.display.is_empty() {
            query_params.push(format!("display={}", self.display));
        }

        // Adding `schema` parameter if present
        if let Some(ref schema) = self.schema {
            query_params.push(format!("schema={}", schema));
        }

        // Adding `filter` parameter if present
        if let Some(ref filter) = self.filter {
            query_params.push(format!("filter[{}]={}", resource_name, filter));
        }

        // Adding `limit` parameter if present
        if let Some((start, end)) = self.limit {
            query_params.push(format!("limit={},{}", start, end));
        }

        // Adding other URL parameters
        for (key, value) in url_params {
            query_params.push(format!("{}={}", key, value));
        }

        // Constructing the final URL
        let query_string = if !query_params.is_empty() {
            format!("?{}", query_params.join("&"))
        } else {
            String::new()
        };

        format!("{}{}", base_url, query_string)
    }

    pub async fn request_subresources_by_id<T>(
        &self, 
        resource: &str, 
        name: &str, 
        id: &i32
    ) 
        -> anyhow::Result<T, anyhow::Error>
            where T: for <'de>Deserialize<'de> + std::fmt::Debug
    {
        let response: String = self.client 
            .get(format!("https://pclaptops-dev.mojo11.com/api/{resource}/{id}"))   // .header(CONTENT_TYPE, "application/json") .header(ACCEPT, "application/json") // .json(&params)
            .header(AUTHORIZATION, AUTH_TOKEN)
            .send()
            .await?
            .text()
            .await?;

        let xml = xml_string_to_json(response, &xmlConfig::new_with_defaults())?;
        info!("XML: {xml:#?}");
        let x: T = from_value(xml["prestashop"][name].clone())?;
        info!("XML: {x:#?}");
        Ok(x)
    }

    pub async fn request_resource_test<T>(
        &self, 
        resource_name: &str, 
        name: &str, 
        _get_subresource: Option<&str>, 
        url_params: HashMap<&str, &str>
    ) 
        -> anyhow::Result<T, anyhow::Error>
        where T: for <'de>Deserialize<'de> + std::fmt::Debug
    {
        let response = self.client // 2063620
            .get(self.query_args(resource_name, url_params)) // ?output_format=JSON
            .header(AUTHORIZATION, AUTH_TOKEN)
            .send()
            .await?
            .text()
            .await?;
        
        let xml = xml_string_to_json(response, &xmlConfig::new_with_defaults())?;
        info!("XML: {xml:#?}");
        let x: T = from_value(xml["prestashop"][resource_name][name].clone())?; // [resource_name.as_str()][name.clone()].clone())?;

        Ok(x)

    }

    pub async fn request_subresources_by_name<T>(
        &self, 
        resource: &str, 
        name: &str, 
        subresource: &str
    ) 
        -> anyhow::Result<T, anyhow::Error>
            where T: for <'de>Deserialize<'de> + std::fmt::Debug + SubResource
    {
        let response: String = self.client 
            .get(format!("https://pclaptops-dev.mojo11.com/api/{resource}/{subresource}"))   // .header(CONTENT_TYPE, "application/json") .header(ACCEPT, "application/json") // .json(&params)
            .header(AUTHORIZATION, AUTH_TOKEN)
            .send()
            .await?
            .text()
            .await?;

        let xml_val = xml_string_to_json(response, &xmlConfig::new_with_defaults())?;
        let new: T = from_value(xml_val["prestashop"][name].clone())?;
        info!("new: {new:#?}");
        Ok(new)
    }

    // pub async fn request_resources<T>(
    //     &self, 
    //     resource_name: &str, 
    //     name: &str, 
    //     get_subresource: Option<&str>, 
    //     url_params: HashMap<&str, &str>
    // ) 
    //     -> anyhow::Result<Vec<T>, anyhow::Error>
    //         where T: for <>Deserialize<'a> + std::fmt::Debug + SubResource
    // {
    //     info!(
    //         "resource_name: {resource_name:#?}, {url_params:#?}\nURL: {:#?}", 
    //         self.query_args(resource_name, url_params.clone())
    //     );
    //     let response = self.client.get(self.query_args(resource_name, url_params))
    //         .header(AUTHORIZATION, AUTH_TOKEN)
    //         .send()
    //         .await?
    //         .text()
    //         .await?;
    //     let xml = xml_string_to_json(response, &xmlConfig::new_with_defaults())?;
    //     let x: Vec<T> = from_value(xml["prestashop"][resource_name][name].clone())?;
    //     if let Some(subresource) = get_subresource{
    //         for item in x.iter().take(10){
    //             if let Some(field_value) = item.get_subresource(&subresource) {
    //                 let _ = self.request_subresources_by_name::<T>(&resource_name, &name, &field_value).await;
    //             } else {
    //                 info!("field {} not found in item: {:#?}", subresource, item);
    //             }
    //         }
    //         return Ok(x);
    //     }else{ return Ok(x); }
    // }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Data{
    #[serde(rename="@id")]
    id: Option<i32>,
    #[serde(rename="@xlink:href")]
    link: String
}