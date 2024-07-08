use eframe::egui::Ui;
use egui::{CentralPanel, Color32, Frame, Margin, RichText, Rounding, Shadow, SidePanel, Stroke, TopBottomPanel, Vec2, Widget};
// use log::info;
// use mtechserver::webworker::Input;
use reqwest_wasm::{header::CONTENT_TYPE, Client, Url};
use rusty_s3::{actions::GetObject, Bucket, Credentials, S3Action};
use wasm_bindgen_futures::spawn_local;
use web_time::Duration;
use crate::app_state::{MtechServerContext, ACCESS_KEY, SECRET_KEY};

pub mod storage_api;


const ONE_HOUR: Duration = Duration::from_secs(3600);

impl MtechServerContext {
    pub fn toolbox(&mut self, ui: &mut Ui){
        ui.ctx().request_repaint();

        let mut inner_margin_top = Margin::default();
        inner_margin_top.left = 3.0;
        inner_margin_top.right = 3.0;
        inner_margin_top.top = 5.0;
        inner_margin_top.bottom = 5.0;

        let top_panel_frame = Frame::default().fill(Color32::from_rgb(20,20,30))
            .inner_margin(inner_margin_top)
            .rounding(Rounding::same(10.0))
            .stroke(Stroke::new(1.0, Color32::from_additive_luminance(20)));


        let mut inner_margin = Margin::default();
        inner_margin.top = 5.0;
        inner_margin.left = 3.0;
        inner_margin.right = 3.0;

        let panel_frame = Frame::default()
            .fill(Color32::from_rgb(15,15,22))
            .inner_margin(inner_margin)
            .rounding(Rounding::same(10.0))
            .stroke(Stroke::new(1.0, Color32::from_additive_luminance(20)));
        
        ui.style_mut().spacing.button_padding = Vec2::new(10.0, 3.0);

        TopBottomPanel::top("FileBrowserTop").frame(top_panel_frame)
            .show_separator_line(false)
            .show_inside(ui, |ui| 
        {
            ui.vertical_centered(|ui |
            {
                if ui.button(RichText::new("Upload").size(9.0)).clicked() {
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
                        // info!("response: {parsed:?}");
                        let file = task.await;
                        if let Some(file) = file{
                            file.write(bytes.to_vec().as_slice()).await.unwrap();
                        }
                    });
                }
            })
        });

        ui.add_space(10.0);
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

