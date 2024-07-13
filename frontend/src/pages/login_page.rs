use crossbeam::channel::Sender;
use database::{Database, DATABASE};
use eframe::egui::{Align, Button, CentralPanel, Color32, Context, Direction, FontId, Frame, Key, KeyboardShortcut, Layout, Modifiers, Spinner, Stroke, TextEdit, Vec2, Widget};
use egui_extras::{Size, StripBuilder};
use log::info;
use wasm_bindgen_futures::spawn_local;
use wasm_cookies::CookieOptions;

use crate::app_state::{AppState, MainPages, MtechServer};

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
    pub fn login(&self, db_tx: Sender<anyhow::Result<Database, anyhow::Error>>, appstate_tx: Sender<AppState>){
        let user = self.username.clone();
        let pass = self.password.clone();
        spawn_local(async move {
            let database = Database::new(user, pass, None).await;

            // #[cfg(target_arch="wasm32-unknown-unknown")]
            match database{
                Ok(db) => {
                    let cookie_opts = CookieOptions::default().with_same_site(wasm_cookies::SameSite::None).secure();
                    if let Some(ref cookie) = db.jwt{
                        if let Some(ref usr) = db.user{
                            wasm_cookies::set("jwt", cookie.as_insecure_token(), &cookie_opts);
                            let usr = serde_json::to_string(&usr).unwrap();
                            wasm_cookies::set("user", &usr, &cookie_opts);
                            info!("set cookies");
                        }else{ 
                            info!("no usr"); 
                            let _ = DATABASE.invalidate().await;
                            match appstate_tx.try_send(AppState::NoAuth("No user was found".to_string())){
                                Ok(_) => info!("Sent appstate"), // drop(appstate_tx)
                                Err(e) => info!("Error {e:?}"),
                            }
                        }
                    }else{ 
                        info!("no cookie"); 
                        let _ = DATABASE.invalidate().await;
                        match appstate_tx.try_send(AppState::NoAuth("No cookie was found".to_string())){
                            Ok(_) => info!("Sent appstate"), // drop(appstate_tx)
                            Err(e) => info!("Error {e:?}"),
                        }
                    }

                    match appstate_tx.try_send(AppState::Authenticated(MainPages::Tasks)){
                        Ok(_) => info!("Sent appstate"), // drop(appstate_tx)
                        Err(e) => info!("Error {e:?}"),
                    }
                    match db_tx.try_send(Ok(db)){
                        Ok(_) => {
                            info!("Sent db connection across thread");
                            drop(db_tx);
                        },
                        Err(err) => info!("Error sending db connection: {err:?}"),
                    }
                },
                Err(e) => {
                    info!("Error with db: {e:?}");
                    if e.to_string().contains("Already connected"){
                        match appstate_tx.try_send(AppState::Authenticated(MainPages::Tasks)){
                            Ok(_) => info!("Sent appstate"), // drop(appstate_tx)
                            Err(e) => info!("Error {e:?}"),
                        }
                    }
                    match appstate_tx.try_send(AppState::NoAuth(e.to_string())){
                        Ok(_) => info!("Sent appstate"), // drop(appstate_tx)
                        Err(e) => info!("Error {e:?}"),
                    }
                },
            }
        });
    }
}

impl MtechServer{
    pub fn login_page(&mut self, ctx: &Context, db_tx: Sender<anyhow::Result<Database, anyhow::Error>>, appstate_tx: Sender<AppState>) {
        CentralPanel::default()
            .frame(Frame::central_panel(&ctx.style()).inner_margin(1.))
            .show(ctx, |ui| 
        {
            StripBuilder::new(ui)
                .cell_layout(Layout::from_main_dir_and_cross_align(Direction::TopDown, Align::Center))
                .sizes(Size::remainder(), 3)
                .vertical(|mut s| 
            {
                s.cell(|ui| {
                    ui.add_space(50.0);
                    let font = FontId::proportional(30.0);
                    ui.style_mut().override_font_id = Some(font);
                    ui.label(format!("Mastertech Server {}", env!("CARGO_PKG_VERSION")));
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
                                let font = FontId::proportional(18.0);
                                ui.style_mut().override_font_id = Some(font);

                                ui.label("Please Login");
                                ui.add_space(20.0);
                                if let Some(login) = self.login_mut(){

                                    TextEdit::singleline(&mut login.username)
                                        .hint_text("Email")
                                        .desired_width(180.0)
                                        .ui(ui);

                                    ui.add_space(2.0);

                                    let enter = ui.input_mut(|i| i.key_pressed(Key::Enter));

                                    if TextEdit::singleline(&mut login.password)
                                        .hint_text("Password")
                                        .desired_width(180.0)
                                        .password(true)
                                        .return_key(KeyboardShortcut::new(Modifiers::SHIFT, Key::Enter))
                                        .ui(ui)
                                        .has_focus()
                                    {
                                        if enter && !login.password.is_empty() && !login.username.is_empty(){
                                            info!("ENTER PRESSED");
                                            login.login(db_tx.clone(), appstate_tx.clone());
                                        }
                                    }

                                    ui.add_space(30.0);

                                    
                                    let button = Button::new("Create Account")
                                        .fill(Color32::from_rgb(30, 30, 35))
                                        .stroke(Stroke::new(2.0, Color32::from_rgb(30, 3, 28)))
                                        .min_size(Vec2::new(140.0, 15.0))
                                        .ui(ui);
                                    
                                    // ui.add_enabled(enabled, button);

                                    if button.clicked()
                                    {
                                        Spinner::new()
                                            .size(30.0)
                                            .color(Color32::from_rgb(100, 10, 80))
                                            .ui(ui);

                                        match appstate_tx.try_send(AppState::CreateAccount){
                                            Ok(_) => info!("Sent appstate"), // drop(appstate_tx)
                                            Err(e) => info!("Error {e:?}"),
                                        }
                                    }

                                    ui.add_space(3.0);

                                    if Button::new("Submit")
                                    .fill(Color32::from_rgb(30, 30, 35))
                                    .stroke(Stroke::new(2.0, Color32::from_rgb(30, 3, 28)))
                                    .min_size(Vec2::new(140.0, 40.0))
                                    .ui(ui)
                                    .clicked()
                                    {
                                        login.login(db_tx, appstate_tx.clone());
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