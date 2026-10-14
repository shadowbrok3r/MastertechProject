//! The selected account's sections: identity, password, tab access, statuses, activity and changes.

use database::schema::user::User;
use database::schema::user::admin::{AdminAccount, AdminActionRow};
use database::schema::user::password_reset::ResetCodeStatus;
use database::schema::{Datetime, RecordId, Store, UserAuthorization};
use eframe::egui::{Context, Grid, Key, RichText, Ui};
use serde_json::Value;

use super::reset_dialog::local_clock;
use super::{UserAdmin, account_icon, code_card};
use crate::tabs::{TabContext, TabId};
use crate::ui_tools::controls::{
    self, ButtonKind, IconBox, IconButton, IconLabel, Metrics, Section, TextButton, TextField,
    Tokens,
};
use crate::ui_tools::icons::svg;

/// Width of the account form's text fields.
const FIELD_W: f32 = 280.0;
/// Columns in the tab-access grid.
const TAB_COLUMNS: usize = 3;

/// "5 min ago"-style age of `at`.
pub(super) fn ago(at: &Datetime) -> String {
    let secs = (chrono::Utc::now() - **at).num_seconds().max(0);
    match secs {
        0..60 => "just now".to_string(),
        60..3_600 => format!("{} min ago", secs / 60),
        3_600..86_400 => format!("{} h ago", secs / 3_600),
        _ => format!("{} days ago", secs / 86_400),
    }
}

/// Local date and time of `at` with its age.
pub(super) fn when(at: &Datetime) -> String {
    let local = at.with_timezone(&chrono::Local);
    format!("{} ({})", local.format("%b %-d, %-I:%M %p"), ago(at))
}

/// One line describing the newest reset code.
fn code_status_line(status: &ResetCodeStatus) -> String {
    let by = status
        .issued_by_name
        .as_deref()
        .map(|name| format!(" by {name}"))
        .unwrap_or_default();
    if let Some(used) = &status.used_at {
        return format!("Last code{by} was used {}.", ago(used));
    }
    if status.is_live(chrono::Utc::now()) {
        return format!(
            "A code issued{by} {} works until {}; {} of {} tries used.",
            ago(&status.created_at),
            local_clock(&status.expires_at),
            status.attempts,
            database::schema::user::password_reset::RESET_MAX_ATTEMPTS
        );
    }
    format!(
        "Last code{by}, issued {}, expired unused.",
        ago(&status.created_at)
    )
}

/// A logged value as short text.
fn value_text(value: &Value) -> String {
    match value {
        Value::Null => "none".to_string(),
        Value::Bool(true) => "on".to_string(),
        Value::Bool(false) => "off".to_string(),
        Value::String(s) if s.is_empty() => "\"\"".to_string(),
        Value::String(s) => s.clone(),
        Value::Array(items) if items.is_empty() => "none".to_string(),
        Value::Array(items) => items.iter().map(value_text).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

/// One line describing an `admin_action` row.
fn history_line(row: &AdminActionRow) -> String {
    match row.action.as_str() {
        "update" => row
            .detail
            .as_object()
            .map(|fields| {
                let mut parts: Vec<String> = fields
                    .iter()
                    .map(|(field, change)| {
                        let old = change.get("old").map(value_text).unwrap_or_default();
                        let new = change.get("new").map(value_text).unwrap_or_default();
                        format!("{field}: {old} → {new}")
                    })
                    .collect();
                parts.sort();
                parts.join("; ")
            })
            .unwrap_or_else(|| "updated".to_string()),
        "reassign_tasks" => {
            let count = row.detail.get("count").and_then(Value::as_u64).unwrap_or(0);
            format!(
                "moved {count} open task{}",
                if count == 1 { "" } else { "s" }
            )
        }
        other => other.to_string(),
    }
}

impl UserAdmin {
    pub(super) fn detail(&mut self, ui: &mut Ui, me: &User) {
        let ctx = ui.ctx().clone();
        let Some(before) = self.selected_account().cloned() else {
            return;
        };
        let is_self = before.id == me.get_id();
        self.header(ui, &before, is_self);
        self.account_section(ui, &before, is_self);
        self.password_section(ui, &ctx, &before, is_self);
        self.tabs_section(ui);
        self.statuses_section(ui);
        self.prestashop_section(ui, &ctx);
        self.activity_section(ui, &ctx, me, &before);
        self.history_section(ui);
    }

    fn header(&self, ui: &mut Ui, account: &AdminAccount, is_self: bool) {
        let t = Tokens::get(ui.ctx());
        ui.horizontal(|ui| {
            let (icon, _) = account_icon(account, &t);
            controls::heading(ui, icon, &account.name);
            controls::badge(ui, &account.authorization, t.primary);
            controls::badge(ui, &account.store, t.secondary);
            if !account.active {
                controls::badge(ui, "Turned off", t.error);
            }
        });
        ui.horizontal(|ui| {
            IconLabel::new(svg::ENVELOPE_SIMPLE, &account.email)
                .tone(t.text_dim)
                .selectable(true)
                .show(ui);
            let id = format!(
                "{}:{}",
                account.id.table,
                database::schema::RecordIdExt::key_string(&account.id)
            );
            IconLabel::new(svg::IDENTIFICATION_BADGE, &id)
                .tone(t.text_faint)
                .mono(true)
                .selectable(true)
                .show(ui);
            if IconButton::new(svg::COPY, IconBox::Sm)
                .tip("Copy the record id")
                .show(ui)
                .clicked()
            {
                ui.ctx().copy_text(id);
            }
        });
        if is_self {
            controls::callout(ui, t.info, svg::INFO, |ui| {
                ui.label(
                    "This is your account. Only another Root can change your role, status, store or \
                     identity. Change your own password from Preferences.",
                );
            });
        }
    }

    fn account_section(&mut self, ui: &mut Ui, before: &AdminAccount, is_self: bool) {
        let (t, m) = (Tokens::get(ui.ctx()), Metrics::get(ui.ctx()));
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        let prestashop_text = &mut self.prestashop_id_text;
        Section::new("user_admin_account", "Account")
            .icon(svg::IDENTIFICATION_CARD)
            .default_open(true)
            .show(ui, |ui| {
                ui.add_enabled_ui(!is_self, |ui| {
                    Grid::new("user_admin_account_grid")
                        .num_columns(2)
                        .spacing(m.gap * 1.5)
                        .show(ui, |ui| {
                            ui.label("Name");
                            TextField::new(&mut draft.name).width(FIELD_W).show(ui);
                            ui.end_row();

                            ui.label("Email");
                            TextField::new(&mut draft.email)
                                .width(FIELD_W)
                                .icon(svg::ENVELOPE_SIMPLE)
                                .show(ui);
                            ui.end_row();

                            ui.label("Role");
                            let roles: Vec<(String, &str)> = UserAuthorization::ALL
                                .iter()
                                .map(|r| (r.as_str().to_string(), r.as_str()))
                                .collect();
                            controls::Dropdown::new(
                                "user_admin_role",
                                &mut draft.authorization,
                                &roles,
                            )
                            .placeholder("Unknown role")
                            .width(FIELD_W)
                            .show(ui);
                            ui.end_row();

                            ui.label("Store");
                            let stores: Vec<(String, &str)> = Store::VALUES
                                .iter()
                                .map(|s| (s.as_str().to_string(), s.as_str()))
                                .collect();
                            controls::Dropdown::new("user_admin_store", &mut draft.store, &stores)
                                .placeholder("Unknown store")
                                .width(FIELD_W)
                                .show(ui);
                            ui.end_row();

                            ui.label("PrestaShop ID");
                            let parsed = parse_prestashop_id(prestashop_text);
                            let field = TextField::new(prestashop_text)
                                .width(FIELD_W)
                                .hint("None")
                                .mono(true)
                                .invalid(parsed.is_none())
                                .show(ui);
                            if field.changed()
                                && let Some(id) = parse_prestashop_id(prestashop_text)
                            {
                                draft.id_prestashop = id;
                            }
                            ui.end_row();

                            ui.label("Sign-in");
                            controls::toggle(ui, &mut draft.active, "Account is on");
                            ui.end_row();
                        });
                });
                if draft.store == Store::WAR.as_str() && before.store != Store::WAR.as_str() {
                    controls::note(
                        ui,
                        svg::WARNING,
                        "MasterTech builds older than 4.8.5 can't sign in to a WAR account.",
                        t.warn,
                    );
                }
                if before.role().is_none() {
                    let text = format!(
                        "{:?} is not a known role; pick one to replace it.",
                        before.authorization
                    );
                    controls::note(ui, svg::WARNING, &text, t.warn);
                }
                if before.store_code().is_none() {
                    let text = format!(
                        "{:?} is not a known store; pick one to replace it.",
                        before.store
                    );
                    controls::note(ui, svg::WARNING, &text, t.warn);
                }
            });
    }

    fn password_section(
        &mut self,
        ui: &mut Ui,
        ctx: &Context,
        before: &AdminAccount,
        is_self: bool,
    ) {
        let t = Tokens::get(ui.ctx());
        let mut issue = false;
        let mut revoke = false;
        Section::new("user_admin_password", "Password")
            .icon(svg::KEY)
            .default_open(true)
            .show(ui, |ui| {
                if let Some((id, code)) = &self.issued
                    && id == &before.id
                {
                    code_card(ui, code, &before.name);
                }
                let status = self
                    .activity
                    .as_ref()
                    .filter(|(id, _)| id == &before.id)
                    .and_then(|(_, a)| a.reset_code.as_ref());
                match status {
                    Some(status) => {
                        ui.label(RichText::new(code_status_line(status)).weak());
                    }
                    None => {
                        ui.label(RichText::new("No reset codes issued.").weak());
                    }
                }
                if is_self {
                    controls::note(ui, svg::INFO, "Change your own password from Preferences.", t.text_dim);
                    return;
                }
                if !before.active {
                    controls::note(
                        ui,
                        svg::INFO,
                        "Turn the account on first; a turned-off account can't use a code.",
                        t.text_dim,
                    );
                    return;
                }
                ui.horizontal(|ui| {
                    let label = if self.issuing { "Issuing…" } else { "Issue reset code" };
                    issue = TextButton::new(label)
                        .icon(svg::KEY)
                        .kind(ButtonKind::Primary)
                        .enabled(!self.issuing)
                        .show(ui)
                        .clicked();
                    let live = status.is_some_and(|s| s.is_live(chrono::Utc::now()));
                    revoke = TextButton::new("Cancel unused codes")
                        .icon(svg::X)
                        .enabled(live || self.issued.as_ref().is_some_and(|(id, _)| id == &before.id))
                        .show(ui)
                        .clicked();
                });
                ui.label(
                    RichText::new(
                        "A code works once, for 60 minutes, and a newer code replaces it. Read it to them; \
                         they enter it under \"Forgot password?\" on the sign-in screen.",
                    )
                    .weak(),
                );
            });
        if issue {
            self.issue_code(ctx, before.id.clone());
        }
        if revoke {
            self.revoke_codes(ctx, before.id.clone());
        }
    }

    fn tabs_section(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        let root_target = draft.role() == Some(UserAuthorization::Root);
        let hidden_count = draft.hidden().len();
        let suffix = format!("{hidden_count} hidden");
        let others: Vec<(Option<RecordId>, String)> = self
            .accounts
            .iter()
            .filter(|a| a.id != draft.id && a.role() != Some(UserAuthorization::Root))
            .map(|a| {
                let n = a.hidden().len();
                (Some(a.id.clone()), format!("{} ({n} hidden)", a.name))
            })
            .collect();
        let accounts = &self.accounts;
        Section::new("user_admin_tabs", "Tab access")
            .icon(svg::SQUARES_FOUR)
            .suffix(&suffix, None)
            .default_open(true)
            .show(ui, |ui| {
                if root_target {
                    controls::note(ui, svg::CROWN_SIMPLE, "Root accounts always see every tab.", t.text_dim);
                }
                ui.add_enabled_ui(!root_target, |ui| {
                    ui.horizontal(|ui| {
                        if controls::icon_text_button(ui, svg::EYE, "Show all").clicked() {
                            draft.hidden_tabs = Some(Vec::new());
                        }
                        let mut copy_from: Option<RecordId> = None;
                        let options: Vec<(Option<RecordId>, &str)> =
                            others.iter().map(|(id, label)| (id.clone(), label.as_str())).collect();
                        controls::Dropdown::new("user_admin_copy_tabs", &mut copy_from, &options)
                            .placeholder("Copy from another account…")
                            .width(FIELD_W)
                            .show(ui);
                        if let Some(source) = copy_from
                            .and_then(|id| accounts.iter().find(|a| a.id == id))
                        {
                            draft.hidden_tabs = Some(source.hidden().to_vec());
                        }
                    });
                    ui.label(RichText::new("Checked tabs are shown; unchecked tabs are turned off for this account.").weak());
                    let tabs = TabId::hideable();
                    Grid::new("user_admin_tab_grid")
                        .num_columns(TAB_COLUMNS)
                        .show(ui, |ui| {
                            for (i, tab) in tabs.iter().enumerate() {
                                let slug = tab.slug();
                                let mut visible = !draft.hidden().iter().any(|h| h == slug);
                                let title = tab.title(TabContext::MastertechNative);
                                if controls::checkbox(ui, &mut visible, title).changed() {
                                    let list = draft.hidden_tabs.get_or_insert_with(Vec::new);
                                    if visible {
                                        list.retain(|h| h != slug);
                                    } else {
                                        list.push(slug.to_string());
                                    }
                                }
                                if (i + 1) % TAB_COLUMNS == 0 {
                                    ui.end_row();
                                }
                            }
                        });
                });
            });
    }

    fn statuses_section(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        let new_status = &mut self.new_status;
        let count = draft.statuses().len();
        let suffix = format!("{count}");
        Section::new("user_admin_statuses", "Custom statuses")
            .icon(svg::TAG)
            .suffix(&suffix, None)
            .default_open(true)
            .show(ui, |ui| {
                ui.label(
                    RichText::new("Todo, In Repair, Complete, Sales and QC are always available.")
                        .weak(),
                );
                let mut remove: Option<usize> = None;
                for (i, status) in draft.statuses().iter().enumerate() {
                    ui.horizontal(|ui| {
                        if IconButton::new(svg::X, IconBox::Sm)
                            .tip("Remove this status")
                            .tint(t.error)
                            .show(ui)
                            .clicked()
                        {
                            remove = Some(i);
                        }
                        controls::icon_label(ui, svg::TAG, status);
                    });
                }
                if let Some(i) = remove
                    && let Some(list) = draft.user_statuses.as_mut()
                {
                    list.remove(i);
                }
                ui.horizontal(|ui| {
                    let field = TextField::new(new_status)
                        .hint("New status")
                        .width(FIELD_W)
                        .show(ui);
                    let entered = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    let added = controls::icon_text_button(ui, svg::PLUS, "Add").clicked();
                    let text = new_status.trim().to_string();
                    if (entered || added) && !text.is_empty() {
                        draft.user_statuses.get_or_insert_with(Vec::new).push(text);
                        new_status.clear();
                    }
                });
            });
    }

    fn activity_section(&mut self, ui: &mut Ui, ctx: &Context, me: &User, before: &AdminAccount) {
        let (t, m) = (Tokens::get(ui.ctx()), Metrics::get(ui.ctx()));
        let mut move_tasks: Option<RecordId> = None;
        let targets: Vec<(Option<RecordId>, String)> = self
            .accounts
            .iter()
            .filter(|a| a.active && a.id != before.id)
            .map(|a| (Some(a.id.clone()), format!("{} · {}", a.name, a.store)))
            .collect();
        let reassign_to = &mut self.reassign_to;
        let activity = self
            .activity
            .as_ref()
            .filter(|(id, _)| id == &before.id)
            .map(|(_, a)| a);
        let loading = self.activity_loading;
        Section::new("user_admin_activity", "Activity")
            .icon(svg::CLOCK_COUNTER_CLOCKWISE)
            .default_open(true)
            .show(ui, |ui| {
                let Some(activity) = activity else {
                    if loading {
                        controls::loading_row(ui, "Loading activity…");
                    }
                    return;
                };
                Grid::new("user_admin_activity_grid")
                    .num_columns(2)
                    .spacing(m.gap * 1.5)
                    .show(ui, |ui| {
                        ui.label("Last sign-in");
                        ui.label(
                            activity
                                .last_sign_in
                                .as_ref()
                                .map(when)
                                .unwrap_or_else(|| "never recorded".into()),
                        );
                        ui.end_row();

                        ui.label("Failed sign-ins, 7 days");
                        let failed = activity.failed_sign_ins_week;
                        let tone = if failed > 0 { t.warn } else { t.text_dim };
                        ui.label(RichText::new(failed.to_string()).color(tone));
                        ui.end_row();

                        ui.label("Open tasks");
                        ui.horizontal(|ui| {
                            ui.label(activity.open_tasks.to_string());
                            if activity.open_tasks > 0 {
                                let options: Vec<(Option<RecordId>, &str)> = targets
                                    .iter()
                                    .map(|(id, label)| (id.clone(), label.as_str()))
                                    .collect();
                                controls::Dropdown::new(
                                    "user_admin_move_tasks",
                                    reassign_to,
                                    &options,
                                )
                                .placeholder("Move them to…")
                                .width(FIELD_W)
                                .show(ui);
                                let go = TextButton::new("Move")
                                    .icon(svg::ARROWS_LEFT_RIGHT)
                                    .enabled(reassign_to.is_some())
                                    .show(ui)
                                    .clicked();
                                if go {
                                    move_tasks = reassign_to.take();
                                }
                            }
                        });
                        ui.end_row();
                    });
                controls::section_label_icon(ui, svg::DESKTOP_TOWER, "Machines signed in on");
                if activity.devices.is_empty() {
                    ui.label(RichText::new("None recorded.").weak());
                }
                for device in &activity.devices {
                    let name = device
                        .friendly_name
                        .clone()
                        .filter(|n| !n.trim().is_empty())
                        .or_else(|| device.connection_string.clone())
                        .unwrap_or_else(|| "unknown machine".to_string());
                    let seen = device
                        .last_update
                        .as_ref()
                        .map(ago)
                        .unwrap_or_else(|| "never".into());
                    let version = device
                        .client_version
                        .as_deref()
                        .unwrap_or("version unknown");
                    let tone = if device.connected == Some(true) {
                        t.success
                    } else {
                        t.text_faint
                    };
                    ui.horizontal(|ui| {
                        IconLabel::new(svg::CIRCLE, &name).icon_tone(tone).show(ui);
                        ui.label(RichText::new(format!("{version} · seen {seen}")).weak());
                    });
                }
            });
        if let Some(to) = move_tasks {
            self.start_reassign(ctx, me, before.id.clone(), to);
        }
    }

    fn history_section(&self, ui: &mut Ui) {
        let Some(id) = self.selected.as_ref() else {
            return;
        };
        let rows = self
            .activity
            .as_ref()
            .filter(|(activity_id, _)| activity_id == id)
            .map(|(_, a)| a.history.as_slice())
            .unwrap_or_default();
        let suffix = rows.len().to_string();
        Section::new("user_admin_history", "Changes")
            .icon(svg::LIST_BULLETS)
            .suffix(&suffix, None)
            .default_open(false)
            .show(ui, |ui| {
                if rows.is_empty() {
                    ui.label(RichText::new("No changes made from User Admin yet.").weak());
                }
                for row in rows {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(ago(&row.created_at)).weak());
                        ui.label(row.actor_name.as_deref().unwrap_or("someone"));
                        ui.label(history_line(row));
                    });
                }
            });
    }
}

/// The PrestaShop id typed in the form: `Some(None)` when blank, `None` when not a number.
fn parse_prestashop_id(text: &str) -> Option<Option<u64>> {
    let text = text.trim();
    if text.is_empty() {
        return Some(None);
    }
    text.parse::<u64>().ok().map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prestashop_ids_parse_blank_as_none() {
        assert_eq!(parse_prestashop_id(" "), Some(None));
        assert_eq!(parse_prestashop_id(" 48 "), Some(Some(48)));
        assert_eq!(parse_prestashop_id("4x"), None);
    }

    #[test]
    fn history_lines_read_as_changes() {
        let row = AdminActionRow {
            created_at: chrono::Utc::now().into(),
            action: "update".into(),
            detail: json!({ "store": { "old": "RIV", "new": "LTN" }, "active": { "old": true, "new": false } }),
            actor_name: Some("Logan".into()),
        };
        assert_eq!(history_line(&row), "active: on → off; store: RIV → LTN");
        let moved = AdminActionRow {
            action: "reassign_tasks".into(),
            detail: json!({ "count": 3 }),
            ..row
        };
        assert_eq!(history_line(&moved), "moved 3 open tasks");
    }

    #[test]
    fn ages_read_in_minutes_hours_and_days() {
        let now = chrono::Utc::now();
        assert_eq!(ago(&now.into()), "just now");
        assert_eq!(
            ago(&(now - chrono::Duration::minutes(5)).into()),
            "5 min ago"
        );
        assert_eq!(ago(&(now - chrono::Duration::hours(3)).into()), "3 h ago");
        assert_eq!(ago(&(now - chrono::Duration::days(2)).into()), "2 days ago");
    }
}
