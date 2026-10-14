//! The main window's title strip: brand mark, menus, task search, tools, account and captions.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use database::{
    db,
    schema::{
        Store,
        utilities::{get_completed_tasks_for_store, get_store_users, get_tasks_for_store},
    },
};
use displays::{
    TaskUiActions,
    app_state::{AppState, MainPages, default_tree},
    pages::view_menu,
    plugins::push_widget_anchor,
    tabs::{TabContext, TabId, WorkMode},
    ui_tools::{
        controls::{
            self, ButtonKind, IconBox, IconButton, MenuButton, MenuItem, Metrics, TextButton,
            Tokens,
        },
        do_not_disturb,
        icons::{
            self,
            svg::{self, Icon},
        },
        theme_config,
    },
};
use eframe::egui::{Rect, Ui, UiKind, ViewportCommand};
use log::{error, info};
use mtech_ui::title_bar::{Side, TitleBar};
use tokio::spawn;

use crate::app_state::MasterTechApp;
use crate::tabs::github::self_updater::run;
use crate::utilities::{
    update_policy::{self, UpdateMode},
    window_prefs,
};

/// Window width below which bar labels collapse to icons.
const COMPACT_BELOW: f32 = 1280.0;
/// Width of the notifications popup.
const NOTIFICATIONS_W: f32 = 380.0;
/// Width of the account menu.
const ACCOUNT_W: f32 = 270.0;
/// Width of the store dropdown in the account menu.
const STORE_W: f32 = 96.0;
/// Characters of a client name shown in the bar.
const CLIENT_CHARS: usize = 22;
const ASK_TIP: &str = "Ask the assistant (Ctrl+K)";
const LAYOUT_HINT: &str =
    "Only available in Everything mode - presets always open their own layout.";

/// Work-mode changes the bar asked for this frame, applied once it has drawn.
#[derive(Default)]
struct BarActions {
    switch_to: Option<WorkMode>,
    back_to_picker: bool,
}

/// What a Layout menu row asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LayoutAction {
    Save,
    Organize,
    Reset,
    ResetMemory,
}

/// A main page the bar can open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Workspace,
    Preferences,
    Downloads,
}

impl Page {
    fn main_page(self) -> MainPages {
        match self {
            Self::Workspace => MainPages::Tasks,
            Self::Preferences => MainPages::UserPreferences,
            Self::Downloads => MainPages::Downloads,
        }
    }
}

/// What an account menu row asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AccountAction {
    Preferences,
    Downloads,
    Theme,
    Refresh,
    Update,
    TerminalMode,
    Logout,
}

/// The icon a work mode shows, matching the picker's glyph.
fn mode_icon(mode: WorkMode) -> Icon {
    Icon::from_glyph(mode.glyph()).unwrap_or(svg::SQUARES_FOUR)
}

/// Window title of the frameless bar.
const TITLE: &str = concat!("MasterTech ", database::version_with_build!());

/// `name` cut to [`CLIENT_CHARS`] characters with an ellipsis.
fn short_name(name: &str) -> Cow<'_, str> {
    if name.chars().count() <= CLIENT_CHARS {
        Cow::Borrowed(name)
    } else {
        let head: String = name.chars().take(CLIENT_CHARS - 1).collect();
        Cow::Owned(format!("{head}…"))
    }
}

impl MasterTechApp {
    /// The window's title strip; menus and tools only while `show_menus`.
    pub fn title_bar(&mut self, ui: &mut Ui, show_menus: bool) {
        let frameless = self.context.chrome.frameless;
        if !frameless && !show_menus {
            return;
        }
        let title = if frameless { TITLE } else { "" };
        let compact = ui.ctx().content_rect().width() < COMPACT_BELOW;
        let mut acts = BarActions::default();
        let mut bar = TitleBar::new("mastertech.title_bar", title).frameless(frameless);
        if frameless {
            bar = bar.mark(svg::MT_LOGO);
        }
        let _ = bar.show(ui, |ui, side| {
            if !show_menus {
                return;
            }
            match side {
                Side::Left => self.bar_left(ui, compact, &mut acts),
                Side::Right => self.bar_right(ui, compact),
            }
        });

        if let Some(mode) = acts.switch_to {
            let ctx = ui.ctx().clone();
            self.apply_work_mode(mode, &ctx);
        }
        if acts.back_to_picker {
            if self.context.work_mode.active == Some(WorkMode::Full) {
                self.context.work_mode.full_snapshot = Some(self.dock.tree.clone());
            }
            self.context.work_mode.active = None;
        }
    }

    fn bar_left(&mut self, ui: &mut Ui, compact: bool, acts: &mut BarActions) {
        if self.context.shared_ctx.current_user.is_none() {
            if TextButton::new("Log in")
                .icon(svg::SIGN_IN)
                .show(ui)
                .clicked()
            {
                let _ = self
                    .context
                    .shared_ctx
                    .app_state_tx
                    .send(AppState::NoAuth("Login".to_string()));
            }
            return;
        }
        let on_workspace = matches!(
            self.context.shared_ctx.state,
            AppState::Authenticated(MainPages::Tasks)
        );
        if !on_workspace
            && TextButton::new("Workspace")
                .icon(svg::ARROW_LEFT)
                .kind(ButtonKind::Ghost)
                .show(ui)
                .on_hover_text("Back to the workspace")
                .clicked()
        {
            self.open_page(ui, Page::Workspace);
        }
        self.view_menu(ui);
        self.mode_menu(ui, acts);
        self.layout_menu(ui, compact);
        let m = Metrics::get(ui.ctx());
        controls::vline(ui, m.control_h - 8.0);
        let label = if compact { "New" } else { "New task" };
        if TextButton::new(label)
            .icon(icons::CREATE_TASK)
            .kind(ButtonKind::Primary)
            .show(ui)
            .on_hover_text("Create a new task")
            .clicked()
        {
            let _ = self
                .context
                .shared_ctx
                .ui_actions_tx
                .try_send(TaskUiActions::CreateTaskModal(None));
        }
        let shared = &mut self.context.shared_ctx;
        displays::ui_data::task_search::search_bar(
            ui,
            displays::ui_data::task_search::TaskSearchCtx {
                state: &mut shared.task_search,
                input: &mut shared.search_input,
                results: &mut shared.search_results,
                index: &mut shared.task_index,
                tasks: &mut shared.tasks,
                store_users: &shared.store_users,
                store_selection: shared.store_selection,
                actions: &shared.ui_actions_tx,
            },
        );
    }

    fn view_menu(&mut self, ui: &mut Ui) {
        let mode = self.context.work_mode.active.unwrap_or_default();
        let dock = &mut self.dock;
        let (button, _) = MenuButton::new("View")
            .icon(svg::TABS)
            .tip("Open or close tabs")
            .show(ui, |ui| {
                let mut anchor = |tab: TabId, rect: Rect| {
                    push_widget_anchor(format!("nav.tab.{}", tab.slug()), rect);
                };
                view_menu(
                    ui,
                    dock,
                    TabContext::MastertechNative,
                    mode,
                    Some(&mut anchor),
                );
            });
        push_widget_anchor("nav.menu.view", button.rect);
    }

    fn mode_menu(&mut self, ui: &mut Ui, acts: &mut BarActions) {
        let t = Tokens::get(ui.ctx());
        let active = self.context.work_mode.active.unwrap_or_default();
        let (button, _) = MenuButton::new(active.title())
            .icon(mode_icon(active))
            .tint(t.primary)
            .tip("Work mode")
            .show(ui, |ui| {
                for &mode in WorkMode::ALL {
                    let item = controls::menu_check_item(
                        ui,
                        mode_icon(mode),
                        mode.title(),
                        mode == active,
                    );
                    if item.on_hover_text(mode.blurb()).clicked() && mode != active {
                        acts.switch_to = Some(mode);
                    }
                }
                controls::menu_separator(ui);
                if controls::menu_item(ui, svg::ARROW_U_UP_LEFT, "Choose again…").clicked() {
                    acts.back_to_picker = true;
                }
            });
        push_widget_anchor("nav.menu.work_mode", button.rect);
    }

    fn layout_menu(&mut self, ui: &mut Ui, compact: bool) {
        let editable = self.context.work_mode.active.unwrap_or_default() == WorkMode::Full;
        let label = if compact { "" } else { "Layout" };
        let mut picked = None;
        let _ = MenuButton::new(label)
            .icon(svg::LAYOUT)
            .tip("UI layout")
            .show(ui, |ui| {
                let row =
                    |ui: &mut Ui, icon: Icon, text: &str, action: LayoutAction, enabled: bool| {
                        let item = MenuItem::new(text).icon(icon).enabled(enabled).show(ui);
                        if item.on_disabled_hover_text(LAYOUT_HINT).clicked() {
                            Some(action)
                        } else {
                            None
                        }
                    };
                picked = picked
                    .or(row(
                        ui,
                        svg::FLOPPY_DISK,
                        "Save layout",
                        LayoutAction::Save,
                        editable,
                    ))
                    .or(row(
                        ui,
                        svg::FRAME_CORNERS,
                        "Organize windows",
                        LayoutAction::Organize,
                        true,
                    ));
                controls::menu_separator(ui);
                picked = picked
                    .or(row(
                        ui,
                        svg::ARROW_COUNTER_CLOCKWISE,
                        "Reset layout",
                        LayoutAction::Reset,
                        editable,
                    ))
                    .or(row(
                        ui,
                        svg::BROOM,
                        "Reset memory",
                        LayoutAction::ResetMemory,
                        true,
                    ));
            });
        if let Some(action) = picked {
            self.apply_layout_action(ui, action);
        }
    }

    fn apply_layout_action(&mut self, ui: &Ui, action: LayoutAction) {
        match action {
            LayoutAction::Save | LayoutAction::Reset => {
                let Some(usr) = self.context.shared_ctx.current_user.as_mut() else {
                    return;
                };
                if action == LayoutAction::Reset {
                    self.dock = default_tree();
                }
                let layout = self.dock.layout_value();
                usr.set_ui_layout_mastertech(layout.clone());
                let mut user = usr.clone();
                spawn(async move {
                    match user.save_mastertech_ui_layout(layout).await {
                        Ok(_) => info!("Updated User Settings"),
                        Err(e) => error!("Error updating User Settings: {e:?}"),
                    }
                });
            }
            LayoutAction::Organize => {
                ui.ctx().memory_mut(|mem| mem.reset_areas());
                ui.ctx().memory_mut(|mem| {
                    for layer in mem.areas_mut().visible_layer_ids().iter() {
                        info!("Visible layers: {layer:?}");
                    }
                });
            }
            LayoutAction::ResetMemory => theme_config::reset_memory(ui.ctx()),
        }
    }

    fn bar_right(&mut self, ui: &mut Ui, compact: bool) {
        let Some(username) = self
            .context
            .shared_ctx
            .current_user
            .as_ref()
            .map(|u| u.get_username().to_owned())
        else {
            return;
        };
        self.account_menu(ui, &username);
        let m = Metrics::get(ui.ctx());
        controls::vline(ui, m.control_h - 8.0);
        self.notifications_button(ui);
        dnd_button(ui);
        let ask = if compact {
            IconButton::new(svg::ROBOT, IconBox::Md)
                .tip(ASK_TIP)
                .show(ui)
        } else {
            TextButton::new("Ask")
                .icon(svg::ROBOT)
                .kind(ButtonKind::Ghost)
                .show(ui)
                .on_hover_text(ASK_TIP)
        };
        if ask.clicked() {
            self.context.shared_ctx.command_bar.request_open();
        }
        self.client_chip(ui, compact);
    }

    /// The client's name and ID, whichever is set, with the tooltip prefix for it.
    fn client_name(&self) -> (&str, &'static str) {
        if self.context.client_friendly_name.is_empty() {
            (&self.context.client_title, "Client ID")
        } else {
            (&self.context.client_friendly_name, "Client")
        }
    }

    /// The client chip; an icon button alone when `compact`.
    fn client_chip(&self, ui: &mut Ui, compact: bool) {
        let (name, kind) = self.client_name();
        if name.is_empty() {
            return;
        }
        let chip = if compact {
            IconButton::new(svg::DESKTOP_TOWER, IconBox::Md)
                .tip(kind)
                .show(ui)
        } else {
            controls::pill(ui, svg::DESKTOP_TOWER, &short_name(name), false)
        };
        let chip = chip.on_hover_ui(|ui| {
            ui.label(format!("{kind}: {name}\nClick to copy"));
        });
        if chip.clicked() {
            ui.ctx().copy_text(name.to_owned());
            let _ = displays::get_toast_sender()
                .try_send(displays::ToastMessage::Info(format!("Copied {name}")));
        }
    }

    fn notifications_button(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        let center = &self.context.shared_ctx.notification_center;
        let (unread, alerts) = (center.unread_count(), center.alert_count());
        let tip: Cow<str> = match (alerts, unread) {
            (0, 0) => "Notifications".into(),
            (0, n) => format!("{n} unread notifications").into(),
            (a, n) => format!("{a} unread alerts, {n} unread notifications").into(),
        };
        let mut button = IconButton::new(
            if alerts > 0 {
                svg::BELL_RINGING
            } else {
                svg::BELL
            },
            IconBox::Md,
        )
        .tip(&tip);
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
        let shared = &mut self.context.shared_ctx;
        controls::popup_menu(&response, true, |ui| {
            ui.reset_style();
            ui.set_width(NOTIFICATIONS_W);
            let mut names = BTreeSet::new();
            for task in shared.task_index.values() {
                names.insert(task.task_name.clone());
                names.insert(task.service_number.clone().unwrap_or_default());
            }
            shared
                .notification_center
                .ui(ui, &names, shared.ui_actions_tx.clone(), &shared.tasks);
        });
    }

    fn account_menu(&mut self, ui: &mut Ui, username: &str) {
        let mut picked = None;
        let mut store_changed = false;
        let (_, _) = MenuButton::new(username)
            .icon(svg::USER_CIRCLE)
            .tip("Account")
            .keep_open(true)
            .show(ui, |ui| {
                ui.set_width(ACCOUNT_W);
                controls::menu_inset_row(ui, |ui| {
                    controls::icon_label(ui, svg::IDENTIFICATION_BADGE, username)
                });
                let (client, _) = self.client_name();
                if !client.is_empty() {
                    let client = short_name(client);
                    controls::menu_inset_row(ui, |ui| {
                        controls::icon_label(ui, svg::DESKTOP_TOWER, &client)
                    });
                }
                store_changed = self.store_row(ui);
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
                controls::menu_separator(ui);
                let update = if self.context.pending_update.is_some() {
                    "Install update"
                } else {
                    "Check for updates"
                };
                item(ui, svg::CLOUD_ARROW_DOWN, update, AccountAction::Update);
                self.update_mode_row(ui);
                self.title_bar_row(ui);
                item(
                    ui,
                    svg::TERMINAL_WINDOW,
                    "Restart in terminal mode",
                    AccountAction::TerminalMode,
                );
                controls::menu_separator(ui);
                item(ui, svg::SIGN_OUT, "Log out", AccountAction::Logout);
            });
        if store_changed {
            self.reload_store();
        }
        if let Some(action) = picked {
            self.apply_account_action(ui, action);
        }
    }

    /// The store picker; true when the selection changed.
    fn store_row(&mut self, ui: &mut Ui) -> bool {
        let selected = &mut self.context.shared_ctx.store_selection;
        let before = *selected;
        let stores: &'static [Store] = &Store::VALUES;
        let options: Vec<(u64, &str)> = stores
            .iter()
            .map(|s| (s.into_store_id() as u64, s.as_str()))
            .collect();
        controls::menu_inset_row(ui, |ui| {
            controls::icon_label(ui, svg::STOREFRONT, "Store");
            ui.with_layout(
                eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
                |ui| {
                    let m = Metrics::get(ui.ctx());
                    ui.add_space(m.pad.x);
                    controls::dropdown(ui, "account.store", selected, &options, STORE_W);
                },
            );
        });
        *selected != before
    }

    fn update_mode_row(&mut self, ui: &mut Ui) {
        let mut auto = self.context.update_mode == UpdateMode::Auto;
        let toggled = controls::menu_inset_row(ui, |ui| {
            controls::toggle(ui, &mut auto, "Install updates automatically")
        })
        .on_hover_text(
            "Off: a notification asks before updating. Either way, updates wait \
                 while remote work or an AI session runs on this computer.",
        )
        .changed();
        if !toggled {
            return;
        }
        let mode = if auto {
            UpdateMode::Auto
        } else {
            UpdateMode::Prompt
        };
        self.context.update_mode = mode;
        if let Err(e) = update_policy::save_mode(mode) {
            error!("menu_bar -> saving the update mode failed: {e}");
            let _ = displays::get_toast_sender().try_send(displays::ToastMessage::Error(format!(
                "Couldn't save the update setting: {e}"
            )));
        }
    }

    fn title_bar_row(&mut self, ui: &mut Ui) {
        if !self.context.chrome.available {
            return;
        }
        let mut custom = self.context.chrome.frameless;
        let toggled = controls::menu_inset_row(ui, |ui| {
            controls::toggle(ui, &mut custom, "MasterTech title bar")
        })
        .on_hover_text(
            "Off: the Windows title bar. --system-titlebar also keeps it for one launch.",
        )
        .changed();
        if !toggled {
            return;
        }
        self.context.chrome.frameless = custom;
        ui.ctx()
            .send_viewport_cmd(ViewportCommand::Decorations(!custom));
        if let Err(e) = window_prefs::save_custom_title_bar(custom) {
            error!("menu_bar -> saving the title bar setting failed: {e}");
            let _ = displays::get_toast_sender().try_send(displays::ToastMessage::Error(format!(
                "Couldn't save the title bar setting: {e}"
            )));
        }
    }

    /// Clears the store's loaded data and fetches the newly selected store's.
    fn reload_store(&mut self) {
        let shared = &mut self.context.shared_ctx;
        shared.store_users.clear();
        shared.tasks.clear();
        shared.task_index.clear();
        shared.live_task_updates.clear();
        shared.task_layouts.clear();
        let tasks_tx = shared.initial_tasks_tx.clone();
        let store_users_tx = shared.store_users_tx.clone();
        let done_tx = shared.completed_load_tx.clone();
        let store = Store::from_presta_store_id(&shared.store_selection.to_string());
        shared.completed_loaded_for = None;
        shared.completed_pending_for = Some(store);

        info!("Store: {store:?}//{:?}", store.as_str());
        spawn(async move {
            let code = store.as_str().to_string();
            let store_tasks = get_tasks_for_store(tasks_tx.clone(), code.clone()).await;
            let completed = get_completed_tasks_for_store(tasks_tx, code).await;
            let _ = done_tx.try_send((store, completed.is_ok()));
            let store_users = get_store_users(store_users_tx, store).await;

            info!("get_tasks_for_store: {store_tasks:?}");
            info!("get_completed_tasks_for_store: {completed:?}");
            info!("get_store_users: {store_users:?}");
        });
    }

    /// Switches the main page and tells the state channel.
    fn open_page(&mut self, ui: &Ui, page: Page) {
        if page == Page::Downloads {
            self.context.shared_ctx.release_browser.refresh(ui.ctx());
        }
        self.context.shared_ctx.state = AppState::Authenticated(page.main_page());
        match self
            .context
            .shared_ctx
            .app_state_tx
            .try_send(AppState::Authenticated(page.main_page()))
        {
            Ok(_) => {
                info!("AppState::Authenticated({:?})", page.main_page());
                if page == Page::Preferences
                    && let Some(usr) = self.context.shared_ctx.current_user.clone()
                {
                    self.context.shared_ctx.account_mod.set_user(usr);
                }
            }
            Err(e) => error!("Error: {e:?}"),
        }
    }

    fn apply_account_action(&mut self, ui: &Ui, action: AccountAction) {
        match action {
            AccountAction::Preferences => self.open_page(ui, Page::Preferences),
            AccountAction::Downloads => self.open_page(ui, Page::Downloads),
            AccountAction::Theme => self.context.shared_ctx.modify_theme = true,
            AccountAction::Refresh => self.context.shared_ctx.first_run = true,
            AccountAction::Update => {
                if let Some(pending) = self.context.pending_update.as_mut() {
                    pending.accepted = true;
                } else {
                    self.context.update_requested = true;
                    if !self.context.update_busy() {
                        self.spawn_update_check();
                    }
                }
            }
            AccountAction::TerminalMode => {
                let result = crate::utilities::app_restart::restart_in_terminal_mode();
                info!("restart_in_terminal_mode: {result:?}");
                ui.ctx().send_viewport_cmd(ViewportCommand::Close);
            }
            AccountAction::Logout => self.logout(),
        }
    }

    /// Downloads the newest release for this OS, or toasts that MasterTech is up to date.
    fn spawn_update_check(&self) {
        let client = self.context.client.clone();
        let tx = self.context.bytes_tx.clone();
        let active = Arc::clone(&self.context.update_download_active);
        active.store(true, Ordering::Release);
        spawn(async move {
            match run(client, tx).await {
                Ok(true) => {}
                Ok(false) => {
                    active.store(false, Ordering::Release);
                    let _ = displays::get_toast_sender().try_send(displays::ToastMessage::Info(
                        "MasterTech is up to date.".to_string(),
                    ));
                }
                Err(e) => {
                    active.store(false, Ordering::Release);
                    error!("update check failed: {e:?}");
                }
            }
        });
    }

    fn logout(&mut self) {
        let file_path = "data.enc";
        match std::fs::exists(file_path) {
            Ok(true) => match std::fs::remove_file(file_path) {
                Ok(_) => info!("Removed data file, logged out."),
                Err(e) => error!("Error removing data file: {e:?}"),
            },
            Ok(false) => info!("Data file does not exist"),
            Err(e) => error!("*.enc File not found {e:?}"),
        }

        self.context.get_settings = true;
        self.context.work_mode = Default::default();

        let shared = &mut self.context.shared_ctx;
        let _ = shared
            .app_state_tx
            .try_send(AppState::NoAuth("Login".to_string()));
        spawn(async move {
            let invalidation = db().invalidate().await;
            info!("invalidated connection: {:?}", invalidation);
        });

        let logout_msg = "Logged out".to_string();
        shared.state = AppState::NoAuth(logout_msg.clone());
        match shared.app_state_tx.try_send(AppState::NoAuth(logout_msg)) {
            Ok(_) => info!("Logged out"),
            Err(e) => error!("Error: {e:?}"),
        }
    }
}

/// Do Not Disturb as a moon toggle carrying the silenced tally while on.
fn dnd_button(ui: &mut Ui) {
    let on = do_not_disturb::is_enabled();
    let count = do_not_disturb::suppressed_count();
    let tip: Cow<str> = if on {
        format!(
            "Do Not Disturb is on: {count} notification(s) silenced.\nClick to resume notifications."
        )
        .into()
    } else {
        "Do Not Disturb: silence all notifications until the app restarts.\n\
         Approval and confirmation dialogs still appear."
            .into()
    };
    let response = IconButton::new(svg::MOON, IconBox::Md)
        .selected(on)
        .tip(&tip)
        .show(ui);
    if on {
        let t = Tokens::get(ui.ctx());
        controls::count_badge(ui, response.rect, count, t.secondary);
    }
    if response.clicked() {
        do_not_disturb::set_enabled(!on);
    }
}
