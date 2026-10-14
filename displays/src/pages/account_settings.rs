#![allow(deprecated)]
use crate::{
    app_state::{AppState, MainPages, SharedContext},
    pages::ai_profile,
    tabs::tasks::task_layout::{SortField, SortOptions},
    ui_tools::{
        controls::{self, ButtonKind, IconBox, IconLabel, Metrics, TextButton, TextField, Tokens},
        fonts,
        glass_card::{self, FixedWidthFrame},
        icons::svg::{self, Icon},
    },
};
use crossbeam::channel::Sender;
use database::{
    DatabaseSelection, PlatformSpawner, Spawner, SurrealValue, db,
    schema::{McpSettings, SortDirection, Status, Store, User},
};
use eframe::egui::{
    Align, CentralPanel, Direction, Frame, InnerResponse, Key, Label, Layout, Rangef, Rect,
    RichText, ScrollArea, Sense, Ui, UiBuilder, Vec2, pos2, vec2,
};
use log::{error, info};
use serde::Serialize;

/// Width of the custom-status field.
const STATUS_FIELD_W: f32 = 115.0;
/// Width of the sort dropdown.
const SORT_W: f32 = 120.0;
/// Height of the custom-status list.
const STATUS_LIST_H: f32 = 152.0;
/// Narrowest content width of a card at normal text scale.
const CARD_CONTENT_MIN_W: f32 = 270.0;
/// Widest content width of a card.
const CARD_CONTENT_MAX_W: f32 = 460.0;
/// Most card columns the page lays out.
const MAX_COLUMNS: usize = 3;
/// Space between the panel edge and the cards.
const PAGE_MARGIN: f32 = 50.0;
/// Inner margin of the page's central panel.
const PANEL_MARGIN: f32 = 1.0;

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

/// An icon label cut with '…' to the remaining width.
fn leading_label(ui: &mut Ui, icon: Icon, text: &str) {
    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        IconLabel::new(icon, text).truncate(true).show(ui);
    });
}

/// One grid cell of the settings page, in reading order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Slot {
    Account,
    TaskPreferences,
    Details,
    Connection,
    Assistant,
    Schedules,
}

impl Slot {
    const ALL: [Self; 6] = [
        Self::Account,
        Self::TaskPreferences,
        Self::Details,
        Self::Connection,
        Self::Assistant,
        Self::Schedules,
    ];

    /// Icon and title of the cell's last card.
    fn header(self) -> (Icon, &'static str) {
        match self {
            Self::Account => (svg::USER_GEAR, "Account settings"),
            Self::TaskPreferences => (svg::KANBAN, "Task page preferences"),
            Self::Details => (svg::IDENTIFICATION_CARD, "Other account details"),
            Self::Connection => (svg::PLUGS_CONNECTED, "MCP / AI endpoint"),
            Self::Assistant => (svg::ROBOT, "AI assistant"),
            Self::Schedules => (svg::CALENDAR_CHECK, "My schedules"),
        }
    }
}

/// Card width limits and column gap of the page grid.
#[derive(Clone, Copy, Debug, PartialEq)]
struct GridSpec {
    min_w: f32,
    max_w: f32,
    gap: f32,
}

impl GridSpec {
    /// Card limits from [`CARD_CONTENT_MIN_W`], grown for large text, to [`CARD_CONTENT_MAX_W`].
    fn new(ui: &Ui) -> Self {
        let frame = glass_card::card_frame(ui);
        let inset = frame.total_margin().sum().x;
        let text = fonts::text_scale(ui.ctx()).factor().max(1.0);
        Self {
            min_w: (CARD_CONTENT_MIN_W * text).ceil() + inset,
            max_w: CARD_CONTENT_MAX_W + inset,
            gap: frame.outer_margin.sum().y + Metrics::get(ui.ctx()).gap.y,
        }
    }

    /// Narrowest width that holds `columns` columns.
    fn span(self, columns: usize) -> f32 {
        columns as f32 * self.min_w + columns.saturating_sub(1) as f32 * self.gap
    }

    /// Most columns that fit in `width`, each a whole number of points and at most `max_w` wide.
    fn fit(self, width: f32) -> Grid {
        let columns = (1..=MAX_COLUMNS)
            .rev()
            .find(|&n| self.span(n) <= width)
            .unwrap_or(1);
        let column_w = ((width - (columns - 1) as f32 * self.gap) / columns as f32)
            .min(self.max_w)
            .max(0.0)
            .floor();
        Grid {
            columns,
            column_w,
            gap: self.gap,
        }
    }
}

/// Column count and width of the page grid.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Grid {
    columns: usize,
    column_w: f32,
    gap: f32,
}

impl Grid {
    /// Width of the columns and the gaps between them.
    fn width(self) -> f32 {
        self.columns as f32 * self.column_w + (self.columns - 1) as f32 * self.gap
    }
}

/// Starts a card as wide as `ui` that opens with `icon` and `title`.
fn begin_card(ui: &mut Ui, icon: Icon, title: &str) -> FixedWidthFrame {
    let frame = glass_card::card_frame(ui);
    let width = ui.available_width();
    let mut card = FixedWidthFrame::begin(ui, frame, width);
    glass_card::card_header(&mut card.content_ui, icon, title, None);
    card
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
        self.account_mod
            .settings_page(ui, self.current_user.as_ref(), &appstate_tx);
    }
}

impl UserPreferences {
    /// The account settings page for `current_user`.
    pub(crate) fn settings_page(
        &mut self,
        ui: &mut Ui,
        current_user: Option<&User>,
        appstate_tx: &Sender<AppState>,
    ) {
        CentralPanel::default()
            .frame(Frame::central_panel(&ui.ctx().global_style()).inner_margin(PANEL_MARGIN))
            .show(ui, |ui| {
                let page = ui.max_rect().x_range().shrink(PAGE_MARGIN);
                ScrollArea::vertical()
                    .id_salt("account_settings_page")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.settings_grid(ui, page, current_user, appstate_tx)
                    });
            });
    }

    /// The page's cards centred in `page`, in rows of up to [`MAX_COLUMNS`] cells of one height.
    fn settings_grid(
        &mut self,
        ui: &mut Ui,
        page: Rangef,
        current_user: Option<&User>,
        appstate_tx: &Sender<AppState>,
    ) {
        let grid = GridSpec::new(ui).fit(page.span());
        let left = (page.center() - grid.width() / 2.0).round();

        ui.add_space(PAGE_MARGIN);
        for row in Slot::ALL.chunks(grid.columns) {
            let top = ui.cursor().top();
            let cells: Vec<(Ui, FixedWidthFrame)> = row
                .iter()
                .enumerate()
                .map(|(i, &slot)| {
                    let x = left + i as f32 * (grid.column_w + grid.gap);
                    let rect =
                        Rect::from_min_max(pos2(x, top), pos2(x + grid.column_w, f32::INFINITY));
                    let mut cell = ui.new_child(
                        UiBuilder::new()
                            .id(ui.id().with(slot))
                            .max_rect(rect)
                            .layout(Layout::top_down(Align::Min)),
                    );
                    let card = self.slot_ui(&mut cell, slot, current_user, appstate_tx);
                    (cell, card)
                })
                .collect();
            let bottom = cells
                .iter()
                .map(|(_, card)| card.outer_rect().bottom())
                .fold(top, f32::max);
            for (mut cell, mut card) in cells {
                // Stretches the card's content to the row's bottom edge.
                let short = bottom - card.outer_rect().bottom();
                if short > 0.0 {
                    let y = card.content_ui.min_rect().bottom() + short;
                    card.content_ui.expand_to_include_y(y);
                }
                card.end(&mut cell);
            }
            ui.advance_cursor_after_rect(Rect::from_min_max(
                pos2(left, top),
                pos2(left + grid.width(), bottom),
            ));
        }
        ui.add_space(PAGE_MARGIN);
    }

    /// Draws `slot`'s cards into `ui` and returns its last card unfinished.
    fn slot_ui(
        &mut self,
        ui: &mut Ui,
        slot: Slot,
        current_user: Option<&User>,
        appstate_tx: &Sender<AppState>,
    ) -> FixedWidthFrame {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if slot == Slot::Connection {
                let mut startup = begin_card(ui, svg::ROCKET_LAUNCH, "Startup");
                self.startup_card(&mut startup.content_ui);
                startup.end(ui);
            }
        }
        let (icon, title) = slot.header();
        let mut card = begin_card(ui, icon, title);
        let content = &mut card.content_ui;
        match slot {
            Slot::Account => self.account_card(content, current_user, appstate_tx),
            Slot::TaskPreferences => self.task_preferences_card(content),
            Slot::Details => self.details_card(content),
            Slot::Connection => self.mcp_card(content),
            Slot::Assistant => ai_profile::persona_card(content, &self.user),
            Slot::Schedules => ai_profile::schedules_card(content, &self.user),
        }
        card
    }

    /// Name, email, store, database and the password change.
    fn account_card(
        &mut self,
        ui: &mut Ui,
        current_user: Option<&User>,
        appstate_tx: &Sender<AppState>,
    ) {
        let (t, m) = (Tokens::get(ui.ctx()), Metrics::get(ui.ctx()));
        let Some(usr) = current_user else {
            return;
        };
        let width = ui.available_width();
        TextField::new(&mut usr.get_name().to_owned())
            .hint("Name")
            .icon(svg::USER)
            .width(width)
            .show(ui);

        ui.add_space(m.gap.y);

        TextField::new(&mut usr.get_email().to_owned())
            .hint("Email")
            .icon(svg::ENVELOPE_SIMPLE)
            .width(width)
            .show(ui);

        ui.add_space(m.gap.y);

        let half = (width - m.gap.x) / 2.0;
        ui.allocate_ui_with_layout(
            vec2(width, m.control_h),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = m.gap.x;
                let stores: Vec<(Store, &str)> =
                    Store::VALUES.iter().map(|s| (*s, s.as_str())).collect();
                controls::dropdown(ui, "StoreComboBox", &mut self.store, &stores, half)
                    .on_hover_text("Store");
                let databases = [
                    (
                        DatabaseSelection::Stable,
                        DatabaseSelection::Stable.as_str(),
                    ),
                    (DatabaseSelection::Beta, DatabaseSelection::Beta.as_str()),
                    (DatabaseSelection::Local, DatabaseSelection::Local.as_str()),
                ];
                let db =
                    controls::dropdown(ui, "Database Editor", &mut self.database, &databases, half)
                        .on_hover_text("Database");
                if db.clicked() {
                    let acc = self.clone();
                    PlatformSpawner::spawn(async move {
                        let set_db = acc.database.set_database().await;
                        log::info!("Set database: {set_db:?}");
                    });
                }
            },
        );

        ui.add_space(m.gap.y);

        let password = TextField::new(&mut self.password)
            .hint("Password")
            .icon(svg::LOCK)
            .password(true)
            .width(width)
            .show(ui);

        ui.add_space(m.gap.y);
        let password_check = self.password.eq(&self.retyped_password.clone());

        let retyped = TextField::new(&mut self.retyped_password)
            .hint("Retype password")
            .icon(svg::LOCK_KEY)
            .password(true)
            .width(width)
            .show(ui);

        if !password_check {
            IconLabel::new(svg::WARNING_CIRCLE, "Passwords must match")
                .small(true)
                .tone(t.error)
                .show(ui);
        }

        ui.add_space(m.gap.y);

        let can_change = self.password.len() > 3 && password_check;
        let enabled_button = TextButton::new("Change password")
            .icon(svg::KEY)
            .kind(ButtonKind::Primary)
            .prominent(true)
            .enabled(can_change)
            .show(ui);
        let accepted_by_keyboard = (password.lost_focus() || retyped.lost_focus())
            && ui.input(|input| input.key_pressed(Key::Enter));

        if can_change && (enabled_button.clicked() || accepted_by_keyboard) {
            self.change_password();
            let tx = appstate_tx.clone();
            match tx.try_send(AppState::Authenticated(MainPages::Tasks)) {
                Ok(_) => info!("Sent appstate"),
                Err(e) => error!("Error {e:?}"),
            }
        }
    }

    /// Custom statuses and the default sort.
    fn task_preferences_card(&mut self, ui: &mut Ui) {
        let (t, m) = (Tokens::get(ui.ctx()), Metrics::get(ui.ctx()));
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let submit_status =
                    controls::icon_button(ui, svg::PLUS, IconBox::Sm, false, "Add status");
                TextField::new(&mut self.new_status)
                    .hint("New status")
                    .width(STATUS_FIELD_W)
                    .show(ui);

                if submit_status.clicked() && !self.new_status.is_empty() {
                    let status = self.new_status.clone();
                    self.new_status.clear();
                    self.user.set_statuses(Status::CustomStatus(status.clone()));
                    log::info!("Got a new status: {status}");
                    PlatformSpawner::spawn(async move {
                        match User::add_custom_status(status.clone().as_str()).await {
                            Ok(_) => log::info!("Created new status: {}", status),
                            Err(e) => log::error!("Error creating new status: {e:?}"),
                        }
                    });
                }
                leading_label(ui, svg::TAG, "Custom status");
            });
        });

        ui.add_space(m.gap.y);

        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let mut field = self.sort_by.field.clone();
                let options = [
                    (SortField::Default, "Priority"),
                    (SortField::Name, "Name"),
                    (SortField::Date, "Date"),
                    (SortField::Completed, "Completed"),
                ];
                let picked =
                    controls::dropdown(ui, "SortBy Settings", &mut field, &options, SORT_W);
                let (icon, tone, tip) = match self.sort_by.direction {
                    SortDirection::Asc => (svg::SORT_ASCENDING, t.warn, "Ascending"),
                    SortDirection::Desc => (svg::SORT_DESCENDING, t.error, "Descending"),
                };
                let (slot, arrow) = ui.allocate_exact_size(Vec2::splat(m.icon_md), Sense::hover());
                icon.paint(ui, slot, m.icon_md, tone);
                arrow.on_hover_text(tip);
                if picked.changed() {
                    self.last_sort_field = Some(field);
                }
                leading_label(ui, svg::SORT_DESCENDING, "Default priority");
            });
        });

        ScrollArea::vertical()
            .id_salt("custom_statuses")
            .auto_shrink([false, false])
            .max_height(STATUS_LIST_H)
            .show(ui, |ui| {
                let clicked: &mut Option<Status> = &mut None;
                for (idx, status) in self.user.get_custom_statuses().iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{idx}:"))
                                .monospace()
                                .color(t.text_faint),
                        );
                        controls::icon_label(ui, svg::TAG_SIMPLE, status.as_str());
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
                        });
                    });
                }
                if let Some(status) = clicked
                    && let Some(statuses) = self.user.get_custom_statuses_mut()
                    && let Some(idx) = statuses.iter().position(|x| *x == *status)
                {
                    statuses.remove(idx);
                }
            });
    }

    /// Storage keys, Prestashop and store ids, and the authorization level.
    fn details_card(&self, ui: &mut Ui) {
        let user = &self.user;
        let access = user.get_minio_access_key().unwrap_or_default();
        let secret = user.get_minio_secret_key().unwrap_or_default();
        let employee = user.get_employee_id().unwrap_or_default().to_string();
        let store = user.get_store_id().unwrap_or_default();
        detail_row(ui, svg::KEY, "Minio access key", &access, true);
        detail_row(ui, svg::LOCK_KEY, "Minio secret key", &secret, true);
        detail_row(ui, svg::STOREFRONT, "Prestashop ID", &employee, false);
        detail_row(ui, svg::HASH, "Store #", &store, false);
        detail_row(
            ui,
            svg::SHIELD_CHECK,
            "Authorization",
            user.get_authorization().as_str(),
            false,
        );
    }

    /// The screen MasterTech opens into.
    #[cfg(not(target_arch = "wasm32"))]
    fn startup_card(&mut self, ui: &mut Ui) {
        use crate::tabs::WorkMode;

        let (t, m) = (Tokens::get(ui.ctx()), Metrics::get(ui.ctx()));
        let current = self
            .user
            .get_user_settings()
            .get_default_work_mode()
            .as_deref()
            .and_then(WorkMode::from_slug);
        let mut picked = current;
        let options: Vec<(Option<WorkMode>, &str)> = std::iter::once((None, "Ask me every time"))
            .chain(WorkMode::ALL.iter().map(|mode| (Some(*mode), mode.title())))
            .collect();
        let width = ui.available_width();
        controls::dropdown(ui, "default_work_mode", &mut picked, &options, width);
        if picked != current {
            self.save_default_work_mode(picked);
        }
        ui.add_space(m.gap.y / 2.0);
        IconLabel::new(svg::INFO, "Which screen MasterTech opens into.")
            .small(true)
            .tone(t.text_dim)
            .show(ui);
    }

    /// The OpenAI-compatible endpoint, key and model.
    fn mcp_card(&mut self, ui: &mut Ui) {
        let m = Metrics::get(ui.ctx());
        let width = ui.available_width();
        TextField::new(&mut self.mcp_endpoint)
            .hint("OpenAI-compatible endpoint")
            .icon(svg::GLOBE)
            .width(width)
            .show(ui);

        ui.add_space(m.gap.y);

        TextField::new(&mut self.mcp_api_key)
            .hint("API key")
            .icon(svg::KEY)
            .password(true)
            .width(width)
            .show(ui);

        ui.add_space(m.gap.y);

        TextField::new(&mut self.mcp_model)
            .hint("Model")
            .icon(svg::BRAIN)
            .width(width)
            .show(ui);

        ui.add_space(m.gap.y);

        let save = TextButton::new("Save endpoint")
            .icon(svg::FLOPPY_DISK)
            .kind(ButtonKind::Primary)
            .prominent(true)
            .show(ui);
        if save.clicked() {
            self.save_mcp_settings();
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_tools::{
        fonts::TextScale,
        headless_render::Headless,
        metrics::Density,
        theme_config::{PresetStyles, set_text_scale},
    };
    use chrono::{TimeZone, Utc};
    use crossbeam::channel::Receiver;
    use database::schema::{Datetime, RecordId, task_schedule::TaskSchedule};
    use eframe::egui::{
        Color32, Event, Modifiers, PointerButton, Pos2, Shape, Stroke, epaint::ClippedPrimitive,
    };
    use std::path::Path;

    /// Frames run per render; the last is kept.
    const FRAMES: usize = 6;
    /// Screen height that shows the whole page in one column.
    const TALL_H: f32 = 2400.0;

    fn user() -> User {
        let mut value = serde_json::to_value(User::default()).expect("serialize user");
        value["name"] = "Dana Whitfield".into();
        value["email"] = "dana.whitfield@example.com".into();
        value["minio_access_key"] = "MTECHDANA01".into();
        value["minio_secret_key"] = "q7XbR2mVd9LpT4sW".into();
        value["id_prestashop"] = 42.into();
        value["id_store"] = "3".into();
        value["authorization"] = "Manager".into();
        let mut user: User = serde_json::from_value(value).expect("deserialize user");
        user.set_statuses(Status::CustomStatus("Waiting on parts".to_owned()));
        user.set_statuses(Status::CustomStatus("Customer callback".to_owned()));
        user.set_mcp_settings(McpSettings {
            endpoint: Some("http://zc-gateway.local:4000/v1".to_owned()),
            api_key: Some("sk-local-test".to_owned()),
            model: Some("zc-quick".to_owned()),
        });
        user
    }

    fn schedule(user: &RecordId, key: &str, title: &str, every: &str, day: u32) -> TaskSchedule {
        let next = Utc
            .with_ymd_and_hms(2026, 10, day, 16, 0, 0)
            .single()
            .expect("valid date");
        TaskSchedule {
            id: RecordId::new("task_schedule", key),
            title: title.to_owned(),
            description: None,
            assignee: user.clone(),
            created_by: Some(user.clone()),
            every: every.to_owned(),
            interval: 1,
            weekdays: if every == "week" { vec![1] } else { Vec::new() },
            month_day: (every == "month").then_some(1),
            at: "10:00".to_owned(),
            next_run: Some(Datetime::from(next)),
            last_run: None,
            runs: 0,
            active: true,
            due_hours: None,
            priority: None,
            service_number: None,
            origin: Some("ai".to_owned()),
            created_at: None,
        }
    }

    fn schedules(user: &RecordId) -> Vec<TaskSchedule> {
        vec![
            schedule(user, "paste", "Count thermal paste", "week", 19),
            schedule(user, "backups", "Check the bench backup drive", "day", 15),
            schedule(user, "inventory", "Monthly parts inventory", "month", 1),
        ]
    }

    #[test]
    fn grid_drops_columns_below_their_minimum_width() {
        let spec = GridSpec {
            min_w: 300.0,
            max_w: 480.0,
            gap: 10.0,
        };
        let fit = |width| spec.fit(width);
        let grid = |columns, column_w| Grid {
            columns,
            column_w,
            gap: 10.0,
        };
        assert_eq!(fit(2000.0), grid(3, 480.0));
        assert_eq!(fit(920.0), grid(3, 300.0));
        assert_eq!(fit(919.0).columns, 2);
        assert_eq!(fit(610.0), grid(2, 300.0));
        assert_eq!(fit(609.0).columns, 1);
        assert_eq!(fit(200.0), grid(1, 200.0));
        assert_eq!(fit(2000.0).width(), 1460.0);
        assert_eq!((spec.span(1), spec.span(3)), (300.0, 920.0));
    }

    /// Serialises the tests that seed the shared AI editor.
    static EDITOR_SEED: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A settings page for a seeded user on a headless screen.
    struct Page {
        h: Headless,
        prefs: UserPreferences,
        user: User,
        tx: Sender<AppState>,
        rx: Receiver<AppState>,
    }

    impl Page {
        fn new(size: Vec2, density: Density, text: TextScale, with_schedules: bool) -> Self {
            let user = user();
            let id = user.get_id();
            let schedules = if with_schedules {
                schedules(&id)
            } else {
                Vec::new()
            };
            ai_profile::seed_for_tests(id, schedules);
            let mut prefs = UserPreferences::default();
            prefs.set_user(user.clone());
            let h = Headless::new(size, 1.0);
            h.theme(PresetStyles::MtechStudio, density);
            set_text_scale(&h.ctx, text);
            let (tx, rx) = crossbeam::channel::unbounded();
            Self {
                h,
                prefs,
                user,
                tx,
                rx,
            }
        }

        /// The page's grid limits under this page's theme.
        fn grid_spec(&mut self) -> GridSpec {
            self.h.frame_root(Vec::new(), |ui| GridSpec::new(ui)).0
        }

        /// Primitives of one frame that receives `events`.
        fn frame(&mut self, events: Vec<Event>) -> Vec<ClippedPrimitive> {
            let Self {
                h, prefs, user, tx, ..
            } = self;
            h.frame_root(events, |ui| prefs.settings_page(ui, Some(user), tx))
                .1
        }

        /// Primitives of the last of [`FRAMES`] frames.
        fn render(&mut self) -> Vec<ClippedPrimitive> {
            let mut prims = Vec::new();
            for _ in 0..FRAMES {
                prims = self.frame(Vec::new());
            }
            prims
        }

        /// Presses and releases the primary button at `pos`, then runs one more frame.
        fn click(&mut self, pos: Pos2) {
            let press = |pressed| Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(pos), press(true)]);
            self.frame(vec![press(false)]);
            self.frame(Vec::new());
        }

        /// Presses and releases `key`, then runs one more frame.
        fn key(&mut self, key: Key) {
            let event = |pressed| Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![event(true)]);
            self.frame(vec![event(false)]);
            self.frame(Vec::new());
        }

        /// Centre of the first text shape whose text starts with `prefix`.
        fn text_at(&mut self, prefix: &str) -> Pos2 {
            let label = format!("text {prefix:?}");
            let label = label.trim_end_matches('"');
            self.capture()
                .others
                .iter()
                .find(|(_, what)| what.starts_with(label))
                .map(|(rect, _)| rect.center())
                .unwrap_or_else(|| panic!("no text starting with {prefix:?}"))
        }

        /// Card rects and the boxes of other shapes in the last of [`FRAMES`] frames.
        fn capture(&mut self) -> Capture {
            let Self {
                h, prefs, user, tx, ..
            } = self;
            let mut capture = Capture::default();
            for _ in 0..FRAMES {
                let ((fill, stroke, inset), shapes) = h.frame_shapes(Vec::new(), |ui| {
                    let frame = glass_card::card_frame(ui);
                    prefs.settings_page(ui, Some(user), tx);
                    (
                        glass_card::card_fill(ui),
                        glass_card::card_stroke(ui),
                        frame.inner_margin.sum().x / 2.0 + frame.stroke.width,
                    )
                });
                capture = Capture {
                    inset,
                    ..Default::default()
                };
                for clipped in &shapes {
                    capture.collect(&clipped.shape, clipped.clip_rect, fill, stroke);
                }
            }
            capture
        }
    }

    /// Card frames and the other shapes of one frame.
    #[derive(Default)]
    struct Capture {
        cards: Vec<Rect>,
        others: Vec<(Rect, String)>,
        /// Inner margin plus stroke width of a card on each side.
        inset: f32,
    }

    impl Capture {
        /// Records `shape` as a card, or as another shape cut to `clip`.
        fn collect(&mut self, shape: &Shape, clip: Rect, fill: Color32, stroke: Stroke) {
            let (rect, what) = match shape {
                Shape::Vec(shapes) => {
                    for s in shapes {
                        self.collect(s, clip, fill, stroke);
                    }
                    return;
                }
                Shape::Rect(r) if r.fill == fill && r.stroke == stroke => {
                    self.cards.push(r.rect);
                    return;
                }
                Shape::Text(text) => (
                    text.galley.rect.translate(text.pos.to_vec2()),
                    format!("text {:?}", text.galley.text()),
                ),
                Shape::Rect(r) => (r.rect, format!("rect filled {:?}", r.fill)),
                _ => return,
            };
            let visible = rect.intersect(clip);
            if visible.is_positive() {
                self.others.push((visible, what));
            }
        }

        /// A text in `card` that partly covers another text or box.
        fn collision(&self, card: Rect) -> Option<String> {
            let inside: Vec<&(Rect, String)> = self
                .others
                .iter()
                .filter(|(rect, _)| card.contains(rect.center()))
                .collect();
            let texts = inside.iter().filter(|(_, what)| what.starts_with("text"));
            for (text, what) in texts {
                let hit = inside.iter().find(|(other, _)| {
                    let shared = text.intersect(*other);
                    other != text
                        && shared.width() > EPS
                        && shared.height() > EPS
                        && !other.expand(EPS).contains_rect(*text)
                });
                if let Some((other, other_what)) = hit {
                    return Some(format!(
                        "{what} at {text:?} collides with {other_what} at {other:?}"
                    ));
                }
            }
            None
        }

        /// Cards grouped into rows of overlapping vertical extent, top to bottom.
        fn rows(&self) -> Vec<Vec<Rect>> {
            let mut cards = self.cards.clone();
            cards.sort_by(|a, b| a.top().total_cmp(&b.top()));
            let mut rows: Vec<Vec<Rect>> = Vec::new();
            for card in cards {
                let bottom = rows
                    .last()
                    .map(|row| row.iter().map(|r| r.bottom()).fold(f32::MIN, f32::max));
                match (rows.last_mut(), bottom) {
                    (Some(row), Some(bottom)) if card.top() < bottom - EPS => row.push(card),
                    _ => rows.push(vec![card]),
                }
            }
            rows
        }
    }

    /// Slack for sub-pixel rounding.
    const EPS: f32 = 0.5;

    /// Distinct left edges of `cards`.
    fn lefts(cards: &[Rect]) -> Vec<f32> {
        let mut lefts: Vec<f32> = Vec::new();
        for card in cards {
            if !lefts.iter().any(|l| (l - card.left()).abs() <= EPS) {
                lefts.push(card.left());
            }
        }
        lefts
    }

    /// Checks card widths, column count, flush rows, shapes leaving a card and colliding text.
    fn grid_fault(capture: &Capture, columns: usize) -> Result<(), String> {
        let cards = &capture.cards;
        if cards.len() != 7 {
            return Err(format!("{} cards", cards.len()));
        }
        let width = cards[0].width();
        if let Some(card) = cards.iter().find(|c| (c.width() - width).abs() > EPS) {
            return Err(format!("card {card:?} is not {width} wide"));
        }
        if let Some(card) = cards.iter().find(|c| c.left() < PAGE_MARGIN - EPS) {
            return Err(format!("card {card:?} is inside the page margin"));
        }
        let found = lefts(cards).len();
        if found != columns {
            return Err(format!("{found} columns"));
        }
        for row in capture.rows() {
            let top = row.iter().map(|r| r.top()).fold(f32::MAX, f32::min);
            let bottom = row.iter().map(|r| r.bottom()).fold(f32::MIN, f32::max);
            for left in lefts(&row) {
                let column: Vec<&Rect> = row
                    .iter()
                    .filter(|r| (r.left() - left).abs() <= EPS)
                    .collect();
                let column_top = column.iter().map(|r| r.top()).fold(f32::MAX, f32::min);
                let column_bottom = column.iter().map(|r| r.bottom()).fold(f32::MIN, f32::max);
                if (column_top - top).abs() > EPS || (column_bottom - bottom).abs() > EPS {
                    return Err(format!(
                        "column at {left} spans {column_top}..{column_bottom}, row {top}..{bottom}"
                    ));
                }
            }
        }
        for card in cards {
            let inner = card.shrink2(vec2(capture.inset, 0.0));
            let escapee = capture.others.iter().find(|(rect, _)| {
                let backdrop = rect.left() <= card.left() && rect.right() >= card.right();
                card.contains(rect.center())
                    && !backdrop
                    && (rect.left() < inner.left() - EPS || rect.right() > inner.right() + EPS)
            });
            if let Some((rect, what)) = escapee {
                return Err(format!(
                    "{what} at {rect:?} leaves the inner rect {inner:?}"
                ));
            }
            if let Some(fault) = capture.collision(*card) {
                return Err(fault);
            }
        }
        Ok(())
    }

    /// Window width whose page spans `span` between its margins.
    fn window_w(span: f32) -> f32 {
        span + 2.0 * (PAGE_MARGIN + PANEL_MARGIN)
    }

    #[test]
    fn account_settings_cards_form_flush_columns_at_every_width() {
        let _seed = EDITOR_SEED.lock().unwrap_or_else(|e| e.into_inner());
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        let _guard = rt.enter();
        // Density, text scale and the column count of a 1024-wide window.
        let themes = [
            (Density::Compact, TextScale::Small, 3),
            (Density::Regular, TextScale::Normal, 3),
            (Density::Comfortable, TextScale::Large, 2),
            (Density::Regular, TextScale::XLarge, 2),
        ];
        let mut faults = Vec::new();
        for (density, text, columns_at_1024) in themes {
            let spec = Page::new(vec2(1000.0, 1000.0), density, text, false).grid_spec();
            let mut cases = vec![
                (vec2(1897.0, 892.0), 3),
                (vec2(1366.0, 768.0), 3),
                (vec2(1024.0, TALL_H), columns_at_1024),
            ];
            for columns in 1..=MAX_COLUMNS {
                let narrowest = window_w(spec.span(columns)).ceil();
                cases.push((vec2(narrowest, TALL_H), columns));
                if columns > 1 {
                    cases.push((vec2(narrowest - 1.0, TALL_H), columns - 1));
                }
            }
            for (size, columns) in cases {
                for with_schedules in [true, false] {
                    let capture = Page::new(size, density, text, with_schedules).capture();
                    if let Err(fault) = grid_fault(&capture, columns) {
                        faults.push(format!(
                            "{size:?} {density:?} {text:?} schedules {with_schedules}: {fault}"
                        ));
                    }
                }
            }
        }
        assert!(faults.is_empty(), "{faults:#?}");
    }

    #[test]
    fn account_settings_field_keeps_focus_when_columns_change() {
        let _seed = EDITOR_SEED.lock().unwrap_or_else(|e| e.into_inner());
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        let _guard = rt.enter();
        let (compact, small) = (Density::Compact, TextScale::Small);
        let mut page = Page::new(vec2(1366.0, 900.0), compact, small, true);
        let name = page.text_at("Jarvis");
        page.click(name);
        let focused = page.h.ctx.memory(|m| m.focused());
        assert!(focused.is_some(), "clicking the assistant name focuses it");

        page.h.resize(vec2(900.0, 1100.0));
        page.render();
        assert_eq!(page.h.ctx.memory(|m| m.focused()), focused);
    }

    #[test]
    fn account_settings_enter_changes_password_only_from_password_fields() {
        let _seed = EDITOR_SEED.lock().unwrap_or_else(|e| e.into_inner());
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        let _guard = rt.enter();
        let (compact, small) = (Density::Compact, TextScale::Small);
        let mut page = Page::new(vec2(1366.0, 900.0), compact, small, true);
        page.prefs.password = "hunter22".to_owned();
        page.prefs.retyped_password = "hunter22".to_owned();

        let about = page.text_at("Bench tech");
        page.click(about);
        page.key(Key::Enter);
        assert!(page.rx.try_recv().is_err(), "Enter in About me");

        let password = page.text_at("•");
        page.click(password);
        page.key(Key::Enter);
        assert!(matches!(
            page.rx.try_recv(),
            Ok(AppState::Authenticated(MainPages::Tasks))
        ));
    }

    /// Renders the settings page for one case and writes `account_settings_{name}.png` to `dir`.
    fn render_case(
        dir: &Path,
        name: &str,
        size: Vec2,
        density: Density,
        text: TextScale,
        with_schedules: bool,
    ) {
        let mut page = Page::new(size, density, text, with_schedules);
        let prims = page.render();
        page.h
            .image(&prims)
            .save(dir.join(format!("account_settings_{name}.png")))
            .expect("write png");
    }

    /// Writes the settings page renders; run with `-- --ignored render_account_settings`.
    #[test]
    #[ignore = "writes PNG previews; set MTECH_RENDER_DIR"]
    fn render_account_settings() {
        let _seed = EDITOR_SEED.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::var_os("MTECH_RENDER_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("mtech-renders"));
        std::fs::create_dir_all(&dir).expect("render directory");
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        let _guard = rt.enter();
        let (compact, small) = (Density::Compact, TextScale::Small);
        let cases = [
            ("1897x892", vec2(1897.0, 892.0), compact, small, true),
            ("1366x768", vec2(1366.0, 768.0), compact, small, true),
            ("1024x700", vec2(1024.0, 700.0), compact, small, true),
            ("1897x892_empty", vec2(1897.0, 892.0), compact, small, false),
            (
                "1897x892_regular",
                vec2(1897.0, 892.0),
                Density::Regular,
                TextScale::Normal,
                true,
            ),
            (
                "1366x768_regular",
                vec2(1366.0, 768.0),
                Density::Regular,
                TextScale::Normal,
                true,
            ),
            (
                "1002x900_regular",
                vec2(1002.0, 900.0),
                Density::Regular,
                TextScale::Normal,
                true,
            ),
            ("998x900", vec2(998.0, 900.0), compact, small, true),
            ("900x700", vec2(900.0, 700.0), compact, small, true),
            (
                "1366x900_xlarge",
                vec2(1366.0, 900.0),
                Density::Regular,
                TextScale::XLarge,
                true,
            ),
            ("900x1100", vec2(900.0, 1100.0), compact, small, true),
            ("620x1900", vec2(620.0, 1900.0), compact, small, true),
        ];
        for (name, size, density, text, with_schedules) in cases {
            render_case(&dir, name, size, density, text, with_schedules);
        }
    }
}
