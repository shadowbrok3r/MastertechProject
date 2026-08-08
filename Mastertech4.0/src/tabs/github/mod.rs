use anyhow::{Error, Result};
use chrono::DateTime;
use crossbeam::channel::Sender;
use eframe::egui::{
    Align, Button, CentralPanel, Color32, Direction, FontId, Frame, Layout, RichText, Stroke,
    TextEdit, Ui,
};
use egui_extras::{Column, TableBuilder};
use futures::StreamExt;
use log::{debug, error};
use reqwest::{
    header::{ACCEPT, CONTENT_TYPE, USER_AGENT},
    Client,
};
use self_updater::{Asset, GithubRelease};
use tokio::spawn;
use displays::{get_toast_sender, ToastMessage};

use crate::app_state::MastertechContext;

use self::issues::create_new_issue;

pub mod issues;
pub mod self_updater;

/// Cloudflare Worker in front of GitHub API / asset redirects — CORS-safe for WASM.
const GIT_MASTER_TECH_REPO_BASE: &str =
    "https://git.master-tech.app/repos/shadowbrok3r/MastertechProject";

#[inline]
fn proxied_github_asset_url(asset_api_url: &str) -> String {
    asset_api_url.replace("api.github.com", "git.master-tech.app")
}

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
        CentralPanel::default()
            .frame(
                Frame::central_panel(&ui.ctx().global_style())
                    .outer_margin(10.)
                    .inner_margin(10.)
            )
            .show(ui, |ui| {
                ui.with_layout(
                    Layout::from_main_dir_and_cross_align(Direction::TopDown, Align::Center),
                    |ui| {
                        ui.style_mut().override_font_id = Some(FontId::monospace(15.0));
                        let releases = self.github_releases.clone();

                        TableBuilder::new(ui)
                            .striped(true)
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
                                // One row per release; offer the OS-appropriate
                                // MasterTech asset (Windows = `.exe`, others =
                                // the extension-less binary) so a multi-asset
                                // release never pairs the wrong file.
                                let want_exe = cfg!(target_os = "windows");
                                let bin_prefix = env!("CARGO_PKG_NAME").to_ascii_lowercase();
                                for release in releases.iter() {
                                    let Some(asset) = release.assets.iter().find(|a| {
                                        let name = a.name.to_ascii_lowercase();
                                        name.starts_with(&bin_prefix)
                                            && name.ends_with(".exe") == want_exe
                                    }) else {
                                        continue;
                                    };
                                    body.row(100.0, |mut row| {
                                        row.col(|ui| {
                                            ui.add_space(5.0);
                                            ui.vertical_centered(|ui| {
                                                ui.add_space(20.0);
                                                let link_txt = RichText::new(&release.name)
                                                    .color(Color32::LIGHT_RED);
                                                let link =
                                                    ui.link(link_txt).on_hover_text(&asset.name);

                                                if link.clicked() {
                                                    let asset = asset.clone();
                                                    let tx = self.bytes_channel.0.clone();
                                                    spawn(async move {
                                                        if let Err(e) = download_release(
                                                            asset,
                                                            tx,
                                                            Client::new(),
                                                        )
                                                        .await
                                                        {
                                                            error!("Download failed: {e:?}");
                                                        }
                                                    });
                                                }

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
                                            ui.label(&release.body);
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

pub async fn download_release(
    asset: Asset,
    tx: Sender<(Vec<u8>, u64)>,
    client: Client,
) -> Result<(), Error> {
    let file = rfd::AsyncFileDialog::new()
        .set_file_name(asset.name.clone())
        .save_file()
        .await;

    if !asset.url.is_empty() {
        let asset_url = proxied_github_asset_url(&asset.url);

        let resp = client
            .get(&asset_url)
            .header(ACCEPT, "application/octet-stream")
            .header(CONTENT_TYPE, "application/octet-stream")
            .header(USER_AGENT, "shadowbrok3r/Mastertech")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await?;

        let content_length = resp.content_length().unwrap_or(0);
        let mut downloaded_bytes: u64 = 0;

        let mut byte_stream = resp.bytes_stream();
        debug!("Content length: {content_length}");

        let mut byte_vec = Vec::new();

        while let Some(item) = byte_stream.next().await {
            let chunk = item?.clone();
            byte_vec.push(chunk.to_vec());
            let _ = tx.try_send((chunk.to_vec(), content_length));
            downloaded_bytes += chunk.len() as u64;
        }

        if downloaded_bytes == content_length {
            debug!("Downloaded: {downloaded_bytes}");
            let x = byte_vec.concat();
            if let Some(ref file) = file {
                file.write(x.as_slice()).await?;
            }
        }
    }

    Ok(())
}

