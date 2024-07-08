use eframe::egui::Ui;
use egui::{Button, CentralPanel, Color32, Frame, Margin, Rangef, Rounding, Shadow, SidePanel, Stroke, TopBottomPanel, Vec2, Widget};
use log::info;
use mtechserver::webworker::Input;
use reqwest_wasm::{header::CONTENT_TYPE, Client, Url};
use rusty_s3::{actions::{GetObject, ListObjectsV2}, Bucket, Credentials, S3Action};
use wasm_bindgen_futures::spawn_local;
use web_time::Duration;
use crate::app_state::{MtechServerContext, ACCESS_KEY, SECRET_KEY};

pub mod storage_api;


const ONE_HOUR: Duration = Duration::from_secs(3600);

impl MtechServerContext {
    pub fn toolbox(&mut self, ui: &mut Ui){
        ui.ctx().request_repaint();
        
        let mut shadow = Shadow::default();
        shadow.blur = 10.0;
        shadow.spread = 2.0;
        shadow.color = Color32::from_rgb_additive(20, 1, 20);

        let top_panel_frame = Frame::default().fill(Color32::from_rgb(8, 7, 10))
            .inner_margin(Margin::same(8.0))
            .rounding(Rounding::same(5.0)).shadow(shadow)
            .stroke(Stroke::new(1.0, Color32::from_rgb_additive(36, 156, 158)));


        let mut inner_margin = Margin::default();
        inner_margin.top = 6.0;
        inner_margin.left = 3.0;
        inner_margin.right = 3.0;

        ui.style_mut().visuals.window_rounding = Rounding::same(10.0);

        let side_panel_frame = Frame::default().fill(Color32::from_rgb(8, 7, 10))
            .inner_margin(inner_margin)
            .rounding(Rounding::same(5.0)).shadow(shadow)
            .stroke(Stroke::new(1.0, Color32::from_rgb_additive(36, 156, 158)));

        let mut shadow_central = Shadow::default();
        shadow_central.blur = 10.0;
        shadow_central.spread = 2.0;
        shadow_central.color = Color32::from_rgb_additive(36, 156, 158);

        let panel_frame = Frame::default()
            .fill(Color32::from_rgb(8, 7, 10))
            .inner_margin(Margin::same(5.0))
            .rounding(Rounding::same(5.0))
            .shadow(shadow_central)
            .stroke(Stroke::new(1.0, Color32::from_rgb_additive(20, 1, 20)));
        
        ui.style_mut().spacing.button_padding = Vec2::new(10.0, 3.0);

        TopBottomPanel::top("FileBrowserTop").frame(top_panel_frame)
            .show_separator_line(false)
            .show_inside(ui, |ui| 
        {
            ui.vertical_centered(|ui |
            {
                // if ui.button("View Tools").clicked() {
                //     if let Some(bridge) = &self.bridge {
                //         bridge.send(Input {
                //             url: "https://storage-api.master-tech.app".to_string(),
                //             access_key: ACCESS_KEY.to_string(),
                //             secret_key: SECRET_KEY.to_string(),
                //         });
                //     }
                // }

                if ui.button("Upload").clicked() {
                    let task = rfd::AsyncFileDialog::new().save_file();
                    // let contents = self.sample_text.clone();
                    spawn_local(async move {
                        let name = "logan";
                        let region = "us-west";
                        let bucket = Bucket::new("https://storage-api.master-tech.app".to_string().parse::<Url>().unwrap(), rusty_s3::UrlStyle::Path, name, region).expect("Url has a valid scheme and host");
                        //  https://storage.master-tech.app/api/v1/buckets/logan/objects/download?prefix=1-TUNEUP%2F1Webroot.exe&version_id=null
                        let credentials = Credentials::new(ACCESS_KEY, SECRET_KEY);
                        
                        let mut action = GetObject::new(&bucket, Some(&credentials), "1Webroot.exe");
                        action
                            .query_mut()
                            .insert("response-cache-control", "no-cache, no-store");

                        let signed_url = action.sign(ONE_HOUR);

                        let client = Client::new();
                        let resp = client.get(signed_url).header(CONTENT_TYPE, "application/x-ms-dos-executable").send().await.unwrap();
                        let bytes = resp.bytes().await.unwrap();
                        // let parsed = ListObjectsV2::parse_response(&text).unwrap();
                        // println!("response: {parsed:?}");
                        let file = task.await;
                        if let Some(file) = file{
                            file.write(bytes.to_vec().as_slice()).await.unwrap();
                        }
                    });
                }
            })
        });

        CentralPanel::default().frame(panel_frame)
            .show_inside(ui, |ui| 
        {
            // let data_update = self.data_update.as_mut().unwrap();
            // if let Some(items) = data_update.take() { 
            //     self.file_system.build_file_system(items);
            // }

            self.file_system.display(ui);
        });
    }
}

