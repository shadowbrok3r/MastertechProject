//! The selected account's PrestaShop employee record, compared field by field with MasterTech.

use database::schema::user::employee_profile::EmployeeProfile;
use eframe::egui::{Context, Grid, RichText, Ui};

use super::UserAdmin;
use crate::ui_tools::controls::{self, Metrics, Section, SpinnerSize, Tokens};
use crate::ui_tools::icons::svg;

/// A draft edit offered next to a mismatched field.
enum Adopt {
    Id(u64),
    Name(String),
    Email(String),
    Store(String),
    Active(bool),
}

impl UserAdmin {
    pub(super) fn prestashop_section(&mut self, ui: &mut Ui, ctx: &Context) {
        let Some(account) = self.selected_account().cloned() else {
            return;
        };
        let t = Tokens::get(ui.ctx());
        let mut look_up = false;
        let mut adopt: Option<Adopt> = None;
        let result = self
            .profile
            .as_ref()
            .filter(|(id, _)| id == &account.id)
            .map(|(_, result)| result);
        let loading = self.profile_loading;
        let draft = self.draft.as_ref();
        Section::new("user_admin_prestashop", "PrestaShop")
            .icon(svg::STOREFRONT)
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let label = if result.is_some() { "Reload from PrestaShop" } else { "Look up in PrestaShop" };
                    look_up = controls::icon_text_button(ui, svg::DOWNLOAD_SIMPLE, label).clicked();
                    let by = match account.id_prestashop {
                        Some(id) => format!("by employee ID {id}"),
                        None => format!("by email {}", account.email),
                    };
                    ui.label(RichText::new(by).weak());
                    if loading {
                        controls::spinner(ui, SpinnerSize::Sm);
                    }
                });
                match result {
                    None => {}
                    Some(Err(e)) => {
                        controls::note(ui, svg::WARNING_CIRCLE, e, t.error);
                    }
                    Some(Ok(None)) => {
                        controls::note(
                            ui,
                            svg::INFO,
                            "No PrestaShop employee has this email. Link one by typing its ID in the account form.",
                            t.text_dim,
                        );
                    }
                    Some(Ok(Some(profile))) => {
                        if let Some(draft) = draft {
                            adopt = compare(ui, profile, draft);
                        }
                        all_fields(ui, profile);
                    }
                }
            });
        if look_up {
            self.load_profile(ctx, &account);
        }
        if let (Some(change), Some(draft)) = (adopt, self.draft.as_mut()) {
            match change {
                Adopt::Id(id) => {
                    draft.id_prestashop = Some(id);
                    self.prestashop_id_text = id.to_string();
                }
                Adopt::Name(name) => draft.name = name,
                Adopt::Email(email) => draft.email = email,
                Adopt::Store(store) => draft.store = store,
                Adopt::Active(active) => draft.active = active,
            }
        }
    }
}

/// PrestaShop beside MasterTech for the shared fields; returns the "Use" clicked.
fn compare(
    ui: &mut Ui,
    profile: &EmployeeProfile,
    draft: &database::schema::user::admin::AdminAccount,
) -> Option<Adopt> {
    let (t, m) = (Tokens::get(ui.ctx()), Metrics::get(ui.ctx()));
    let record = &profile.record;
    let ps_store = profile.store().map(|s| s.as_str().to_string());
    let yes_no = |b: bool| if b { "active" } else { "inactive" }.to_string();
    let rows: Vec<(&str, String, String, Option<Adopt>)> = vec![
        (
            "Employee ID",
            record.id.to_string(),
            draft
                .id_prestashop
                .map(|id| id.to_string())
                .unwrap_or_else(|| "none".into()),
            (draft.id_prestashop != Some(record.id)).then_some(Adopt::Id(record.id)),
        ),
        (
            "Name",
            record.name(),
            draft.name.clone(),
            (record.name() != draft.name.trim()).then(|| Adopt::Name(record.name())),
        ),
        (
            "Email",
            record.email.clone(),
            draft.email.clone(),
            (record.email != draft.email.trim().to_lowercase())
                .then(|| Adopt::Email(record.email.clone())),
        ),
        (
            "Store",
            ps_store
                .clone()
                .unwrap_or_else(|| format!("id_store {}", record.id_store)),
            draft.store.clone(),
            ps_store
                .clone()
                .filter(|s| s != &draft.store)
                .map(Adopt::Store),
        ),
        (
            "Status",
            yes_no(record.active),
            yes_no(draft.active),
            (record.active != draft.active).then_some(Adopt::Active(record.active)),
        ),
    ];
    let mut picked = None;
    Grid::new("user_admin_prestashop_compare")
        .num_columns(4)
        .spacing(m.gap * 1.5)
        .striped(true)
        .show(ui, |ui| {
            for heading in ["", "PrestaShop", "MasterTech", ""] {
                ui.label(RichText::new(heading).strong());
            }
            ui.end_row();
            for (label, ps, mt, offer) in rows {
                ui.label(label);
                ui.label(RichText::new(&ps).color(t.text));
                match &offer {
                    Some(_) => controls::status_label(ui, svg::WARNING, &mt, t.warn),
                    None => controls::status_label(ui, svg::CHECK_CIRCLE, &mt, t.success),
                };
                match offer {
                    Some(change) => {
                        if controls::icon_text_button(ui, svg::ARROW_LEFT, "Use PrestaShop")
                            .clicked()
                        {
                            picked = Some(change);
                        }
                    }
                    None => {
                        ui.label("");
                    }
                }
                ui.end_row();
            }
            ui.label("Profile");
            ui.label(
                profile
                    .profile_name
                    .clone()
                    .unwrap_or_else(|| "unknown".into()),
            );
            ui.end_row();
            if let Some(last) = profile.field("last_connection_date") {
                ui.label("Last back-office login");
                ui.label(last);
                ui.end_row();
            }
        });
    picked
}

/// Every PrestaShop field, credentials excluded, in a collapsed list.
fn all_fields(ui: &mut Ui, profile: &EmployeeProfile) {
    let m = Metrics::get(ui.ctx());
    let title = format!("All PrestaShop fields ({})", profile.fields.len());
    Section::new("user_admin_prestashop_fields", &title)
        .icon(svg::LIST_BULLETS)
        .default_open(false)
        .show(ui, |ui| {
            Grid::new("user_admin_prestashop_fields_grid")
                .num_columns(2)
                .spacing(m.gap)
                .striped(true)
                .show(ui, |ui| {
                    for (key, value) in &profile.fields {
                        ui.label(RichText::new(key).monospace().weak());
                        let width = ui.available_width().max(m.combo_w);
                        controls::elided_label(ui, value, width);
                        ui.end_row();
                    }
                });
        });
}
