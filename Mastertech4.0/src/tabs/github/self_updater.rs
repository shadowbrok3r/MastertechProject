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
        // Pick the asset for this OS: Windows takes the `.exe`, others the
        // extension-less binary. Match the package bin name so a sibling asset
        // (e.g. `qc_app.exe`) is never selected when multiple assets exist.
        let want_exe = cfg!(target_os = "windows");
        let bin_prefix = env!("CARGO_PKG_NAME").to_ascii_lowercase();
        let Some(asset0) = assets.iter().find(|a| {
            let name = a
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            name.starts_with(&bin_prefix) && name.ends_with(".exe") == want_exe
        }) else {
            error!("self_updater: no release asset matched this OS (want_exe={want_exe}, bin={bin_prefix})");
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

        let response = client
            .get(&url)
            .header(ACCEPT, "application/octet-stream")
            .header(CONTENT_TYPE, "application/octet-stream")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header(USER_AGENT, "shadowbrok3r/Mastertech")
            .send()
            .await?
            .error_for_status()?;

        info!("response: {response:?}");

        let staged = crate::utilities::safe_swap::staged_update_path()?;
        info!("staged update path: {staged:?}");
        let mut staged_file = File::create(&staged).await?;

        let mut stream = response.bytes_stream();
        let mut stream_result: anyhow::Result<()> = Ok(());
        while let Some(item) = stream.next().await {
            let chunk = match item {
                Ok(chunk) => chunk,
                Err(e) => {
                    stream_result = Err(e.into());
                    break;
                }
            };
            if let Err(e) = staged_file.write_all(&chunk).await {
                stream_result = Err(e.into());
                break;
            }
            downloaded_bytes += chunk.len() as u64;
            // The completion message is sent once, after validation below.
            if downloaded_bytes != total_length {
                let _ = tx.try_send((downloaded_bytes, total_length));
            }
        }

        if stream_result.is_ok() {
            if let Err(e) = staged_file.flush().await {
                stream_result = Err(e.into());
            }
        }
        drop(staged_file);

        if let Err(e) = stream_result {
            let _ = tokio::fs::remove_file(&staged).await;
            return Err(e);
        }
        if total_length > 0 && downloaded_bytes != total_length {
            let _ = tokio::fs::remove_file(&staged).await;
            anyhow::bail!("update download incomplete: {downloaded_bytes}/{total_length} bytes");
        }

        // Equal fields signal the receiver to install the staged update.
        let final_total = if total_length > 0 {
            total_length
        } else {
            downloaded_bytes
        };
        if let Err(e) = tx.send((downloaded_bytes, final_total)) {
            error!("Error sending completion: {e}");
        }
        info!("DONE");
    }

    Ok(())
}
