#![allow(deprecated)]
use database::schema::User;
use eframe::egui::{Align, Layout, Ui};
use log::{error, info};
use reqwest::Client;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    app_state::SharedContext,
    get_toast_sender,
    ui_tools::{
        controls::{self, ButtonKind, TextButton},
        icons::svg,
        metrics::Metrics,
    },
    PlatformSpawner, Spawner, ToastMessage,
};

/// Width of the bug-report form fields.
const FORM_W: f32 = 420.0;
/// Visible rows of the description box.
const DESCRIPTION_ROWS: usize = 8;

mod releases;
pub use releases::ReleaseBrowser;

/// Cloudflare Worker in front of GitHub API / asset redirects — CORS-safe for browser WASM.
const GIT_MASTER_TECH_REPO_BASE: &str =
    "https://git.master-tech.app/repos/shadowbrok3r/MastertechProject";

#[cfg(not(any(target_os = "ios", target_os = "android")))]
#[inline]
fn proxied_github_asset_url(asset_api_url: &str) -> String {
    asset_api_url.replace("api.github.com", "git.master-tech.app")
}

pub use mtech_ui::github::{build_github_issue_body, create_new_issue, GITHUB_ISSUE_BODY_CHAR_LIMIT};

pub struct GithubIssue {
    pub github_issue_descript: String,
    pub github_issue_title: String,
    pub user: Option<User>,
}

impl SharedContext {
    pub fn github(&mut self, ui: &mut Ui) {
        if let Some(user) = &self.current_user {
            if self.github_issue.user.is_none() {
                self.github_issue.set_user(user.clone());
            }
            self.github_issue.display(ui);
        }
    }
}

impl GithubIssue {
    pub fn new() -> Self {
        Self {
            github_issue_descript: String::new(),
            github_issue_title: String::new(),
            user: None
        }
    }

    pub fn set_user(&mut self, user: User) {
        self.user = Some(user);
    }

    fn display(&mut self, ui: &mut Ui) {
        let m = Metrics::get(ui.ctx());
        controls::panel_header(ui, svg::BUG, "MtechServer Bug Report", |_| {});
        ui.add_space(m.gap.y);
        ui.with_layout(Layout::top_down(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.y = m.gap.y;
            controls::TextField::new(&mut self.github_issue_title)
                .hint("Issue Title")
                .icon(svg::TAG_SIMPLE)
                .width(FORM_W)
                .show(ui);

            ui.add_space(m.gap.y);

            controls::subheading(ui, svg::NOTE_PENCIL, "Description");
            controls::TextArea::new(&mut self.github_issue_descript)
                .hint("Explain your issue")
                .width(FORM_W)
                .rows(DESCRIPTION_ROWS)
                .show(ui);

            let ready =
                !self.github_issue_descript.is_empty() && !self.github_issue_title.is_empty();
            let submit = TextButton::new("Submit")
                .icon(svg::PAPER_PLANE_TILT)
                .kind(ButtonKind::Primary)
                .prominent(true)
                .enabled(ready)
                .show(ui);

            if submit.clicked() {
                let github_issue_title = self.github_issue_title.clone();
                let current_user = self.user.clone().unwrap_or_default();
                
                // Get logs before clearing the form
                let logs = crate::ui_tools::egui_logger::get_logs_for_issue();
                
                let github_issue_descript = crate::tabs::github::build_github_issue_body(
                    &self.github_issue_descript,
                    &current_user.get_name(),
                    &current_user.get_email(),
                    &logs,
                );

                self.github_issue_descript.clear();
                self.github_issue_title.clear();

                PlatformSpawner::spawn(async move {
                    let client = Client::new();
                    let toast_tx = get_toast_sender();
                    match create_new_issue(github_issue_title, github_issue_descript, client).await {
                        Ok(res) => {
                            info!("GitHub issue API ok: {res:?}");
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
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct GithubRelease {
    pub url: String,
    pub html_url: String,
    #[serde(deserialize_with = "null_as_default")]
    pub name: String,
    pub tag_name: String,
    pub created_at: String,
    #[serde(deserialize_with = "null_as_default")]
    pub published_at: String,
    pub prerelease: bool,
    pub draft: bool,
    #[serde(deserialize_with = "null_as_default")]
    pub body: String,
    pub assets: Vec<Asset>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub browser_download_url: String,
    pub size: u64,
    pub created_at: String,
}

impl SharedContext {
    pub fn downloads_page(&mut self, ui: &mut Ui) {
        self.release_browser.ui(ui);
    }
}

fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}
