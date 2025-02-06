// use database::schema::{ExtendedSeb, LocalSebData};
// use displays::tabs::json_viewer::Show;
use eframe::egui::{
    Button, CentralPanel, Frame, Margin, ScrollArea, TextEdit, TextStyle, TopBottomPanel, Ui,
    Widget,
};
use log::info;
use reqwest::{header::CONTENT_TYPE, Client};
use serde_json::Value;
use std::collections::HashMap;

use wasm_bindgen_futures::spawn_local;

use crate::app_state::MtechServerContext;

impl MtechServerContext {
    pub fn seb_lookup(&mut self, ui: &mut Ui) {
        TopBottomPanel::top("SebLookupTopPanel")
            .exact_height(30.)
            .show_inside(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.heading("SEB Lookup Tool");

                    ui.add_space(ui.available_width() / 3.);

                    TextEdit::singleline(&mut self.seb_email)
                        .hint_text("Search with Email or Device ID")
                        .ui(ui);

                    ui.add_space(10.);

                    if Button::new("Lookup SEB Info").ui(ui).clicked() {
                        let tx = self.seb_channel.0.clone();
                        let client = Client::new();
                        let search_string = self.seb_email.clone();
                        spawn_local(async move {
                            let mut params: HashMap<&str, &str> = HashMap::new();
                            params.insert("user_email", "logan.lees@pclaptops.com");
                            params.insert("user_password", "Poolparty1");
                            params.insert("application", "carbonite");
                            params.insert("action", "search");
                            params.insert("search", &search_string.trim());

                            let response = client
                                .post("https://scaffold.pclaptops.com/api/index")
                                .header(CONTENT_TYPE, "application/json")
                                .form(&params)
                                .send()
                                .await
                                .unwrap();

                            let response_json: Vec<Value> = response.json().await.unwrap();
                            info!("response_json: {:?}", response_json);

                            tx.try_send(response_json).unwrap();
                        });
                    }
                    ui.add_space(10.);
                });
            });

        let c_frame = Frame::default();
        let _ = c_frame.inner_margin(Margin::same(10));

        CentralPanel::default()
            .frame(c_frame)
            .show_inside(ui, |ui| {
                let available_height = ui.available_height();
                let font_id = TextStyle::Body.resolve(ui.style());
                let row_height = ui.fonts(|f| f.row_height(&font_id)) + ui.spacing().item_spacing.y;
                let total_rows = (available_height / row_height).floor() as usize;
                ScrollArea::new([false, true])
                    .max_width(f32::INFINITY)
                    .auto_shrink(false)
                    .show_rows(ui, row_height, total_rows, |_ui, _row_range| {
                        // if self.shared_ctx.json_editor.value.is_null() {
                        //     let mut local_seb = LocalSebData::default();
                        //     local_seb.ExtendedSeb = Some(ExtendedSeb::default());

                        //     let _ = self.shared_ctx.json_editor.set_value(local_seb);
                        // }
                        // self.shared_ctx.json_editor.show(ui);
                    });
            });
    }
}
