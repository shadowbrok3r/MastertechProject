use crossbeam::channel::Sender;
use database::{Database, DATABASE};
use eframe::egui::{Align, Button, CentralPanel, Color32, Context, Direction, FontId, Frame, Key, KeyboardShortcut, Layout, Modifiers, Pos2, Spinner, Stroke, TextEdit, Vec2, Widget};
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
    pub async fn login(email: String, pass: String, db_tx: Sender<anyhow::Result<Database, anyhow::Error>>, appstate_tx: Sender<AppState>) -> anyhow::Result<(), anyhow::Error>{
        let database = Database::new(email, pass, None).await;
        match database{
            Ok(db) => {
                let duration = web_time::Duration::from_secs(345600);
                let cookie_opts = CookieOptions::default().with_same_site(wasm_cookies::SameSite::None).secure().expires_after(duration);
                let database = db.clone();
                if let (Some(ref cookie), Some(ref usr)) = (database.jwt, database.user){
                    wasm_cookies::set("jwt", cookie.as_insecure_token(), &cookie_opts);
                    let usr = serde_json::to_string(&usr)?;
                    wasm_cookies::set("user", &usr, &cookie_opts);
                    info!("set cookies");
                }else{ 
                    info!("no usr or no cookie"); 
                    let _ = DATABASE.invalidate().await;
                    appstate_tx.try_send(AppState::NoAuth("No cookie or user was found".to_string()))?;
                }
                appstate_tx.try_send(AppState::Authenticated(MainPages::Tasks))?;
                db_tx.try_send(Ok(db))?;
            },
            Err(e) => {
                let check = e.to_string().contains("Already connected");
                info!("{e:?} // Already connected? {:?}", check);
                if check { appstate_tx.try_send(AppState::Authenticated(MainPages::Tasks))?; }
                else { appstate_tx.try_send(AppState::NoAuth(e.to_string()))?; }
            },
        }
        Ok(())
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
                                ui.style_mut().override_font_id = Some(font.clone());

                                ui.label("Please Login");
                                ui.add_space(20.0);
                                if let Some(login) = self.login_mut(){
                                    let text_edit = TextEdit::singleline(&mut login.username)
                                        .desired_width(180.0);
                                    
                                    let output = text_edit.show(ui);
                                    let chars = login.username.chars().count() as f32;
                                    let painter = ui.painter_at(output.response.rect);
                                    let text_color = Color32::from_rgba_premultiplied(100, 100, 100, 100);
                                    let galley = painter.layout(
                                        String::from("@pclaptops.com"),
                                        font,
                                        text_color,
                                        f32::INFINITY
                                    );
                                    painter.galley(Pos2::new(output.galley_pos.x + (chars as f32 * 11.75), output.galley_pos.y), galley, text_color);
                                    ui.add_space(4.0);

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
                                            let user = login.username.clone();
                                            let pass = login.password.clone();
                                            let email = format!("{user}@pclaptops.com");
                                            let tx = db_tx.clone();
                                            let app_tx = appstate_tx.clone();
                                            spawn_local(async move {
                                                let res = Login::login(email, pass, tx, app_tx.clone()).await;
                                                match res {
                                                    Ok(_) => app_tx.try_send(AppState::Authenticated(MainPages::Tasks)).unwrap(), 
                                                    Err(e) => app_tx.try_send(AppState::NoAuth(e.to_string())).unwrap()
                                                }
                                            });
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
                                        let app_tx = appstate_tx.clone();
                                        match app_tx.try_send(AppState::CreateAccount){
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
                                        let user = login.username.clone();
                                        let pass = login.password.clone();
                                        let email = format!("{user}@pclaptops.com");
                                        spawn_local(async move {
                                                let res = Login::login(email, pass, db_tx.clone(), appstate_tx.clone()).await;
                                                match res {
                                                    Ok(_) => appstate_tx.try_send(AppState::Authenticated(MainPages::Tasks)).unwrap(), 
                                                    Err(e) => appstate_tx.try_send(AppState::NoAuth(e.to_string())).unwrap()
                                                }
                                        });
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