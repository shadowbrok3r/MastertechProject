use crossbeam::channel::Sender;
use futures::StreamExt;
use log::{error, info};
use reqwest::{
    header::{ACCEPT, CONTENT_TYPE, USER_AGENT},
    Client,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{fs::File, io::AsyncWriteExt};



#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct GithubRelease {
    pub url: String,
    pub html_url: String,
    pub name: String,
    pub created_at: String,
    pub body: String,
    pub tag_name: String,
    pub assets: Vec<Asset>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub browser_download_url: String,
    pub size: u64,
    pub created_at: String,
}

/// Proxied GitHub API base (Cloudflare Worker — CORS for WASM).
const GIT_MASTER_TECH_REPO_BASE: &str =
    "https://git.master-tech.app/repos/shadowbrok3r/MastertechProject";

#[inline]
fn proxied_github_asset_url(asset_api_url: &str) -> String {
    asset_api_url.replace("api.github.com", "git.master-tech.app")
}

pub async fn run(client: Client, tx: Sender<(u64, u64)>) -> anyhow::Result<(), anyhow::Error> {
    let mut downloaded_bytes: u64 = 0;

    let response: Value = client
        .get(format!("{GIT_MASTER_TECH_REPO_BASE}/releases/latest"))
        .header(ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header(USER_AGENT, "shadowbrok3r/Mastertech")
        .send()
        .await?
        .json()
        .await?;

    let releases = response.get("assets").and_then(|a| a.as_array());
    if let Some(assets) = releases {
        let Some(asset0) = assets.first() else {
            return Ok(());
        };
        let Some(url) = asset0
            .get("url")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        else {
            return Ok(());
        };
        let total_length: u64 = asset0.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
        info!("response: {url}\nLen: {total_length}");

        let url = proxied_github_asset_url(url);

        if !url.is_empty() {
            let response = client
                .get(&url)
                .header(ACCEPT, "application/octet-stream")
                .header(CONTENT_TYPE, "application/octet-stream")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .header(USER_AGENT, "shadowbrok3r/Mastertech")
                .send()
                .await
                .map_err(|e| {
                    error!("e.source() {:?}", e.to_string());
                    info!("URL: {:?}", e.url());
                    info!("{}", e.to_string());
                })
                .unwrap();

            info!("response: {response:?}");

            let tmp_dir = ::std::env::current_dir()?;
            info!("tmp_tarball_path: {tmp_dir:?}");
            let tmp_tarball_path = tmp_dir.as_path().join(&"git-MasterTech.exe");
            info!("tmp_tarball_path: {tmp_tarball_path:?}");
            let mut tmp_file = File::create(&tmp_tarball_path).await?;

            // Copy the response content into the file
            let mut stream = response.bytes_stream();

            while let Some(item) = stream.next().await {
                let chunk = item?;
                tmp_file.write_all(&chunk).await?;
                downloaded_bytes += chunk.len() as u64;
                let sender = (downloaded_bytes, total_length);

                if let Err(e) = tx.try_send(sender) {
                    error!("Error sending bytes: {e}");
                }
            }

            if downloaded_bytes == total_length {
                drop(tx);
                info!("DONE");
            }
        }
    }

    Ok(())
}
