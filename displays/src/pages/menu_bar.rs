#![allow(deprecated)]
//! The web console's top bar: brand, menus, task search, notifications, account and store.

use crate::{
    PlatformSpawner, Spawner, TaskUiActions,
    app_state::{AppState, MainPages, SharedContext, default_tree, default_tree_wasm},
    pages::view_menu,
    tabs::{TabContext, WorkMode},
    ui_tools::{
        controls::{
            self, BarKind, ButtonKind, IconBox, IconButton, MenuButton, Metrics, TextButton, Tokens,
        },
        icons::{
            self,
            svg::{self, Icon},
        },
    },
};
use database::{
    db,
    schema::{
        Notification, Store,
        utilities::{get_completed_tasks_for_store, get_store_users, get_tasks_for_store},
    },
};
use eframe::egui::{Align, Frame, Layout, Margin, Ui, UiKind};

/// Brand button label.
const TITLE: &str = concat!("MasterTech Server ", database::version_with_build!());
/// Width of the store dropdown.
const STORE_W: f32 = 96.0;
/// Width of the account menu.
const ACCOUNT_W: f32 = 270.0;
/// Width of the admin notification field.
const ADMIN_NOTE_W: f32 = 196.0;
/// Width of the notifications popup.
const NOTIFICATIONS_W: f32 = 380.0;
/// Tallest the notifications popup grows.
const NOTIFICATIONS_MAX_H: f32 = 560.0;

/// What a Layout menu row asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LayoutAction {
    Save,
    Organize,
    Reset,
    ResetMemory,
}

/// What an account menu row asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AccountAction {
    Preferences,
    Downloads,
    Theme,
    Refresh,
    ResetPassword,
    Logout,
}

/// Writes the signed-in user to the session cookie.
#[cfg(target_arch = "wasm32")]
fn store_user_cookie(user: &database::schema::User) {
    use base64::{Engine as _, engine::general_purpose};
    use brotli::CompressorReader;

    fn compress_string(input: &str) -> Vec<u8> {
        let mut compressed = Vec::new();
        {
            let mut compressor = CompressorReader::new(input.as_bytes(), 4096, 11, 22);
            std::io::copy(&mut compressor, &mut compressed).unwrap();
        }
        compressed
    }

    let user_string = serde_json::to_string(user).unwrap();
    let compressed: Vec<u8> = compress_string(&user_string);
    let encoded: String = general_purpose::STANDARD.encode(&compressed);
    log::info!(
        "Compressed data: {}\nEncoded: {}\nOriginal: {}",
        compressed.len(),
        encoded.len(),
        user_string.len()
    );
    wasm_cookies::delete("user");
    let duration = web_time::Duration::from_secs(172800);
    let cookie_opts = wasm_cookies::CookieOptions::default()
        .with_same_site(wasm_cookies::SameSite::Strict)
        .secure()
        .expires_after(duration);
    wasm_cookies::set("user", &encoded, &cookie_opts);
}

impl SharedContext {
    pub fn menu_bar(&mut self, ui: &mut Ui) {
        let m = Metrics::get(ui.ctx());
        let frame = Frame::side_top_panel(&ui.global_style()).inner_margin(Margin::ZERO);
        eframe::egui::Panel::top("egui_dock::MenuBar")
            .frame(frame)
            .exact_size(m.bar_h)
            .show_separator_line(false)
            .show(ui, |ui| {
                controls::bar(ui, BarKind::Tool, |ui| {
                    if self.current_user.is_some() {
                        self.bar_left(ui);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            self.bar_right(ui)
                        });
                    } else {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            self.signed_out(ui)
                        });
                    }
                });
            });
    }

    fn bar_left(&mut self, ui: &mut Ui) {
        let Some(user) = self.current_user.as_ref() else {
            return;
        };
        let tab_ctx = TabContext::for_user(user.is_warehouse());
        if TextButton::new(TITLE)
            .icon(svg::MT_LOGO)
            .kind(ButtonKind::Ghost)
            .show(ui)
            .on_hover_text("Back to the workspace")
            .clicked()
        {
            self.state = AppState::Authenticated(MainPages::Tasks);
            match self
                .app_state_tx
                .try_send(AppState::Authenticated(MainPages::Tasks))
            {
                Ok(_) => log::info!("AppState::Authenticated(MainPages::Tasks)"),
                Err(e) => log::error!("Error: {e:?}"),
            }
        }
        let dock = &mut self.dock;
        let _ = MenuButton::new("View")
            .icon(svg::TABS)
            .tip("Open or close tabs")
            .show(ui, |ui| view_menu(ui, dock, tab_ctx, WorkMode::Full, None));
        self.layout_menu(ui);

        let m = Metrics::get(ui.ctx());
        controls::vline(ui, m.control_h - 8.0);
        if TextButton::new("New task")
            .icon(icons::CREATE_TASK)
            .kind(ButtonKind::Primary)
            .show(ui)
            .on_hover_text("Create a new task")
            .clicked()
        {
            let _ = self
                .ui_actions_tx
                .try_send(TaskUiActions::CreateTaskModal(None));
        }

        crate::ui_data::task_search::search_bar(
            ui,
            crate::ui_data::task_search::TaskSearchCtx {
                state: &mut self.task_search,
                input: &mut self.search_input,
                results: &mut self.search_results,
                index: &mut self.task_index,
                tasks: &mut self.tasks,
                store_users: &self.store_users,
                store_selection: self.store_selection,
                actions: &self.ui_actions_tx,
            },
        );
    }

    fn layout_menu(&mut self, ui: &mut Ui) {
        let mut picked = None;
        let _ = MenuButton::new("Layout")
            .icon(svg::LAYOUT)
            .tip("UI layout")
            .show(ui, |ui| {
                let mut row = |ui: &mut Ui, icon: Icon, text: &str, action: LayoutAction| {
                    if controls::menu_item(ui, icon, text).clicked() {
                        picked = Some(action);
                    }
                };
                row(ui, svg::FLOPPY_DISK, "Save layout", LayoutAction::Save);
                row(
                    ui,
                    svg::FRAME_CORNERS,
                    "Organize windows",
                    LayoutAction::Organize,
                );
                controls::menu_separator(ui);
                row(
                    ui,
                    svg::ARROW_COUNTER_CLOCKWISE,
                    "Reset layout",
                    LayoutAction::Reset,
                );
                row(ui, svg::BROOM, "Reset memory", LayoutAction::ResetMemory);
            });
        if let Some(action) = picked {
            self.apply_layout_action(ui, action);
        }
    }

    fn apply_layout_action(&mut self, ui: &Ui, action: LayoutAction) {
        match action {
            LayoutAction::Save | LayoutAction::Reset => {
                let Some(user) = self.current_user.as_mut() else {
                    return;
                };
                if action == LayoutAction::Reset {
                    self.dock = if cfg!(target_arch = "wasm32") {
                        default_tree_wasm()
                    } else {
                        default_tree()
                    };
                }
                let layout = self.dock.layout_value();
                self.user_settings.set_ui_layout_mtechserver(layout.clone());
                user.set_ui_layout_mtechserver(layout.clone());
                log::debug!("user_settings: {:#?}", user.get_user_settings());
                #[cfg(target_arch = "wasm32")]
                store_user_cookie(user);
                let mut user = user.clone();
                PlatformSpawner::spawn(async move {
                    match user.save_mtechserver_ui_layout(layout).await {
                        Ok(_) => log::info!("Updated User Settings"),
                        Err(e) => log::error!("Error updating User Settings: {e:?}"),
                    }
                });
                self.update_settings = true;
            }
            LayoutAction::Organize => {
                ui.ctx().memory_mut(|mem| mem.reset_areas());
                ui.ctx().memory_mut(|mem| {
                    for layer in mem.areas_mut().visible_layer_ids().iter() {
                        log::info!("Visible layers: {layer:?}");
                    }
                });
            }
            LayoutAction::ResetMemory => crate::ui_tools::theme_config::reset_memory(ui.ctx()),
        }
    }

    fn bar_right(&mut self, ui: &mut Ui) {
        crate::ui_tools::do_not_disturb::toggle_button(ui);
        if crate::ui_tools::command_bar::open_button(ui) {
            self.command_bar.request_open();
        }
        self.notifications_button(ui);
        let m = Metrics::get(ui.ctx());
        controls::vline(ui, m.control_h - 8.0);
        self.account_menu(ui);
        self.store_picker(ui);
    }

    fn notifications_button(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        let (unread, alerts) = (
            self.notification_center.unread_count(),
            self.notification_center.alert_count(),
        );
        let tip: std::borrow::Cow<str> = match (alerts, unread) {
            (0, 0) => "Notifications".into(),
            (0, n) => format!("{n} unread notifications").into(),
            (a, n) => format!("{a} unread alerts, {n} unread notifications").into(),
        };
        let icon = if alerts > 0 {
            svg::BELL_RINGING
        } else {
            svg::BELL
        };
        let mut button = IconButton::new(icon, IconBox::Md).tip(&tip);
        if alerts > 0 {
            button = button.tint(t.error);
        }
        let response = button.show(ui);
        let (count, tone) = if alerts > 0 {
            (alerts, t.error)
        } else {
            (unread, t.primary)
        };
        controls::count_badge(ui, response.rect, count, tone);
        if response.clicked() {
            let category = (alerts > 0).then(|| "ALERT".to_string());
            self.notification_center.open_unread(category);
        }
        controls::popup_menu(&response, true, |ui| {
            ui.reset_style();
            ui.set_width(NOTIFICATIONS_W);
            ui.set_max_height(NOTIFICATIONS_MAX_H);
            let task_names: std::collections::BTreeSet<String> = self
                .task_index
                .values()
                .map(|t| t.task_name.clone())
                .collect();
            self.notification_center
                .ui(ui, &task_names, self.ui_actions_tx.clone(), &self.tasks);
        });
    }

    fn account_menu(&mut self, ui: &mut Ui) {
        let Some(user) = self.current_user.as_ref() else {
            return;
        };
        let name = user.get_name().to_owned();
        let is_admin = user.is_admin();
        let resets = crate::tabs::user_admin::ResetCodeDialog::available_to(Some(user));
        let mut picked = None;
        let note = &mut self.admin_notification_text;
        let _ = MenuButton::new(&name)
            .icon(svg::USER_CIRCLE)
            .tip("Account")
            .keep_open(true)
            .show(ui, |ui| {
                ui.set_width(ACCOUNT_W);
                controls::menu_inset_row(ui, |ui| {
                    controls::icon_label(ui, svg::IDENTIFICATION_BADGE, &name)
                });
                controls::menu_separator(ui);
                let mut item = |ui: &mut Ui, icon: Icon, text: &str, action: AccountAction| {
                    if controls::menu_item(ui, icon, text).clicked() {
                        picked = Some(action);
                        ui.close_kind(UiKind::Menu);
                    }
                };
                item(ui, svg::GEAR, "Preferences", AccountAction::Preferences);
                item(
                    ui,
                    svg::DOWNLOAD_SIMPLE,
                    "Downloads",
                    AccountAction::Downloads,
                );
                item(ui, svg::PALETTE, "Theme", AccountAction::Theme);
                item(
                    ui,
                    svg::ARROW_CLOCKWISE,
                    "Refresh data",
                    AccountAction::Refresh,
                );
                if resets {
                    item(
                        ui,
                        svg::KEY,
                        "Reset a password…",
                        AccountAction::ResetPassword,
                    );
                }
                if is_admin {
                    controls::menu_separator(ui);
                    controls::menu_inset_row(ui, |ui| admin_notification(ui, note));
                }
                controls::menu_separator(ui);
                item(ui, svg::SIGN_OUT, "Log out", AccountAction::Logout);
            });
        if let Some(action) = picked {
            self.apply_account_action(ui, action);
        }
    }

    fn apply_account_action(&mut self, ui: &Ui, action: AccountAction) {
        match action {
            AccountAction::Preferences => {
                self.state = AppState::Authenticated(MainPages::UserPreferences);
                match self
                    .app_state_tx
                    .try_send(AppState::Authenticated(MainPages::UserPreferences))
                {
                    Ok(_) => {
                        if let Some(user) = self.current_user.clone() {
                            self.account_mod.set_user(user);
                        }
                    }
                    Err(e) => log::error!("Error: {e:?}"),
                }
            }
            AccountAction::Downloads => {
                self.state = AppState::Authenticated(MainPages::Downloads);
                self.release_browser.refresh(ui.ctx());
                match self
                    .app_state_tx
                    .try_send(AppState::Authenticated(MainPages::Downloads))
                {
                    Ok(_) => log::info!("Switching to Downloads Page"),
                    Err(e) => log::error!("Error: {e:?}"),
                }
            }
            AccountAction::Theme => self.modify_theme = true,
            AccountAction::Refresh => self.first_run = true,
            AccountAction::ResetPassword => self.reset_code_dialog.open(),
            AccountAction::Logout => self.logout(),
        }
    }

    fn logout(&mut self) {
        #[cfg(target_arch = "wasm32")]
        {
            wasm_cookies::delete("user");
            wasm_cookies::delete("jwt");
        }
        PlatformSpawner::spawn(async move {
            let invalidation = db().invalidate().await;
            log::info!("invalidated connection: {:?}", invalidation);
        });

        #[cfg(target_arch = "wasm32")]
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                let clear = storage.clear();
                log::info!("Clearing localStorage: {clear:?}");
            }
            if let Ok(Some(storage)) = window.session_storage() {
                let clear = storage.clear();
                log::info!("Clearing sessionStorage: {clear:?}");
            }

            PlatformSpawner::spawn(async move {
                use js_sys::wasm_bindgen::JsCast;
                use wasm_bindgen_futures::JsFuture;

                let win = web_sys::window().expect("window");

                if let Ok(caches) = win.caches() {
                    if let Ok(keys_js) = JsFuture::from(caches.keys()).await {
                        let keys = js_sys::Array::from(&keys_js);
                        for key in keys.iter() {
                            if let Some(k) = key.as_string() {
                                let _ = JsFuture::from(caches.delete(&k)).await;
                            }
                        }
                    }
                }

                let swc = win.navigator().service_worker();
                if let Ok(regs_js) = JsFuture::from(swc.get_registrations()).await {
                    let regs = js_sys::Array::from(&regs_js);
                    for reg_val in regs.iter() {
                        if let Ok(reg) = reg_val.dyn_into::<web_sys::ServiceWorkerRegistration>() {
                            if let Ok(promise) = reg.unregister() {
                                let _ = JsFuture::from(promise).await;
                            }
                        }
                    }
                }

                let reload = win.location().reload();
                log::info!("Reloading window: {reload:?}");
            });
        } else {
            log::info!("No window");
        }

        let logout_msg = "Logged out".to_string();
        self.state = AppState::NoAuth(logout_msg.clone());
        match self.app_state_tx.try_send(AppState::NoAuth(logout_msg)) {
            Ok(_) => log::info!("Logged out"),
            Err(e) => log::error!("Error: {e:?}"),
        }
    }

    fn store_picker(&mut self, ui: &mut Ui) {
        let current = self.store_selection;
        let options: Vec<(u64, &str)> = Store::VALUES
            .iter()
            .map(|s| (s.into_store_id() as u64, s.as_str()))
            .collect();
        controls::dropdown(
            ui,
            "Store_Selection",
            &mut self.store_selection,
            &options,
            STORE_W,
        )
        .on_hover_text("Store");
        let (t, m) = (Tokens::get(ui.ctx()), Metrics::get(ui.ctx()));
        svg::STOREFRONT.show_tinted(ui, m.icon_md, t.icon);
        if self.store_selection == current {
            return;
        }
        let tasks_tx = self.initial_tasks_tx.clone();
        let store_users_tx = self.store_users_tx.clone();
        let store_selection = Store::from_presta_store_id(&self.store_selection.to_string());
        self.store_users.clear();
        self.tasks.clear();
        self.layout_configs = None;
        log::info!("Switching to store: {:?}", store_selection.as_str());
        log::info!(
            "Store: {store_selection:?}//{:?}",
            store_selection.clone().as_str().to_string()
        );
        PlatformSpawner::spawn(async move {
            let store_tasks = get_tasks_for_store(
                tasks_tx.clone(),
                store_selection.clone().as_str().to_string(),
            )
            .await;
            let tasks = get_completed_tasks_for_store(
                tasks_tx.clone(),
                store_selection.clone().as_str().to_string(),
            )
            .await;
            let get_store_users = get_store_users(store_users_tx, store_selection).await;
            log::info!("get_completed_tasks_for_store: {tasks:?}");
            log::info!("get_tasks_for_store: {store_tasks:?}");
            log::info!("get_store_users: {get_store_users:?}");
        });
    }

    fn signed_out(&mut self, ui: &mut Ui) {
        if TextButton::new("Log in")
            .icon(svg::SIGN_IN)
            .show(ui)
            .clicked()
        {
            self.state = AppState::NoAuth("Needs Login".to_string());
            match self
                .app_state_tx
                .try_send(AppState::NoAuth("clicked login button".to_string()))
            {
                Ok(_) => log::info!("Switching to Login Page"),
                Err(e) => log::error!("Error: {e:?}"),
            }
        }
        if TextButton::new("Downloads")
            .icon(svg::DOWNLOAD_SIMPLE)
            .kind(ButtonKind::Ghost)
            .show(ui)
            .clicked()
        {
            self.state = AppState::Authenticated(MainPages::Downloads);
            match self
                .app_state_tx
                .try_send(AppState::Authenticated(MainPages::Downloads))
            {
                Ok(_) => log::info!("Switching to Downloads Page"),
                Err(e) => log::error!("Error: {e:?}"),
            }
        }
    }
}

/// The admin broadcast field and its send button.
fn admin_notification(ui: &mut Ui, text: &mut String) {
    controls::TextField::new(text)
        .hint("Admin notification")
        .icon(svg::MEGAPHONE)
        .width(ADMIN_NOTE_W)
        .show(ui);
    if controls::icon_button(
        ui,
        svg::PAPER_PLANE_TILT,
        IconBox::Md,
        false,
        "Send to everyone",
    )
    .clicked()
    {
        let txt = text.clone();
        PlatformSpawner::spawn(async move {
            let res = Notification::default()
                .set_description(txt)
                .set_type("Admin")
                .create()
                .await;
            log::info!("Notification Response: {:?}", res);
        });
    }
}
