use anyhow::{Error, Result};
use eframe::egui::{
    Align, CentralPanel, Color32, Context, Direction, FontId, Frame, Layout, RichText,
};
// use displays::markdown_editor::viewer::easy_mark;
use crate::app_state::MtechServer;
use chrono::DateTime;
use crossbeam::channel::Sender;
use egui_extras::{Column, TableBuilder};
use futures::StreamExt;
use gloo_net::http::Request;
use log::{debug, info};
use reqwest::{
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT},
    Client,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen_futures::spawn_local;

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

fn format_date(date_str: &str) -> String {
    let datetime = DateTime::parse_from_rfc3339(date_str).unwrap();
    let naive_date = datetime.naive_local().date();
    naive_date.format("%m/%d/%Y").to_string()
}

fn bytes_to_megabytes(bytes: u64) -> f64 {
    bytes as f64 / 1_048_576.0
}

impl MtechServer {
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
                        let releases = self.context.github_releases.clone();

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
                                                    let tx = self.context.bytes_channel.0.clone();
                                                    spawn_local(async move {
                                                        download_release(asset, tx).await;
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

pub async fn get_github_releases(tx: Sender<Vec<GithubRelease>>) -> Result<(), Error> {
    let response: Vec<GithubRelease> =
        Request::get("https://git.master-tech.app/repos/shadowbrok3r/MastertechProject/releases") // /latest
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "shadowbrok3r")
            .send()
            .await?
            .json()
            .await?;

    debug!("response {:?}", response.clone());
    tx.try_send(response.clone())?;
    Ok(())
}

pub async fn download_release(asset: Asset, tx: Sender<(Vec<u8>, u64)>) -> Result<(), Error> {
    let file = rfd::AsyncFileDialog::new()
        .set_file_name(asset.name.clone())
        .save_file()
        .await;

    if !asset.url.is_empty() {
        let client = Client::new();
        let asset_url = asset.url.replace("api.github.com", "git.master-tech.app");

        let resp = client
            .get(&asset_url)
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
