#![allow(deprecated)]
use std::sync::{Arc, Mutex};

use crate::{
    PlatformSpawner, Spawner,
    app_state::{AppState, SharedContext},
};
use crossbeam::channel::Sender;
use database::{
    Database,
    schema::{
        COMPANY_EMAIL_DOMAINS, SignupInput, SignupParams, Store, employee_directory, prepare_signup,
    },
};
use eframe::egui::{
    Align, CentralPanel, Context, Direction, Frame, Layout, RichText, Sense, Ui, Vec2,
};
use egui_extras::{Size, StripBuilder};
use log::{error, info};
#[cfg(target_arch = "wasm32")]
#[allow(unused_imports)]
use wasm_cookies::CookieOptions;

use crate::ui_tools::controls::{
    self, ButtonKind, IconLabel, Metrics, TextButton, TextField, Tokens,
};
use crate::ui_tools::{fonts, icons::svg};

const SIGNUP_FIELD_WIDTH: f32 = 260.0;
/// Size of the icon above the title.
const LOGO: f32 = 40.0;
/// Title size.
const TITLE_PT: f32 = 20.0;

/// Progress of the employee lookup and account creation.
#[derive(Clone, Default)]
enum SignupStatus {
    #[default]
    Idle,
    Working(&'static str),
    Found {
        params: SignupParams,
        summary: String,
    },
    Failed(String),
}

/// Signup form; the account name and store come from the employee record.
#[derive(Default)]
pub struct SignupForm {
    email: String,
    password: String,
    domain_idx: usize,
    fallback_store: Store,
    status: Arc<Mutex<SignupStatus>>,
}

fn set_status(status: &Mutex<SignupStatus>, next: SignupStatus) {
    if let Ok(mut current) = status.lock() {
        *current = next;
    }
}

impl SignupForm {
    fn status(&self) -> SignupStatus {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn domain(&self) -> &'static str {
        COMPANY_EMAIL_DOMAINS
            .get(self.domain_idx)
            .copied()
            .unwrap_or(COMPANY_EMAIL_DOMAINS[0])
    }

    /// Looks up the employee matching the entered email and stores the resulting params.
    fn find_employee(&self, ctx: &Context) {
        let input = SignupInput {
            email: self.email.clone(),
            password: self.password.clone(),
            domain: self.domain().to_string(),
            fallback_store: self.fallback_store,
        };
        let status = Arc::clone(&self.status);
        let ctx = ctx.clone();
        set_status(&status, SignupStatus::Working("Looking up employee..."));
        PlatformSpawner::spawn(async move {
            let directory = employee_directory();
            let next = match prepare_signup(&directory, input).await {
                Ok((params, _)) => SignupStatus::Found {
                    summary: format!("Employee found: {}, {}", params.name, params.store.as_str()),
                    params,
                },
                Err(e) => {
                    log::warn!("Signup lookup refused: {e:?}");
                    SignupStatus::Failed(e.to_string())
                }
            };
            set_status(&status, next);
            ctx.request_repaint();
        });
    }

    /// Creates the account and sends the signed-in connection on `db_tx`.
    fn create_account(
        &self,
        params: SignupParams,
        ctx: &Context,
        db_tx: Sender<anyhow::Result<Database, anyhow::Error>>,
    ) {
        let status = Arc::clone(&self.status);
        let ctx = ctx.clone();
        set_status(&status, SignupStatus::Working("Creating account..."));
        PlatformSpawner::spawn(async move {
            match Database::signup(params).await {
                Ok(db) => {
                    #[allow(unused_variables)]
                    if let Some(ref cookie) = db.jwt {
                        if let Some(ref usr) = db.user {
                            #[cfg(target_arch = "wasm32")]
                            {
                                let usr = serde_json::to_string(&usr).unwrap();
                                let duration = web_time::Duration::from_secs(172800);
                                let cookie_opts = CookieOptions::default()
                                    .with_same_site(wasm_cookies::SameSite::Strict)
                                    .secure()
                                    .expires_after(duration);
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

                                let compressed: Vec<u8> = compress_string(&usr);
                                let encoded: String = general_purpose::STANDARD.encode(&compressed);
                                info!(
                                    "Compressed data: {}\nEncoded: {}\nOriginal: {}",
                                    compressed.len(),
                                    encoded.len(),
                                    usr.len()
                                );
                                wasm_cookies::set("user", &encoded, &cookie_opts);
                                wasm_cookies::set("jwt", cookie, &cookie_opts);
                            }
                            info!("set cookies");
                        } else {
                            info!("no usr");
                        }
                    } else {
                        info!("no cookie");
                    }

                    set_status(&status, SignupStatus::Idle);
                    match db_tx.try_send(Ok(db)) {
                        Ok(_) => info!("Sent db connection across thread"),
                        Err(err) => error!("Error sending db connection: {err:?}"),
                    }
                }
                Err(text) => set_status(&status, SignupStatus::Failed(text)),
            }
            ctx.request_repaint();
        });
    }
}

/// Domain, email, password and fallback store inputs with the lookup and create buttons.
fn signup_form(
    ui: &mut Ui,
    form: &mut SignupForm,
    db_tx: &Sender<anyhow::Result<Database, anyhow::Error>>,
    appstate_tx: &Sender<AppState>,
) {
    let (t, m) = (Tokens::get(ui.ctx()), Metrics::get(ui.ctx()));
    let status = form.status();
    let working = matches!(status, SignupStatus::Working(_));
    let mut edited = false;

    ui.add_enabled_ui(!working, |ui| {
        let previous_domain = form.domain_idx;
        let labels: Vec<String> = COMPANY_EMAIL_DOMAINS
            .iter()
            .map(|d| format!("@{d}"))
            .collect();
        let domains: Vec<(usize, &str)> = labels.iter().map(String::as_str).enumerate().collect();
        controls::dropdown(
            ui,
            "signup_domain_combo",
            &mut form.domain_idx,
            &domains,
            SIGNUP_FIELD_WIDTH,
        )
        .on_hover_text("Domain added to an email typed without @");
        edited |= form.domain_idx != previous_domain;

        ui.add_space(m.gap.y);
        edited |= TextField::new(&mut form.email)
            .hint("Work email")
            .icon(svg::ENVELOPE_SIMPLE)
            .width(SIGNUP_FIELD_WIDTH)
            .show(ui)
            .changed();

        ui.add_space(m.gap.y);
        edited |= TextField::new(&mut form.password)
            .hint("Password")
            .icon(svg::LOCK)
            .password(true)
            .width(SIGNUP_FIELD_WIDTH)
            .show(ui)
            .changed();

        ui.add_space(m.gap.y);
        let previous_store = form.fallback_store;
        let store_labels: Vec<String> = Store::RETAIL
            .iter()
            .map(|s| format!("Store: {}", s.as_str()))
            .collect();
        let stores: Vec<(Store, &str)> = Store::RETAIL
            .iter()
            .zip(store_labels.iter())
            .map(|(s, label)| (*s, label.as_str()))
            .collect();
        controls::dropdown(
            ui,
            "StoreComboBox",
            &mut form.fallback_store,
            &stores,
            SIGNUP_FIELD_WIDTH,
        )
        .on_hover_text("Used when the employee record has no retail store");
        edited |= form.fallback_store != previous_store;
    });

    if edited && !working {
        set_status(&form.status, SignupStatus::Idle);
    }

    ui.add_space(m.gap.y);

    match &status {
        SignupStatus::Idle => {}
        SignupStatus::Working(text) => {
            controls::loading_row(ui, text);
        }
        SignupStatus::Found { summary, .. } if !edited => {
            IconLabel::new(svg::USER_CHECK, summary)
                .tone(t.success)
                .wrap(true)
                .show(ui);
        }
        SignupStatus::Found { .. } => {}
        SignupStatus::Failed(text) => {
            IconLabel::new(svg::WARNING_CIRCLE, text)
                .tone(t.error)
                .wrap(true)
                .show(ui);
        }
    }

    ui.add_space(m.gap.y);

    let find = TextButton::new("Find employee")
        .icon(svg::MAGNIFYING_GLASS)
        .prominent(true)
        .min_width(SIGNUP_FIELD_WIDTH)
        .enabled(!working)
        .show(ui)
        .clicked();
    if find {
        form.find_employee(ui.ctx());
    }

    ui.add_space(m.gap.y);

    let found = match &status {
        SignupStatus::Found { params, .. } if !edited && !find => Some(params.clone()),
        _ => None,
    };
    let create = TextButton::new("Create account")
        .icon(svg::USER_PLUS)
        .kind(ButtonKind::Primary)
        .prominent(true)
        .min_width(SIGNUP_FIELD_WIDTH)
        .enabled(found.is_some())
        .show(ui)
        .clicked();
    if create && let Some(params) = found {
        form.create_account(params, ui.ctx(), db_tx.clone());
    }

    ui.add_space(m.gap.y * 2.0);

    let login = TextButton::new("Back to log in")
        .icon(svg::SIGN_IN)
        .kind(ButtonKind::Ghost)
        .show(ui);
    if login.clicked() {
        match appstate_tx.try_send(AppState::NoAuth("Login".to_string())) {
            Ok(_) => info!("Sent appstate"),
            Err(e) => error!("Error {e:?}"),
        }
    }
}

impl SharedContext {
    pub fn signup_page(
        &mut self,
        ui: &mut eframe::egui::Ui,
        db_tx: Sender<anyhow::Result<Database, anyhow::Error>>,
        appstate_tx: Sender<AppState>,
    ) {
        CentralPanel::default()
            .frame(Frame::central_panel(&ui.ctx().global_style()).inner_margin(1.))
            .show(ui, |ui| {
                StripBuilder::new(ui)
                    .cell_layout(Layout::from_main_dir_and_cross_align(
                        Direction::TopDown,
                        Align::Center,
                    ))
                    .sizes(Size::remainder(), 3)
                    .horizontal(|mut s| {
                        s.empty();
                        s.strip(|s| {
                            s.cell_layout(Layout::centered_and_justified(Direction::TopDown))
                                .sizes(Size::remainder(), 3)
                                .vertical(|mut s| {
                                    s.cell(|ui| {
                                        crate::ui_tools::glass_card::group(ui, |ui| {
                                            ui.vertical_centered(|ui| {
                                                let (t, m) =
                                                    (Tokens::get(ui.ctx()), Metrics::get(ui.ctx()));
                                                ui.add_space(m.bar_h * 2.0);

                                                let (slot, _) = ui.allocate_exact_size(
                                                    Vec2::splat(LOGO),
                                                    Sense::hover(),
                                                );
                                                svg::USER_PLUS.paint_duo(
                                                    ui,
                                                    slot,
                                                    LOGO,
                                                    t.primary,
                                                    t.primary.gamma_multiply(0.35),
                                                );
                                                ui.add_space(m.gap.y);
                                                ui.label(
                                                    RichText::new("Sign up")
                                                        .font(fonts::semibold(TITLE_PT))
                                                        .color(t.text),
                                                );

                                                ui.add_space(m.gap.y * 3.0);
                                                if let Some(signup) = self.signup_mut() {
                                                    signup_form(ui, signup, &db_tx, &appstate_tx);
                                                }
                                                ui.add_space(m.bar_h * 2.0);
                                            });
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
