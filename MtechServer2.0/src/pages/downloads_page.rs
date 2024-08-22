use eframe::egui::{Align, CentralPanel, Context, Direction, FontId, Frame, Layout};
// use displays::markdown_editor::viewer::easy_mark;
use egui_extras::{Column, TableBuilder};
use serde::{Deserialize, Serialize};
use crate::app_state::MtechServer;
use crossbeam::channel::Sender;
use gloo_net::http::Request;
use chrono::DateTime;
use log::info;

const TOKEN: &str = "Bearer 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";

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


impl MtechServer{
    pub fn downloads_page(&mut self, ctx: &Context){
        CentralPanel::default()
            .frame(Frame::central_panel(&ctx.style()).outer_margin(10.).inner_margin(10.))
            .show(ctx, |ui| 
        {
            ui.with_layout(
                Layout::from_main_dir_and_cross_align(Direction::TopDown, Align::Center), 
                |ui|
            {

                ui.style_mut().override_font_id = Some(FontId::proportional(15.0));
                let releases = self.context.github_releases.clone();

                TableBuilder::new(ui)
                    .striped(true)
                    .cell_layout(Layout::top_down_justified(Align::Center))
                    .column(Column::auto().resizable(true))
                    .column(Column::auto().resizable(true))
                    .column(Column::remainder().resizable(true))
                    .header(20.0, |mut header| 
                {
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
                    for release in releases.iter() {
                        body.row(100.0,  |mut row| {
                            // let row_index = row.index();
                            row.col(|ui| {
                                // for asset in &release.assets {format!("{} Mb", bytes_to_megabytes(asset.size));}
                                ui.horizontal_centered(|ui| {
                                    ui.add_space(15.0);
                                    let link = ui.link(&release.name); // .on_hover_text(text)
                                    if link.clicked() { }// download asset asset.browser_download_url
                                });
                            });

                            row.col(|ui| {
                                ui.horizontal_centered(|ui| {
                                    // ui.add_space(ui.available_width() * 0.2);
                                    ui.add_space(15.0);
                                    ui.label(format_date(&release.created_at));
                                });
                            });
                            row.col(|ui| {
                                ui.label(&release.body);
                                // easy_mark(ui, );
                            });
                        });
                    }
                });
            });
        });
    }
}


pub async fn get_github_releases(tx: Sender<Vec<GithubRelease>>) -> anyhow::Result<(), anyhow::Error> {
    // let mut downloaded_bytes: u64 = 0;
    
    let response: Vec<GithubRelease> = Request::get("https://api.github.com/repos/shadowbrok3r/MastertechProject/releases") // /latest 
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "shadowbrok3r")
        .header("Authorization", TOKEN)
        .send()
        .await?
        .json()
        .await?;

    info!("response {:?}", response.clone());
    tx.try_send(response.clone())?;
    // let releases = response.get("assets");
    // if let Some(release) = releases{
    //     tx.try_send(response.clone())?;
        // let url: &str = release[0].get("url").unwrap().as_str().unwrap();
        // let total_length: u64 = release[0].get("size").unwrap().as_u64().unwrap();
        // info!("response: {url}\nLen: {total_length}");
    
        // if !url.is_empty(){
        //     let response: Value = Request::get(url) 
        //         .header("Accept", "application/octet-stream")
        //         .header("Content-Type", "application/octet-stream")
        //         .header("X-GitHub-Api-Version", "2022-11-28")
        //         .header("User-Agent", "shadowbrok3r")
        //         .header("Authorization", TOKEN)
        //         .send()
        //         .await?
        //         .json()
        //         .await?;

        //     tx.try_send(response)?;
        //     // info!("response: {response:?}");
        // }
    // }

    Ok(())
}