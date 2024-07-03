use std::error::Error;

use log::info;
use reqwest::{header::{ACCEPT, CONTENT_TYPE, USER_AGENT}, Client};
use self_update::{
    self, backends::github::{
        ReleaseList, UpdateBuilder,
    }, cargo_crate_version
};
use serde_json::Value;

pub async fn run(client: Client) -> core::result::Result<(String, String), Box<dyn ::std::error::Error>> {
    // let token = var("GITHUB_KEY").unwrap();
    let token = "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";
    let auth_tok = "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";

    let response: Value = client.get("https://api.github.com/repos/shadowbrok3r/Mastertech4.0/releases/latest") 
        .header(ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header(USER_AGENT, "shadowbrok3r")
        .bearer_auth(token)
        .send()
        .await?
        .json()
        .await?;

    let releases = response.get("assets");
    if let Some(release) = releases{
        let url: &str = release[0].get("url").unwrap().as_str().unwrap();
        info!("response: {url}");
    
        if !url.is_empty(){
            let response = client.get(url) 
                .header(CONTENT_TYPE, "application/octet-stream")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .header(USER_AGENT, "shadowbrok3r")
                .bearer_auth(token)
                .send()
                .await
                .map_err(|e| {
                    info!("e.source() {:?}", e.source());
                    info!("URL: {:?}", e.url());
                    info!("{}", e.to_string());
                }).unwrap();

            info!("response: {response:?}");

            let tmp_dir = ::std::env::current_dir()?;
            info!("tmp_tarball_path: {tmp_dir:?}");
            let tmp_tarball_path = tmp_dir.as_path().join(&"git-MasterTech.exe");
            info!("tmp_tarball_path: {tmp_tarball_path:?}");
            let mut tmp_file = std::fs::File::create(&tmp_tarball_path)?;

            // Copy the response content into the file
            let content = response.bytes().await?;
            std::io::copy(&mut content.as_ref(), &mut tmp_file)?;
            info!("Download completed successfully!");
        }
        
    }

    let release_versions = format!("releases:#?");
    Ok((release_versions, "".to_string()))
}