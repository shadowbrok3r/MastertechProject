use std::time::{Duration, Instant};

use displays::modals::update_toast::{self, UpdateChoice};
use displays::{ToastMessage, get_toast_sender};
use egui::{UserAttentionType, ViewportCommand};
use log::{debug, error, info};
use semver::Version;
use tokio::spawn;

use crate::utilities::update_policy::{Activity, PendingUpdate, SNOOZE, UpdateStep, next_step};
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
                let version = self
                    .context
                    .announced_release
                    .clone()
                    .unwrap_or_else(|| "update".to_string());
                info!("update {version} downloaded ({} bytes)", res.1);
                let accepted = std::mem::take(&mut self.context.update_requested);
                self.context.pending_update = Some(PendingUpdate::new(res.1, version, accepted));
                self.context.next_update_check = None;
            }
        }

        self.drive_pending_update(ctx);

        if let Ok(releases) = self.context.github_releases_channel.1.try_recv() {
            debug!("Releases: {releases:?}");
            let os = std::env::consts::OS;
            let current_version =
                Version::parse(env!("CARGO_PKG_VERSION")).expect("Invalid version format");

            for release in releases.iter().filter(|r| !r.draft && !r.prerelease) {
                let Ok(github_release_version) =
                    Version::parse(release.tag_name.trim_start_matches('v'))
                else {
                    debug!("skipping release with unparseable tag {:?}", release.tag_name);
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
                self.context.announced_release = Some(github_release_version.to_string());

                let toast_tx = get_toast_sender();
                let _ = toast_tx.try_send(ToastMessage::Info(format!(
                    "New release v{} found! Downloading update...",
                    github_release_version
                )));

                let tx = self.context.bytes_tx.clone();
                spawn(async move {
                    if let Err(e) = run(client, tx.clone()).await {
                        error!("self-update download failed: {e:?}");
                    }
                });
                break;
            }
            self.context.github_releases = releases;
        }
    }

    /// Installs, offers or holds a downloaded update, checking at most every two seconds.
    fn drive_pending_update(&mut self, ctx: &eframe::egui::Context) {
        if self.context.pending_update.is_none() {
            return;
        }
        let now = Instant::now();
        if self.context.next_update_check.is_some_and(|at| now < at) {
            return;
        }
        self.context.next_update_check = Some(now + Duration::from_secs(2));

        let reason = Activity::current().reason();
        let Some(pending) = self.context.pending_update.as_mut() else {
            return;
        };
        match update_toast::take_choice() {
            Some(UpdateChoice::Now) => pending.accepted = true,
            Some(UpdateChoice::Later) => pending.snoozed_until = Some(now + SNOOZE),
            None => {}
        }

        match next_step(self.context.update_mode, reason.is_some(), pending.accepted) {
            UpdateStep::Wait => {
                if pending.waiting_on != reason {
                    let why = reason.as_deref().unwrap_or_default();
                    info!("update {} waits: {why}", pending.version);
                    if pending.accepted && pending.waiting_on.is_none() {
                        let _ = get_toast_sender().try_send(ToastMessage::Info(format!(
                            "MasterTech will update once nothing is running ({why})."
                        )));
                    }
                    pending.waiting_on = reason;
                }
            }
            UpdateStep::Prompt => {
                pending.waiting_on = None;
                if pending.snoozed_until.is_some_and(|until| now < until) {
                    return;
                }
                if update_toast::post(&mut self.context.shared_ctx.toasts, &pending.version) {
                    ctx.send_viewport_cmd(ViewportCommand::RequestUserAttention(
                        UserAttentionType::Informational,
                    ));
                }
            }
            UpdateStep::Install => {
                update_toast::retire();
                let size = pending.size;
                self.context.pending_update = None;
                install_update(ctx, size);
            }
        }
    }
}

/// Swaps in the staged executable and relaunches MasterTech.
fn install_update(ctx: &eframe::egui::Context, size: u64) {
    #[cfg(target_os = "windows")]
    {
        use crate::utilities::safe_swap;
        let toast_tx = get_toast_sender();
        let applied = safe_swap::staged_update_path()
            .and_then(|staged| safe_swap::apply_staged_update(&staged, size));
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
    #[cfg(not(target_os = "windows"))]
    let _ = (ctx, size);
}
