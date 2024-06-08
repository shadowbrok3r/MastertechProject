use egui::{TextEdit, Ui};
use egui_form::{validator::validator::Validate, Form, FormField, _validator_field_path};
use egui_form::validator::field_path;
use crate::app_state::MtechServer;
use crossbeam::channel::Sender;
use database::Database;
use egui::{Align, Button, CentralPanel, Direction, Frame, Layout, RichText, TextEdit, Vec2, Widget};
use egui_extras::{Size, StripBuilder};
use log::info;
use wasm_bindgen_futures::spawn_local;
use wasm_cookies::CookieOptions;

use crate::app_state::MtechServer;


#[derive(Validate, Debug)]
struct Signup {
    // #[validate(length(min = 3, max = 10))]
    pub user_name: String,
    // #[validate(email)]
    pub email: String,
    // #[validate(nested)]
    pub nested: Nested,
    // #[validate(nested)]
    pub vec: Vec<Nested>,
}

impl Signup{
    pub fn signup(&self, db_tx: Sender<anyhow::Result<Database, anyhow::Error>>, appstate_tx: Sender<AppState>){
        let user = self.username.clone();
        let pass = self.password.clone();
        spawn_local(async move {
            let database = Database::new(user, pass, None).await;

            // #[cfg(target_arch="wasm32-unknown-unknown")]
            match database{
                Ok(db) => {
                    let cookie_opts = CookieOptions::default().with_same_site(wasm_cookies::SameSite::Strict);
                    if let Some(ref cookie) = db.jwt{
                        if let Some(ref usr) = db.user{
                            wasm_cookies::set("jwt", cookie.as_insecure_token(), &cookie_opts);
                            let usr = serde_json::to_string(&usr).unwrap();
                            wasm_cookies::set("user", &usr, &cookie_opts);
                            info!("set cookies");
                        }else{ info!("no usr"); }
                    }else{ info!("no cookie"); }

                    
                    match db_tx.send(Ok(db)){
                        Ok(_) => {
                            info!("Sent db connection across thread");
                            drop(db_tx);
                        },
                        Err(err) => info!("Error sending db connection: {err:?}"),
                    }
                },
                Err(e) => info!("Error with db: {e:?}"),
            }
        });
    }
}

impl MtechServer{
    pub fn signup_page(&mut self, ctx: &egui::Context, db_tx: Sender<anyhow::Result<Database, anyhow::Error>>) {
        // wasm_cookies::delete("user");
        // wasm_cookies::delete("jwt");
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

                                    ui.label(RichText::new("Create Account").heading());
                                    ui.add_space(20.0);
                                    if let Some(login) = self.login_mut(){

                                        let mut bo = false;
                                        if ui.toggle_value(&mut  bo, RichText::new("Login").small_raised())
                                            .clicked()
                                        {
                                            // self.state = AppState::CreateAccount;
                                        }

                                        TextEdit::singleline(&mut login.username)
                                            .hint_text("First Name")
                                            .desired_width(70.0)
                                            .ui(ui);

                                        ui.add_space(5.0);

                                        TextEdit::singleline(&mut login.password)
                                            .hint_text("Last Name")
                                            .desired_width(70.0)
                                            .password(true)
                                            .ui(ui);

                                            

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

                                        ComboBox::new("Store");
                                            
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