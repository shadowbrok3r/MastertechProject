#![allow(deprecated)]
use crate::{
    app_state::{AppState, MainPages, SharedContext},
    tabs::tasks::task_layout::{SortField, SortOptions},
    ui_tools::{
        controls::{self, ButtonKind, IconBox, IconLabel, Metrics, TextButton, TextField, Tokens},
        glass_card,
        icons::svg::{self, Icon},
    },
};
use crossbeam::channel::Sender;
use database::{
    DatabaseSelection, PlatformSpawner, Spawner, SurrealValue, db,
    schema::{McpSettings, SortDirection, Status, Store, User},
};
use eframe::egui::{
    Align, CentralPanel, Direction, Frame, InnerResponse, Key, Label, Layout, Rect, RichText,
    ScrollArea, Sense, Ui, UiBuilder, Vec2, vec2,
};
use egui_extras::{Size, StripBuilder};
use log::{error, info};
use serde::Serialize;

/// Width of the account fields.
const FIELD_W: f32 = 230.0;
/// Width of the custom-status field.
const STATUS_FIELD_W: f32 = 115.0;
/// Width of the sort dropdown.
const SORT_W: f32 = 120.0;
/// Width of the startup and endpoint fields.
const WIDE_FIELD_W: f32 = 330.0;
/// Widest the startup and endpoint cards grow.
const WIDE_CARD_W: f32 = 360.0;
/// Height the three top cards keep.
const CARD_MIN_H: f32 = 222.0;
/// Widest the custom-status list grows.
const STATUS_LIST_W: f32 = 160.0;

/// An icon label and its value in the tertiary colour.
fn detail_row(ui: &mut Ui, icon: Icon, label: &str, value: &str, mono: bool) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        controls::icon_label(ui, icon, label);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let text = RichText::new(value).color(t.tertiary);
            let text = if mono { text.monospace() } else { text };
            ui.add(Label::new(text).truncate());
        });
    });
}

#[derive(Serialize, Debug, Clone, Default, database::SurrealValue)]
pub struct UserPreferences {
    name: String,
    email: String,
    password: String,
    retyped_password: String,
    store: Store,
    database: DatabaseSelection,
    sort_by: SortOptions,
    last_sort_field: Option<SortField>,
    new_status: String,
    mcp_endpoint: String,
    mcp_api_key: String,
    mcp_model: String,
    user: User,
}

impl UserPreferences {
    pub fn set_user(&mut self, user: User) {
        self.mcp_endpoint = user.get_mcp_endpoint().unwrap_or_default();
        self.mcp_api_key = user.get_mcp_api_key().unwrap_or_default();
        self.mcp_model = user.get_mcp_model().unwrap_or_default();
        self.user = user.clone();
    }

    /// Persists the startup mode. `None` means the picker runs every launch.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn save_default_work_mode(&mut self, mode: Option<crate::tabs::WorkMode>) {
        let slug = mode.map(|m| m.slug().to_owned());
        self.user.set_default_work_mode_local(slug.clone());
        let mut user = self.user.clone();
        PlatformSpawner::spawn(async move {
            if let Err(e) = user.save_default_work_mode(slug).await {
                error!("Error saving the default work mode: {e:?}");
            }
        });
    }

    pub fn save_mcp_settings(&mut self) {
        let to_opt = |s: &str| {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        };
        let settings = McpSettings {
            endpoint: to_opt(&self.mcp_endpoint),
            api_key: to_opt(&self.mcp_api_key),
            model: to_opt(&self.mcp_model),
        };
        self.user.set_mcp_settings(settings.clone());
        crate::ai::apply_mcp_settings(&self.user);
        PlatformSpawner::spawn(async move {
            if let Err(e) = User::save_mcp_settings(settings).await {
                error!("Error saving mcp settings: {e:?}");
            }
        });
    }

    pub async fn mod_account(&self, user_id: String) {
        let acc_mod: UserPreferences = Self {
            name: self.name.clone(),
            email: self.email.clone(),
            password: self.password.clone(),
            ..Default::default()
        };

        let mod_user_result: Result<_, surrealdb::Error> = db()
            .query("fn::modify_account($user, $new)")
            .bind(("user", user_id))
            .bind(("new", acc_mod))
            .await;

        info!("mod_user_result: {mod_user_result:?}");
    }

    pub fn change_password(&self) {
        let password = self.password.clone();
        PlatformSpawner::spawn(async move {
            let x: Result<_, surrealdb::Error> = db()
                .query("UPDATE $auth.id SET password = crypto::argon2::generate($pass)")
                .bind(("pass", password))
                .await;
            info!("X: {x:?}");
        });
    }
}

impl SharedContext {
    pub fn account_settings_page(
        &mut self,
        ui: &mut eframe::egui::Ui,
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
                    .size(Size::initial(300.0).at_least(200.).at_most(500.0))
                    .size(Size::remainder())
                    .size(Size::initial(300.0).at_least(200.).at_most(500.0))
                    .horizontal(|mut s| {
                        s.empty();
                        s.strip(|s| {
                            s.cell_layout(Layout::left_to_right(Align::Center))
                                .size(Size::exact(50.0))
                                .size(Size::remainder())
                                .size(Size::exact(50.0))
                                .vertical(|mut s| {
                                    s.empty();
                                    s.cell(|ui| {
                                        ui.vertical_centered(|ui| {
                                            let m = Metrics::get(ui.ctx());
                                            let avail_size =
                                                vec2(ui.available_width() / 3.2, CARD_MIN_H);

                                            ui.horizontal_top(|ui| {
                                                ui.set_min_width(avail_size.x);
                                                ui.columns(3, |ui| {
                                                    ui[0].vertical_centered(|ui| {
                                                        glass_card::group(ui, |ui| {
                                                            ui.set_min_size(avail_size);
                                                            glass_card::card_header(
                                                                ui,
                                                                svg::USER_GEAR,
                                                                "Account settings",
                                                                None,
                                                            );
                                                            self.account_card(ui, &appstate_tx);
                                                        });
                                                    });
                                                    ui[1].vertical_centered(|ui| {
                                                        glass_card::group(ui, |ui| {
                                                            ui.set_min_size(avail_size);
                                                            glass_card::card_header(
                                                                ui,
                                                                svg::KANBAN,
                                                                "Task page preferences",
                                                                None,
                                                            );
                                                            self.task_preferences_card(
                                                                ui,
                                                                avail_size.y,
                                                            );
                                                        });
                                                    });
                                                    ui[2].vertical_centered(|ui| {
                                                        glass_card::group(ui, |ui| {
                                                            ui.set_min_size(avail_size);
                                                            glass_card::card_header(
                                                                ui,
                                                                svg::IDENTIFICATION_CARD,
                                                                "Other account details",
                                                                None,
                                                            );
                                                            let user = &self.account_mod.user;
                                                            let access = user
                                                                .get_minio_access_key()
                                                                .unwrap_or_default();
                                                            let secret = user
                                                                .get_minio_secret_key()
                                                                .unwrap_or_default();
                                                            let employee = user
                                                                .get_employee_id()
                                                                .unwrap_or_default()
                                                                .to_string();
                                                            let store = user
                                                                .get_store_id()
                                                                .unwrap_or_default();
                                                            detail_row(
                                                                ui,
                                                                svg::KEY,
                                                                "Minio access key",
                                                                &access,
                                                                true,
                                                            );
                                                            detail_row(
                                                                ui,
                                                                svg::LOCK_KEY,
                                                                "Minio secret key",
                                                                &secret,
                                                                true,
                                                            );
                                                            detail_row(
                                                                ui,
                                                                svg::STOREFRONT,
                                                                "Prestashop ID",
                                                                &employee,
                                                                false,
                                                            );
                                                            detail_row(
                                                                ui,
                                                                svg::HASH,
                                                                "Store #",
                                                                &store,
                                                                false,
                                                            );
                                                            detail_row(
                                                                ui,
                                                                svg::SHIELD_CHECK,
                                                                "Authorization",
                                                                user.get_authorization().as_str(),
                                                                false,
                                                            );
                                                        });
                                                    });
                                                });
                                            });

                                            ui.add_space(m.gap.y);
                                            #[cfg(not(target_arch = "wasm32"))]
                                            {
                                                use crate::tabs::WorkMode;

                                                glass_card::group(ui, |ui| {
                                                    ui.set_max_width(WIDE_CARD_W);
                                                    glass_card::card_header(
                                                        ui,
                                                        svg::ROCKET_LAUNCH,
                                                        "Startup",
                                                        None,
                                                    );
                                                    ui.vertical_centered(|ui| {
                                                        let current = self
                                                            .account_mod
                                                            .user
                                                            .get_user_settings()
                                                            .get_default_work_mode()
                                                            .as_deref()
                                                            .and_then(WorkMode::from_slug);
                                                        let mut picked = current;
                                                        let options: Vec<(Option<WorkMode>, &str)> =
                                                            std::iter::once((
                                                                None,
                                                                "Ask me every time",
                                                            ))
                                                            .chain(WorkMode::ALL.iter().map(
                                                                |mode| (Some(*mode), mode.title()),
                                                            ))
                                                            .collect();
                                                        controls::dropdown(
                                                            ui,
                                                            "default_work_mode",
                                                            &mut picked,
                                                            &options,
                                                            WIDE_FIELD_W,
                                                        );
                                                        if picked != current {
                                                            self.account_mod
                                                                .save_default_work_mode(picked);
                                                        }
                                                        ui.add_space(m.gap.y / 2.0);
                                                        IconLabel::new(
                                                            svg::INFO,
                                                            "Which screen MasterTech opens into.",
                                                        )
                                                        .small(true)
                                                        .tone(Tokens::get(ui.ctx()).text_dim)
                                                        .show(ui);
                                                    });
                                                });

                                                ui.add_space(m.gap.y);
                                            }

                                            glass_card::group(ui, |ui| {
                                                ui.set_max_width(WIDE_CARD_W);
                                                glass_card::card_header(
                                                    ui,
                                                    svg::PLUGS_CONNECTED,
                                                    "MCP / AI endpoint",
                                                    None,
                                                );
                                                ui.vertical_centered(|ui| {
                                                    TextField::new(
                                                        &mut self.account_mod.mcp_endpoint,
                                                    )
                                                    .hint("OpenAI-compatible endpoint")
                                                    .icon(svg::GLOBE)
                                                    .width(WIDE_FIELD_W)
                                                    .show(ui);

                                                    ui.add_space(m.gap.y);

                                                    TextField::new(
                                                        &mut self.account_mod.mcp_api_key,
                                                    )
                                                    .hint("API key")
                                                    .icon(svg::KEY)
                                                    .password(true)
                                                    .width(WIDE_FIELD_W)
                                                    .show(ui);

                                                    ui.add_space(m.gap.y);

                                                    TextField::new(&mut self.account_mod.mcp_model)
                                                        .hint("Model")
                                                        .icon(svg::BRAIN)
                                                        .width(WIDE_FIELD_W)
                                                        .show(ui);

                                                    ui.add_space(m.gap.y);

                                                    let save = TextButton::new("Save endpoint")
                                                        .icon(svg::FLOPPY_DISK)
                                                        .kind(ButtonKind::Primary)
                                                        .prominent(true)
                                                        .show(ui);
                                                    if save.clicked() {
                                                        self.account_mod.save_mcp_settings();
                                                    }
                                                });
                                            });

                                            ui.add_space(m.gap.y);
                                            crate::pages::ai_profile::ai_profile_section(
                                                ui,
                                                &self.account_mod.user,
                                            );
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

impl SharedContext {
    /// Name, email, store, database and the password change.
    fn account_card(&mut self, ui: &mut Ui, appstate_tx: &Sender<AppState>) {
        let (t, m) = (Tokens::get(ui.ctx()), Metrics::get(ui.ctx()));
        let (Some(usr), Some(acc_mod)) = (self.current_user.clone(), self.account_mut()) else {
            return;
        };
        TextField::new(&mut usr.get_name().to_owned())
            .hint("Name")
            .icon(svg::USER)
            .width(FIELD_W)
            .show(ui);

        ui.add_space(m.gap.y);

        TextField::new(&mut usr.get_email().to_owned())
            .hint("Email")
            .icon(svg::ENVELOPE_SIMPLE)
            .width(FIELD_W)
            .show(ui);

        ui.add_space(m.gap.y);

        let half = (FIELD_W - m.gap.x) / 2.0;
        ui.allocate_ui_with_layout(
            vec2(FIELD_W, m.control_h),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = m.gap.x;
                let stores: Vec<(Store, &str)> =
                    Store::VALUES.iter().map(|s| (*s, s.as_str())).collect();
                controls::dropdown(ui, "StoreComboBox", &mut acc_mod.store, &stores, half)
                    .on_hover_text("Store");
                let databases = [
                    (
                        DatabaseSelection::Stable,
                        DatabaseSelection::Stable.as_str(),
                    ),
                    (DatabaseSelection::Beta, DatabaseSelection::Beta.as_str()),
                    (DatabaseSelection::Local, DatabaseSelection::Local.as_str()),
                ];
                let db = controls::dropdown(
                    ui,
                    "Database Editor",
                    &mut acc_mod.database,
                    &databases,
                    half,
                )
                .on_hover_text("Database");
                if db.clicked() {
                    let acc = acc_mod.clone();
                    PlatformSpawner::spawn(async move {
                        let set_db = acc.database.set_database().await;
                        log::info!("Set database: {set_db:?}");
                    });
                }
            },
        );

        ui.add_space(m.gap.y);

        TextField::new(&mut acc_mod.password)
            .hint("Password")
            .icon(svg::LOCK)
            .password(true)
            .width(FIELD_W)
            .show(ui);

        ui.add_space(m.gap.y);
        let password_check = acc_mod.password.eq(&acc_mod.retyped_password.clone());

        TextField::new(&mut acc_mod.retyped_password)
            .hint("Retype password")
            .icon(svg::LOCK_KEY)
            .password(true)
            .width(FIELD_W)
            .show(ui);

        if !password_check {
            IconLabel::new(svg::WARNING_CIRCLE, "Passwords must match")
                .small(true)
                .tone(t.error)
                .show(ui);
        }

        ui.add_space(m.gap.y);

        let enabled_button = TextButton::new("Change password")
            .icon(svg::KEY)
            .kind(ButtonKind::Primary)
            .prominent(true)
            .enabled(acc_mod.password.len() > 3 && password_check)
            .show(ui);
        let accepted_by_keyboard = ui.input_mut(|input| input.key_pressed(Key::Enter));

        if enabled_button.clicked() || accepted_by_keyboard {
            acc_mod.change_password();
            let tx = appstate_tx.clone();
            match tx.try_send(AppState::Authenticated(MainPages::Tasks)) {
                Ok(_) => info!("Sent appstate"),
                Err(e) => error!("Error {e:?}"),
            }
        }
    }

    /// Custom statuses and the default sort.
    fn task_preferences_card(&mut self, ui: &mut Ui, card_h: f32) {
        let (t, m) = (Tokens::get(ui.ctx()), Metrics::get(ui.ctx()));
        ui.horizontal(|ui| {
            controls::icon_label(ui, svg::TAG, "Custom status");

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let submit_status =
                    controls::icon_button(ui, svg::PLUS, IconBox::Sm, false, "Add status");
                TextField::new(&mut self.account_mod.new_status)
                    .hint("New status")
                    .width(STATUS_FIELD_W)
                    .show(ui);

                if submit_status.clicked() && !self.account_mod.new_status.is_empty() {
                    let status = self.account_mod.new_status.clone();
                    self.account_mod.new_status.clear();
                    self.account_mod
                        .user
                        .set_statuses(Status::CustomStatus(status.clone()));
                    log::info!("Got a new status: {status}");
                    PlatformSpawner::spawn(async move {
                        match User::add_custom_status(status.clone().as_str()).await {
                            Ok(_) => log::info!("Created new status: {}", status),
                            Err(e) => log::error!("Error creating new status: {e:?}"),
                        }
                    });
                }
            });
        });

        ui.add_space(m.gap.y);

        ui.horizontal(|ui| {
            controls::icon_label(ui, svg::SORT_DESCENDING, "Default priority");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let mut field = self.account_mod.sort_by.field.clone();
                let options = [
                    (SortField::Default, "Priority"),
                    (SortField::Name, "Name"),
                    (SortField::Date, "Date"),
                    (SortField::Completed, "Completed"),
                ];
                let picked =
                    controls::dropdown(ui, "SortBy Settings", &mut field, &options, SORT_W);
                let (icon, tone, tip) = match self.account_mod.sort_by.direction {
                    SortDirection::Asc => (svg::SORT_ASCENDING, t.warn, "Ascending"),
                    SortDirection::Desc => (svg::SORT_DESCENDING, t.error, "Descending"),
                };
                let (slot, arrow) = ui.allocate_exact_size(Vec2::splat(m.icon_md), Sense::hover());
                icon.paint(ui, slot, m.icon_md, tone);
                arrow.on_hover_text(tip);
                if picked.changed() {
                    self.account_mod.last_sort_field = Some(field);
                }
            });
        });

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_width(STATUS_LIST_W)
            .max_height(card_h - 70.)
            .show(ui, |ui| {
                let clicked: &mut Option<Status> = &mut None;
                for (idx, status) in self
                    .account_mod
                    .user
                    .get_custom_statuses()
                    .iter()
                    .enumerate()
                {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{idx}:")).color(t.text_faint));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if controls::icon_button(
                                ui,
                                svg::X,
                                IconBox::Sm,
                                false,
                                "Remove status",
                            )
                            .clicked()
                            {
                                *clicked = Some(status.clone());
                                let status = status.clone();
                                PlatformSpawner::spawn(async move {
                                    match User::remove_custom_status(status.as_str()).await {
                                        Ok(_) => log::info!("Deleted status: {:?}", status),
                                        Err(e) => log::error!("Error deleting status: {e:?}"),
                                    }
                                });
                            }
                            controls::icon_label(ui, svg::TAG_SIMPLE, status.as_str());
                        });
                    });
                }
                if let Some(status) = clicked
                    && let Some(statuses) = self.account_mod.user.get_custom_statuses_mut()
                    && let Some(idx) = statuses.iter().position(|x| *x == *status)
                {
                    statuses.remove(idx);
                }
            });
    }
}

/// Runs `add_contents` centred horizontally, measuring it on a hidden first pass.
pub fn centered_ui<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    let id = ui.make_persistent_id("centered_ui");
    let is_first_frame = ui.memory(|mem| !mem.data.get_temp(id).unwrap_or(false));

    let max_rect = ui.cursor().intersect(ui.max_rect());
    let mut ui_builder =
        UiBuilder::new()
            .max_rect(max_rect)
            .layout(Layout::from_main_dir_and_cross_align(
                Direction::LeftToRight,
                Align::Min,
            ));

    if is_first_frame {
        if ui.is_visible() {
            ui.ctx()
                .request_discard("Initial sizing pass for centered_ui");
        }
        ui_builder = ui_builder.sizing_pass().invisible();
    }

    ui.scope_builder(ui_builder, |ui| {
        let is_sizing_pass = ui.is_sizing_pass();

        let content_response = add_contents(ui);
        let content_rect: Rect = ui.min_rect();
        let content_width = content_rect.width();

        if !is_sizing_pass {
            let available_width = ui.available_width();
            if available_width > content_width {
                let padding = (available_width - content_width) / 2.0;
                ui.add_space(padding);
            }
        }

        if is_first_frame {
            ui.memory_mut(|mem| mem.data.insert_temp(id, true));
        }

        content_response
    })
}
