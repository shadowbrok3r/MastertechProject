//! Root account administration: roles, stores, tab access, statuses, reset codes and PrestaShop records.

mod detail;
mod prestashop;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod render_tests;
pub mod reset_dialog;

use crossbeam::channel::{Receiver, Sender};
use database::schema::user::User;
use database::schema::user::admin::{
    self, AccountActivity, AccountChange, AccountUpdate, AdminAccount,
};
use database::schema::user::employee_profile::{self, EmployeeProfile};
use database::schema::user::password_reset::{self, IssuedCode};
use database::schema::{RecordId, Store, UserAuthorization};
use eframe::egui::{self, Align, Context, Layout, RichText, ScrollArea, Ui};

use crate::ui_tools::controls::{
    self, BarKind, ListItem, Metrics, ModalWidth, Role, Segment, SpinnerSize, TextField, Tokens,
};
use crate::ui_tools::icons::svg;
use crate::{PlatformSpawner, Spawner};

pub use reset_dialog::{ResetCodeDialog, code_card};

/// Starting width of the account list.
const LIST_W: f32 = 300.0;

/// Which accounts the list shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Shown {
    #[default]
    Active,
    Inactive,
    All,
}

enum Msg {
    Accounts(Result<Vec<AdminAccount>, String>),
    Saved {
        id: RecordId,
        result: Result<AccountUpdate, String>,
        reassign_to: Option<RecordId>,
    },
    Activity(RecordId, Result<AccountActivity, String>),
    Issued(RecordId, Result<IssuedCode, String>),
    Revoked(RecordId, Result<usize, String>),
    Profile(RecordId, Result<Option<EmployeeProfile>, String>),
    Reassigned(RecordId, Result<usize, String>),
}

/// A question asked before a save goes out.
enum Confirm {
    Deactivate { reassign_to: Option<RecordId> },
    PromoteRoot,
}

/// Outcome of the last action, shown in the footer.
struct Notice {
    ok: bool,
    text: String,
}

pub struct UserAdmin {
    accounts: Vec<AdminAccount>,
    loaded: bool,
    loading: bool,
    list_error: Option<String>,
    search: String,
    shown: Shown,
    store_filter: Option<Store>,
    selected: Option<RecordId>,
    draft: Option<AdminAccount>,
    prestashop_id_text: String,
    new_status: String,
    saving: bool,
    notice: Option<Notice>,
    confirm: Option<Confirm>,
    activity: Option<(RecordId, AccountActivity)>,
    activity_loading: bool,
    issued: Option<(RecordId, IssuedCode)>,
    issuing: bool,
    profile: Option<(RecordId, Result<Option<EmployeeProfile>, String>)>,
    profile_loading: bool,
    reassign_to: Option<RecordId>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl Default for UserAdmin {
    fn default() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        Self {
            accounts: Vec::new(),
            loaded: false,
            loading: false,
            list_error: None,
            search: String::new(),
            shown: Shown::default(),
            store_filter: None,
            selected: None,
            draft: None,
            prestashop_id_text: String::new(),
            new_status: String::new(),
            saving: false,
            notice: None,
            confirm: None,
            activity: None,
            activity_loading: false,
            issued: None,
            issuing: false,
            profile: None,
            profile_loading: false,
            reassign_to: None,
            tx,
            rx,
        }
    }
}

/// `Send` on native builds, where spawned futures must be; every type on wasm.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) trait MaybeSend: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send> MaybeSend for T {}
#[cfg(target_arch = "wasm32")]
pub(crate) trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T> MaybeSend for T {}

/// Runs `job` off the UI thread and hands its result to `tx`, then repaints.
pub(crate) fn spawn_job<M, F>(ctx: &Context, tx: &Sender<M>, job: F)
where
    M: MaybeSend + 'static,
    F: std::future::Future<Output = M> + MaybeSend + 'static,
{
    let (ctx, tx) = (ctx.clone(), tx.clone());
    PlatformSpawner::spawn(async move {
        let _ = tx.send(job.await);
        ctx.request_repaint();
    });
}

impl UserAdmin {
    fn refresh(&mut self, ctx: &Context) {
        if self.loading {
            return;
        }
        self.loading = true;
        spawn_job(ctx, &self.tx, async {
            Msg::Accounts(admin::list_accounts().await.map_err(|e| format!("{e:#}")))
        });
    }

    fn load_activity(&mut self, ctx: &Context, account: &AdminAccount) {
        self.activity_loading = true;
        let account = account.clone();
        spawn_job(ctx, &self.tx, async move {
            let result = admin::account_activity(&account)
                .await
                .map_err(|e| format!("{e:#}"));
            Msg::Activity(account.id, result)
        });
    }

    fn select(&mut self, ctx: &Context, account: &AdminAccount) {
        if self.selected.as_ref() == Some(&account.id) {
            return;
        }
        self.selected = Some(account.id.clone());
        self.draft = Some(account.clone());
        self.prestashop_id_text = account
            .id_prestashop
            .map(|id| id.to_string())
            .unwrap_or_default();
        self.new_status.clear();
        self.notice = None;
        self.confirm = None;
        self.reassign_to = None;
        self.activity = None;
        self.load_activity(ctx, account);
    }

    fn selected_account(&self) -> Option<&AdminAccount> {
        let id = self.selected.as_ref()?;
        self.accounts.iter().find(|a| &a.id == id)
    }

    fn drain(&mut self, ctx: &Context, me: &User) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Accounts(result) => {
                    self.loading = false;
                    self.loaded = true;
                    match result {
                        Ok(accounts) => {
                            self.accounts = accounts;
                            self.list_error = None;
                            let still_there = self.selected_account().cloned();
                            match still_there {
                                Some(account) if self.draft.is_none() => {
                                    self.draft = Some(account);
                                }
                                Some(_) => {}
                                None => {
                                    self.selected = None;
                                    self.draft = None;
                                }
                            }
                        }
                        Err(e) => self.list_error = Some(e),
                    }
                }
                Msg::Saved {
                    id,
                    result,
                    reassign_to,
                } => {
                    self.saving = false;
                    match result {
                        Ok(update) => {
                            let text = if update.refused.is_empty() {
                                format!("Saved {}.", update.account.name)
                            } else {
                                format!(
                                    "Saved, but the database kept the old {}.",
                                    update.refused.join(", ")
                                )
                            };
                            self.notice = Some(Notice {
                                ok: update.refused.is_empty(),
                                text,
                            });
                            if let Some(slot) = self.accounts.iter_mut().find(|a| a.id == id) {
                                *slot = update.account.clone();
                            }
                            if self.selected.as_ref() == Some(&id) {
                                self.prestashop_id_text = update
                                    .account
                                    .id_prestashop
                                    .map(|pid| pid.to_string())
                                    .unwrap_or_default();
                                self.draft = Some(update.account.clone());
                            }
                            if let Some(to) = reassign_to {
                                self.start_reassign(ctx, me, id.clone(), to);
                            }
                            self.load_activity(ctx, &update.account);
                        }
                        Err(e) => {
                            self.notice = Some(Notice { ok: false, text: e });
                        }
                    }
                }
                Msg::Activity(id, result) => {
                    if self.selected.as_ref() == Some(&id) {
                        self.activity_loading = false;
                        match result {
                            Ok(activity) => self.activity = Some((id, activity)),
                            Err(e) => {
                                self.activity = None;
                                self.notice = Some(Notice {
                                    ok: false,
                                    text: format!("Activity failed to load: {e}"),
                                });
                            }
                        }
                    }
                }
                Msg::Issued(id, result) => {
                    self.issuing = false;
                    match result {
                        Ok(code) => {
                            self.issued = Some((id.clone(), code));
                            self.notice = None;
                        }
                        Err(e) => self.notice = Some(Notice { ok: false, text: e }),
                    }
                    self.reload_activity_for(ctx, &id);
                }
                Msg::Revoked(id, result) => {
                    match result {
                        Ok(n) => {
                            if self
                                .issued
                                .as_ref()
                                .is_some_and(|(issued_for, _)| issued_for == &id)
                            {
                                self.issued = None;
                            }
                            self.notice = Some(Notice {
                                ok: true,
                                text: format!(
                                    "Cancelled {n} unused code{}.",
                                    if n == 1 { "" } else { "s" }
                                ),
                            });
                        }
                        Err(e) => self.notice = Some(Notice { ok: false, text: e }),
                    }
                    self.reload_activity_for(ctx, &id);
                }
                Msg::Profile(id, result) => {
                    self.profile_loading = false;
                    self.profile = Some((id, result));
                }
                Msg::Reassigned(id, result) => {
                    self.notice = Some(match result {
                        Ok(n) => Notice {
                            ok: true,
                            text: format!("Moved {n} open task{}.", if n == 1 { "" } else { "s" }),
                        },
                        Err(e) => Notice {
                            ok: false,
                            text: format!("Moving open tasks failed: {e}"),
                        },
                    });
                    self.reload_activity_for(ctx, &id);
                }
            }
        }
    }

    fn reload_activity_for(&mut self, ctx: &Context, id: &RecordId) {
        if self.selected.as_ref() != Some(id) {
            return;
        }
        if let Some(account) = self.selected_account().cloned() {
            self.load_activity(ctx, &account);
        }
    }

    fn start_save(&mut self, ctx: &Context, reassign_to: Option<RecordId>) {
        let (Some(before), Some(draft)) = (self.selected_account().cloned(), self.draft.clone())
        else {
            return;
        };
        let changes = admin::diff_accounts(&before, &draft);
        if changes.is_empty() {
            return;
        }
        self.saving = true;
        self.notice = None;
        spawn_job(ctx, &self.tx, async move {
            let result = admin::apply_account_changes(&before, changes)
                .await
                .map_err(|e| e.to_string());
            Msg::Saved {
                id: before.id,
                result,
                reassign_to,
            }
        });
    }

    /// Saves the draft, asking first when it turns the account off or makes it Root.
    fn request_save(&mut self, ctx: &Context) {
        let (Some(before), Some(draft)) = (self.selected_account(), self.draft.as_ref()) else {
            return;
        };
        let changes = admin::diff_accounts(before, draft);
        if changes.contains(&AccountChange::Active(false)) {
            self.confirm = Some(Confirm::Deactivate { reassign_to: None });
        } else if changes.contains(&AccountChange::Role(UserAuthorization::Root)) {
            self.confirm = Some(Confirm::PromoteRoot);
        } else {
            self.start_save(ctx, None);
        }
    }

    fn start_reassign(&mut self, ctx: &Context, me: &User, from: RecordId, to: RecordId) {
        let me = me.clone();
        spawn_job(ctx, &self.tx, async move {
            let result = admin::reassign_open_tasks(&from, &to, &me)
                .await
                .map_err(|e| format!("{e:#}"));
            Msg::Reassigned(from, result)
        });
    }

    fn issue_code(&mut self, ctx: &Context, id: RecordId) {
        self.issuing = true;
        self.notice = None;
        spawn_job(ctx, &self.tx, async move {
            let result = password_reset::issue_reset_code(&id)
                .await
                .map_err(|e| e.to_string());
            Msg::Issued(id, result)
        });
    }

    fn revoke_codes(&mut self, ctx: &Context, id: RecordId) {
        spawn_job(ctx, &self.tx, async move {
            let result = password_reset::revoke_reset_codes(&id)
                .await
                .map_err(|e| e.to_string());
            Msg::Revoked(id, result)
        });
    }

    fn load_profile(&mut self, ctx: &Context, account: &AdminAccount) {
        self.profile_loading = true;
        let (id, pid, email) = (
            account.id.clone(),
            account.id_prestashop,
            account.email.clone(),
        );
        spawn_job(ctx, &self.tx, async move {
            let result = match pid {
                Some(pid) => employee_profile::employee_profile(pid).await.map(Some),
                None => employee_profile::employee_profile_by_email(&email).await,
            };
            Msg::Profile(id, result.map_err(|e| format!("{e:#}")))
        });
    }

    pub fn ui(&mut self, ui: &mut Ui, me: Option<&User>) {
        let Some(me) = me.filter(|u| u.is_admin() && u.is_active()) else {
            crate::tabs::hidden_tab_state(ui);
            return;
        };
        let ctx = ui.ctx().clone();
        self.drain(&ctx, me);
        if !self.loaded {
            self.refresh(&ctx);
        }

        self.toolbar(ui);

        egui::Panel::left("user_admin_list")
            .resizable(true)
            .default_size(LIST_W)
            .min_size(LIST_W * 0.75)
            .max_size(LIST_W * 1.75)
            .show(ui, |ui| self.list(ui));

        egui::CentralPanel::default().show(ui, |ui| match self.draft.is_some() {
            true => {
                egui::Panel::bottom("user_admin_footer")
                    .show_separator_line(false)
                    .show(ui, |ui| self.footer(ui));
                ScrollArea::vertical()
                    .id_salt("user_admin_detail")
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.detail(ui, me));
            }
            false => {
                controls::empty_state(
                    ui,
                    svg::USER_GEAR,
                    "Pick an account",
                    "Change roles, stores, tab access and statuses, issue reset codes and check PrestaShop.",
                    &[],
                );
            }
        });

        self.confirm_dialog(&ctx);
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        controls::bar(ui, BarKind::Tool, |ui| {
            if controls::icon_text_button(ui, svg::ARROW_CLOCKWISE, "Refresh").clicked() {
                self.refresh(&ctx);
            }
            crate::tabs::bar_rule(ui);
            TextField::new(&mut self.search)
                .hint("Search name, email or store")
                .icon(svg::MAGNIFYING_GLASS)
                .clear_button(true)
                .width(Metrics::get(ui.ctx()).combo_w * 1.6)
                .show(ui);
            controls::segmented(
                ui,
                &mut self.shown,
                &[
                    Segment::new(Shown::Active, "Active"),
                    Segment::new(Shown::Inactive, "Inactive"),
                    Segment::new(Shown::All, "All"),
                ],
            );
            let mut options: Vec<(Option<Store>, &str)> = vec![(None, "All stores")];
            options.extend(Store::VALUES.iter().map(|s| (Some(*s), s.as_str())));
            controls::dropdown(
                ui,
                "user_admin_store_filter",
                &mut self.store_filter,
                &options,
                Metrics::get(ui.ctx()).combo_w,
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.loading {
                    controls::spinner(ui, SpinnerSize::Sm);
                } else {
                    let active = self.accounts.iter().filter(|a| a.active).count();
                    ui.label(
                        RichText::new(format!("{active} active · {} total", self.accounts.len()))
                            .weak(),
                    );
                }
            });
        });
    }

    fn visible_accounts(&self) -> Vec<&AdminAccount> {
        self.accounts
            .iter()
            .filter(|a| match self.shown {
                Shown::Active => a.active,
                Shown::Inactive => !a.active,
                Shown::All => true,
            })
            .filter(|a| self.store_filter.is_none_or(|s| a.store == s.as_str()))
            .filter(|a| a.matches(&self.search))
            .collect()
    }

    fn list(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        if let Some(e) = &self.list_error {
            controls::note(ui, svg::WARNING_CIRCLE, e, t.error);
        }
        if self.loading && self.accounts.is_empty() {
            controls::loading_state(ui, "Loading accounts…");
            return;
        }
        let mut picked: Option<AdminAccount> = None;
        let rows: Vec<AdminAccount> = self.visible_accounts().into_iter().cloned().collect();
        if rows.is_empty() {
            controls::empty_state(ui, svg::FUNNEL_SIMPLE, "No accounts match", "", &[]);
            return;
        }
        ScrollArea::vertical()
            .id_salt("user_admin_rows")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for account in &rows {
                    let detail = format!("{} · {}", account.store, account.authorization);
                    let (icon, tone) = account_icon(account, &t);
                    let response = ListItem::new(&account.name)
                        .icon(icon)
                        .icon_tone(tone)
                        .subtitle(account.username())
                        .detail(&detail)
                        .selected(self.selected.as_ref() == Some(&account.id))
                        .show(ui);
                    if response.clicked() {
                        picked = Some(account.clone());
                    }
                }
            });
        if let Some(account) = picked {
            let ctx = ui.ctx().clone();
            self.select(&ctx, &account);
        }
    }

    fn footer(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let t = Tokens::get(ui.ctx());
        let pending = match (self.selected_account(), self.draft.as_ref()) {
            (Some(before), Some(draft)) => admin::diff_accounts(before, draft).len(),
            _ => 0,
        };
        controls::bar(ui, BarKind::Footer, |ui| {
            if let Some(notice) = &self.notice {
                let (icon, tone) = if notice.ok {
                    (svg::CHECK_CIRCLE, t.success)
                } else {
                    (svg::WARNING_CIRCLE, t.error)
                };
                controls::status_label(ui, icon, &notice.text, tone);
            } else if pending > 0 {
                controls::status_label(
                    ui,
                    svg::INFO,
                    &format!(
                        "{pending} unsaved change{}",
                        if pending == 1 { "" } else { "s" }
                    ),
                    t.warn,
                );
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let label = if self.saving {
                    "Saving…"
                } else {
                    "Save changes"
                };
                let saved = controls::TextButton::new(label)
                    .icon(svg::FLOPPY_DISK)
                    .kind(controls::ButtonKind::Primary)
                    .enabled(pending > 0 && !self.saving)
                    .show(ui)
                    .clicked();
                let discarded = controls::TextButton::new("Discard")
                    .icon(svg::ARROW_COUNTER_CLOCKWISE)
                    .enabled(pending > 0 && !self.saving)
                    .show(ui)
                    .clicked();
                if saved {
                    self.request_save(&ctx);
                }
                if discarded && let Some(before) = self.selected_account().cloned() {
                    self.prestashop_id_text = before
                        .id_prestashop
                        .map(|id| id.to_string())
                        .unwrap_or_default();
                    self.draft = Some(before);
                    self.notice = None;
                }
            });
        });
    }

    fn confirm_dialog(&mut self, ctx: &Context) {
        if self.confirm.is_none() {
            return;
        }
        let Some(draft) = self.draft.clone() else {
            self.confirm = None;
            return;
        };
        let open_tasks = self
            .activity
            .as_ref()
            .filter(|(id, _)| id == &draft.id)
            .map(|(_, a)| a.open_tasks);
        let targets: Vec<(Option<RecordId>, String)> =
            std::iter::once((None, "Leave them assigned".to_string()))
                .chain(
                    self.accounts
                        .iter()
                        .filter(|a| a.active && a.id != draft.id)
                        .map(|a| (Some(a.id.clone()), format!("{} · {}", a.name, a.store))),
                )
                .collect();
        let Some(confirm) = self.confirm.as_mut() else {
            return;
        };
        let outcome = match confirm {
            Confirm::Deactivate { reassign_to } => {
                let title = format!("Turn off {}?", draft.name);
                controls::modal(
                    ctx,
                    egui::Id::unique("user_admin_confirm_deactivate"),
                    svg::PROHIBIT,
                    &title,
                    ModalWidth::Narrow,
                    &[(Role::Danger, "Turn off account"), (Role::Cancel, "Cancel")],
                    |ui| {
                        ui.label("They can't sign in until the account is turned back on.");
                        let tasks = match open_tasks {
                            Some(0) => "No open tasks are assigned to them.".to_string(),
                            Some(n) => format!(
                                "{n} open task{} assigned to them.",
                                if n == 1 { " is" } else { "s are" }
                            ),
                            None => "Open tasks are still loading.".to_string(),
                        };
                        ui.label(tasks);
                        if open_tasks.unwrap_or(1) > 0 {
                            let options: Vec<(Option<RecordId>, &str)> = targets
                                .iter()
                                .map(|(id, label)| (id.clone(), label.as_str()))
                                .collect();
                            let width = ui.available_width();
                            controls::dropdown(
                                ui,
                                "user_admin_reassign_on_deactivate",
                                reassign_to,
                                &options,
                                width,
                            );
                        }
                    },
                )
            }
            Confirm::PromoteRoot => {
                let title = format!("Give {} Root access?", draft.name);
                controls::modal(
                    ctx,
                    egui::Id::unique("user_admin_confirm_root"),
                    svg::CROWN_SIMPLE,
                    &title,
                    ModalWidth::Narrow,
                    &[(Role::Danger, "Make Root"), (Role::Cancel, "Cancel")],
                    |ui| {
                        ui.label(
                            "Root can change every account, issue reset codes for anyone and open the Root-only tabs.",
                        );
                    },
                )
            }
        };
        if outcome.clicked == Some(0) {
            let reassign_to = match self.confirm.take() {
                Some(Confirm::Deactivate { reassign_to }) => reassign_to,
                _ => None,
            };
            self.start_save(ctx, reassign_to);
        } else if outcome.should_close {
            self.confirm = None;
        }
    }
}

/// List icon and tone for an account's role and status.
fn account_icon(account: &AdminAccount, t: &Tokens) -> (controls::Icon, egui::Color32) {
    if !account.active {
        return (svg::PROHIBIT, t.text_faint);
    }
    match account.role() {
        Some(UserAuthorization::Root) => (svg::CROWN_SIMPLE, t.primary),
        Some(UserAuthorization::Manager) => (svg::SHIELD_CHECK, t.secondary),
        Some(UserAuthorization::Warehouse) => (svg::HARD_DRIVES, t.tertiary),
        _ => (svg::USER_CIRCLE, t.icon),
    }
}
