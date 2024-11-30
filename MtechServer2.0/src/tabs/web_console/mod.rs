use eframe::egui::{Button, CollapsingHeader, Color32, Frame, Margin, Rounding, ScrollArea, Stroke, TopBottomPanel, Ui, Vec2, Widget};
use database::schema::utilities::get_connected_clients;
use crate::app_state::MtechServerContext;
use wasm_bindgen_futures::spawn_local;

pub mod websockets;
pub mod charts;
pub mod display;

impl MtechServerContext {
    pub fn web_console(&mut self, ui: &mut Ui){
        ui.ctx().request_repaint();

        let side_panel_frame = Frame::default()
            .inner_margin(Margin::same(6.0))
            .outer_margin(Margin::same(6.0))
            .fill(Color32::from_rgb(17,17,19))
            .rounding(Rounding::same(5.0)) ;

        ui.style_mut().spacing.button_padding = Vec2::new(10.0, 4.0);

        TopBottomPanel::top("Client_Top_panel").frame(side_panel_frame)
        .show_separator_line(false)
        .show_animated_inside(ui, true, |ui |{
            ui.vertical_centered(|ui |{
                if Button::new("Refresh").min_size(Vec2::new(50.0, 15.0)).ui(ui).clicked()
                {
                    let usr = self.shared_ctx.current_user.clone();
                    if let Some(user) = usr{
                        let tx = self.connected_clients_tx.clone();
                        spawn_local(async move {
                            get_connected_clients(tx, user).await.unwrap();
                        });
                    }
                }
            });
        });

        if !self.error.is_empty() {
            TopBottomPanel::bottom("error").show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Error:");
                    ui.colored_label(Color32::RED, &self.error);
                });
            });
        }
        
        ui.style_mut().visuals.window_rounding = Rounding::same(10.);

        ScrollArea::vertical()
            .show_viewport(ui, |ui, _|
        {
            for client in self.clients.clone(){
                let connection_string = client.connection_string.clone();
                let color = if client.connected{ Color32::LIGHT_BLUE } else { Color32::LIGHT_RED };
        
                let column_frame = Frame::default().fill(Color32::from_rgb(12, 12, 14))
                    .inner_margin(Margin::same(4.0)).outer_margin(Margin::symmetric(5.0, 3.0))
                    .rounding(Rounding::same(10.0)).stroke(Stroke::new(1.0, color));
        
                let undock = if let Some(undock) = self.undock_client.get(&connection_string){
                    undock
                } else { &false };
                
                if !*undock {
                    CollapsingHeader::new(connection_string.clone()).show_unindented(ui, |ui| {
                        column_frame.show(ui, |ui| {
                            ui.set_min_size(Vec2::new(400., 400.));
                            ui.vertical_centered_justified(|ui| {
                                ui.horizontal(|ui| self.headers(ui, client));
                                if let Some(ws_client) = self.ws_clients.get_mut(&connection_string) {
                                    ws_client.show(ui);
                                }
                            });
                        });
                    });
                }
            }
        });
    }
}

