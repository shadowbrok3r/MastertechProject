use database::schema::utilities::get_connected_clients;
use eframe::egui::{Button, Color32, Frame, Margin, Rangef, Rounding, SidePanel, Stroke, TopBottomPanel, Ui, Vec2, Widget};
use crate::app_state::MtechServerContext;
use wasm_bindgen_futures::spawn_local;

pub mod websockets;
pub mod charts;
pub mod display;

impl MtechServerContext {
    pub fn web_console(&mut self, ui: &mut Ui){
        ui.ctx().request_repaint();
        

        let mut inner_margin = Margin::default();
        inner_margin.top = 6.0;
        inner_margin.left = 3.0;
        inner_margin.right = 3.0;

        let side_panel_frame = Frame::default()
            .inner_margin(inner_margin)
            .fill(Color32::from_rgb(17,17,19))
            .stroke(Stroke::new(0.5, Color32::WHITE))
            .rounding(Rounding::same(5.0)) ;

        ui.style_mut().spacing.button_padding = Vec2::new(10.0, 4.0);

        SidePanel::right("Client_Side_panel").frame(side_panel_frame)
        .show_separator_line(false)
        .width_range(Rangef::new(80.0, 150.0)).default_width(80.0)
        .show_animated_inside(ui, true, |ui |{
            ui.vertical_centered(|ui |{
                if Button::new("Refresh").min_size(Vec2::new(50.0, 15.0)).ui(ui).clicked()
                {
                    let usr = self.current_user.clone();
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
        
        self.client_display(ui);
    }
}

