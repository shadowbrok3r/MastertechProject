use serde::Serialize;
use crate::app_state::{AppState, MtechServer};
use crossbeam::channel::Sender;
use database::{schema::Store, Database};
use egui::{Align, Button, CentralPanel, Color32, ComboBox, Direction, FontId, Frame, Layout, RichText, Stroke, TextEdit, Vec2, Widget};
use egui_extras::{Size, StripBuilder};
use log::info;
use wasm_bindgen_futures::spawn_local;
use wasm_cookies::CookieOptions;
// use egui_form::{validator::validator::Validate, Form, FormField, _validator_field_path};
// use egui_form::validator::field_path;

#[derive(Serialize, Debug, Default, Clone)]
pub struct Signup {
    // #[validate(length(min = 3, max = 10))]
    #[serde(skip)]
    pub first_name: String,
    #[serde(skip)]
    pub last_name: String,
    pub name: String,
    pub store: Store,
    pub everest_initials: String,
    pub email: String,
    pub password: String,
}

impl Signup{
    pub fn signup(&self, db_tx: Sender<anyhow::Result<Database, anyhow::Error>>, _appstate_tx: Sender<AppState>){
        let first_initial = self.first_name.chars().nth(0).unwrap();
        let last_initial = self.last_name.chars().nth(0).unwrap();

        let initials = format!("{first_initial}{last_initial}").to_uppercase();


        let signup: Signup = Self { // serde_json::json!(
            name: format!("{} {}", self.first_name.clone(), self.last_name.clone()),
            email: self.email.clone(),
            password: self.password.clone(),
            everest_initials: initials,
            store: self.store.clone(),
            ..Default::default()
        };

        let email = signup.email.clone();
        spawn_local(async move {
            let database = Database::signup(signup.clone(), email).await;

            // #[cfg(target_arch="wasm32-unknown-unknown")]
            match database{
                Ok(db) => {
                    let cookie_opts = CookieOptions::default().with_same_site(wasm_cookies::SameSite::None);
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
    pub fn signup_page(&mut self, ctx: &egui::Context, db_tx: Sender<anyhow::Result<Database, anyhow::Error>>, appstate_tx: Sender<AppState>) {
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

                                    ui.label(RichText::new("Signup").heading());
                                    let font = FontId::proportional(18.0);
                                    ui.style_mut().override_font_id = Some(font);
    
                                    // ui.label("Please Login");
                                    ui.add_space(20.0);
                                    if let Some(signup) = self.signup_mut(){

                                    

                                        TextEdit::singleline(&mut signup.first_name)
                                            .hint_text("First Name")
                                            .desired_width(180.0)
                                            .ui(ui);

                                        ui.add_space(5.0);

                                        TextEdit::singleline(&mut signup.last_name)
                                            .hint_text("Last Name")
                                            .desired_width(180.0)
                                            .ui(ui);

                                        ui.add_space(5.0);

                                        TextEdit::singleline(&mut signup.email)
                                            .hint_text("Email")
                                            .desired_width(180.0)
                                            .ui(ui);

                                        ui.add_space(5.0);

                                        TextEdit::singleline(&mut signup.password)
                                            .hint_text("Password")
                                            .desired_width(180.0)
                                            .password(true)
                                            .ui(ui);

                                        ui.add_space(5.0);

                                        ui.horizontal_centered(|ui| {

                                            ComboBox::new("StoreComboBox", "")
                                                .selected_text("RIV")
                                                .width(180.0)
                                                .show_ui(ui, |ui| 
                                            {
                                                for mut store in Store::VALUES{
                                                    ui.selectable_value(&mut signup.store, store.to_owned(), store.as_str());
                                                }
                                            });
                                        });
                                                
                                        ui.add_space(10.0);

                                        if Button::new("Login")
                                            .fill(Color32::from_rgb(30, 30, 35))
                                            .stroke(Stroke::new(2.0, Color32::from_rgb(30, 3, 28)))
                                            .min_size(Vec2::new(140.0, 15.0))
                                            .ui(ui)
                                            .clicked()
                                        {
                                            match appstate_tx.send(AppState::NoAuth("Login".to_string())){
                                                Ok(_) => info!("Sent appstate"), // drop(appstate_tx)
                                                Err(e) => info!("Error {e:?}"),
                                            }
                                        }

                                        if Button::new("Create Account")
                                            .fill(Color32::from_rgb(30, 30, 35))
                                            .stroke(Stroke::new(2.0, Color32::from_rgb(30, 3, 28)))
                                            .min_size(Vec2::new(180.0, 40.0))
                                            .ui(ui)
                                            .clicked()
                                        {
                                            signup.signup(db_tx.clone(), appstate_tx.clone());
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