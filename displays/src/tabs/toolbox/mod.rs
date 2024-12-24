use database::{schema::buckets::list_buckets, STORAGE_URL};
use eframe::egui::{Button, CentralPanel, Color32, Frame, Layout, Margin, Rounding, Stroke, TopBottomPanel, Ui, Vec2, Widget};
use log::info;
use crate::{app_state::SharedContext, PlatformSpawner, Spawner};

impl SharedContext {
    pub fn toolbox(&mut self, ui: &mut Ui){
        ui.ctx().request_repaint();

        let mut inner_margin_top = Margin::default();
        inner_margin_top.top = 5.0;

        let btm_panel_frame = Frame::default().inner_margin(inner_margin_top.clone())
            .rounding(Rounding::same(10.0));


        let mut inner_margin = Margin::default();
        inner_margin.top = 3.0;
        inner_margin.left = 3.0;
        inner_margin.right = 3.0;
        inner_margin.bottom = 5.0;

        let panel_frame = Frame::default()
            .fill(Color32::from_rgb(12, 12, 14))
            .inner_margin(inner_margin)
            .rounding(Rounding::same(10.0))
            .stroke(Stroke::new(1.0, Color32::from_additive_luminance(50)));
        
        ui.style_mut().spacing.button_padding = Vec2::new(10.0, 3.0);

        TopBottomPanel::top("FileBrowserTop").frame(btm_panel_frame)
        .show_separator_line(false)
        .show_inside(ui, |ui| {
            ui.with_layout(Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                if Button::new("Refresh").ui(ui).clicked(){
                    let user = self.current_user.as_ref();
                    if let Some(usr) = user {
                        if let (
                            Some(access_key), 
                            Some(secret_key)
                        ) = (
                            usr.minio_access_key.clone(),
                            usr.minio_secret_key.clone(),
                        ) {
                            info!("Retrieving minio files: {access_key:?}");
                            self.filesystem.access_key = access_key.clone();
                            self.filesystem.secret_key = secret_key.clone();
                            let tx = self.filesystem.paths_channel.0.clone();
                            let name = usr.email.clone();
                            let parsed = name.split_once('@').unwrap().0.to_string().clone();
                            // PlatformSpawner::spawn(async move {
                            //     let result = list_buckets(STORAGE_URL.to_string(), access_key, secret_key, parsed).await;
                            //     match result {
                            //         Ok(buckets) => {let _ = tx.try_send(buckets);},
                            //         Err(err) => log::warn!("Error: {err:?}"),
                            //     }
                            // });
                        }
                    }
                }
            });
        });

        TopBottomPanel::bottom("FileBrowserBottom").frame(btm_panel_frame)
            .show_separator_line(false)
            .show_inside(ui, |ui| {
                ui.vertical_centered(|ui |
                {
                    self.filesystem.show_progress(ui);
                })
            });

        ui.add_space(10.0);
        CentralPanel::default().frame(panel_frame)
            .show_inside(ui, |ui| 
        {
            self.filesystem.display(ui);
        });
    }
}

