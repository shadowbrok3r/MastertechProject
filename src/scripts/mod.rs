use std::{error::Error, io::Cursor};

use reqwest::Client;

pub struct Scripts{
    pub wrsa: String,
    pub sas: String,
    pub check_driver: String,
    pub running_tasks: String,
}

impl Default for Scripts{
    fn default() -> Self{
        Self{
            wrsa: "Install Webroot".to_string(),
            sas: "Install SAS".to_string(),
            check_driver: "Check Driver Issues".to_string(),
            running_tasks: "Running Tasks".to_string()
        }
    }
}

impl Scripts{
    async fn run_script(client: Client, script: String) -> Result<String, Box<dyn Error>>{


        let response = client.get(format!("https://anywhere.webrootcloudav.com/zerol/wsainstall.exe")) 
            .send()
            .await?;
    
        let mut file = std::fs::File::create("%temp%\\wrv.exe")?;
        let mut content =  Cursor::new(response.bytes().await?);
        std::io::copy(&mut content, &mut file)?;
        
        // tokio::process::Command::new("")
        Ok("".to_string())
    }
}