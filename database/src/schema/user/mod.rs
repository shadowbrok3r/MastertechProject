// use structdiff::{Difference, StructDiff};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use surrealdb::RecordId;
use serde_json::Value;

use crate::DATABASE;

use super::{prestashop_schema::{self, Prestashop}, ChatThreads, Status, Store, USER_TABLE};


#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct User {
    id: RecordId,
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
    chat_threads: Option<Vec<ChatThreads>>,
    user_statuses: Option<Vec<Status>>,
    authorization: UserAuthorization
}

impl Default for User {
    fn default() -> Self {
        Self {
            id: RecordId::from((USER_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand()))),
            name: String::new(),
            everest_initials: String::new(),
            email: String::new(),
            store: Store::default(),
            minio_access_key: None,
            minio_secret_key: None,
            user_settings: UserSettings::default(),
            id_store: None,
            id_prestashop: None,
            chat_threads: None,
            user_statuses: None,
            authorization: UserAuthorization::User
        }
    }
}

impl Eq for User {}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default, Eq)]
pub enum UserAuthorization {
    #[default]
    User,
    Admin
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default, Eq)]
pub struct UserSettings {
    color_scheme: Value,
    ui_layout: UiLayout
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default, Eq)]
pub struct UiLayout {
    mtechserver: Value,
    mastertech: Value,
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
}

impl UserAuthorization {
    pub fn as_str(&self) -> &str {
        match self {
            Self::User => "User",
            Self::Admin => "Admin",
        }
    }
    pub fn from_str(authorization: &str) -> Self {
        match authorization {
            "User" => Self::User,
            "Admin" => Self::Admin,
            _ => Self::User
        }
    }
}

impl User {
    pub fn get_id(&self) -> RecordId {
        self.id.clone()
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

    pub fn get_color_scheme(&self) -> Value {
        self.user_settings.color_scheme.clone()
    }

    pub fn get_minio_secret_key(&self) -> Option<String> {
        self.minio_secret_key.clone()
    }

    pub fn get_minio_access_key(&self) -> Option<String> {
        self.minio_access_key.clone()
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

    pub fn set_color_scheme(&mut self, color_scheme: Value) -> &mut Self {
        self.user_settings.color_scheme = color_scheme;
        self
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
            Store::AF => 72,
            Store::WJ => 78,
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
            78 => Store::WJ,
            75 => Store::ORE,
            72 => Store::AF,
            77 => Store::SAN,
            _ => Store::RIV,
        };
        Ok(store)
    }
    
    pub fn add_custom_status(&mut self, _new_status: &str) {
        // if let Status::CustomStatus(ref mut user_statuses) = self {
        //     user_statuses.push(new_status.to_string());
        // }
    }

    pub async fn get_current_user_from_auth() -> anyhow::Result<Option<Self>, anyhow::Error> {
        let user_record: Option<Self> = DATABASE
            .query("SELECT * FROM user WHERE id == $auth.id")
            .await?
            .take(0)?;

        Ok(user_record)
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
                id: RecordId::from((USER_TABLE, emp.id.clone())),
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
}