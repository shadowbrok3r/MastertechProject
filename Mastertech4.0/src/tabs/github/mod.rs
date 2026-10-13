use anyhow::{Error, Result};
use crossbeam::channel::Sender;
use displays::{
    ToastMessage, get_toast_sender,
    ui_tools::{
        controls::{self, ButtonKind, Metrics, TextArea, TextButton, TextField},
        glass_card,
        icons::svg,
    },
};
use eframe::egui::{Align, Layout, Ui};
use log::{debug, error};
use reqwest::Client;
use self_updater::GithubRelease;
use tokio::spawn;

use crate::app_state::MastertechContext;

use self::issues::create_new_issue;

pub mod issues;
pub mod self_updater;

/// Cloudflare Worker in front of GitHub API / asset redirects — CORS-safe for WASM.
const GIT_MASTER_TECH_REPO_BASE: &str =
    "https://git.master-tech.app/repos/shadowbrok3r/MastertechProject";
/// Widest the bug-report form grows.
const FORM_W: f32 = 640.0;
/// Rows the description box shows.
const DESCRIPTION_ROWS: usize = 8;
impl MastertechContext {
    pub fn github(&mut self, ui: &mut Ui) {
        let m = Metrics::get(ui.ctx());
        ui.vertical(|ui| {
            ui.set_max_width(FORM_W.min(ui.available_width()));
            glass_card::titled_group(ui, svg::BUG, "Mastertech bug report", None, |ui| {
                controls::section_label(ui, "Title");
                TextField::new(&mut self.github_issue_title)
                    .hint("Issue Title")
                    .icon(svg::TAG_SIMPLE)
                    .width(ui.available_width())
                    .id_salt("github.issue_title")
                    .show(ui);

                ui.add_space(m.gap.y * 2.0);

                controls::section_label(ui, "Description");
                TextArea::new(&mut self.github_issue_descript)
                    .hint("Explain your issue")
                    .rows(DESCRIPTION_ROWS)
                    .show(ui);

                ui.add_space(m.gap.y);
                let ready =
                    !self.github_issue_descript.is_empty() && !self.github_issue_title.is_empty();
                let submit = ui
                    .with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let submit = TextButton::new("Submit")
                            .icon(svg::PAPER_PLANE_RIGHT)
                            .kind(ButtonKind::Primary)
                            .prominent(true)
                            .enabled(ready)
                            .show(ui);
                        controls::icon_label(ui, svg::PAPERCLIP, "Recent logs are attached");
                        submit
                    })
                    .inner;

                if submit.clicked() {
                    let github_issue_title = self.github_issue_title.clone();
                    let current_user = self.shared_ctx.current_user.clone().unwrap_or_default();

                    // Get logs before clearing the form
                    let logs = displays::ui_tools::egui_logger::get_logs_for_issue();

                    let github_issue_descript = displays::tabs::github::build_github_issue_body(
                        &self.github_issue_descript,
                        current_user.get_name(),
                        current_user.get_email(),
                        &logs,
                    );
                    let client = self.client.clone();

                    // Clear the form fields immediately
                    self.github_issue_title.clear();
                    self.github_issue_descript.clear();

                    spawn(async move {
                        let create_issue =
                            create_new_issue(github_issue_title, github_issue_descript, client)
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
