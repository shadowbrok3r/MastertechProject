use eframe::egui::{CentralPanel, Frame, Layout};
use egui::{Align, Direction};
use egui_extras::TableBuilder;
use gloo_net::http::Request;
use log::info;
use serde_json::Value;
use wasm_bindgen_futures::spawn_local;
use crate::app_state::MtechServer;
use crossbeam::channel::Sender;

const TOKEN: &str = "Bearer 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";

impl MtechServer{
    pub fn downloads_page(&mut self, ctx: &egui::Context){
        CentralPanel::default()
            .frame(Frame::central_panel(&ctx.style()).inner_margin(1.))
            .show(ctx, |ui| 
        {
            ui.with_layout(
                Layout::from_main_dir_and_cross_align(Direction::TopDown, Align::Center), 
                |ui|
            {
                

                let (tx, rx) = crossbeam::channel::unbounded();
                if ui.button("Get Releases").clicked(){
                    spawn_local(async move {
                        let x = run(tx).await;
                    });
                }
                
                // TableBuilder::new(ui)
                //     .columns(column, count)
            });
        });
    }
}


pub async fn run(tx: Sender<(u64, u64)>) -> anyhow::Result<(), anyhow::Error> {
    // let mut downloaded_bytes: u64 = 0;
    
    let response: Value = Request::get("https://api.github.com/repos/shadowbrok3r/Mastertech4.0/releases/latest") 
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "shadowbrok3r")
        .header("Authorization", TOKEN)
        .send()
        .await?
        .json()
        .await?;

    info!("response {response:?}");
    let releases = response.get("assets");
    // if let Some(release) = releases{
    //     let url: &str = release[0].get("url").unwrap().as_str().unwrap();
    //     let total_length: u64 = release[0].get("size").unwrap().as_u64().unwrap();
    //     info!("response: {url}\nLen: {total_length}");
    
        // if !url.is_empty(){
            // let response = client.get(url) 
            //     .header(ACCEPT, "application/octet-stream")
            //     .header(CONTENT_TYPE, "application/octet-stream")
            //     .header("X-GitHub-Api-Version", "2022-11-28")
            //     .header(USER_AGENT, "shadowbrok3r")
            //     .bearer_auth(TOKEN)
            //     .send()
            //     .await
            //     .map_err(|e| {
            //         info!("e.source() {:?}", e.source());
            //         info!("URL: {:?}", e.url());
            //         info!("{}", e.to_string());
            //     }).unwrap();

            // info!("response: {response:?}");
        // }
    // }

    Ok(())
}