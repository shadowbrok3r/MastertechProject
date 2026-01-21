use egui::ViewportCommand;
use log::{debug, info};
use semver::Version;
use tokio::spawn;
use displays::{get_toast_sender, ToastMessage};

use crate::{app_state::MasterTechApp, tabs::github::self_updater::{run, Asset}};

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
            self.context.progress.0 += res.0 as f32;
            
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
            
            if res.0 == res.1 {
                // Download complete
                let toast_tx = get_toast_sender();
                let _ = toast_tx.try_send(ToastMessage::Success(
                    "Update downloaded! Installing and restarting...".to_string()
                ));
                
                self.context.progress = (0.0, 0.0);
                #[cfg(target_os = "windows")]
                {
                    let current_exe = std::env::current_dir().unwrap().join("git-MasterTech.exe");
                    use std::os::windows::process::CommandExt;
                    std::process::Command::new("cmd")
                        .arg("/C")
                        .arg(&current_exe)
                        .creation_flags(0x00000010) // CREATE_NEW_CONSOLE flag
                        .creation_flags(0x00000008) // DETACHED_PROCESS flag
                        .spawn()
                        .unwrap();
                    let replacement = self_replace::self_replace(&current_exe);
                    log::info!("Replacement: {replacement:?}");
                    let rm = std::fs::remove_file(&current_exe);
                    log::info!("Removal: {rm:?}");
                }

                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }

        if let Ok(releases) = self.context.github_releases_channel.1.try_recv() {
            debug!("Releases: {releases:?}");
            let assets: Vec<Asset> = releases
                .iter()
                .flat_map(|r| r.assets.iter().cloned())
                .collect();

            let os = std::env::consts::OS;

            for (release, asset) in releases.iter().zip(assets.iter()) {
                let current_version = Version::parse(env!("CARGO_PKG_VERSION")).expect("Invalid version format");
                let github_release_version = Version::parse(&release.tag_name).expect("Invalid version format");
                info!("TagName: {:?}", release.tag_name);

                if current_version < github_release_version {
                    let is_compatible_asset = match os {
                        "windows" => asset.name.ends_with(".exe"),
                        "linux" => asset.name.ends_with("-linux"),
                        _ => false,
                    };

                    if is_compatible_asset {
                        let client = self.context.client.clone();
                        info!("Found a new release! {:?}", &github_release_version);
                        
                        // Show toast notification for new release found
                        let toast_tx = get_toast_sender();
                        let _ = toast_tx.try_send(ToastMessage::Info(
                            format!("New release v{} found! Downloading update...", github_release_version)
                        ));
                        
                        let tx = self.context.bytes_tx.clone();

                        spawn(async move {
                            let download = run(client, tx.clone()).await;
                            info!("Download: {download:?}");
                        });
                    }
                }
            }
            self.context.github_releases = releases;
        }
    }
}