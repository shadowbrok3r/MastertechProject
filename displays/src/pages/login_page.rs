#![allow(deprecated)]
use anyhow::{Error, Result};
use crossbeam::channel::Sender;
use database::{Database, db};
use serde::{Deserialize, Serialize};
use crate::{PlatformSpawner, Spawner};
use eframe::egui::{
    Align, Button, CentralPanel, Color32, Context, Direction, FontId, Frame, Id, Key, KeyboardShortcut, Layout, Modifiers, Pos2, Spinner, Stroke, TextEdit, Vec2, Widget
};
use egui_extras::{Size, StripBuilder};
use log::{error, info};

use crate::app_state::{AppState, MainPages, SharedContext};

pub const HASH: &[u8; 31] = b"TheUltimagicalSecretestPassword";

#[derive(Serialize, Deserialize, Debug)]
pub struct Login {
    pub username: String,
    pub password: String,
}

impl Default for Login {
    fn default() -> Self {
        Self {
            username: Default::default(),
            password: Default::default(),
        }
    }
}

impl Login {
    pub async fn login(
        email: String,
        pass: String,
        db_tx: Sender<anyhow::Result<Database, anyhow::Error>>,
        appstate_tx: Sender<AppState>,
    ) -> Result<(), Error> {
        log::info!("Logging in");
        let database = Database::new(email, pass, None).await;
        match database {
            Ok(db) => {
                let database = db.clone();
                #[allow(unused_variables)]
                if let (Some(ref cookie), Some(ref usr)) = (database.jwt, database.user) {
                    log::info!("Got a cookie and user");
                    #[cfg(target_arch = "wasm32")]
                    {
                        let duration = web_time::Duration::from_secs(172800);
                        let cookie_opts = wasm_cookies::CookieOptions::default()
                            .with_same_site(wasm_cookies::SameSite::Strict)
                            .secure()
                            .expires_after(duration);

                        let usr_json = serde_json::to_string(&usr)?;
                        
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

                        let compressed: Vec<u8> = compress_string(&usr_json);
                        let encoded: String = general_purpose::STANDARD.encode(&compressed);
                        log::info!("Compressed data: {}\nEncoded: {}\nOriginal: {}", compressed.len(), encoded.len(), usr_json.len());

                        wasm_cookies::set("jwt", cookie, &cookie_opts);
                        wasm_cookies::set("user", &encoded, &cookie_opts);
                        info!("set cookies");
                    }
                    appstate_tx.try_send(AppState::Authenticated(MainPages::Tasks))?;
                    db_tx.try_send(Ok(db))?;
                } else {
                    info!("no usr or no cookie");
                    let _ = database::db().invalidate().await;
                    appstate_tx.try_send(AppState::NoAuth("No cookie or user was found".to_string()))?;
                }
            }
            Err(e) => {
                
                log::error!("{}", e.to_string());
                if e.to_string().contains("Already connected") {
                    log::error!("1. Already connected");
                    appstate_tx.try_send(AppState::Authenticated(MainPages::Tasks))?;
                } else {
                    log::error!("2. No Auth");
                    appstate_tx.try_send(AppState::NoAuth(e.to_string()))?;
                }
            },
        }
        Ok(())
    }
}

impl SharedContext {
    pub fn login_page(
        &mut self,
        ui: &mut eframe::egui::Ui,
        db_tx: Sender<anyhow::Result<Database, anyhow::Error>>,
        appstate_tx: Sender<AppState>,
    ) {
        eframe::egui::Panel::bottom(Id::new("logger_ui")).exact_size(400.).show(ui, |ui| crate::ui_tools::egui_logger::logger_ui().show(ui));

        CentralPanel::default()
            .frame(Frame::central_panel(&ui.ctx().global_style()).inner_margin(1.))
            .show(ui, |ui| {
                StripBuilder::new(ui)
                    .cell_layout(Layout::from_main_dir_and_cross_align(
                        Direction::TopDown,
                        Align::Center,
                    ))
                    .sizes(Size::remainder(), 3)
                    .vertical(|mut s| {
                        s.cell(|ui| {
                            ui.add_space(50.0);
                            let font = FontId::monospace(30.0);
                            ui.style_mut().override_font_id = Some(font);
                            ui.label(format!("Mastertech Server {}", database::version_with_build!()));
                        });
                        s.strip(|s| {
                            s.cell_layout(Layout::centered_and_justified(Direction::TopDown))
                                .sizes(Size::remainder(), 3)
                                .horizontal(|mut s| {
                                    s.empty();
                                    s.cell(|ui| {
                                        ui.vertical_centered(|ui| {
                                            ui.add_space(ui.available_height() / 2.5);
                                            let font = FontId::monospace(18.0);
                                            ui.style_mut().override_font_id = Some(font.clone());

                                            ui.label("Please Login");
                                            ui.add_space(20.0);
                                            let mut refresh = self.refresh.clone();
                                            if let Some(login) = self.login_mut() {
                                                let text_edit =
                                                    TextEdit::singleline(&mut login.username)
                                                        .font(font.clone())
                                                        .desired_width(180.0);

                                                let output = text_edit.show(ui);

                                                let chars = login.username.chars().count() as f32;
                                                let painter = ui.painter_at(output.response.rect);
                                                let text_color = Color32::from_rgba_premultiplied(
                                                    100, 100, 100, 100,
                                                );

                                                let galley = painter.layout(
                                                    String::from("@pclaptops.com"),
                                                    font,
                                                    text_color,
                                                    f32::INFINITY,
                                                );

                                                painter.galley(
                                                    Pos2::new(
                                                        output.galley_pos.x
                                                            + (chars as f32 * 11.5) / 1.25,
                                                        output.galley_pos.y,
                                                    ),
                                                    galley,
                                                    text_color,
                                                );
                                                ui.add_space(4.0);

                                                let enter =
                                                    ui.input_mut(|i| i.key_pressed(Key::Enter));

                                                if TextEdit::singleline(&mut login.password)
                                                    .hint_text("Password")
                                                    .desired_width(180.0)
                                                    .password(true)
                                                    .return_key(KeyboardShortcut::new(
                                                        Modifiers::SHIFT,
                                                        Key::Enter,
                                                    ))
                                                    .ui(ui)
                                                    .has_focus()
                                                {
                                                    if enter
                                                        && !login.password.is_empty()
                                                        && !login.username.is_empty()
                                                    {
                                                        refresh = true;
                                                        // info!("ENTER PRESSED");
                                                        let user = login.username.clone();
                                                        let pass = login.password.clone();
                                                        let tx = db_tx.clone();
                                                        let app_tx = appstate_tx.clone();
                                                        PlatformSpawner::spawn(async move {
                                                            let _ = Login::login(
                                                                user,
                                                                pass,
                                                                tx,
                                                                app_tx.clone(),
                                                            )
                                                            .await;
                                                        });
                                                    }
                                                }

                                                ui.add_space(30.0);

                                                let button = Button::new("Create Account")
                                                    .min_size(Vec2::new(140.0, 15.0))
                                                    .ui(ui);

                                                // ui.add_enabled(enabled, button);

                                                if button.clicked() {
                                                    Spinner::new()
                                                        .size(30.0)
                                                        .color(Color32::from_rgb(100, 10, 80))
                                                        .ui(ui);
                                                    let app_tx = appstate_tx.clone();
                                                    match app_tx.try_send(AppState::CreateAccount) {
                                                        Ok(_) => info!("Sent appstate"), // drop(appstate_tx)
                                                        Err(e) => error!("Error {e:?}"),
                                                    }
                                                }

                                                ui.add_space(3.0);

                                                if Button::new("Submit")
                                                    .min_size(Vec2::new(140.0, 40.0))
                                                    .ui(ui)
                                                    .clicked()
                                                    && !login.password.is_empty()
                                                    && !login.username.is_empty()
                                                {
                                                    refresh = true;
                                                    let user = login.username.clone();
                                                    let pass = login.password.clone();
                                                    let email = format!("{user}@pclaptops.com");
                                                    PlatformSpawner::spawn(async move {
                                                        let res = Login::login(
                                                            email,
                                                            pass,
                                                            db_tx.clone(),
                                                            appstate_tx.clone(),
                                                        )
                                                        .await;
                                                        log::warn!("Result: {res:?}");
                                                    });
                                                }

                                                if refresh {
                                                    ui.label("Logging in..");
                                                    Spinner::new().size(50.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
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
