#![allow(deprecated)]
use std::sync::{Arc, Mutex};

use crate::{app_state::{AppState, SharedContext}, PlatformSpawner, Spawner};
use crossbeam::channel::Sender;
use database::{schema::{employee_directory, prepare_signup, SignupInput, SignupParams, Store, COMPANY_EMAIL_DOMAINS}, Database};
use eframe::egui::{Align, Button, CentralPanel, Color32, ComboBox, Context, Direction, FontId, Frame, Layout, RichText, Spinner, TextEdit, Ui, Vec2, Widget};
use egui_extras::{Size, StripBuilder};
use log::{error, info};
#[cfg(target_arch = "wasm32")]
#[allow(unused_imports)]
use wasm_cookies::CookieOptions;

const SIGNUP_FIELD_WIDTH: f32 = 260.0;

/// Progress of the employee lookup and account creation.
#[derive(Clone, Default)]
enum SignupStatus {
    #[default]
    Idle,
    Working(&'static str),
    Found { params: SignupParams, summary: String },
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
        COMPANY_EMAIL_DOMAINS.get(self.domain_idx).copied().unwrap_or(COMPANY_EMAIL_DOMAINS[0])
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
    fn create_account(&self, params: SignupParams, ctx: &Context, db_tx: Sender<anyhow::Result<Database, anyhow::Error>>) {
        let status = Arc::clone(&self.status);
        let ctx = ctx.clone();
        set_status(&status, SignupStatus::Working("Creating account..."));
        PlatformSpawner::spawn(async move {
            match Database::signup(params).await {
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
                                wasm_cookies::set("jwt", cookie, &cookie_opts);
                            }
                            info!("set cookies");
                        }else{ info!("no usr"); }
                    }else{ info!("no cookie"); }

                    set_status(&status, SignupStatus::Idle);
                    match db_tx.try_send(Ok(db)){
                        Ok(_) => info!("Sent db connection across thread"),
                        Err(err) => error!("Error sending db connection: {err:?}"),
                    }
                },
                Err(text) => set_status(&status, SignupStatus::Failed(text)),
            }
            ctx.request_repaint();
        });
    }
}

/// Domain, email, password and fallback store inputs with the lookup and create buttons.
fn signup_form(ui: &mut Ui, form: &mut SignupForm, db_tx: &Sender<anyhow::Result<Database, anyhow::Error>>, appstate_tx: &Sender<AppState>) {
    let status = form.status();
    let working = matches!(status, SignupStatus::Working(_));
    let mut edited = false;

    ui.add_enabled_ui(!working, |ui| {
        let previous_domain = form.domain_idx;
        ui.allocate_ui(Vec2::new(SIGNUP_FIELD_WIDTH, ui.spacing().interact_size.y), |ui| {
            ComboBox::new("signup_domain_combo", "")
                .width(SIGNUP_FIELD_WIDTH)
                .selected_text(format!("@{}", form.domain()))
                .show_ui(ui, |ui| {
                    for (idx, domain) in COMPANY_EMAIL_DOMAINS.iter().enumerate() {
                        ui.selectable_value(&mut form.domain_idx, idx, format!("@{domain}"));
                    }
                })
                .response
                .on_hover_text("Domain added to an email typed without @");
        });
        edited |= form.domain_idx != previous_domain;

        ui.add_space(5.0);
        edited |= TextEdit::singleline(&mut form.email)
            .hint_text("Work email")
            .desired_width(SIGNUP_FIELD_WIDTH)
            .ui(ui)
            .changed();

        ui.add_space(5.0);
        edited |= TextEdit::singleline(&mut form.password)
            .hint_text("Password")
            .desired_width(SIGNUP_FIELD_WIDTH)
            .password(true)
            .ui(ui)
            .changed();

        ui.add_space(5.0);
        let previous_store = form.fallback_store;
        ui.allocate_ui(Vec2::new(SIGNUP_FIELD_WIDTH, ui.spacing().interact_size.y), |ui| {
            ComboBox::new("StoreComboBox", "")
                .width(SIGNUP_FIELD_WIDTH)
                .selected_text(format!("Store: {}", form.fallback_store.as_str()))
                .show_ui(ui, |ui| {
                    for store in Store::RETAIL {
                        ui.selectable_value(&mut form.fallback_store, store, store.as_str());
                    }
                })
                .response
                .on_hover_text("Used when the employee record has no retail store");
        });
        edited |= form.fallback_store != previous_store;
    });

    if edited && !working {
        set_status(&form.status, SignupStatus::Idle);
    }

    ui.add_space(10.0);

    match &status {
        SignupStatus::Idle => {}
        SignupStatus::Working(text) => {
            ui.label(*text);
            Spinner::new().ui(ui);
        }
        SignupStatus::Found { summary, .. } if !edited => {
            ui.label(RichText::new(summary).strong());
        }
        SignupStatus::Found { .. } => {}
        SignupStatus::Failed(text) => {
            ui.colored_label(ui.visuals().error_fg_color, text);
        }
    }

    ui.add_space(10.0);

    let find = ui
        .add_enabled(!working, Button::new("Find employee").fill(Color32::from_rgb(30, 30, 35)).min_size(Vec2::new(180.0, 30.0)))
        .clicked();
    if find {
        form.find_employee(ui.ctx());
    }

    ui.add_space(5.0);

    let found = match &status {
        SignupStatus::Found { params, .. } if !edited && !find => Some(params.clone()),
        _ => None,
    };
    let create = ui
        .add_enabled(found.is_some(), Button::new("Create Account").fill(Color32::from_rgb(30, 30, 35)).min_size(Vec2::new(180.0, 40.0)))
        .clicked();
    if create && let Some(params) = found {
        form.create_account(params, ui.ctx(), db_tx.clone());
    }

    ui.add_space(10.0);

    if Button::new("Login")
        .fill(Color32::from_rgb(30, 30, 35))
        .min_size(Vec2::new(140.0, 15.0))
        .ui(ui)
        .clicked()
    {
        match appstate_tx.try_send(AppState::NoAuth("Login".to_string())){
            Ok(_) => info!("Sent appstate"),
            Err(e) => error!("Error {e:?}"),
        }
    }
}

impl SharedContext {
    pub fn signup_page(&mut self, ui: &mut eframe::egui::Ui, db_tx: Sender<anyhow::Result<Database, anyhow::Error>>, appstate_tx: Sender<AppState>) {
        CentralPanel::default()
            .frame(Frame::central_panel(&ui.ctx().global_style()).inner_margin(1.))
            .show(ui, |ui|
        {
            StripBuilder::new(ui)
                .cell_layout(Layout::from_main_dir_and_cross_align(Direction::TopDown, Align::Center))
                .sizes(Size::remainder(), 3)
                .horizontal(|mut s| {
                    s.empty();
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
                                    ui.vertical_centered(|ui| {
                                        ui.add_space(100.0);

                                        ui.label(RichText::new("Signup").heading());
                                        let font = FontId::monospace(18.0);
                                        ui.style_mut().override_font_id = Some(font);

                                        ui.add_space(20.0);
                                        if let Some(signup) = self.signup_mut() {
                                            signup_form(ui, signup, &db_tx, &appstate_tx);
                                        }
                                        ui.add_space(100.0);
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
