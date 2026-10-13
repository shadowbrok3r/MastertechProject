#![allow(deprecated)]
use crate::{PlatformSpawner, Spawner};
use anyhow::{Error, Result};
use crossbeam::channel::Sender;
use database::Database;
use database::schema::{COMPANY_EMAIL_DOMAINS, domain_suffix, normalize_email_with};
use eframe::egui::{
    Align, Align2, CentralPanel, Direction, FontId, Frame, Id, Key, Layout, Modifiers, Rect,
    RichText, Sense, Stroke, StrokeKind, TextEdit, TextStyle, Ui, UiBuilder, Vec2, pos2,
    text::{CCursor, CCursorRange},
    vec2,
};
use egui_extras::{Size, StripBuilder};
use log::{error, info};
use serde::{Deserialize, Serialize};

use crate::app_state::{AppState, MainPages, SharedContext};
use crate::ui_tools::controls::{self, ButtonKind, Metrics, TextButton, TextField, Tokens};
use crate::ui_tools::{fonts, icons::svg};

pub const HASH: &[u8; 31] = b"TheUltimagicalSecretestPassword";

const LOGIN_FIELD_WIDTH: f32 = 260.0;
/// Size of the brand mark above the title.
const LOGO: f32 = 48.0;
/// Title size.
const TITLE_PT: f32 = 24.0;
/// Subtitle size.
const SUBTITLE_PT: f32 = 15.0;
/// Horizontal inset of the username text and icon inside its box.
const FIELD_PAD_X: f32 = 8.0;
/// Gap between the username icon and its text.
const ICON_GAP: f32 = 6.0;

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

                        use base64::{Engine as _, engine::general_purpose};
                        use brotli::CompressorReader;

                        fn compress_string(input: &str) -> Vec<u8> {
                            let mut compressed = Vec::new();
                            {
                                let mut compressor =
                                    CompressorReader::new(input.as_bytes(), 4096, 11, 22);
                                std::io::copy(&mut compressor, &mut compressed).unwrap();
                            }
                            compressed
                        }

                        let compressed: Vec<u8> = compress_string(&usr_json);
                        let encoded: String = general_purpose::STANDARD.encode(&compressed);
                        log::info!(
                            "Compressed data: {}\nEncoded: {}\nOriginal: {}",
                            compressed.len(),
                            encoded.len(),
                            usr_json.len()
                        );

                        wasm_cookies::set("jwt", cookie, &cookie_opts);
                        wasm_cookies::set("user", &encoded, &cookie_opts);
                        info!("set cookies");
                    }
                    appstate_tx.try_send(AppState::Authenticated(MainPages::Tasks))?;
                    db_tx.try_send(Ok(db))?;
                } else {
                    info!("no usr or no cookie");
                    let _ = database::db().invalidate().await;
                    appstate_tx
                        .try_send(AppState::NoAuth("No cookie or user was found".to_string()))?;
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
            }
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
        eframe::egui::Panel::bottom(Id::new("logger_ui"))
            .exact_size(400.)
            .show(ui, |ui| crate::ui_tools::egui_logger::logger_ui().show(ui));

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
                            let (t, m) = (Tokens::get(ui.ctx()), Metrics::get(ui.ctx()));
                            ui.add_space(m.bar_h);
                            let (slot, _) =
                                ui.allocate_exact_size(Vec2::splat(LOGO), Sense::hover());
                            svg::MT_LOGO.paint_duo(
                                ui,
                                slot,
                                LOGO,
                                t.primary,
                                t.primary.gamma_multiply(0.35),
                            );
                            ui.add_space(m.gap.y);
                            ui.label(
                                RichText::new(format!(
                                    "MasterTech Server {}",
                                    database::version_with_build!()
                                ))
                                .font(fonts::semibold(TITLE_PT))
                                .color(t.text),
                            );
                        });
                        s.strip(|s| {
                            s.cell_layout(Layout::centered_and_justified(Direction::TopDown))
                                .sizes(Size::remainder(), 3)
                                .horizontal(|mut s| {
                                    s.empty();
                                    s.cell(|ui| {
                                        ui.vertical_centered(|ui| {
                                            let m = Metrics::get(ui.ctx());
                                            ui.add_space(ui.available_height() / 2.5);
                                            let font = TextStyle::Body.resolve(ui.style());

                                            ui.label(
                                                RichText::new("Please log in")
                                                    .font(fonts::medium(SUBTITLE_PT)),
                                            );
                                            ui.add_space(m.gap.y * 2.0);
                                            let mut refresh = self.refresh;
                                            if let Some(login) = self.login_mut() {
                                                refresh |= login_form(
                                                    ui,
                                                    login,
                                                    &font,
                                                    &db_tx,
                                                    &appstate_tx,
                                                );

                                                if refresh {
                                                    controls::loading_state(ui, "Logging in…");
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
    let (t, m) = (Tokens::get(ui.ctx()), Metrics::get(ui.ctx()));
    let mut domain_idx = stored_domain;
    let labels: Vec<String> = COMPANY_EMAIL_DOMAINS
        .iter()
        .map(|d| format!("@{d}"))
        .collect();
    let options: Vec<(usize, &str)> = labels.iter().map(String::as_str).enumerate().collect();
    controls::dropdown(
        ui,
        "login_domain_combo",
        &mut domain_idx,
        &options,
        LOGIN_FIELD_WIDTH,
    )
    .on_hover_text("Domain added to a username typed without @");
    if domain_idx != stored_domain {
        ui.ctx()
            .data_mut(|d| d.insert_persisted(domain_id, domain_idx));
    }
    let domain = COMPANY_EMAIL_DOMAINS[domain_idx];

    ui.add_space(m.gap.y);

    let suffix = domain_suffix(&login.username, domain);
    let completes = suffix.is_some() && login.username.contains('@');
    let inner = username_box(ui, username_id);
    let mut field = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let mut output = TextEdit::singleline(&mut login.username)
        .id(username_id)
        .font(font.clone())
        .hint_text("Username")
        .frame(Frame::NONE)
        .text_color(t.text)
        .desired_width(inner.width())
        .min_size(inner.size())
        .vertical_align(Align::Center)
        .lock_focus(completes)
        .show(&mut field);
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
            output
                .state
                .cursor
                .set_char_range(Some(CCursorRange::one(end)));
            output.state.store(ui.ctx(), username_id);
        }
        Some(suffix) => {
            let ghost_pos = output.galley_pos + vec2(output.galley.size().x, 0.0);
            ui.painter_at(output.response.rect).text(
                ghost_pos,
                Align2::LEFT_TOP,
                suffix,
                font.clone(),
                t.text_faint,
            );
        }
        None => {}
    }

    ui.add_space(m.gap.y);

    let enter = ui.input_mut(|i| i.key_pressed(Key::Enter));
    let password = TextField::new(&mut login.password)
        .hint("Password")
        .icon(svg::LOCK)
        .password(true)
        .width(LOGIN_FIELD_WIDTH)
        .show(ui);
    if enter && password.lost_focus() {
        password.request_focus();
    }
    let password_focused = password.has_focus() || password.lost_focus();

    ui.add_space(m.gap.y * 3.0);

    let submit = TextButton::new("Sign in")
        .icon(svg::SIGN_IN)
        .kind(ButtonKind::Primary)
        .prominent(true)
        .min_width(LOGIN_FIELD_WIDTH)
        .show(ui)
        .clicked();

    ui.add_space(m.gap.y);

    let create = TextButton::new("Create account")
        .icon(svg::USER_PLUS)
        .kind(ButtonKind::Ghost)
        .min_width(LOGIN_FIELD_WIDTH)
        .show(ui);
    if create.clicked() {
        match appstate_tx.try_send(AppState::CreateAccount) {
            Ok(_) => info!("Sent appstate"),
            Err(e) => error!("Error {e:?}"),
        }
    }

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
                let text = format!(
                    "{:?} is not a valid email or username",
                    login.username.trim()
                );
                ui.ctx().data_mut(|d| d.insert_temp(error_id, text));
            }
        }
    }

    if let Some(text) = ui.ctx().data(|d| d.get_temp::<String>(error_id)) {
        ui.add_space(m.gap.y);
        controls::status_label(ui, svg::WARNING_CIRCLE, &text, t.error);
    }
    started
}

/// Paints the username field box and its icon; returns the rect the text fills.
fn username_box(ui: &mut Ui, id: Id) -> Rect {
    let (t, m) = (Tokens::get(ui.ctx()), Metrics::get(ui.ctx()));
    let (rect, response) =
        ui.allocate_exact_size(vec2(LOGIN_FIELD_WIDTH, m.control_h), Sense::CLICK);
    if response.clicked() {
        ui.memory_mut(|mem| mem.request_focus(id));
    }
    let focused = ui.memory(|mem| mem.has_focus(id));
    let border = if focused {
        t.primary_border
    } else {
        t.field_border
    };
    ui.painter().rect(
        rect,
        t.radius_sm,
        t.field,
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );
    let slot = Rect::from_center_size(
        pos2(rect.left() + FIELD_PAD_X + m.icon_sm / 2.0, rect.center().y),
        Vec2::splat(m.icon_sm),
    );
    let ink = if focused { t.text_dim } else { t.text_faint };
    svg::USER.paint(ui, slot, m.icon_sm, ink);
    Rect::from_min_max(
        pos2(slot.right() + ICON_GAP, rect.top()),
        pos2(rect.right() - FIELD_PAD_X, rect.bottom()),
    )
}

#[cfg(test)]
mod login_form_tests {
    use super::*;
    use eframe::egui::{Context, Event, RawInput};

    fn pass(ctx: &Context, login: &mut Login, events: Vec<Event>) {
        let (db_tx, _db_rx) = crossbeam::channel::unbounded();
        let (app_tx, _app_rx) = crossbeam::channel::unbounded();
        let input = RawInput {
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            login_form(ui, login, &FontId::monospace(18.0), &db_tx, &app_tx);
        });
        output.textures_delta.clear();
    }

    fn tab() -> Event {
        Event::Key {
            key: Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }

    /// Focuses the username field, then presses Tab.
    fn focus_then_tab(username: &str) -> (Context, Login) {
        let ctx = Context::default();
        let mut login = Login {
            username: username.into(),
            password: String::new(),
        };
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
