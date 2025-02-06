use database::schema::ConnectedClient;
use eframe::egui::{Context, Ui};

use crate::app_state::MtechServer;

// use std::collections::BTreeSet;
// use crate::{
//     tabs::web_console::websockets::{ClientHandler, WebSocketClient},
// };
// use chrono::{DateTime, Utc};
// use database::schema::{utilities::get_connected_clients, ConnectedClient};
// use displays::{ui_tools::toasts::{Toast, ToastKind, ToastOptions}, FilterClients};
// use eframe::egui::{
//     text::LayoutJob, Align, Button, CentralPanel, CollapsingHeader, Color32, Context, FontFamily,
//     FontId, Frame, Layout, Margin, RichText, CornerRadius, ScrollArea, Stroke, TextEdit, TextFormat,
//     TopBottomPanel, Ui, Vec2, Widget, WidgetText,
// };
// use log::info;
// use wasm_bindgen_futures::spawn_local;
// We reserve this much space for eterm to show some stats.
// The rest is used for the view of the remove server.
// const TOP_BAR_HEIGHT: f32 = 24.0;
// Repaint every so often to check connection status etc.
// const MIN_REPAINT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

impl MtechServer {
    pub fn headers(&mut self, _ui: &mut Ui, _client: ConnectedClient) {
        // let header_frame = Frame::default()
        //     .fill(Color32::from_rgb(13, 13, 15))
        //     .inner_margin(Margin::same(4.0))
        //     .outer_margin(Margin::symmetric(3.0, 0.0))
        //     .corner_radius(eframe::egui::CornerRadius::same(5.0))
        //     .stroke(Stroke::new(1.0, Color32::from_additive_luminance(50)));

        // let mut client = client.clone();
        // header_frame.show(ui, |ui| {
        //     ui.horizontal_top(|ui| {
        //         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
        //             let mut cloned_client = client.clone();
        //             let button = Button::new(RichText::new("✖").color(Color32::LIGHT_RED))
        //                 .fill(Color32::TRANSPARENT)
        //                 .min_size(Vec2::new(30.0, ui.available_height()))
        //                 .ui(ui);

        //             if button.clicked() {
        //                 // CONNECT
        //                 let _url = format!(
        //                     "{WS_MASTER_URL}&role=master&room_id={}",
        //                     client.connection_string.clone()
        //                 );
        //                 cloned_client.connected = false;
        //                 cloned_client.delete_client();
        //                 if let Some(ws_client) =
        //                     self.context.ws_clients.get_mut(&client.connection_string)
        //                 {
        //                     ws_client.ws_sender.close();
        //                 }
        //             }

        //             let txt = if let Some(docked) = self
        //                 .context
        //                 .undock_client
        //                 .get(&cloned_client.connection_string)
        //             {
        //                 if !*docked {
        //                     "🔓"
        //                 } else {
        //                     "🔒"
        //                 }
        //             } else {
        //                 "🔒"
        //             };

        //             let undock = Button::new(RichText::new(txt).color(Color32::LIGHT_RED))
        //                 .fill(Color32::TRANSPARENT)
        //                 .min_size(Vec2::new(30.0, ui.available_height()))
        //                 .ui(ui);

        //             if undock.clicked() {
        //                 if let Some(docked) = self
        //                     .context
        //                     .undock_client
        //                     .get_mut(&cloned_client.connection_string)
        //                 {
        //                     if *docked {
        //                         *docked = false;
        //                         self.context.wants_to_undock = false;
        //                     } else {
        //                         *docked = true;
        //                         self.context.wants_to_undock = true;
        //                     };
        //                 }
        //             }
        //         });

        //         let cloned_client = client.clone();
        //         ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        //             ui.add_space(ui.available_width() / 3.1);
        //             let txt = if let Some(friendly_name) = cloned_client.clone().friendly_name {
        //                 friendly_name
        //             } else {
        //                 cloned_client.clone().connection_string
        //             };
        //             if ui
        //                 .button(RichText::new(txt.to_owned()).size(14.0))
        //                 .clicked()
        //             {};
        //         });

        //         let mut cli_clone = cloned_client.clone();
        //         ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
        //             let button = Button::new(RichText::new("⮫").color(Color32::LIGHT_RED))
        //                 .fill(Color32::TRANSPARENT)
        //                 .min_size(Vec2::new(30.0, ui.available_height()))
        //                 .ui(ui);

        //             if button.clicked() {
        //                 let url = format!(
        //                     "{WS_MASTER_URL}&role=master&room_id={}",
        //                     // "ws://localhost:8081/websocket?role=master&room_id={}",
        //                     cli_clone.connection_string.clone()
        //                 );

        //                 match ewebsock::connect(&url, Default::default()) {
        //                     Ok((mut ws_sender, ws_receiver)) => {
        //                         cli_clone.connected = true;

        //                         ws_sender.send(ewebsock::WsMessage::Text(
        //                             "Server Connected".to_string(),
        //                         ));

        //                         let ws_client = WebSocketClient::new(
        //                             ws_sender,
        //                             ws_receiver,
        //                             cli_clone.clone(),
        //                             self.context.shared_ctx.filesystem.clone(),
        //                         );
        //                         self.context
        //                             .ws_clients
        //                             .entry(cli_clone.connection_string.clone())
        //                             .or_insert(ws_client);
        //                     }
        //                     Err(error) => {
        //                         cli_clone.connected = false;
        //                         info!("Failed to connect to {:?}: {}", &url, error);
        //                         let toast = &mut self.context.shared_ctx.toasts;

        //                         let error_toast = Toast {
        //                             kind: ToastKind::Error,
        //                             text: format!("{error:?}").into(),
        //                             options: ToastOptions::default()
        //                                 .show_progress(true)
        //                                 .duration_in_seconds(6.0),
        //                         };
        //                         toast.add(error_toast);
        //                     }
        //                 };
        //             }

        //             ui.add_space(10.0);

        //             let export =
        //                 Button::new(RichText::new("Export").size(10.0).color(Color32::LIGHT_RED))
        //                     .fill(Color32::TRANSPARENT)
        //                     .min_size(Vec2::new(30.0, ui.available_height()))
        //                     .ui(ui);

        //             if export.clicked() {
        //                 if let Some(ws_client) =
        //                     self.context.ws_clients.get(&cli_clone.connection_string)
        //                 {
        //                     client.export_logs(ws_client.history.clone());
        //                 }
        //             }

        //             ui.add_space(45.0);
        //         });
        //     });
        // });
    }

    pub fn web_console(&mut self, _ctx: &Context) {
        // ctx.request_repaint();

        // let side_panel_frame = Frame::default()
        //     .inner_margin(Margin::same(6.0))
        //     .outer_margin(Margin::same(6.0))
        //     .fill(Color32::from_rgb(17, 17, 19))
        //     .corner_radius(eframe::egui::CornerRadius::same(5.0));

        // let central_panel_frame = Frame::default()
        //     .inner_margin(Margin::same(20.0))
        //     .fill(Color32::from_rgb(10, 10, 12));

        // TopBottomPanel::top("Client_Top_panel")
        //     .frame(side_panel_frame)
        //     .show_separator_line(false)
        //     .show_animated(ctx, true, |ui| {
        //         ui.vertical_centered(|ui| {
        //             if Button::new("Refresh")
        //                 .min_size(Vec2::new(50.0, 15.0))
        //                 .ui(ui)
        //                 .clicked()
        //             {
        //                 let tx = self.context.shared_ctx.connected_clients_tx.clone();
        //                 spawn_local(async move {
        //                     get_connected_clients(tx).await.unwrap();
        //                 });
        //             }
        //         });

        //         // let _result =
        //         //     AutoCompleteTextEdit::new(&mut self.context.search_input, inputs.clone())
        //         //         .highlight_matches(true)
        //         //         .max_suggestions(10)
        //         //         .set_text_edit_properties(|text_edit: TextEdit<'_>| {
        //         //             text_edit
        //         //                 .hint_text("Search for client")
        //         //                 .desired_width(150.0)
        //         //                 .font(FontId::proportional(12.0))
        //         //                 .frame(true)
        //         //         })
        //         //         .ui(ui);

        //         let mut margin = Margin::default();
        //         margin.top = 6.0;
        //         margin.left = 4.0;

        //         TextEdit::singleline(&mut self.context.client_search_input)
        //             .hint_text("Search")
        //             .desired_width(100.0)
        //             .margin(margin)
        //             .ui(ui);
        //     });

        // if !self.context.error.is_empty() {
        //     TopBottomPanel::bottom("error").show(ctx, |ui| {
        //         ui.horizontal(|ui| {
        //             ui.label("Error:");
        //             ui.colored_label(Color32::RED, &self.context.error);
        //         });
        //     });
        // }

        // // ui.style_mut().visuals.window_corner_radius = eframe::egui::CornerRadius::same(10.);
        // CentralPanel::default()
        //     .frame(central_panel_frame)
        //     .show(ctx, |ui| {
        //         ui.columns(2, |columns| {
        //             columns[0].vertical_centered(|ui| ui.heading("Connected"));
        //             columns[1].vertical_centered(|ui| ui.heading("Disconnected"));
        //         });

        //         let mut inputs = BTreeSet::new();

        //         for client in self.context.shared_ctx.clients.clone() {
        //             let connection_string = client.connection_string.clone();
        //             inputs.insert(connection_string.clone());
        //             if let Some(friendly_name) = client.friendly_name {
        //                 inputs.insert(friendly_name.clone());
        //             }
        //         }

        //         ScrollArea::vertical().show_viewport(ui, |ui, _| {
        //             let search_input = self.context.client_search_input.clone();

        //             let clients = self.context.shared_ctx.clients.clone();
        //             let mut client_vec = Vec::new();
        //             if !search_input.is_empty() {
        //                 for client in
        //                     clients.filter_by_client(inputs.clone(), search_input.clone())
        //                 {
        //                     client_vec.push(client);
        //                 }
        //             } else {
        //                 for client in clients {
        //                     client_vec.push(client);
        //                 }
        //             }
        //             for client in client_vec.clone() {
        //                 let connection_string = client.connection_string.clone();
        //                 let connected_color = if client.connected {
        //                     Color32::LIGHT_BLUE
        //                 } else {
        //                     Color32::LIGHT_RED
        //                 };

        //                 let column_frame = Frame::default()
        //                     .fill(Color32::from_rgb(12, 12, 14))
        //                     .inner_margin(Margin::same(4.0))
        //                     .outer_margin(Margin::symmetric(5.0, 3.0))
        //                     .corner_radius(eframe::egui::CornerRadius::same(10.0))
        //                     .stroke(Stroke::new(1.0, connected_color));

        //                 let undock = if let Some(undock) =
        //                     self.context.undock_client.get(&connection_string)
        //                 {
        //                     undock
        //                 } else {
        //                     &false
        //                 };

        //                 if !*undock {
        //                     ui.columns(2, |columns| {
        //                         if client.connected {
        //                             ScrollArea::vertical()
        //                                 .max_height(f32::INFINITY)
        //                                 .id_salt(format!(
        //                                     "connected-{:?}",
        //                                     connection_string.clone()
        //                                 ))
        //                                 .show(&mut columns[0], |ui| {
        //                                     // ui.heading("Connected Clients");
        //                                     CollapsingHeader::new(connection_string.clone())
        //                                         .show_unindented(ui, |ui| {
        //                                             column_frame.show(ui, |ui| {
        //                                                 ui.set_min_size(Vec2::new(400., 400.));
        //                                                 ui.vertical_centered_justified(|ui| {
        //                                                     // ui.horizontal(|ui| {
        //                                                     //     self.context.client_header(ui, client)
        //                                                     // });
        //                                                     if let Some(ws_client) = self
        //                                                         .context
        //                                                         .ws_clients
        //                                                         .get_mut(&connection_string)
        //                                                     {
        //                                                         ws_client.show(ui);
        //                                                     }
        //                                                 });
        //                                             });
        //                                         });
        //                                 });
        //                         } else {
        //                             // Create a new LayoutJob
        //                             let mut job = LayoutJob::default();

        //                             job.append(
        //                                 &connection_string,
        //                                 0.0,
        //                                 TextFormat {
        //                                     font_id: FontId::new(14.0, FontFamily::Proportional),
        //                                     color: Color32::WHITE, // Set the color for the first part
        //                                     valign: Align::Center,
        //                                     ..Default::default()
        //                                 },
        //                             );

        //                             if let Some(update) = &client.last_update {
        //                                 let date = update.parse::<DateTime<Utc>>();
        //                                 if let Ok(date) = date {
        //                                     job.append(
        //                                         &date.date_naive().to_string(),
        //                                         20.0,
        //                                         TextFormat {
        //                                             font_id: FontId::new(
        //                                                 14.0,
        //                                                 FontFamily::Proportional,
        //                                             ),
        //                                             color: Color32::LIGHT_RED,
        //                                             valign: Align::Center,
        //                                             ..Default::default()
        //                                         },
        //                                     );
        //                                 }
        //                             }

        //                             // Convert LayoutJob to WidgetText
        //                             let formatted_text = WidgetText::from(job);

        //                             ScrollArea::vertical()
        //                                 .id_salt(format!(
        //                                     "disconnected-{:?}",
        //                                     connection_string.clone()
        //                                 ))
        //                                 .max_height(f32::INFINITY)
        //                                 .show(&mut columns[1], |ui| {
        //                                     // ui.heading("Disconnected Clients");
        //                                     CollapsingHeader::new(formatted_text).show_unindented(
        //                                         ui,
        //                                         |ui| {
        //                                             column_frame.show(ui, |ui| {
        //                                                 ui.set_min_size(Vec2::new(400., 400.));
        //                                                 ui.vertical_centered_justified(|ui| {
        //                                                     // ui.horizontal(|ui| {
        //                                                     //     self.context.headers(ui, client)
        //                                                     // });
        //                                                     if let Some(ws_client) = self
        //                                                         .context
        //                                                         .ws_clients
        //                                                         .get_mut(&connection_string)
        //                                                     {
        //                                                         ws_client.show(ui);
        //                                                     }
        //                                                 });
        //                                             });
        //                                         },
        //                                     );
        //                                 });
        //                         }
        //                     });
        //                 }
        //             }
        //         });
        //     });
    }
}
