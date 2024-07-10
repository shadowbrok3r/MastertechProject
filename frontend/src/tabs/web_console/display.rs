use std::borrow::BorrowMut;
use eframe::egui::{epaint::Shadow, Ui, Align, Button, CentralPanel, Color32, Frame, Layout, Margin, RichText, Rounding, ScrollArea,  Stroke, Vec2, Widget};
use egui_extras::{Size, StripBuilder};
use log::info;
use crate::utilities::ui_tools::toasts::{Toast, ToastKind, ToastOptions};
use crate::app_state::MtechServerContext;

use super::websockets::{ClientHandler, WebSocketClient};

impl MtechServerContext{
    pub fn client_display(&mut self, ui: &mut Ui){
        let mut shadow = Shadow::default();
        shadow.blur = 10.0;
        shadow.spread = 2.0;
        shadow.color = Color32::from_rgb_additive(36, 156, 158);

        let mut outer_margin = Margin::default();
        outer_margin.left = 8.0;

        let mut inner_margin = Margin::default();
        inner_margin.top = 2.0;
        inner_margin.left = 2.0;
        inner_margin.right = 2.0;

        let panel_frame = Frame::default()
            .fill(Color32::from_rgb(8, 7, 10))
            .inner_margin(Margin::same(2.0))
            .outer_margin(Margin::symmetric(5.0, 0.0))
            .rounding(Rounding::same(5.0))
            .shadow(shadow)
            .stroke(Stroke::new(1.0, Color32::from_rgb_additive(20, 1, 20)));

        ui.style_mut().visuals.window_rounding = Rounding::same(10.0);
        let column_width = Size::exact(450.0);
        
        CentralPanel::default().frame(panel_frame)
            .show_inside(ui, |ui| 
        {
            ScrollArea::horizontal()
                .show_viewport(ui, |ui, _|
            {
                let x: f32 = ui.available_height() - 40.0;
                StripBuilder::new(ui)
                    .cell_layout(Layout::top_down_justified(Align::Center))
                    .size(Size::exact(30.0))
                    .size(Size::exact(5.0))
                    .size(Size::exact(x))
                    .vertical(|mut strip| 
                {
                    strip
                        .strip(|strip| 
                    {
                        strip
                            .sizes(column_width, self.clients.len())
                            .horizontal( |strip| self.headers(strip));
                    });
                    strip.empty();
                    strip
                        .strip(|strip| 
                    {
                        strip
                            .sizes(column_width, self.clients.len())
                            .horizontal( |mut strip| 
                        {
                            self.columns(
                                strip.borrow_mut(),
                            );
                        });
                    });
                });
            });
        });
    }

    pub fn columns(&mut self, strip: &mut egui_extras::Strip) {
        for (name, client) in self.clients.iter(){
            let color = if client.connected{ Color32::LIGHT_BLUE } else { Color32::LIGHT_RED };
            
            let column_frame = Frame::default().fill(Color32::from_rgb(12, 12, 18))
                .inner_margin(Margin::same(4.0)).rounding(Rounding::same(10.0))
                .stroke(Stroke::new(1.0, color));

            strip.strip(|s | 
            {
                s
                    .size(Size::remainder())
                    .vertical(|mut s| 
                {
                    s.cell(|ui| 
                    {
                        column_frame.show(ui, |ui| {
                            ui.vertical_centered_justified(|ui| {
                                let height = ui.available_height();
                                StripBuilder::new(ui)
                                    .size(Size::exact(25.0))
                                    .size(Size::exact(25.0))
                                    .size(Size::remainder().at_most(height - 15.0))
                                    .vertical(| strip| 
                                {
                                    if let Some(ws_client) = &mut self.ws_client{
                                        if client.connected {
                                            ws_client.show(strip, name.clone());
                                        }
                                    }
                                });
                            });
                        });
                    });
                });
            });
        }
    }

    pub fn headers(&mut self, mut s: egui_extras::Strip) {
        let header_frame = Frame::default()
            .fill(Color32::from_rgb(12, 12, 18))
            .inner_margin(Margin::same(4.0))
            .outer_margin(Margin::symmetric(3.0, 0.0))
            .rounding(Rounding::same(5.0))
            .stroke(Stroke::new(1.0, Color32::from_additive_luminance(50)));

        for (name, client) in self.clients.iter_mut(){
            s.cell(|ui|
            {
                header_frame.show(ui, |ui|
                {
                    ui.horizontal_top(|ui| 
                    {
                        ui.with_layout(Layout::left_to_right(Align::Min), 
                        |ui| {
                            let button = Button::new(
                                RichText::new("✖")
                                    .color(Color32::LIGHT_RED)
                                )
                                .fill(Color32::TRANSPARENT)
                                .min_size(Vec2::new(30.0, 20.0))
                                .ui(ui);
                            if button.clicked(){ // CONNECT
                                // let url = format!("{}/websocket?role=master&room_id={}", dotenv::from_filename("WS_URL").unwrap(), name.clone());
                                let _url = format!("wss://sock.master-tech.app/websocket?role=master&room_id={}", name.clone());
                                client.connected = false;
                                if let Some(ws_client) = &mut self.ws_client{
                                    ws_client.ws_sender.close();
                                }
                            }
                            ui.add_space(20.0)
                        });

                        ui.with_layout(Layout::left_to_right(Align::Center), 
                        |ui| ui.colored_label(Color32::WHITE, RichText::new(name.to_owned()).size(14.0)));
                        
                        ui.with_layout(Layout::right_to_left(Align::Max), |ui| 
                        {
                            let button = Button::new(
                                RichText::new("⮫")
                                    .color(Color32::LIGHT_RED)
                                )
                                .fill(Color32::TRANSPARENT)
                                .min_size(Vec2::new(30.0, 20.0))
                                .ui(ui);

                            if button.clicked(){
                                let url = format!("wss://sock.master-tech.app/websocket?role=master&room_id={}", name.clone());
                                
                                match ewebsock::connect(&url, Default::default()) {
                                    Ok((mut ws_sender, ws_receiver)) => {
                                        client.connected = true;
                                        ws_sender.send(ewebsock::WsMessage::Text("Server Connected".to_string()));
                                        self.ws_client = Some(WebSocketClient::new(ws_sender, ws_receiver));
                                    }
                                    Err(error) => {
                                        client.connected = false;
                                        info!("Failed to connect to {:?}: {}", &url, error);
                                        let toast = &mut self.toasts;
                
                                        let error_toast = Toast{
                                            kind: ToastKind::Error,
                                            text: format!("{error:?}").into(),
                                            options: ToastOptions::default()
                                                .show_progress(true)
                                                .duration_in_seconds(6.0)
                                        };
                                        toast.add(error_toast);
                                    }
                                };
                            }

                            ui.add_space(10.0);

                            let export = Button::new(
                                RichText::new("Export")
                                    .size(10.0)
                                    .color(Color32::LIGHT_RED)
                                )
                                .fill(Color32::TRANSPARENT)
                                .min_size(Vec2::new(30.0, 20.0))
                                .ui(ui);

                            if export.clicked() {
                                if let Some(ws_client) = &self.ws_client {
                                    if let Some(db) = &self.database{
                                        // info!("History: {:?}", ws_client.history.clone());
                                        client.export_logs(db.clone(), ws_client.history.clone());
                                    }
                                }
                            }

                            ui.add_space(45.0);
                        });
                    });
                });
            });
        }
    }
}
