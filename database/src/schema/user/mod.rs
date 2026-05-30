use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Display};
use serde_json::Value;
use crate::DATABASE;

use super::{prestashop_schema::{self, Prestashop}, random_record_id, Bytes, RecordId, Status, Store, SurrealValue, USER_TABLE};

pub mod chats;
pub use chats::*;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, SurrealValue)]
pub struct User {
    pub id: RecordId,
    active: bool,
    name: String,
    everest_initials: String,
    email: String,
    store: Store,
    // pub notifications: Option<Vec<NotificationId>>,
    minio_access_key: Option<String>,
    minio_secret_key: Option<String>,
    user_settings: UserSettings,
    id_prestashop: Option<u64>,
    id_store: Option<String>,
    user_statuses: Option<Vec<Status>>,
    authorization: UserAuthorization,
    version: String,
    sales: Option<Vec<RecordId>>,
    #[serde(default)]
    mcp_settings: Option<McpSettings>,
}

impl Default for User {
    fn default() -> Self {
        Self {
            id: random_record_id(USER_TABLE),
            active: false,
            name: String::new(),
            everest_initials: String::new(),
            email: String::new(),
            store: Store::default(),
            minio_access_key: None,
            minio_secret_key: None,
            user_settings: UserSettings::default(),
            id_store: None,
            id_prestashop: None,
            user_statuses: Some(Status::VALUES.to_vec()),
            authorization: UserAuthorization::User,
            version: String::new(),
            sales: None,
            mcp_settings: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default, Eq, SurrealValue)]
pub struct McpSettings {
    pub endpoint: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
}

impl Eq for User {}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default, Eq, SurrealValue)]
#[surreal(untagged)]
pub enum UserAuthorization {
    #[default]
    User,
    Root,
    Manager,
    /// Warehouse floor employees: QC technicians and logistics staff.
    /// Gets the warehouse tab set (fleet health, QC status) instead of the
    /// standard frontline set (tasks, sales, inventory).
    Warehouse,
}


#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default, Eq, SurrealValue)]
pub struct UserSettings {
    color_scheme: Option<Bytes>,
    /// Color scheme serialized egui::Style for desktop/egui environments
    /// Mobile specific color scheme allowing different palette (e.g. Dioxus / CSS usage)
    mobile_color_scheme: Option<Bytes>,
    ui_layout: UiLayout
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default, Eq, SurrealValue)]
pub struct UiLayout {
    mtechserver: Value,
    mastertech: Value,
    task_column_layout: Option<Value>
}

impl UserSettings {
    pub fn get_ui_layout_mastertech(&self) -> Value {
        self.ui_layout.mastertech.clone()
    }

    pub fn get_ui_layout_mtechserver(&self) -> Value {
        self.ui_layout.mtechserver.clone()
    }

    pub fn set_ui_layout_mastertech(&mut self, ui_layout_mastertech: Value) -> &mut Self {
        self.ui_layout.mastertech = ui_layout_mastertech;
        self
    }

    pub fn set_ui_layout_mtechserver(&mut self, ui_layout_mtechserver: Value) -> &mut Self {
        self.ui_layout.mtechserver = ui_layout_mtechserver;
        self
    }

    pub fn get_task_column_layout(&self) -> Option<Value> {
        self.ui_layout.task_column_layout.clone()
    }

    pub fn set_task_column_layout(&mut self, layout: Value) -> &mut Self {
        self.ui_layout.task_column_layout = Some(layout);
        self
    }
}

impl UserAuthorization {
    pub fn as_str(&self) -> &str {
        match self {
            Self::User => "User",
            Self::Root => "Root",
            Self::Manager => "Manager",
            Self::Warehouse => "Warehouse",
        }
    }
    pub fn from_str(authorization: &str) -> Self {
        match authorization {
            "User" => Self::User,
            "Root" => Self::Root,
            "Manager" => Self::Manager,
            "Warehouse" => Self::Warehouse,
            _ => Self::User
        }
    }
}

impl User {
    pub fn get_id(&self) -> RecordId {
        self.id.clone()
    }

    pub fn is_active(&self) -> bool {
        self.active.clone()
    }

    pub fn get_version(&self) -> String {
        self.version.clone()
    }

    pub fn is_admin(&self) -> bool {
        match self.authorization {
            UserAuthorization::User      => false,
            UserAuthorization::Root      => true,
            UserAuthorization::Manager   => false,
            UserAuthorization::Warehouse => false,
        }
    }

    pub fn is_manager(&self) -> bool {
        match self.authorization {
            UserAuthorization::User      => false,
            UserAuthorization::Root      => false,
            UserAuthorization::Manager   => true,
            UserAuthorization::Warehouse => false,
        }
    }

    /// Returns `true` for warehouse floor employees (QC techs, logistics).
    /// This role gets the warehouse tab set in `MtechServer2.0`.
    pub fn is_warehouse(&self) -> bool {
        matches!(self.authorization, UserAuthorization::Warehouse)
    }

    pub fn get_authorization(&self) -> UserAuthorization {
        self.authorization.clone()
    }

    pub fn get_employee_id(&self) -> Option<u64> {
        self.id_prestashop
    }

    pub fn get_store_id(&self) -> Option<String> {
        self.id_store.clone()
    }

    pub fn get_store(&self) -> Store {
        self.store.clone()
    }

    pub fn get_username(&self) -> &str {
        &self.email.split('@').next().unwrap_or(self.email.as_str())
    }

    pub fn get_user_bucket_name(&self) -> String {
        let username = self.get_username();
        let bucket = username.replace('.', "_");
        bucket
    }   

    pub fn get_email(&self) -> &str {
        &self.email
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_initials(&self) -> &str {
        &self.everest_initials
    }

    pub fn get_user_settings(&self) -> UserSettings {
        self.user_settings.clone()
    }

    /// Returns the raw Value of task column layout map (page -> [columns]) if any.
    pub fn get_task_column_layout(&self) -> Option<Value> {
        self.user_settings.get_task_column_layout()
    }

    /// Returns the saved column order for a given page, if present and valid.
    pub fn get_page_task_columns(&self, page: &str) -> Option<Vec<String>> {
        match self.get_task_column_layout() {
            Some(Value::Object(map)) => map.get(page).and_then(|v| {
                if let Value::Array(arr) = v {
                    Some(arr.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
                } else { None }
            }),
            _ => None,
        }
    }

    pub fn get_statuses(&self) -> Vec<Status> {
        let mut statuses = Status::VALUES.to_vec();
        if let Some(custom_statuses) = &self.user_statuses {
            // log::info!("Statuses: {:?}", custom_statuses);
            statuses.extend(custom_statuses.iter().cloned());
        }
        statuses
            .into_iter()
            .filter(|s| !s.as_str().is_empty())
            .collect::<Vec<Status>>()
    }

    pub fn get_custom_statuses(&self) -> Vec<Status> {
       self.user_statuses.clone().unwrap_or_default()
    }

    pub fn get_custom_statuses_mut(&mut self) -> Option<&mut Vec<Status>> {
       self.user_statuses.as_mut()
    }

    pub fn get_color_scheme(&self) -> Vec<u8> {
        if let Some(bytes) = self.user_settings.color_scheme.clone() {
            bytes.to_vec()
        } else {
            Vec::new()
        }
    }

    /// Returns the stored mobile (web/dioxus) color scheme bytes if any
    pub fn get_mobile_color_scheme(&self) -> Vec<u8> {
        if let Some(bytes) = self.user_settings.mobile_color_scheme.clone() {
            bytes.to_vec()
        } else {
            Vec::new()
        }
    }

    pub fn get_minio_secret_key(&self) -> Option<String> {
        self.minio_secret_key.clone()
    }

    pub fn get_minio_access_key(&self) -> Option<String> {
        self.minio_access_key.clone()
    }

    pub fn get_mcp_settings(&self) -> McpSettings {
        self.mcp_settings.clone().unwrap_or_default()
    }

    pub fn get_mcp_endpoint(&self) -> Option<String> {
        self.mcp_settings.as_ref().and_then(|m| m.endpoint.clone())
    }

    pub fn get_mcp_api_key(&self) -> Option<String> {
        self.mcp_settings.as_ref().and_then(|m| m.api_key.clone())
    }

    pub fn get_mcp_model(&self) -> Option<String> {
        self.mcp_settings.as_ref().and_then(|m| m.model.clone())
    }

    pub fn set_mcp_settings(&mut self, mcp_settings: McpSettings) -> &mut Self {
        self.mcp_settings = Some(mcp_settings);
        self
    }

    pub fn set_email(&mut self, email: &str) -> &mut Self {
        self.email = email.to_string();
        self
    }

    pub fn set_user_settings(&mut self, user_settings: UserSettings) -> &mut Self {
        self.user_settings = user_settings;
        self
    }

    pub fn set_ui_layout_mastertech(&mut self, ui_layout_mastertech: Value) -> &mut Self {
        self.user_settings.ui_layout.mastertech = ui_layout_mastertech;
        self
    }

    pub fn set_ui_layout_mtechserver(&mut self, ui_layout_mtechserver: Value) -> &mut Self {
        self.user_settings.ui_layout.mtechserver = ui_layout_mtechserver;
        self
    }

    pub fn set_color_scheme(&mut self, color_scheme: Vec<u8>) -> &mut Self {
        self.user_settings.color_scheme = Some(color_scheme.into());
        self
    }

    /// Sets the mobile (web/dioxus) color scheme bytes.
    pub fn set_mobile_color_scheme(&mut self, color_scheme: Vec<u8>) -> &mut Self {
        self.user_settings.mobile_color_scheme = Some(color_scheme.into());
        self
    }

    pub fn set_statuses(&mut self, status: Status) -> &mut Self {
        self.user_statuses.as_mut().unwrap_or(&mut Status::VALUES.to_vec()).push(status);
        self
    }

    pub async fn get_user_record_from_id(id: RecordId) -> anyhow::Result<Self, anyhow::Error> {
        let user: Option<Self> = DATABASE
            .query("SELECT * FROM user WHERE id == $id")
            .bind(("id", id))
            .await?
            .take(0)?;

        match user {
            Some(user) => Ok(user),
            None => Err(anyhow::anyhow!("User not found")),
        }
    }

    /// Finds and retrieves the associated Employee record based on the User information.
    ///
    /// # Returns
    /// - `Ok(Employee)` on success, where `Employee` is a struct representing the employee record.
    /// - `Err(Error)` if the employee cannot be found or an error occurs during the operation.
    pub async fn find_employee_by_email(&mut self) -> anyhow::Result<prestashop_schema::Employee, anyhow::Error> {
        let api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();

        query.insert("filter[email]", &mut self.email);
        query.insert("output_format", "JSON");

        let employee: prestashop_schema::Employee = api_call
            .find_resource_wasm("employees", query.clone())
            .await?;
        Ok(employee)
    }

    /// Saves the user settings to the database or persistent storage.
    ///
    /// # Returns
    /// - `Ok(())` on success.
    /// - `Err(Error)` if an error occurs while saving the settings.
    pub async fn save_mastertech_ui_layout(&mut self, settings: Value) -> anyhow::Result<(), anyhow::Error>{
        log::info!("helper_traits -> Settings for MASTERTECH: {:?}", settings.clone());
        match DATABASE
            .query("UPDATE $auth.id SET user_settings.ui_layout.mastertech = $settings")
            .bind(("settings", settings))
            .await
        {
            Ok(res) => log::info!("helper_traits -> Result: {res:?}"),
            Err(e) => log::error!("helper_traits -> Error updating User Settings: {e:?}"),
        }
        Ok(())
    }
    
    pub async fn save_version(&mut self, version: impl Display + Serialize + 'static + SurrealValue) -> anyhow::Result<(), anyhow::Error> {
        log::info!("helper_traits -> save_version -> {version}");
        match DATABASE
            .query("UPDATE $auth.id SET version = $version")
            .bind(("version", version))
            .await
        {
            Ok(res) => log::info!("helper_traits -> save_version -> Result: {res:?}"),
            Err(e) => log::error!("helper_traits -> save_version -> Error updating User Settings: {e:?}"),
        }
        Ok(())
    }

    /// Saves the user settings to the database or persistent storage.
    ///
    /// # Returns
    /// - `Ok(())` on success.
    /// - `Err(Error)` if an error occurs while saving the settings.
    pub async fn save_mtechserver_ui_layout(&mut self, settings: Value) -> anyhow::Result<(), anyhow::Error>{
        log::info!("helper_traits -> Settings for MTECHSERVER: {:?}", settings.clone());
        match DATABASE
            .query("UPDATE $auth.id SET user_settings.ui_layout.mtechserver = $settings")
            .bind(("settings", settings))
            .await
        {
            Ok(res) => log::info!("helper_traits -> Result: {res:?}"),
            Err(e) => log::error!("helper_traits -> Error updating User Settings: {e:?}"),
        }
        Ok(())
    }

    /// Saves the task column layout map (page -> [columns]) to the current user's settings.
    /// This method merges the provided page order into existing settings.
    pub async fn save_page_task_columns(&mut self, page: &str, order: Vec<String>) -> anyhow::Result<(), anyhow::Error> {
        // Merge into existing map
        let mut root = match self.get_task_column_layout() {
            Some(Value::Object(map)) => map,
            _ => serde_json::Map::new(),
        };

        root.insert(page.to_string(), serde_json::Value::Array(order.into_iter().map(serde_json::Value::from).collect()));
        let settings = serde_json::Value::Object(root);

        match DATABASE
            .query("UPDATE $auth.id SET user_settings.ui_layout.task_column_layout = $settings")
            .bind(("settings", settings.clone()))
            .await
        {
            Ok(_) => log::info!("helper_traits -> save_page_task_columns Ran Ok"),
            Err(e) => log::error!("helper_traits -> save_page_task_columns -> Error updating User Settings: {e:?}"),
        }
        Ok(())
    }

    /// Retrieves the store number from the Odoo system.
    ///
    /// # Returns
    /// - `Ok(u64)` containing the store number on success.
    /// - `Err(Error)` if the store number cannot be retrieved or an error occurs.
    pub fn get_odoo_store_number(&mut self) -> anyhow::Result<u64, anyhow::Error> {
        let store = match self.store {
            Store::RIV => 76,
            Store::LTN => 73,
            Store::MUR => 74,
            Store::ORE => 75,
            Store::SAN => 77,
        };
        Ok(store)
    }

    /// Retrieves the store details associated with a given Odoo ID.
    ///
    /// # Returns
    /// - `Ok(Store)` containing the store information on success.
    /// - `Err(Error)` if the store cannot be found or an error occurs.
    pub fn get_store_from_odoo_id(&mut self) -> anyhow::Result<Store, anyhow::Error> {
        let store = match self.get_odoo_store_number()? {
            76 => Store::RIV,
            73 => Store::LTN,
            74 => Store::MUR,
            75 => Store::ORE,
            77 => Store::SAN,
            _ => Store::RIV,
        };
        Ok(store)
    }

    pub async fn get_current_user_from_auth() -> anyhow::Result<Option<Self>, anyhow::Error> {
        let user_record: Option<Self> = DATABASE
            .query("SELECT * FROM user WHERE id == $auth.id")
            .await?
            .take(0)?;

        Ok(user_record)
    }

    pub async fn get_users() -> anyhow::Result<Vec<Self>, anyhow::Error> {
        let user_records: Vec<Self> = DATABASE
            .query("SELECT * FROM user ")
            .await?
            .take(0)?;

        Ok(user_records)
    }

    pub async fn query_user_from_email(email: String) -> anyhow::Result<Self, anyhow::Error> {
        let query = if email.contains("checkinshelf") || email.is_empty() {
            "RETURN (SELECT * FROM user WHERE id == $auth.id)"
        } else { "SELECT * FROM user WHERE email == $email" };

        let full_email = if email.ends_with("@pclaptops.com") {
            email.clone()
        } else {
            format!("{}@pclaptops.com", email.clone())
        };

        log::info!("schema/utilities.rs -> Full Email: {full_email}");

        DATABASE.set("email", full_email.clone()).await?;
        let user: Option<Self> = DATABASE.query(query).await?.take(0)?;

        if let Some(usr) = user {
            Ok(usr)
        } else {
            let mut usr = Self::default();
            usr.email = full_email;
            let emp = usr.find_employee_by_email().await?;
            Ok(Self {
                id: RecordId::new(USER_TABLE, emp.id.clone()),
                name: format!("{} {}", emp.firstname, emp.lastname),
                everest_initials: emp.initials,
                email: usr.email,
                store: Store::from_presta_store_id(&emp.id_store),
                id_prestashop: Some(emp.id.parse::<u64>()?),
                id_store: Some(emp.id_store),
                ..Default::default()
            })
        }
    }

    pub async fn add_custom_status(status: &str) -> anyhow::Result<(), anyhow::Error> {
        let _: Option<User> = DATABASE
            .query(r#"
                LET $statuses = $auth.id.user_statuses;
                IF $statuses.is_empty() {
                    UPDATE $auth.id SET user_statuses = array::append($statuses, $status);
                } ELSE {
                    LET $idx = array::find_index($statuses, $status);
                    IF $idx == NONE {
                        UPDATE $auth.id SET user_statuses = array::append($statuses, $status)
                    };
                };
            "#)
            .bind(("status", status.to_string()))
            .await?
            .take(0)?;

        log::info!("user/mod.rs -> Inserted user status");
        Ok(())
    }

    pub async fn remove_custom_status(status: &str) -> anyhow::Result<(), anyhow::Error> {
        let _: Option<User> = DATABASE
            .query(r#"
                LET $idx = array::find_index($auth.id.user_statuses, $status);
                IF $idx != NONE {
                    UPDATE $auth.id SET user_statuses = array::remove($this.user_statuses, $idx)
                }
            "#)
            .bind(("status", status.to_string()))
            .await?
            .take(0)?;

        log::info!("user/mod.rs -> Inserted user status");
        Ok(())
    }

    pub async fn update_color_scheme(color_scheme: Bytes) -> anyhow::Result<(), anyhow::Error> {
        log::error!("color_scheme BYTES: {color_scheme:?}");

        match DATABASE  
            .query("UPDATE $auth.id SET user_settings.color_scheme = $color_scheme")
            .bind(("color_scheme", color_scheme))
            .await 
        {
            Ok(res) => log::info!("Res: {res:?}"),
            Err(e) => log::error!("Error updating User Settings: {e:?}"),
        };

        Ok(())
    }

    /// Update just the mobile color scheme (serialized egui::Style bytes) in the database
    pub async fn update_mobile_color_scheme(color_scheme: Bytes) -> anyhow::Result<(), anyhow::Error> {
        log::info!("mobile_color_scheme BYTES: {color_scheme:?}");
        match DATABASE  
            .query("UPDATE $auth.id SET user_settings.mobile_color_scheme = $color_scheme")
            .bind(("color_scheme", color_scheme))
            .await 
        {
            Ok(res) => log::info!("update_mobile_color_scheme -> Res: {res:?}"),
            Err(e) => log::error!("update_mobile_color_scheme -> Error updating User Settings: {e:?}"),
        };
        Ok(())
    }

    pub async fn save_mcp_settings(settings: McpSettings) -> anyhow::Result<(), anyhow::Error> {
        match DATABASE
            .query("UPDATE $auth.id SET mcp_settings = $settings")
            .bind(("settings", settings))
            .await
        {
            Ok(res) => log::info!("user/mod.rs -> save_mcp_settings -> Result: {res:?}"),
            Err(e) => log::error!("user/mod.rs -> save_mcp_settings -> Error: {e:?}"),
        }
        Ok(())
    }

    /// Upserts one AI-playground chat thread for the current user. `messages` is
    /// the serialized message array; the record id is the thread's local uuid.
    pub async fn save_ai_chat_thread(
        thread_id: &str,
        title: &str,
        messages: serde_json::Value,
    ) -> anyhow::Result<(), anyhow::Error> {
        DATABASE
            .query("UPSERT type::record('ai_chat', $tid) SET user = $auth.id, title = $title, messages = $messages, updated_at = time::now()")
            .bind(("tid", thread_id.to_string()))
            .bind(("title", title.to_string()))
            .bind(("messages", messages))
            .await?;
        Ok(())
    }

    /// Loads the current user's AI-playground chat threads, newest first, as an
    /// array of `{ thread_id, title, messages }` objects.
    pub async fn load_ai_chat_threads() -> anyhow::Result<serde_json::Value, anyhow::Error> {
        let rows: Vec<serde_json::Value> = DATABASE
            .query("SELECT record::id(id) AS thread_id, title, messages, updated_at FROM ai_chat WHERE user = $auth.id ORDER BY updated_at DESC")
            .await?
            .take(0)?;
        Ok(serde_json::Value::Array(rows))
    }

    pub async fn load_user_threads() -> anyhow::Result<Vec<ChatThread>, anyhow::Error> {
        let user_threads: Vec<ChatThread> = DATABASE
            .query("SELECT * FROM chat_thread WHERE user == $auth.id")
            .await?
            .take(0)?;

        Ok(user_threads)
    }
}