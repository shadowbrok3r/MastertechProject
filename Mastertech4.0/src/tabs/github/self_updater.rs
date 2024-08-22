use std::error::Error;

use crossbeam::channel::Sender;
use futures::StreamExt;
use log::info;
use reqwest::{header::{ACCEPT, CONTENT_TYPE, USER_AGENT}, Client};
use serde_json::Value;
use tokio::{fs::File, io::AsyncWriteExt};

const TOKEN: &str = "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";

pub async fn run(client: Client, tx: Sender<(u64, u64)>) -> anyhow::Result<(), anyhow::Error> {
    let mut downloaded_bytes: u64 = 0;
    
    let response: Value = client.get("https://api.github.com/repos/shadowbrok3r/MastertechProject/releases/latest") 
        .header(ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header(USER_AGENT, "shadowbrok3r")
        .bearer_auth(TOKEN)
        .send()
        .await?
        .json()
        .await?;

    let releases = response.get("assets");
    if let Some(release) = releases{
        let url: &str = release[0].get("url").unwrap().as_str().unwrap();
        let total_length: u64 = release[0].get("size").unwrap().as_u64().unwrap();
        info!("response: {url}\nLen: {total_length}");
    
        if !url.is_empty(){
            let response = client.get(url) 
                .header(ACCEPT, "application/octet-stream")
                .header(CONTENT_TYPE, "application/octet-stream")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .header(USER_AGENT, "shadowbrok3r")
                .bearer_auth(TOKEN)
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
            let mut tmp_file = File::create(&tmp_tarball_path).await?;

            // Copy the response content into the file
            let mut stream = response.bytes_stream();

            while let Some(item) = stream.next().await{
                let chunk = item?;
                tmp_file.write_all(&chunk).await?;
                downloaded_bytes += chunk.len() as u64;
                let sender = (downloaded_bytes, total_length);
                
                if let Err(e) = tx.try_send(sender){
                    info!("Error sending bytes: {e}");
                }
            }

            if downloaded_bytes == total_length {
                drop(tx);
                info!("DONE");

                // #[cfg(target_os="windows")]{    
                //     let cmd_stdout = Command::new(tmp_tarball_path)
                //         .creation_flags(CREATE_NO_WINDOW)
                //         .output()
                //         .await?
                //         .stdout;
                
                //     info!("cmd_stdout: {:?}", cmd_stdout);
                // }
            }
        }
    }

    Ok(())
}