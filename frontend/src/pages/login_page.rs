use crossbeam::channel::Sender;
use database::Database;
use egui::{Align, Button, CentralPanel, Direction, Frame, Layout, RichText, TextEdit, Vec2, Widget};
use egui_extras::{Size, StripBuilder};
use log::{error, info};
use wasm_bindgen_futures::spawn_local;
use wasm_cookies::CookieOptions;

use crate::app_state::MtechServer;

pub struct Login {
    pub username: String,
    pub password: String,
}
impl Default for Login{
    fn default() -> Self {
        Self { 
            username: Default::default(), 
            password: Default::default() 
        }
    } 
}

impl Login{
    pub fn login(&self, db_tx: Sender<Database>){
        let user = self.username.clone();
        let pass = self.password.clone();
        spawn_local(async move {
            let database = Database::new(user, pass, None).await;

            match database{
                Ok(db) => {
                    let cookie_opts = CookieOptions::default();
                    if let Some(ref cookie) = db.jwt{
                        if let Some(ref usr) = db.user{
                            wasm_cookies::set("jwt", cookie.as_insecure_token(), &cookie_opts);
                            let usr = serde_json::to_string(&usr).unwrap();
                            wasm_cookies::set("user", &usr, &cookie_opts);
                        }else{ info!("no usr"); }
                    }else{ info!("no cookie"); }

                    
                    match db_tx.send(db){
                        Ok(_) => {
                            info!("Sent db connection across thread");
                            drop(db_tx);
                        },
                        Err(err) => info!("Error sending db connection: {err:?}"),
                    }
                },
                Err(e) => error!("Error with db: {e:?}"),
            }
        });
    }
}

impl MtechServer{
    pub fn login_page(&mut self, ctx: &egui::Context, db_tx: Sender<Database>) {
        CentralPanel::default()
            .frame(Frame::central_panel(&ctx.style()).inner_margin(1.))
            .show(ctx, |ui| 
        {
            StripBuilder::new(ui)
                .cell_layout(Layout::from_main_dir_and_cross_align(Direction::TopDown, Align::Center))
                .sizes(Size::remainder(), 3)
                .vertical(|mut s| {
                    s.cell(|ui| {
                        ui.add_space(50.0);
                        ui.label(RichText::new("Mastertech Server").raised().heading());
                    });
                    s.strip(|s| 
                    {
                        s
                            .cell_layout(Layout::centered_and_justified(Direction::TopDown))
                            .sizes(Size::remainder(), 3)
                            .horizontal(|mut s| 
                        {
                            s.empty();
                            s.cell(|ui| 
                            {
                                ui.vertical_centered(|ui| 
                                { 
                                    ui.add_space(ui.available_height() / 2.5);

                                    ui.label(RichText::new("Please Login").heading());
                                    ui.add_space(20.0);
                                    if let Some(login) = self.login_mut(){

                                        let mut bo = false;
                                        if ui.toggle_value(&mut  bo, RichText::new("Create Account").small_raised())
                                            .clicked()
                                        {

                                        }

                                        TextEdit::singleline(&mut login.username)
                                            .hint_text("Email")
                                            .desired_width(130.0)
                                            .ui(ui);

                                        ui.add_space(5.0);

                                        TextEdit::singleline(&mut login.password)
                                            .hint_text("Password")
                                            .desired_width(130.0)
                                            .password(true)
                                            .ui(ui);

                                        ui.add_space(10.0);

                                        if Button::new("Submit")
                                            .min_size(Vec2::new(100.0, 16.0))
                                            .ui(ui)
                                            .clicked()
                                        {
                                            login.login(db_tx);
                                        }

                                    }
                                });
                            });
                            s.empty();
                        });
                    });
                    s.empty();
                });
        });
    }

}