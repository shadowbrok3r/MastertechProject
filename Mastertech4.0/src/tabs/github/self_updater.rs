use crossbeam::channel::Sender;
use futures::StreamExt;
use log::{debug, error};
use mtech_update::{HostOs, pick_update_asset};
use reqwest::{
    header::{ACCEPT, CONTENT_TYPE, USER_AGENT},
    Client,
};
use semver::Version;
use tokio::{fs::File, io::AsyncWriteExt};

pub use displays::tabs::github::{Asset, GithubRelease};

/// Proxied GitHub API base (Cloudflare Worker — CORS for WASM).
const GIT_MASTER_TECH_REPO_BASE: &str =
    "https://git.master-tech.app/repos/shadowbrok3r/MastertechProject";

#[inline]
fn proxied_github_asset_url(asset_api_url: &str) -> String {
    asset_api_url.replace("api.github.com", "git.master-tech.app")
}

/// Stages the newest release with a binary for this OS; `Ok(false)` when MasterTech is up to date.
pub async fn run(client: Client, tx: Sender<(u64, u64)>) -> anyhow::Result<bool> {
    let releases: Vec<GithubRelease> = client
        .get(format!("{GIT_MASTER_TECH_REPO_BASE}/releases"))
        .header(ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header(USER_AGENT, "shadowbrok3r/Mastertech")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let current = Version::parse(env!("CARGO_PKG_VERSION"))?;
    let Some((release, _)) = newer_release(&releases, &current, HostOs::current()) else {
        return Ok(false);
    };
    run_release(client, tx, release.clone()).await?;
    Ok(true)
}

/// First non-draft, non-prerelease release newer than `current` that ships a MasterTech binary for `os`.
pub(crate) fn newer_release<'a>(
    releases: &'a [GithubRelease],
    current: &Version,
    os: HostOs,
) -> Option<(&'a GithubRelease, Version)> {
    releases
        .iter()
        .filter(|r| !r.draft && !r.prerelease)
        .find_map(|release| {
            let Ok(version) = Version::parse(release.tag_name.trim_start_matches('v')) else {
                debug!("skipping release with unparseable tag {:?}", release.tag_name);
                return None;
            };
            let has_asset =
                pick_update_asset(release.assets.iter().map(|a| a.name.as_str()), os).is_some();
            (version > *current && has_asset).then_some((release, version))
        })
}

/// Stages `release`'s binary for this OS, reporting `(done, total)` on `tx`; `Ok` once equal fields are sent.
pub async fn run_release(
    client: Client,
    tx: Sender<(u64, u64)>,
    release: GithubRelease,
) -> anyhow::Result<(), anyhow::Error> {
    let mut downloaded_bytes: u64 = 0;

    let asset_names = release.assets.iter().map(|a| a.name.as_str());
    let Some(asset0) = pick_update_asset(asset_names, HostOs::current()).map(|i| &release.assets[i])
    else {
        anyhow::bail!(
            "release {} has no MasterTech asset for {:?}",
            release.tag_name,
            HostOs::current()
        );
    };
    if asset0.url.is_empty() {
        anyhow::bail!("release {} asset {} has no download URL", release.tag_name, asset0.name);
    }
    let total_length = asset0.size;
    debug!("update asset {} ({total_length} bytes)", asset0.url);

    let url = proxied_github_asset_url(&asset0.url);

    let response = client
        .get(&url)
        .header(ACCEPT, "application/octet-stream")
        .header(CONTENT_TYPE, "application/octet-stream")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header(USER_AGENT, "shadowbrok3r/Mastertech")
        .send()
        .await?
        .error_for_status()?;

    let staged = crate::utilities::safe_swap::staged_update_path()?;
    debug!("staged update path: {staged:?}");
    let mut staged_file = match File::create(&staged).await {
        Ok(file) => file,
        Err(e) => {
            #[cfg(unix)]
            toast_failure(format!(
                "Can't save the update next to MasterTech ({e}). Move MasterTech to a folder you own."
            ));
            return Err(e.into());
        }
    };

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

    if stream_result.is_ok()
        && let Err(e) = staged_file.flush().await
    {
        stream_result = Err(e.into());
    }
    drop(staged_file);

    if let Err(e) = stream_result {
        let _ = tokio::fs::remove_file(&staged).await;
        return Err(e);
    }
    if downloaded_bytes == 0 || (total_length > 0 && downloaded_bytes != total_length) {
        let _ = tokio::fs::remove_file(&staged).await;
        anyhow::bail!("update download incomplete: {downloaded_bytes}/{total_length} bytes");
    }

    #[cfg(unix)]
    {
        let prepared = crate::utilities::safe_swap::prepare_staged_blocking(
            staged.clone(),
            release.tag_name.clone(),
        )
        .await;
        if let Err(e) = prepared {
            let _ = tokio::fs::remove_file(&staged).await;
            toast_failure(format!("Downloaded update can't run on this machine: {e:#}"));
            return Err(e);
        }
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

    Ok(())
}

/// Error toast for an update this machine cannot stage or run.
#[cfg(unix)]
fn toast_failure(text: String) {
    let _ = displays::get_toast_sender().try_send(displays::ToastMessage::Error(text));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, assets: &[&str]) -> GithubRelease {
        GithubRelease {
            tag_name: tag.to_string(),
            assets: assets
                .iter()
                .map(|name| Asset {
                    name: name.to_string(),
                    ..Asset::default()
                })
                .collect(),
            ..GithubRelease::default()
        }
    }

    fn picked(releases: &[GithubRelease], current: &str, os: HostOs) -> Option<String> {
        let current = Version::parse(current).unwrap();
        newer_release(releases, &current, os).map(|(_, version)| version.to_string())
    }

    #[test]
    fn picks_the_newest_release_with_a_binary_for_each_os() {
        let releases = [
            release("5.0.3", &["Mastertech.exe", "qc_app.exe"]),
            release("5.0.2", &["Mastertech-linux", "Mastertech.exe", "qc_app.exe"]),
            release("5.0.1", &["Mastertech-linux", "Mastertech.exe"]),
        ];
        assert_eq!(picked(&releases, "5.0.1", HostOs::Windows).as_deref(), Some("5.0.3"));
        assert_eq!(picked(&releases, "5.0.1", HostOs::Linux).as_deref(), Some("5.0.2"));
        assert_eq!(picked(&releases, "5.0.1", HostOs::Other), None);
    }

    #[test]
    fn skips_drafts_prereleases_older_and_unparseable_tags() {
        let mut draft = release("6.0.0", &["Mastertech.exe", "Mastertech-linux"]);
        draft.draft = true;
        let mut pre = release("5.9.0", &["Mastertech.exe", "Mastertech-linux"]);
        pre.prerelease = true;
        let releases = [
            draft,
            pre,
            release("nightly", &["Mastertech.exe", "Mastertech-linux"]),
            release("5.0.1", &["Mastertech.exe", "Mastertech-linux"]),
            release("v5.0.0", &["Mastertech.exe", "Mastertech-linux"]),
        ];
        assert_eq!(picked(&releases, "5.0.1", HostOs::Windows), None);
        assert_eq!(picked(&releases, "5.0.1", HostOs::Linux), None);
        assert_eq!(picked(&releases, "4.9.0", HostOs::Linux).as_deref(), Some("5.0.1"));
    }

    #[test]
    fn strips_a_leading_v_and_ignores_qc_app_only_releases() {
        let releases = [
            release("5.1.0", &["qc_app.exe"]),
            release("v5.0.2", &["Mastertech.exe"]),
        ];
        assert_eq!(picked(&releases, "5.0.1", HostOs::Windows).as_deref(), Some("5.0.2"));
    }
}
