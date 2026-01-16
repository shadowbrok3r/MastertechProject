use std::str::FromStr;

use chrono::DateTime;
use crossbeam::channel::Sender;
use database::schema::User;
use eframe::egui::{Align, Button, CentralPanel, Color32, Context, Direction, FontId, Frame, Layout, RichText, Stroke, TextEdit, Ui};
use egui_extras::{Column, TableBuilder};
use futures::StreamExt;
use log::{error, info};
use reqwest::{
    header::{HeaderName, ACCEPT, CONTENT_TYPE, USER_AGENT},
    Client,
};
use serde::{Deserialize, Serialize};

use crate::{app_state::SharedContext, markdown_editor, PlatformSpawner, Spawner};

pub struct GithubIssue {
    pub github_issue_descript: String,
    pub github_issue_title: String,
    pub user: Option<User>,
}

impl SharedContext {
    pub fn github(&mut self, ui: &mut Ui) {
        ui.style_mut().visuals.selection.stroke.color = Color32::BLACK;
        ui.style_mut().visuals.selection.bg_fill = Color32::from_rgb(120, 10, 120);
        ui.style_mut().visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(20, 20, 25);
        ui.style_mut().visuals.widgets.inactive.bg_stroke =
            Stroke::new(1.0, Color32::from_rgb(80, 80, 80));
        ui.style_mut().visuals.widgets.open.bg_fill = Color32::from_black_alpha(50);
        ui.style_mut().visuals.widgets.open.weak_bg_fill = Color32::from_black_alpha(50);
        ui.style_mut().visuals.widgets.active.weak_bg_fill = Color32::from_rgb(30, 30, 30);
        ui.style_mut().visuals.widgets.hovered.weak_bg_fill = Color32::TRANSPARENT;
        ui.style_mut().visuals.widgets.hovered.bg_fill = Color32::from_rgb(12, 12, 12);
        ui.style_mut().visuals.widgets.hovered.bg_stroke =
            Stroke::new(1.0, Color32::from_rgb(200, 20, 200));

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
                let github_issue_descript = format!(
                    "{}\nUser: {} - {}", 
                    self.github_issue_descript.clone(), 
                    current_user.get_name(), 
                    current_user.get_email()
                );

                self.github_issue_descript.clear();
                self.github_issue_title.clear();

                PlatformSpawner::spawn(async move {
                    let client = Client::new();

                    let create_issue =
                        create_new_issue(github_issue_title, github_issue_descript, client).await;

                    match create_issue {
                        Ok(val) => info!("Sent request ok: {val:?}"),
                        Err(e) => error!("Error creating issue: {e:?}"),
                    }
                });
            }
        });
    }
}

pub async fn create_new_issue(
    title: String,
    body: String,
    client: Client,
) -> anyhow::Result<String, anyhow::Error> {
    let params = serde_json::json!({ "title": title, "body": body, "assignees": ["shadowbrok3r"], "labels": ["bug"] });
    let res = client
        .post("https://api.github.com/repos/shadowbrok3r/MastertechProject/issues")
        .bearer_auth(database::ISSUE_TOKEN)
        .header(ACCEPT, "application/vnd.github+json")
        .header(USER_AGENT, "MtechServer")
        // .header("X-GitHub-Api-Version", "2022-11-28")
        .json(&params)
        .send()
        .await?
        .text()
        .await?;

    Ok(res)
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
        pub fn downloads_page(&mut self, ctx: &Context) {
        CentralPanel::default()
            .frame(
                Frame::central_panel(&ctx.style())
                    .outer_margin(10.)
                    .inner_margin(10.),
            )
            .show(ctx, |ui| {
                ui.with_layout(
                    Layout::from_main_dir_and_cross_align(Direction::TopDown, Align::Center),
                    |ui| {
                        ui.style_mut().override_font_id = Some(FontId::proportional(15.0));
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

                                                if link.clicked() {
                                                    let asset = asset.clone();
                                                    let tx = self.bytes_channel.0.clone();
                                                    PlatformSpawner::spawn(async move {
                                                        let _ = download_release(asset, tx).await;
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
    let response: Vec<GithubRelease> = client.get("https://git.master-tech.app/repos/shadowbrok3r/MastertechProject/releases") // /latest
        // .bearer_auth(database::DOWNLOAD_TOKEN)
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

pub async fn download_release(asset: Asset, tx: Sender<(Vec<u8>, u64)>) -> Result<(), anyhow::Error> {
    let file = rfd::AsyncFileDialog::new()
        .set_file_name(asset.name.clone())
        .save_file()
        .await;

    if !asset.url.is_empty() {
        let client = Client::new();
        let asset_url = asset.url.replace("api.github.com", "git.master-tech.app");

        let resp = client
            .get(&asset_url)
            // .bearer_auth(database::DOWNLOAD_TOKEN)
            .header(ACCEPT, "application/octet-stream")
            .header(CONTENT_TYPE, "application/octet-stream")
            .header(USER_AGENT, "shadowbrok3r")
            .header("X-GitHub-Api-Version", "2022-11-28")
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
