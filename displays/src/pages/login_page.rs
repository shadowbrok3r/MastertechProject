#![allow(deprecated)]
use anyhow::{Error, Result};
use crossbeam::channel::Sender;
use database::Database;
use database::schema::{domain_suffix, normalize_email_with, COMPANY_EMAIL_DOMAINS};
use serde::{Deserialize, Serialize};
use crate::{PlatformSpawner, Spawner};
use eframe::egui::{
    text::{CCursor, CCursorRange},
    vec2, Align, Align2, Button, CentralPanel, Color32, ComboBox, Direction, FontId, Frame, Id, Key, KeyboardShortcut, Layout, Modifiers, Spinner, TextEdit, Ui, Vec2, Widget
};
use egui_extras::{Size, StripBuilder};
use log::{error, info};

use crate::app_state::{AppState, MainPages, SharedContext};

pub const HASH: &[u8; 31] = b"TheUltimagicalSecretestPassword";

const LOGIN_FIELD_WIDTH: f32 = 260.0;

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
                                            let mut refresh = self.refresh;
                                            if let Some(login) = self.login_mut() {
                                                refresh |= login_form(ui, login, &font, &db_tx, &appstate_tx);

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

/// Domain picker, username with ghost domain, password and buttons; true when a sign-in started.
fn login_form(
    ui: &mut Ui,
    login: &mut Login,
    font: &FontId,
    db_tx: &Sender<anyhow::Result<Database, anyhow::Error>>,
    appstate_tx: &Sender<AppState>,
) -> bool {
    let domain_id = Id::new("login_domain");
    let username_id = Id::new("login_username");
    let error_id = Id::new("login_error");

    let stored_domain = ui
        .ctx()
        .data_mut(|d| d.get_persisted::<usize>(domain_id))
        .filter(|idx| *idx < COMPANY_EMAIL_DOMAINS.len())
        .unwrap_or(0);
    let mut domain_idx = stored_domain;
    ui.allocate_ui(Vec2::new(LOGIN_FIELD_WIDTH, ui.spacing().interact_size.y), |ui| {
        ComboBox::new("login_domain_combo", "")
            .width(LOGIN_FIELD_WIDTH)
            .selected_text(format!("@{}", COMPANY_EMAIL_DOMAINS[domain_idx]))
            .show_ui(ui, |ui| {
                for (idx, domain) in COMPANY_EMAIL_DOMAINS.iter().enumerate() {
                    ui.selectable_value(&mut domain_idx, idx, format!("@{domain}"));
                }
            })
            .response
            .on_hover_text("Domain added to a username typed without @");
    });
    if domain_idx != stored_domain {
        ui.ctx().data_mut(|d| d.insert_persisted(domain_id, domain_idx));
    }
    let domain = COMPANY_EMAIL_DOMAINS[domain_idx];

    ui.add_space(4.0);

    let suffix = domain_suffix(&login.username, domain);
    let completes = suffix.is_some() && login.username.contains('@');
    let mut output = TextEdit::singleline(&mut login.username)
        .id(username_id)
        .font(font.clone())
        .desired_width(LOGIN_FIELD_WIDTH)
        .lock_focus(completes)
        .show(ui);
    if output.response.changed() {
        ui.ctx().data_mut(|d| d.remove::<String>(error_id));
    }
    let accept_suffix = completes
        && output.response.has_focus()
        && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab));
    match suffix {
        Some(suffix) if accept_suffix => {
            login.username.push_str(&suffix);
            let end = CCursor::new(login.username.chars().count());
            output.state.cursor.set_char_range(Some(CCursorRange::one(end)));
            output.state.store(ui.ctx(), username_id);
        }
        Some(suffix) => {
            let ghost_pos = output.galley_pos + vec2(output.galley.size().x, 0.0);
            ui.painter_at(output.response.rect).text(
                ghost_pos,
                Align2::LEFT_TOP,
                suffix,
                font.clone(),
                Color32::from_rgba_premultiplied(100, 100, 100, 100),
            );
        }
        None => {}
    }

    ui.add_space(4.0);

    let enter = ui.input_mut(|i| i.key_pressed(Key::Enter));
    let password_focused = TextEdit::singleline(&mut login.password)
        .hint_text("Password")
        .desired_width(LOGIN_FIELD_WIDTH)
        .password(true)
        .return_key(KeyboardShortcut::new(Modifiers::SHIFT, Key::Enter))
        .ui(ui)
        .has_focus();

    ui.add_space(30.0);

    if Button::new("Create Account")
        .min_size(Vec2::new(140.0, 15.0))
        .ui(ui)
        .clicked()
    {
        match appstate_tx.try_send(AppState::CreateAccount) {
            Ok(_) => info!("Sent appstate"),
            Err(e) => error!("Error {e:?}"),
        }
    }

    ui.add_space(3.0);

    let submit = Button::new("Submit")
        .min_size(Vec2::new(140.0, 40.0))
        .ui(ui)
        .clicked();

    let mut started = false;
    if (submit || (enter && password_focused))
        && !login.password.is_empty()
        && !login.username.is_empty()
    {
        match normalize_email_with(&login.username, domain) {
            Some(email) => {
                ui.ctx().data_mut(|d| d.remove::<String>(error_id));
                login.username = email.clone();
                started = true;
                let pass = login.password.clone();
                let tx = db_tx.clone();
                let app_tx = appstate_tx.clone();
                PlatformSpawner::spawn(async move {
                    let res = Login::login(email, pass, tx, app_tx).await;
                    log::warn!("Result: {res:?}");
                });
            }
            None => {
                let text = format!("{:?} is not a valid email or username", login.username.trim());
                ui.ctx().data_mut(|d| d.insert_temp(error_id, text));
            }
        }
    }

    if let Some(text) = ui.ctx().data(|d| d.get_temp::<String>(error_id)) {
        ui.colored_label(ui.visuals().error_fg_color, text);
    }
    started
}

#[cfg(test)]
mod login_form_tests {
    use super::*;
    use eframe::egui::{Context, Event, RawInput};

    fn pass(ctx: &Context, login: &mut Login, events: Vec<Event>) {
        let (db_tx, _db_rx) = crossbeam::channel::unbounded();
        let (app_tx, _app_rx) = crossbeam::channel::unbounded();
        let input = RawInput { events, ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| {
            login_form(ui, login, &FontId::monospace(18.0), &db_tx, &app_tx);
        });
        output.textures_delta.clear();
    }

    fn tab() -> Event {
        Event::Key { key: Key::Tab, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
    }

    /// Focuses the username field, then presses Tab.
    fn focus_then_tab(username: &str) -> (Context, Login) {
        let ctx = Context::default();
        let mut login = Login { username: username.into(), password: String::new() };
        pass(&ctx, &mut login, vec![]);
        ctx.memory_mut(|m| m.request_focus(Id::new("login_username")));
        pass(&ctx, &mut login, vec![]);
        pass(&ctx, &mut login, vec![]);
        pass(&ctx, &mut login, vec![tab()]);
        pass(&ctx, &mut login, vec![]);
        (ctx, login)
    }

    #[test]
    fn tab_completes_a_partly_typed_company_domain() {
        let (ctx, login) = focus_then_tab("bob@x");
        assert_eq!(login.username, "bob@xidax.com");
        assert!(ctx.memory(|m| m.has_focus(Id::new("login_username"))));
    }

    #[test]
    fn tab_leaves_a_bare_username_and_moves_focus_on() {
        let (ctx, login) = focus_then_tab("bob");
        assert_eq!(login.username, "bob");
        assert!(!ctx.memory(|m| m.has_focus(Id::new("login_username"))));
    }
}
