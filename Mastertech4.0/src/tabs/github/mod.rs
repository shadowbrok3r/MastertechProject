use anyhow::{Error, Result};
use crossbeam::channel::Sender;
use eframe::egui::{Align, Button, Color32, Layout, Stroke, TextEdit, Ui};
use log::{debug, error};
use reqwest::Client;
use self_updater::GithubRelease;
use tokio::spawn;
use displays::{get_toast_sender, ToastMessage};

use crate::app_state::MastertechContext;

use self::issues::create_new_issue;

pub mod issues;
pub mod self_updater;

/// Cloudflare Worker in front of GitHub API / asset redirects — CORS-safe for WASM.
const GIT_MASTER_TECH_REPO_BASE: &str =
    "https://git.master-tech.app/repos/shadowbrok3r/MastertechProject";

impl MastertechContext {
    pub fn github(&mut self, ui: &mut Ui) {
        ui.style_mut().visuals.selection.stroke.color = Color32::BLACK;
        ui.style_mut().visuals.selection.bg_fill = Color32::from_rgb(120, 10, 120);
        ui.style_mut().visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(20, 20, 25);
        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(80, 80, 80));
        ui.style_mut().visuals.widgets.open.bg_fill = Color32::from_black_alpha(50);
        ui.style_mut().visuals.widgets.open.weak_bg_fill = Color32::from_black_alpha(50);
        ui.style_mut().visuals.widgets.active.weak_bg_fill = Color32::from_rgb(30, 30, 30);
        ui.style_mut().visuals.widgets.hovered.weak_bg_fill = Color32::TRANSPARENT;
        ui.style_mut().visuals.widgets.hovered.bg_fill = Color32::from_rgb(12, 12, 12);
        ui.style_mut().visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(200, 20, 200));

        ui.with_layout(Layout::top_down(Align::Center), |ui| {
            // vertical_centered(|ui| {

            ui.heading("Mastertech bug report");
            TextEdit::singleline(&mut self.github_issue_title)
                .hint_text("Issue Title")
                .show(ui);

            ui.add_space(12.0);

            ui.heading("Description");
            TextEdit::multiline(&mut self.github_issue_descript)
                .hint_text("Explain your issue")
                .show(ui);

            let submit = ui.add_enabled(
                !self.github_issue_descript.is_empty() && !self.github_issue_title.is_empty(),
                Button::new("Submit"),
            );

            if submit.clicked() {
                let github_issue_title = self.github_issue_title.clone();
                let current_user = self.shared_ctx.current_user.clone().unwrap_or_default();
                
                // Get logs before clearing the form
                let logs = displays::ui_tools::egui_logger::get_logs_for_issue();
                
                let github_issue_descript = displays::tabs::github::build_github_issue_body(
                    &self.github_issue_descript,
                    &current_user.get_name(),
                    &current_user.get_email(),
                    &logs,
                );
                let client = self.client.clone();

                // Clear the form fields immediately
                self.github_issue_title.clear();
                self.github_issue_descript.clear();

                spawn(async move {
                    let create_issue = create_new_issue(
                        github_issue_title,
                        github_issue_descript,
                        client,
                    )
                    .await;

                    let toast_tx = get_toast_sender();
                    match create_issue {
                        Ok(val) => {
                            debug!("Issue created: {val:?}");
                            let _ = toast_tx.try_send(ToastMessage::Success(
                                "GitHub issue submitted successfully".to_string(),
                            ));
                        }
                        Err(e) => {
                            error!("Error creating issue: {e:?}");
                            let _ = toast_tx.try_send(ToastMessage::Error(format!(
                                "Failed to submit issue: {e:?}"
                            )));
                        }
                    }
                });
            }
        });
    }

    pub fn downloads_page(&mut self, ui: &mut Ui) {
        self.shared_ctx.release_browser.ui(ui);
    }
}

pub async fn get_github_releases(
    tx: Sender<Vec<GithubRelease>>,
    client: Client,
) -> Result<(), Error> {
    let response: Vec<GithubRelease> = client
        .get(format!("{GIT_MASTER_TECH_REPO_BASE}/releases"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "shadowbrok3r/Mastertech")
        .send()
        .await?
        .json()
        .await?;
    tx.try_send(response.clone())?;
    Ok(())
}
