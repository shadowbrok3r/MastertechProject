// use crate::app_state::MtechServerContext;
// use database::schema::ConnectedClient;
// use displays::ui_tools::toasts::{Toast, ToastKind, ToastOptions};
// use eframe::egui::{
//     text::LayoutJob, Align, Button, Color32, FontFamily, FontId, Frame, Layout, Margin, RichText,
//     Rounding, Stroke, TextFormat, Ui, Vec2, Widget, WidgetText,
// };
// use log::info;

// use super::websockets::{ClientHandler, WebSocketClient};

// // We reserve this much space for eterm to show some stats.
// // The rest is used for the view of the remove server.
// // const TOP_BAR_HEIGHT: f32 = 24.0;
// // Repaint every so often to check connection status etc.
// // const MIN_REPAINT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

// impl MtechServerContext {
//     pub fn headers(&mut self, ui: &mut Ui, client: ConnectedClient) {
//         let header_frame = Frame::default()
//             .fill(Color32::from_rgb(13, 13, 15))
//             .inner_margin(Margin::same(4.0))
//             .outer_margin(Margin::symmetric(3.0, 0.0))
//             .rounding(Rounding::same(5.0))
//             .stroke(Stroke::new(1.0, Color32::from_additive_luminance(50)));

//         let mut client = client.clone();
//         header_frame.show(ui, |ui| {
//             ui.horizontal_top(|ui| {
//                 ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
//                     let mut cloned_client = client.clone();
//                     let button = Button::new(RichText::new("✖").color(Color32::LIGHT_RED))
//                         .fill(Color32::TRANSPARENT)
//                         .min_size(Vec2::new(30.0, ui.available_height()))
//                         .ui(ui);

//                     if button.clicked() {
//                         // CONNECT
//                         let _url = format!(
//                             "wss://sock.master-tech.app/websocket?role=master&room_id={}",
//                             client.connection_string.clone()
//                         );
//                         cloned_client.connected = false;
//                         cloned_client.delete_client();
//                         if let Some(ws_client) = self.ws_clients.get_mut(&client.connection_string)
//                         {
//                             ws_client.ws_sender.close();
//                         }
//                     }

//                     let txt = if let Some(docked) =
//                         self.undock_client.get(&cloned_client.connection_string)
//                     {
//                         if !*docked {
//                             "🔓"
//                         } else {
//                             "🔒"
//                         }
//                     } else {
//                         "🔒"
//                     };

//                     let undock = Button::new(RichText::new(txt).color(Color32::LIGHT_RED))
//                         .fill(Color32::TRANSPARENT)
//                         .min_size(Vec2::new(30.0, ui.available_height()))
//                         .ui(ui);

//                     if undock.clicked() {
//                         if let Some(docked) =
//                             self.undock_client.get_mut(&cloned_client.connection_string)
//                         {
//                             if *docked {
//                                 *docked = false;
//                                 self.wants_to_undock = false;
//                             } else {
//                                 *docked = true;
//                                 self.wants_to_undock = true;
//                             };
//                         }
//                     }
//                 });

//                 let cloned_client = client.clone();
//                 ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
//                     ui.add_space(ui.available_width() / 3.1);
//                     // Create a new LayoutJob
//                     let mut job = LayoutJob::default();

//                     if let Some(friendly_name) = cloned_client.clone().friendly_name {
//                         job.append(
//                             &friendly_name,
//                             0.0,
//                             TextFormat {
//                                 font_id: FontId::new(14.0, FontFamily::Proportional),
//                                 color: Color32::from_rgb(51, 255, 189), // Set the color for the first part
//                                 valign: Align::Min,
//                                 ..Default::default()
//                             },
//                         );
//                     } else {
//                         let conn_string = cloned_client.clone().connection_string;
//                         let txt = conn_string.split_once(':');
//                         if let Some(txt) = txt {
//                             let text = format!("{}:", txt.0);
//                             job.append(
//                                 &text,
//                                 0.0,
//                                 TextFormat {
//                                     font_id: FontId::new(14.0, FontFamily::Proportional),
//                                     color: Color32::from_rgb(51, 255, 189), // Set the color for the first part
//                                     valign: Align::Min,
//                                     ..Default::default()
//                                 },
//                             );
//                             job.append(
//                                 txt.1,
//                                 0.0,
//                                 TextFormat {
//                                     font_id: FontId::new(14.0, FontFamily::Proportional),
//                                     color: Color32::from_rgb(199, 202, 245),
//                                     valign: Align::Min,
//                                     ..Default::default()
//                                 },
//                             );
//                         }
//                     };

//                     // Convert LayoutJob to WidgetText
//                     let formatted_text = WidgetText::from(job);

//                     if ui.button(formatted_text).clicked() {};
//                 });

//                 let mut cli_clone = cloned_client.clone();
//                 ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
//                     let button = Button::new(RichText::new("⮫").color(Color32::LIGHT_RED))
//                         .fill(Color32::TRANSPARENT)
//                         .min_size(Vec2::new(30.0, ui.available_height()))
//                         .ui(ui);

//                     if button.clicked() {
//                         let url = format!(
//                             "wss://sock.master-tech.app/websocket?role=master&room_id={}",
//                             cli_clone.connection_string.clone()
//                         );

//                         match ewebsock::connect(&url, Default::default()) {
//                             Ok((mut ws_sender, ws_receiver)) => {
//                                 cli_clone.connected = true;

//                                 ws_sender.send(ewebsock::WsMessage::Text(
//                                     "Server Connected".to_string(),
//                                 ));

//                                 let ws_client = WebSocketClient::new(
//                                     ws_sender,
//                                     ws_receiver,
//                                     cli_clone.clone(),
//                                     self.file_system.clone(),
//                                 );
//                                 self.ws_clients
//                                     .entry(cli_clone.connection_string.clone())
//                                     .or_insert(ws_client);
//                             }
//                             Err(error) => {
//                                 cli_clone.connected = false;
//                                 info!("Failed to connect to {:?}: {}", &url, error);
//                                 let toast = &mut self.shared_ctx.toasts;

//                                 let error_toast = Toast {
//                                     kind: ToastKind::Error,
//                                     text: format!("{error:?}").into(),
//                                     options: ToastOptions::default()
//                                         .show_progress(true)
//                                         .duration_in_seconds(6.0),
//                                 };
//                                 toast.add(error_toast);
//                             }
//                         };
//                     }

//                     ui.add_space(10.0);

//                     let export =
//                         Button::new(RichText::new("Export").size(10.0).color(Color32::LIGHT_RED))
//                             .fill(Color32::TRANSPARENT)
//                             .min_size(Vec2::new(30.0, ui.available_height()))
//                             .ui(ui);

//                     if export.clicked() {
//                         if let Some(ws_client) = self.ws_clients.get(&cli_clone.connection_string) {
//                             client.export_logs(ws_client.history.clone());
//                         }
//                     }

//                     ui.add_space(45.0);
//                 });
//             });
//         });
//     }
// }

// /*
// // let viewer_button = Button::new(
// //     RichText::new("Viewer")
// //         .color(Color32::LIGHT_RED)
// //     )
// //     .fill(Color32::TRANSPARENT)
// //     .min_size(Vec2::new(30.0, ui.available_height()))
// //     .ui(ui);
// // if viewer_button.clicked() {
// //     let url = format!("wss://sock.master-tech.app/websocket?role=master&room_id={}", name.clone());
// //     match ewebsock::connect(&url, Default::default()) {
// //         Ok((_ws_sender, _ws_receiver)) => {
// //             client.connected = true;
// //             self.current_client = name.clone();
// //             // let mut viewer = RemoteViewer::new(ws_sender, ws_receiver);
// //             // let mut last_sent_input = None;
// //             // let mut sent_input = ui.ctx().input_mut(|i| i.raw.clone());
// //             // let mut latest_eterm_meshes = Default::default();
// //             // let mut last_repaint = std::time::Instant::now();
// //             // let mut needs_repaint = true;
// //             // sent_input.time = None;
// //             // if let Some(screen_rect) = &mut sent_input.screen_rect {
// //             //     screen_rect.min.y += TOP_BAR_HEIGHT;
// //             //     screen_rect.max.y = screen_rect.max.y.max(screen_rect.min.y);
// //             // }
// //             // if last_sent_input.as_ref() != Some(&sent_input) {
// //             //     viewer.send_input(sent_input.clone());
// //             //     last_sent_input = Some(sent_input);
// //             //     needs_repaint = true;
// //             // }
// //             // let pixels_per_point = ui.ctx().pixels_per_point();
// //             // if let Some(frame) = viewer.update(pixels_per_point) {
// //             //     // We got something new from the server!
// //             //     let EguiFrame {
// //             //         frame_index: _,
// //             //         output,
// //             //         clipped_meshes,
// //             //         pixels_per_point
// //             //     } = frame;
// //             //     // let full_out = FullOutput {
// //             //     //     platform_output: output,
// //             //     //     textures_delta: todo!(),
// //             //     //     shapes: clipped_meshes,
// //             //     //     pixels_per_point,
// //             //     //     viewport_output: todo!(),
// //             //     // };
// //             //     latest_eterm_meshes = clipped_meshes;
// //             //     // FullOutput::default().append(newer)
// //             //     needs_repaint = true;
// //             // }
// //             // if needs_repaint || last_repaint.elapsed() > MIN_REPAINT_INTERVAL {
// //             //     needs_repaint = false;
// //             //     last_repaint = std::time::Instant::now();
// //             //     // THIS IS WRONG, I NEED TO DO SOMETHING WITH FRAME TO DISPLAY THE GUI
// //             //     //
// //             //     let ctx = ui.ctx().clone();
// //             //     // ctx.begin_frame(sent_input.clone());
// //             //     // todo!(
// //             //     //     r#"
// //             //     //         I need to begin frame here, take raw input, but it needs to
// //             //     //         be RECEIVED input, which ill need to get through ws_receiver
// //             //     //     "#
// //             //     // );
// //             //     // Window::new("Hello world!")
// //             //     //     .default_pos(pos2(100.0, 0.0))
// //             //     //     .show(&ctx, |ui|
// //             //     // {
// //             //     //     ui.label("Hello, World!");
// //             //     // });
// //             // }
// //             // ui.ctx().request_repaint();
// //         }
// //         Err(error) => {
// //             client.connected = false;
// //             info!("Failed to connect to {:?}: {}", &url, error);
// //             let toast = &mut self.shared_ctx.toasts;
// //             let error_toast = Toast{
// //                 kind: ToastKind::Error,
// //                 text: format!("{error:?}").into(),
// //                 options: ToastOptions::default()
// //                     .show_progress(true)
// //                     .duration_in_seconds(6.0)
// //             };
// //             toast.add(error_toast);
// //         }
// //     };
// // }
// */

