//! PNG previews of the User Admin tab and the reset-code card from fixture data.

use database::schema::user::User;
use database::schema::user::admin::{AccountActivity, AccountDevice, AdminAccount, AdminActionRow};
use database::schema::user::employee_profile::profile_from_json;
use database::schema::user::password_reset::{IssuedCode, ResetCodeStatus};
use database::schema::{Datetime, RecordId, UserAuthorization};
use eframe::egui::vec2;
use serde_json::json;
use surrealdb_types::SurrealValue;

use super::{ResetCodeDialog, UserAdmin, code_card};
use crate::ui_tools::headless_render::Headless;
use crate::ui_tools::metrics::Density;
use crate::ui_tools::theme_config::PresetStyles;

fn account(key: &str, name: &str, role: &str, store: &str, active: bool) -> AdminAccount {
    AdminAccount {
        id: RecordId::new("user", key),
        name: name.into(),
        email: format!("{key}@pclaptops.com"),
        active,
        authorization: role.into(),
        store: store.into(),
        id_store: Some("7".into()),
        id_prestashop: Some(100),
        user_statuses: Some(vec!["Waiting on parts".into(), "Customer called".into()]),
        hidden_tabs: Some(vec![
            "stress_lab".into(),
            "terminal".into(),
            "query_editor".into(),
        ]),
    }
}

fn user(key: &str, name: &str, role: UserAuthorization, store: &str) -> User {
    let mut value = User::default().into_value();
    if let surrealdb_types::Value::Object(ref mut row) = value {
        let text = |t: &str| surrealdb_types::Value::String(t.into());
        row.insert("id".to_string(), RecordId::new("user", key).into_value());
        row.insert("authorization".to_string(), text(role.as_str()));
        row.insert("active".to_string(), surrealdb_types::Value::Bool(true));
        row.insert("name".to_string(), text(name));
        row.insert("email".to_string(), text(&format!("{key}@pclaptops.com")));
        row.insert("store".to_string(), text(store));
    }
    User::from_value(value).expect("user decodes")
}

fn root_user() -> User {
    user("avery.root", "Avery Root", UserAuthorization::Root, "RIV")
}

fn minutes_ago(m: i64) -> Datetime {
    (chrono::Utc::now() - chrono::Duration::minutes(m)).into()
}

fn fixture() -> UserAdmin {
    let accounts = vec![
        account("sam.tech", "Sam Tech", "User", "RIV", true),
        account("morgan.mgr", "Morgan Manager", "Manager", "RIV", true),
        account("wren.wh", "Wren Warehouse", "Warehouse", "WAR", true),
        account("old.hand", "Old Hand", "User", "LTN", false),
    ];
    let selected = accounts[0].clone();
    let mut admin = UserAdmin {
        loaded: true,
        selected: Some(selected.id.clone()),
        draft: Some(selected.clone()),
        prestashop_id_text: "100".into(),
        ..UserAdmin::default()
    };
    admin.accounts = accounts;
    if let Some(draft) = admin.draft.as_mut() {
        draft.store = "LTN".into();
    }
    admin.activity = Some((
        selected.id.clone(),
        AccountActivity {
            open_tasks: 3,
            last_sign_in: Some(minutes_ago(42)),
            failed_sign_ins_week: 2,
            devices: vec![AccountDevice {
                connection_string: Some("BENCH-PC-04".into()),
                friendly_name: None,
                client_version: Some("5.0.1".into()),
                last_update: Some(minutes_ago(3)),
                connected: Some(true),
            }],
            reset_code: Some(ResetCodeStatus {
                created_at: minutes_ago(5),
                expires_at: (chrono::Utc::now() + chrono::Duration::minutes(55)).into(),
                used_at: None,
                attempts: 1,
                issued_by_name: Some("Avery Root".into()),
            }),
            history: vec![AdminActionRow {
                created_at: minutes_ago(60),
                action: "update".into(),
                detail: json!({ "store": { "old": "RIV", "new": "LTN" } }),
                actor_name: Some("Avery Root".into()),
            }],
        },
    ));
    admin.issued = Some((
        selected.id.clone(),
        IssuedCode {
            code: "482913".into(),
            expires_at: (chrono::Utc::now() + chrono::Duration::minutes(60)).into(),
        },
    ));
    let employee = json!({
        "id": 100, "firstname": "Sam", "lastname": "Tech", "email": "sam.tech@pclaptops.com",
        "active": "1", "id_store": "8", "id_profile": "4", "last_connection_date": "2026-10-13",
        "passwd": "$2y$10$never-shown"
    });
    admin.profile = Some((
        selected.id.clone(),
        Ok(Some(
            profile_from_json(employee, Some("Salesman".into())).expect("profile"),
        )),
    ));
    admin
}

fn render_dir() -> std::path::PathBuf {
    let dir = std::env::var_os("MTECH_RENDER_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("mtech-renders"));
    std::fs::create_dir_all(&dir).expect("render directory");
    dir
}

/// Writes the tab and the code card; run with `-- --ignored render_user_admin`.
#[test]
#[ignore = "writes PNG previews; set MTECH_RENDER_DIR"]
fn render_user_admin() {
    let dir = render_dir();
    let me = root_user();
    let mut admin = fixture();
    let mut h = Headless::new(vec2(1400.0, 1500.0), 1.0);
    h.theme(PresetStyles::MtechStudio, Density::Regular);
    let (_, image) = h.render(6, |ui| admin.ui(ui, Some(&me)));
    image
        .save(dir.join("user_admin_tab.png"))
        .expect("write png");

    let code = IssuedCode {
        code: "482913".into(),
        expires_at: (chrono::Utc::now() + chrono::Duration::minutes(60)).into(),
    };
    let mut h = Headless::new(vec2(460.0, 220.0), 1.0);
    h.theme(PresetStyles::MtechStudio, Density::Regular);
    let (_, image) = h.render(4, |ui| code_card(ui, &code, "Sam Tech"));
    image
        .save(dir.join("user_admin_code_card.png"))
        .expect("write png");
}

/// Writes the account-menu dialog as a Manager sees it; run with `-- --ignored render_reset_dialog`.
#[test]
#[ignore = "writes PNG previews; set MTECH_RENDER_DIR"]
fn render_reset_dialog() {
    let dir = render_dir();
    let me = user(
        "morgan.mgr",
        "Morgan Manager",
        UserAuthorization::Manager,
        "RIV",
    );
    let staff = vec![
        me.clone(),
        user("sam.tech", "Sam Tech", UserAuthorization::User, "RIV"),
        user(
            "wren.wh",
            "Wren Warehouse",
            UserAuthorization::Warehouse,
            "RIV",
        ),
        user("lee.ltn", "Lee Lehi", UserAuthorization::User, "LTN"),
        user("avery.root", "Avery Root", UserAuthorization::Root, "RIV"),
    ];
    if let Ok(mut users) = database::STORE_USERS.lock() {
        *users = staff;
    }
    let mut dialog = ResetCodeDialog::default();
    dialog.open();
    let mut h = Headless::new(vec2(700.0, 420.0), 1.0);
    h.theme(PresetStyles::MtechStudio, Density::Regular);
    let (_, image) = h.render(6, |ui| dialog.ui(ui.ctx(), Some(&me)));
    image
        .save(dir.join("user_admin_reset_dialog.png"))
        .expect("write png");
    let names: Vec<String> =
        database::schema::user::password_reset::reset_targets(&me, &database::get_database_users())
            .iter()
            .map(|u| u.get_name().to_string())
            .collect();
    assert_eq!(names, ["Sam Tech", "Wren Warehouse"]);
}
