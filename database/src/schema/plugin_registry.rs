use crate::DATABASE;
use serde::{Deserialize, Serialize};
use super::{Datetime, RecordId, SurrealValue};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct PluginToolInfo {
    pub name: String,
    pub description: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct PluginRegistryEntry {
    pub id: RecordId,
    pub plugin_id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: Option<String>,
    pub tools: Vec<PluginToolInfo>,
    pub tags: Vec<String>,
    pub wasm_bucket_path: Option<String>,
    pub source_code: Option<String>,
    pub created_at: Datetime,
    pub updated_at: Datetime,
}

impl Default for PluginRegistryEntry {
    fn default() -> Self {
        let now: Datetime = chrono::Utc::now().into();
        Self {
            id: super::random_record_id(super::PLUGIN_REGISTRY_TABLE),
            plugin_id: String::new(),
            name: String::new(),
            description: String::new(),
            version: String::from("0.1.0"),
            author: None,
            tools: Vec::new(),
            tags: Vec::new(),
            wasm_bucket_path: None,
            source_code: None,
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

impl PluginRegistryEntry {
    /// Search the plugin registry by keyword (case-insensitive substring match on name, description, plugin_id).
    pub async fn search(query: &str, tags: Option<&[String]>) -> anyhow::Result<Vec<Self>> {
        let q = query.to_lowercase();
        let entries: Vec<Self> = if let Some(tag_list) = tags {
            DATABASE
                .query(
                    "SELECT * FROM plugin_registry
                     WHERE (string::lowercase(name) CONTAINS $q
                         OR string::lowercase(description) CONTAINS $q
                         OR string::lowercase(plugin_id) CONTAINS $q)
                       AND tags CONTAINSANY $tags
                     ORDER BY updated_at DESC LIMIT 25"
                )
                .bind(("q", q))
                .bind(("tags", tag_list.to_vec()))
                .await?
                .take(0)?
        } else {
            DATABASE
                .query(
                    "SELECT * FROM plugin_registry
                     WHERE string::lowercase(name) CONTAINS $q
                        OR string::lowercase(description) CONTAINS $q
                        OR string::lowercase(plugin_id) CONTAINS $q
                     ORDER BY updated_at DESC LIMIT 25"
                )
                .bind(("q", q))
                .await?
                .take(0)?
        };
        Ok(entries)
    }

    /// Get a plugin registry entry by plugin_id (uses plugin_id as record key).
    pub async fn get_by_plugin_id(plugin_id: &str) -> anyhow::Result<Option<Self>> {
        let rid = RecordId::new(super::PLUGIN_REGISTRY_TABLE, plugin_id);
        let entry: Option<Self> = DATABASE.select(rid).await?;
        Ok(entry)
    }

    /// Upsert a plugin registry entry — uses plugin_id as the record key so duplicates
    /// are impossible and lookups are O(1).
    pub async fn upsert(entry: &Self) -> anyhow::Result<()> {
        let rid = RecordId::new(super::PLUGIN_REGISTRY_TABLE, entry.plugin_id.clone());
        let mut e = entry.clone();
        e.id = rid.clone();
        e.updated_at = chrono::Utc::now().into();
        let _: Option<Self> = DATABASE.upsert(rid).content(e).await?;
        Ok(())
    }

    /// List all plugins in the registry.
    pub async fn list_all() -> anyhow::Result<Vec<Self>> {
        let entries: Vec<Self> = DATABASE
            .query("SELECT * FROM plugin_registry ORDER BY updated_at DESC LIMIT 50")
            .await?
            .take(0)?;
        Ok(entries)
    }
}
