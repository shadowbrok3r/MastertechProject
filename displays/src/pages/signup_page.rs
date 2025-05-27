use serde::Serialize;
use crate::{app_state::{AppState, SharedContext}, PlatformSpawner, Spawner};
use crossbeam::channel::Sender;
use database::{schema::{Store, User}, Database};
use eframe::egui::{Align, Button, CentralPanel, Color32, ComboBox, Context, Direction, FontId, Frame, Layout, RichText, TextEdit, Vec2, Widget};
use egui_extras::{Size, StripBuilder};
use log::{error, info};
#[cfg(target_arch = "wasm32")]
#[allow(unused_imports)]
use wasm_cookies::CookieOptions;

#[derive(Serialize, Debug, Default, Clone)]
pub struct Signup {
    #[serde(skip)]
    pub first_name: String,
    #[serde(skip)]
    pub last_name: String,
    pub name: String,
    pub store: Store,
    pub everest_initials: String,
    pub email: String,
    pub password: String,
    pub id_prestashop: Option<u64>,
    pub id_store: Option<String>,
}

impl Signup {
    pub fn signup(&self, db_tx: Sender<anyhow::Result<Database, anyhow::Error>>, _appstate_tx: Sender<AppState>){
        let first_initial = self.first_name.clone().chars().nth(0).unwrap_or_default();
        let last_initial = self.last_name.clone().chars().nth(0).unwrap_or_default();

        let initials = format!("{first_initial}{last_initial}").to_uppercase();

        let signup = Self { // serde_json::json!(
            name: format!("{} {}", self.first_name.clone(), self.last_name.clone()),
            email: self.email.clone(),
            password: self.password.clone(),
            everest_initials: initials,
            store: self.store.clone(),
            first_name: self.first_name.clone(),
            last_name: self.last_name.clone(),
            ..Default::default()
        };

        let email = signup.email.clone();
        
        PlatformSpawner::spawn(async move {
            let signup = &mut signup.clone();
            if let Ok(employee) = User::default().set_email(&email).find_employee_by_email().await {
                *signup = Signup {
                    id_prestashop: Some(employee.id.parse::<u64>().unwrap_or_default()),
                    id_store: Some(employee.id_store),
                    everest_initials: employee.initials,
                    ..signup.clone()
                };
            }

            match Database::signup(signup.clone(), email).await {
                Ok(db) => {
                    #[allow(unused_variables)]
                    if let Some(ref cookie) = db.jwt{
                        if let Some(ref usr) = db.user{
                            #[cfg(target_arch="wasm32")]{
                                let usr = serde_json::to_string(&usr).unwrap();
                                let duration = web_time::Duration::from_secs(172800);
                                let cookie_opts = CookieOptions::default().with_same_site(wasm_cookies::SameSite::Strict).secure().expires_after(duration);
                                use brotli::CompressorReader;
                                use base64::{engine::general_purpose, Engine as _};
        
                                fn compress_string(input: &str) -> Vec<u8> {
                                    let mut compressed = Vec::new();
                                    {
                                        let mut compressor = CompressorReader::new(input.as_bytes(), 4096, 11, 22);
                                        std::io::copy(&mut compressor, &mut compressed).unwrap();
                                    }
                                    compressed
                                }
        
                                let compressed: Vec<u8> = compress_string(&usr);
                                let encoded: String = general_purpose::STANDARD.encode(&compressed);
                                info!("Compressed data: {}\nEncoded: {}\nOriginal: {}", compressed.len(), encoded.len(), usr.len());
                                wasm_cookies::set("user", &encoded, &cookie_opts);
                                wasm_cookies::set("jwt", cookie.as_insecure_token(), &cookie_opts);
                            }
                            info!("set cookies");
                        }else{ info!("no usr"); }
                    }else{ info!("no cookie"); }

                    
                    match db_tx.try_send(Ok(db)){
                        Ok(_) => {
                            info!("Sent db connection across thread");
                            drop(db_tx);
                        },
                        Err(err) => error!("Error sending db connection: {err:?}"),
                    }
                },
                Err(e) => error!("Error with db: {e:?}"),
            }
        });
    }
}

impl SharedContext {
    pub fn signup_page(&mut self, ctx: &Context, db_tx: Sender<anyhow::Result<Database, anyhow::Error>>, appstate_tx: Sender<AppState>) {
        CentralPanel::default()
            .frame(Frame::central_panel(&ctx.style()).inner_margin(1.))
            .show(ctx, |ui| 
        {
            StripBuilder::new(ui)
                .cell_layout(Layout::from_main_dir_and_cross_align(Direction::TopDown, Align::Center))
                .sizes(Size::remainder(), 3)
                .horizontal(|mut s| {
                    s.empty();
                    // s.cell(|ui| {
                    //     ui.add_space(ui.available_width() / 3.0);
                    //     ui.label(RichText::new("Mastertech Server").raised().heading());
                    // });
                    s.strip(|s| 
                    {
                        s
                            .cell_layout(Layout::centered_and_justified(Direction::TopDown))
                            .sizes(Size::remainder(), 3)
                            .vertical(|mut s| 
                        {
                            s.cell(|ui| 
                            {
                                ui.group(|ui| 
                                { 
                                    ui.add_space(100.0);

                                    ui.label(RichText::new("Signup").heading());
                                    let font = FontId::proportional(18.0);
                                    ui.style_mut().override_font_id = Some(font);
    
                                    ui.add_space(20.0);
                                    if let Some(signup) = self.signup_mut() {
                                        let width = ui.available_width() / 5.9;
                                        ui.horizontal_top(|ui| {
                                            ui.add_space(width);
                                            TextEdit::singleline(&mut signup.first_name)
                                                .hint_text("First Name")
                                                .desired_width(180.0)
                                                .ui(ui);

                                            ui.add_space(5.0);

                                            TextEdit::singleline(&mut signup.last_name)
                                                .hint_text("Last Name")
                                                .desired_width(180.0)
                                                .ui(ui);
                                        });

                                        ui.add_space(5.0);

                                        ui.horizontal_top(|ui| {
                                            ui.add_space(width);
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
                                        });

                                        ui.add_space(5.0);
                                        ui.horizontal(|ui| {
                                            ui.add_space(ui.available_width() / 2.8);
                                            ComboBox::new("StoreComboBox", "")
                                                .selected_text(signup.store.as_str())
                                                .width(180.0)
                                                .show_ui(ui, |ui| 
                                            {
                                                for store in Store::VALUES{
                                                    ui.selectable_value(&mut signup.store, store.to_owned(), store.as_str());
                                                }
                                            });
                                        });
                                                
                                        ui.add_space(10.0);

                                        ui.vertical_centered(|ui| {
                                            if Button::new("Login")
                                                .fill(Color32::from_rgb(30, 30, 35))
                                                .min_size(Vec2::new(140.0, 15.0))
                                                .ui(ui)
                                                .clicked()
                                            {
                                                match appstate_tx.try_send(AppState::NoAuth("Login".to_string())){
                                                    Ok(_) => info!("Sent appstate"), // drop(appstate_tx)
                                                    Err(e) => error!("Error {e:?}"),
                                                }
                                            }
                                            
                                            ui.add_space(10.0);

                                            if Button::new("Create Account")
                                                .fill(Color32::from_rgb(30, 30, 35))
                                                .min_size(Vec2::new(180.0, 40.0))
                                                .ui(ui)
                                                .clicked()
                                            {
                                                signup.signup(db_tx.clone(), appstate_tx.clone());
                                            }
                                        });
                                    }
                                    ui.add_space(100.0);
                                });
                            });
                            s.empty();
                            s.empty();
                        });
                    });
                    s.empty();
                });
        });
    }
}