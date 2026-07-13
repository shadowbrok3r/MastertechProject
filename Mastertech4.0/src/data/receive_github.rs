use egui::ViewportCommand;
use log::{debug, error, info};
use semver::Version;
use tokio::spawn;
use displays::{get_toast_sender, ToastMessage};

use crate::{app_state::MasterTechApp, tabs::github::self_updater::run};

impl MasterTechApp {
    pub fn receive_github(&mut self, ctx: &eframe::egui::Context) {
        // Track download progress for toast notifications
        while let Ok(res) = self.context.bytes_rx.try_recv() {
            ctx.request_repaint();
            
            // Calculate previous progress percentage before updating
            let prev_pct = if self.context.progress.1 > 0.0 {
                (self.context.progress.0 / self.context.progress.1 * 100.0) as u32
            } else {
                0
            };
            
            self.context.progress.1 = res.1 as f32;
            self.context.progress.0 = res.0 as f32;
            
            // Calculate current progress percentage
            let current_pct = if self.context.progress.1 > 0.0 {
                (self.context.progress.0 / self.context.progress.1 * 100.0) as u32
            } else {
                0
            };
            
            // Show progress toast when crossing milestones (25%, 50%, 75%)
            for milestone in [25u32, 50, 75] {
                if prev_pct < milestone && current_pct >= milestone {
                    let toast_tx = get_toast_sender();
                    let _ = toast_tx.try_send(ToastMessage::Info(
                        format!("Downloading update... {}%", milestone)
                    ));
                }
            }
            
            if res.1 > 0 && res.0 == res.1 {
                self.context.progress = (0.0, 0.0);
                #[cfg(target_os = "windows")]
                {
                    use crate::utilities::safe_swap;
                    let toast_tx = get_toast_sender();
                    let applied = safe_swap::staged_update_path()
                        .and_then(|staged| safe_swap::apply_staged_update(&staged, res.1));
                    match applied {
                        Ok(exe) => match safe_swap::relaunch(&exe, &[]) {
                            Ok(()) => {
                                let _ = toast_tx.try_send(ToastMessage::Success(
                                    "Update installed! Restarting...".to_string(),
                                ));
                                ctx.send_viewport_cmd(ViewportCommand::Close);
                            }
                            Err(e) => {
                                log::error!("update installed but relaunch failed: {e:?}");
                                let _ = toast_tx.try_send(ToastMessage::Warning(
                                    "Update installed — restart MasterTech to finish.".to_string(),
                                ));
                            }
                        },
                        Err(e) => {
                            log::error!("self-update failed: {e:?}");
                            if let Ok(staged) = safe_swap::staged_update_path() {
                                let _ = std::fs::remove_file(staged);
                            }
                            let _ = toast_tx.try_send(ToastMessage::Error(format!(
                                "Update failed — still running the current version. {e}"
                            )));
                        }
                    }
                }
            }
        }

        if let Ok(releases) = self.context.github_releases_channel.1.try_recv() {
            debug!("Releases: {releases:?}");
            let os = std::env::consts::OS;
            let current_version =
                Version::parse(env!("CARGO_PKG_VERSION")).expect("Invalid version format");

            for release in releases.iter() {
                info!("TagName: {:?}", release.tag_name);
                let Ok(github_release_version) =
                    Version::parse(release.tag_name.trim_start_matches('v'))
                else {
                    error!("skipping release with unparseable tag {:?}", release.tag_name);
                    continue;
                };
                if current_version >= github_release_version {
                    continue;
                }

                let has_compatible_asset = release.assets.iter().any(|asset| match os {
                    "windows" => asset.name.ends_with(".exe"),
                    "linux" => asset.name.ends_with("-linux"),
                    _ => false,
                });
                if !has_compatible_asset {
                    continue;
                }

                let client = self.context.client.clone();
                info!("Found a new release! {:?}", &github_release_version);

                let toast_tx = get_toast_sender();
                let _ = toast_tx.try_send(ToastMessage::Info(format!(
                    "New release v{} found! Downloading update...",
                    github_release_version
                )));

                let tx = self.context.bytes_tx.clone();
                spawn(async move {
                    let download = run(client, tx.clone()).await;
                    info!("Download: {download:?}");
                });
                break;
            }
            self.context.github_releases = releases;
        }
    }
}