use eframe::egui::{epaint::Shadow, Button, Color32, Frame, Margin, Rangef, Rounding, SidePanel, Stroke, TopBottomPanel, Ui, Vec2, Widget};
use log::info;
// use ratatui::layout::{Constraint, Direction, Layout, Rect};
use wasm_bindgen_futures::spawn_local;
use crate::{app_state::MtechServerContext, utilities::{ai::tool_call::call_with_response, get_other::get_connected_clients}};

pub mod websockets;
pub mod charts;
pub mod display;

impl MtechServerContext {
    pub fn web_console(&mut self, ui: &mut Ui){
        ui.ctx().request_repaint();
        
        let mut shadow = Shadow::default();
        shadow.blur = 10.0;
        shadow.spread = 2.0;
        shadow.color = Color32::from_rgb_additive(20, 1, 20);

        let top_panel_frame = Frame::default().fill(Color32::from_rgb(8, 7, 10))
            .inner_margin(Margin::same(8.0)).outer_margin(Margin::symmetric(1.0, 1.0))
            .rounding(Rounding::same(5.0)).shadow(shadow)
            .stroke(Stroke::new(1.0, Color32::from_rgb_additive(36, 156, 158)));

        let mut outer_margin = Margin::default();
        outer_margin.right = 8.0;

        let mut inner_margin = Margin::default();
        inner_margin.top = 6.0;
        inner_margin.left = 3.0;
        inner_margin.right = 3.0;

        let side_panel_frame = Frame::default().fill(Color32::from_rgb(8, 7, 10))
            .inner_margin(inner_margin).outer_margin(outer_margin)
            .rounding(Rounding::same(5.0)).shadow(shadow)
            .stroke(Stroke::new(1.0, Color32::from_rgb_additive(36, 156, 158)));

        ui.style_mut().spacing.button_padding = Vec2::new(10.0, 3.0);

        SidePanel::left("Client_Side_panel").frame(side_panel_frame)
        .show_separator_line(false)
        .width_range(Rangef::new(50.0, 200.0))
        .show_animated_inside(ui, true, |ui |{
            ui.vertical_centered(|ui |{
                // let x = Button::new("Some other things").min_size(Vec2::new(ui.available_width(), 15.0)).ui(ui);
                // if x.clicked() {
                //     spawn_local(async move {
                //         let y = call_with_response().await;
                //         info!("{y:?}");
                //     });
                // }
            });
        });

        TopBottomPanel::top("Client_panel_top").frame(top_panel_frame)
            .show_separator_line(false)
            .show_inside(ui, |ui| 
        {
            ui.vertical_centered(|ui |
            {
                if Button::new("Refresh").min_size(Vec2::new(50.0, 15.0)).ui(ui).clicked()
                {
                    if let Some(db) = self.database.clone(){
                        let usr = self.current_user.clone();
                        if let Some(user) = usr{
                            let tx = self.connected_clients_tx.clone();
                            spawn_local(async move {
                                get_connected_clients(db, tx, user).await.unwrap();
                            });
                        }
                    }
                }
            })
        });

        if !self.error.is_empty() {
            egui::TopBottomPanel::bottom("error").show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Error:");
                    ui.colored_label(egui::Color32::RED, &self.error);
                });
            });
        }
        
        ui.add_space(8.0);
        
        self.client_display(ui);
    }
}

