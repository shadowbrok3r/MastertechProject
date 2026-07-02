#![allow(deprecated)]
use std::str::FromStr;

use chrono::DateTime;
use crossbeam::channel::Sender;
use database::schema::User;
use eframe::egui::{Align, Button, CentralPanel, Color32, Context, Direction, FontId, Frame, Layout, RichText, Stroke, TextEdit, Ui};
use egui_extras::{Column, TableBuilder};
#[cfg(not(any(target_os = "ios", target_os = "android")))]
use futures::StreamExt;
use log::{error, info};
#[cfg(not(any(target_os = "ios", target_os = "android")))]
use reqwest::header::CONTENT_TYPE;
use reqwest::{
    header::{HeaderName, ACCEPT, USER_AGENT},
    Client,
};
use serde::{Deserialize, Serialize};

use crate::{app_state::SharedContext, get_toast_sender, markdown_editor, PlatformSpawner, Spawner, ToastMessage};

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
        ui.style_mut().visuals.selection.stroke.color = Color32::BLACK;
        ui.style_mut().visuals.selection.bg_fill = Color32::from_rgb(120, 10, 120);
        ui.style_mut().visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
        ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(20, 20, 25);
        ui.style_mut().visuals.widgets.inactive.bg_stroke =
            Stroke::new(1.0_f32, Color32::from_rgb(80, 80, 80));
        ui.style_mut().visuals.widgets.open.bg_fill = Color32::from_black_alpha(50);
        ui.style_mut().visuals.widgets.open.weak_bg_fill = Color32::from_black_alpha(50);
        ui.style_mut().visuals.widgets.active.weak_bg_fill = Color32::from_rgb(30, 30, 30);
        ui.style_mut().visuals.widgets.hovered.weak_bg_fill = Color32::TRANSPARENT;
        ui.style_mut().visuals.widgets.hovered.bg_fill = Color32::from_rgb(12, 12, 12);
        ui.style_mut().visuals.widgets.hovered.bg_stroke =
            Stroke::new(1.0_f32, Color32::from_rgb(200, 20, 200));

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
        ui.with_layout(Layout::top_down(Align::Center), |ui| {
            // vertical_centered(|ui| {

            ui.heading("MtechServer Bug Report");
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
pub struct GithubRelease {
    pub url: String,
    pub html_url: String,
    pub name: String,
    pub created_at: String,
    pub body: String,
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

impl SharedContext {
        pub fn downloads_page(&mut self, ui: &mut eframe::egui::Ui) {
        CentralPanel::default()
            .frame(
                Frame::central_panel(&ui.ctx().global_style())
                    .outer_margin(10.)
                    .inner_margin(10.),
            )
            .show(ui, |ui| {
                ui.with_layout(
                    Layout::from_main_dir_and_cross_align(Direction::TopDown, Align::Center),
                    |ui| {
                        ui.style_mut().override_font_id = Some(FontId::monospace(15.0));
                        let releases = self.github_releases.clone();

                        TableBuilder::new(ui)
                            .striped(false)
                            .cell_layout(Layout::top_down_justified(Align::Min))
                            .cell_layout(Layout::top_down_justified(Align::Min))
                            .cell_layout(Layout::top_down_justified(Align::Min))
                            .column(Column::exact(180.0))
                            .column(Column::exact(130.0))
                            .column(Column::remainder().resizable(true))
                            .header(20.0, |mut header| {
                                header.col(|ui| {
                                    ui.heading("Release Name");
                                });
                                header.col(|ui| {
                                    ui.heading("Created At");
                                });
                                header.col(|ui| {
                                    ui.heading("Description");
                                });
                            })
                            .body(|mut body| {
                                let assets: Vec<Asset> = releases
                                    .iter()
                                    .flat_map(|r| r.assets.iter().cloned())
                                    .collect();
                                for (release, asset) in releases.iter().zip(assets.iter()) {
                                    body.row(100.0, |mut row| {
                                        row.col(|ui| {
                                            ui.add_space(5.0);
                                            ui.vertical_centered(|ui| {
                                                ui.add_space(20.0);
                                                let link_txt = RichText::new(&release.name)
                                                    .color(Color32::from_rgb(113, 156, 202));
                                                let link =
                                                    ui.link(link_txt).on_hover_text(&asset.name);

                                                #[cfg(not(any(target_os = "ios", target_os = "android")))]
                                                if link.clicked() {
                                                    let asset = asset.clone();
                                                    let tx = self.bytes_channel.0.clone();
                                                    PlatformSpawner::spawn(async move {
                                                        let _ = download_release(asset, tx).await;
                                                    });
                                                }
                                                #[cfg(any(target_os = "ios", target_os = "android"))]
                                                let _ = link;

                                                ui.add_space(10.0);
                                                ui.label(&asset.name);
                                            });
                                        });

                                        row.col(|ui| {
                                            ui.horizontal_centered(|ui| {
                                                ui.add_space(5.0);
                                                ui.label(format_date(&release.created_at));
                                            });
                                        });
                                        row.col(|ui| {
                                            ui.add_space(5.0);
                                            markdown_editor::viewer::easy_mark(ui, &release.body);
                                        });
                                    });
                                }
                            });
                    },
                );
            });
    }
}

fn format_date(date_str: &str) -> String {
    let datetime = DateTime::parse_from_rfc3339(date_str).unwrap();
    let naive_date = datetime.naive_local().date();
    naive_date.format("%m/%d/%Y").to_string()
}

fn _bytes_to_megabytes(bytes: u64) -> f64 {
    bytes as f64 / 1_048_576.0
}

pub async fn get_github_releases(tx: Sender<Vec<GithubRelease>>) -> Result<(), anyhow::Error> {
    let client = Client::new();
    let response: Vec<GithubRelease> = client
        .get(format!("{GIT_MASTER_TECH_REPO_BASE}/releases"))
        .header(ACCEPT, "application/vnd.github+json")
        .header(HeaderName::from_str("X-GitHub-Api-Version").unwrap(), "2022-11-28")
        .header(USER_AGENT, "shadowbrok3r")
        .send()
        .await?
        .json()
        .await?;

    log::info!("response {:?}", response.clone());
    tx.try_send(response.clone())?;
    Ok(())
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub async fn download_release(asset: Asset, tx: Sender<(Vec<u8>, u64)>) -> Result<(), anyhow::Error> {
    let file = rfd::AsyncFileDialog::new()
        .set_file_name(asset.name.clone())
        .save_file()
        .await;

    if !asset.url.is_empty() {
        let client = Client::new();

        let asset_url = proxied_github_asset_url(&asset.url);

        let resp = client
            .get(&asset_url)
            .header(ACCEPT, "application/octet-stream")
            .header(CONTENT_TYPE, "application/octet-stream")
            .header(USER_AGENT, "shadowbrok3r/Mastertech")
            .header(HeaderName::from_str("X-GitHub-Api-Version").unwrap(), "2022-11-28")
            .send()
            .await?;

        let content_length = resp.content_length().unwrap_or(0);
        let mut downloaded_bytes: u64 = 0;

        let mut byte_stream = resp.bytes_stream();
        info!("Content length: {content_length}");

        let mut byte_vec = Vec::new();

        while let Some(item) = byte_stream.next().await {
            let chunk = item?.clone();
            byte_vec.push(chunk.to_vec());
            let _ = tx.try_send((chunk.to_vec(), content_length));
            downloaded_bytes += chunk.len() as u64;
        }

        if downloaded_bytes == content_length {
            info!("Downloaded: {downloaded_bytes}");
            let x = byte_vec.concat();
            if let Some(ref file) = file {
                file.write(x.as_slice()).await?;
            }
        }
    }

    Ok(())
}
