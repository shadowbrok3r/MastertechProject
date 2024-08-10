use eframe::egui::{Align, Button, CollapsingHeader, Color32, Frame, Layout, Margin, Rangef, RichText, Rounding, ScrollArea, Stroke, Ui, Vec2, Widget, Window};
use displays::ui_tools::toasts::{Toast, ToastKind, ToastOptions};
use crate::app_state::MtechServerContext;
use egui_extras::{Size, StripBuilder};
use database::schema::ConnectedClient;
use log::info;

use super::websockets::{ClientHandler, WebSocketClient};

// We reserve this much space for eterm to show some stats.
// The rest is used for the view of the remove server.
// const TOP_BAR_HEIGHT: f32 = 24.0;
// Repaint every so often to check connection status etc.
// const MIN_REPAINT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

impl MtechServerContext{
    pub fn client_display(&mut self, ui: &mut Ui){
        ui.style_mut().visuals.window_rounding = Rounding::same(10.);
        let column_width = Size::exact(ui.available_width());

        ScrollArea::vertical()
            .show_viewport(ui, |ui, _|
        {
            let size = ui.available_size_before_wrap().y;
            for client in self.clients.clone(){
                StripBuilder::new(ui)
                    .cell_layout(Layout::top_down_justified(Align::Center))
                    .size(Size::exact(30.0))
                    .size(Size::exact(5.0))
                    .size(Size::initial(20.0)
                        .with_range(
                            Rangef::new(20.0, size)
                        )
                    ).vertical(|mut strip| 
                {
                    strip.strip(|strip| 
                    {
                        strip
                            .size(column_width)
                            .horizontal( |mut strip| {
                                strip.cell(|ui| self.headers(ui, client.clone()) );
                        });
                    });

                    strip.empty();

                    strip.strip(|strip| 
                    {
                        strip.size(column_width)
                            .horizontal( |mut strip| 
                        {
                            let connection_string = &*client.connection_string;
                            let color = if client.connected{ Color32::LIGHT_BLUE } else { Color32::LIGHT_RED };
                            strip.strip(|s | 
                            {
                                s.size(Size::remainder())
                                    .vertical(|mut s| 
                                {
                                    s.cell(|ui| 
                                    {
                                        self.columns(ui, color, connection_string);
                                    });
                                });
                            });
                        });
                    });
                    
                });
            }
        });
    }

    pub fn columns(&mut self, ui: &mut Ui, color: Color32, connection_string: &str) {
        let column_frame = Frame::default().fill(Color32::from_rgb(12, 12, 14))
            .inner_margin(Margin::same(4.0)).outer_margin(Margin::symmetric(5.0, 3.0))
            .rounding(Rounding::same(10.0)).stroke(Stroke::new(1.0, color));
        let undock = if let Some(undock) = self.undock_client.get(*&connection_string){
            undock
        } else { &false };
        
        if *undock {
            Window::new(connection_string)
                .min_size(Vec2::new(400., 300.))
                .frame(column_frame)
                .show(ui.ctx(), |ui| {
                    ui.set_min_size(Vec2::new(400., 300.));
                    ui.vertical_centered_justified(|ui| {
                        if let Some(ws_client) = self.ws_clients.get_mut(*&connection_string) {
                            ws_client.show(ui);
                        }
                    });
                });
        } else {
            CollapsingHeader::new(connection_string).show_unindented(ui, |ui| {
                column_frame.show(ui, |ui| {
                    ui.set_min_size(Vec2::new(400., 300.));
                    ui.vertical_centered_justified(|ui| {
                        if let Some(ws_client) = self.ws_clients.get_mut(*&connection_string) {
                            ws_client.show(ui);
                        }
                    });
                });
            });
        }
    }

    pub fn headers(&mut self, ui: &mut Ui, client: ConnectedClient) {
        let header_frame = Frame::default()
            .fill(Color32::from_rgb(13, 13, 15))
            .inner_margin(Margin::same(4.0))
            .outer_margin(Margin::symmetric(3.0, 0.0))
            .rounding(Rounding::same(5.0))
            .stroke(Stroke::new(1.0, Color32::from_additive_luminance(50)));

        let mut client = client.clone();
        header_frame.show(ui, |ui|
        {
            ui.horizontal_top(|ui| 
            {
                ui.with_layout(Layout::left_to_right(Align::Min), 
                |ui| {
                    let mut cloned_client = client.clone();
                    let button = Button::new(
                        RichText::new("✖")
                            .color(Color32::LIGHT_RED)
                        )
                        .fill(Color32::TRANSPARENT)
                        .min_size(Vec2::new(30.0, ui.available_height()))
                        .ui(ui);

                    if button.clicked(){ // CONNECT
                        let _url = format!("wss://sock.master-tech.app/websocket?role=master&room_id={}", client.connection_string.clone());
                        cloned_client.connected = false;
                        cloned_client.delete_client();
                        if let Some(ws_client) = self.ws_clients.get_mut(&client.connection_string){
                            ws_client.ws_sender.close();
                        }
                    }

                    let txt = if let Some(docked) = self.undock_client.get(&cloned_client.connection_string) {
                        if !*docked { "🔓" } else { "🔒" }
                    } else { "🔒" };

                    let undock = Button::new(
                        RichText::new(txt)
                            .color(Color32::LIGHT_RED)
                        )
                        .fill(Color32::TRANSPARENT)
                        .min_size(Vec2::new(30.0, ui.available_height()))
                        .ui(ui);

                    if undock.clicked() {
                        if let Some(docked) = self.undock_client.get_mut(&cloned_client.connection_string) {
                            if *docked { *docked = false; } else { *docked = true; };
                        }
                    }
                    // let viewer_button = Button::new(
                    //     RichText::new("Viewer")
                    //         .color(Color32::LIGHT_RED)
                    //     )
                    //     .fill(Color32::TRANSPARENT)
                    //     .min_size(Vec2::new(30.0, ui.available_height()))
                    //     .ui(ui);
                    // if viewer_button.clicked() {
                    //     let url = format!("wss://sock.master-tech.app/websocket?role=master&room_id={}", name.clone());
                    //     match ewebsock::connect(&url, Default::default()) {
                    //         Ok((_ws_sender, _ws_receiver)) => {
                    //             client.connected = true;
                    //             self.current_client = name.clone();
                    //             // let mut viewer = RemoteViewer::new(ws_sender, ws_receiver);
                    //             // let mut last_sent_input = None;
                    //             // let mut sent_input = ui.ctx().input_mut(|i| i.raw.clone());
                    //             // let mut latest_eterm_meshes = Default::default();
                    //             // let mut last_repaint = std::time::Instant::now();
                    //             // let mut needs_repaint = true;
                    //             // sent_input.time = None;
                    //             // if let Some(screen_rect) = &mut sent_input.screen_rect {
                    //             //     screen_rect.min.y += TOP_BAR_HEIGHT;
                    //             //     screen_rect.max.y = screen_rect.max.y.max(screen_rect.min.y);
                    //             // }
                    //             // if last_sent_input.as_ref() != Some(&sent_input) {
                    //             //     viewer.send_input(sent_input.clone());
                    //             //     last_sent_input = Some(sent_input);
                    //             //     needs_repaint = true;
                    //             // }
                    //             // let pixels_per_point = ui.ctx().pixels_per_point();
                    //             // if let Some(frame) = viewer.update(pixels_per_point) {
                    //             //     // We got something new from the server!
                    //             //     let EguiFrame {
                    //             //         frame_index: _,
                    //             //         output,
                    //             //         clipped_meshes,
                    //             //         pixels_per_point
                    //             //     } = frame;
                    //             //     // let full_out = FullOutput {
                    //             //     //     platform_output: output,
                    //             //     //     textures_delta: todo!(),
                    //             //     //     shapes: clipped_meshes,
                    //             //     //     pixels_per_point,
                    //             //     //     viewport_output: todo!(),
                    //             //     // };
                    //             //     latest_eterm_meshes = clipped_meshes;
                    //             //     // FullOutput::default().append(newer)
                    //             //     needs_repaint = true;
                    //             // }
                    //             // if needs_repaint || last_repaint.elapsed() > MIN_REPAINT_INTERVAL {
                    //             //     needs_repaint = false;
                    //             //     last_repaint = std::time::Instant::now();                          
                    //             //     // THIS IS WRONG, I NEED TO DO SOMETHING WITH FRAME TO DISPLAY THE GUI
                    //             //     // 
                    //             //     let ctx = ui.ctx().clone();
                    //             //     // ctx.begin_frame(sent_input.clone());
                    //             //     // todo!(
                    //             //     //     r#"
                    //             //     //         I need to begin frame here, take raw input, but it needs to 
                    //             //     //         be RECEIVED input, which ill need to get through ws_receiver
                    //             //     //     "#
                    //             //     // );
                    //             //     // Window::new("Hello world!")
                    //             //     //     .default_pos(pos2(100.0, 0.0))
                    //             //     //     .show(&ctx, |ui| 
                    //             //     // {
                    //             //     //     ui.label("Hello, World!");
                    //             //     // });
                    //             // }      
                    //             // ui.ctx().request_repaint();
                    //         }
                    //         Err(error) => {
                    //             client.connected = false;
                    //             info!("Failed to connect to {:?}: {}", &url, error);
                    //             let toast = &mut self.toasts;
                    //             let error_toast = Toast{
                    //                 kind: ToastKind::Error,
                    //                 text: format!("{error:?}").into(),
                    //                 options: ToastOptions::default()
                    //                     .show_progress(true)
                    //                     .duration_in_seconds(6.0)
                    //             };
                    //             toast.add(error_toast);
                    //         }
                    //     };
                    // }

                });

                let cloned_client = client.clone();
                ui.with_layout(Layout::left_to_right(Align::Center), 
                |ui| {
                    ui.add_space(ui.available_width() / 2.5);
                    let txt = if let Some(friendly_name) = cloned_client.clone().friendly_name{
                        friendly_name
                    } else { cloned_client.clone().connection_string };
                    if ui.button(RichText::new(txt.to_owned()).size(14.0)).clicked() {

                    };
                });
                
                let mut cli_clone = cloned_client.clone();
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| 
                {
                    
                    let button = Button::new(
                        RichText::new("⮫")
                            .color(Color32::LIGHT_RED)
                        )
                        .fill(Color32::TRANSPARENT)
                        .min_size(Vec2::new(30.0, ui.available_height()))
                        .ui(ui);

                    if button.clicked(){
                        let url = format!("wss://sock.master-tech.app/websocket?role=master&room_id={}", cli_clone.connection_string.clone());
                        
                        match ewebsock::connect(&url, Default::default()) {
                            Ok((mut ws_sender, ws_receiver)) => {
                                cli_clone.connected = true;

                                ws_sender.send(ewebsock::WsMessage::Text("Server Connected".to_string()));

                                let ws_client = WebSocketClient::new(
                                    ws_sender, 
                                    ws_receiver, 
                                    cli_clone.clone(), 
                                    self.file_system.clone()
                                );
                                self.ws_clients.entry(cli_clone.connection_string.clone()).or_insert(ws_client);
                            }
                            Err(error) => {
                                cli_clone.connected = false;
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
                        .min_size(Vec2::new(30.0, ui.available_height()))
                        .ui(ui);

                    if export.clicked() {
                        if let Some(ws_client) = self.ws_clients.get(&cli_clone.connection_string) {
                            client.export_logs(ws_client.history.clone());
                        }
                    }

                    ui.add_space(45.0);
                });
            });
        });
    }
}
