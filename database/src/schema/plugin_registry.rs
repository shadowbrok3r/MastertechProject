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
    pub async fn search(query: &str, tags: Option<&[String]>) -> anyhow::Result<Vec<Self>> {
        let q = format!("%{query}%");
        let entries: Vec<Self> = if let Some(tag_list) = tags {
            DATABASE
                .query(
                    "SELECT * FROM plugin_registry WHERE \
                     (name ~ $q OR description ~ $q OR plugin_id ~ $q OR tools[*].name ~ $q) \
                     AND tags CONTAINSANY $tags \
                     ORDER BY updated_at DESC LIMIT 25"
                )
                .bind(("q", q))
                .bind(("tags", tag_list.to_vec()))
                .await?
                .take(0)?
        } else {
            DATABASE
                .query(
                    "SELECT * FROM plugin_registry WHERE \
                     name ~ $q OR description ~ $q OR plugin_id ~ $q OR tools[*].name ~ $q \
                     ORDER BY updated_at DESC LIMIT 25"
                )
                .bind(("q", q))
                .await?
                .take(0)?
        };
        Ok(entries)
    }

    pub async fn get_by_plugin_id(plugin_id: &str) -> anyhow::Result<Option<Self>> {
        let entry: Option<Self> = DATABASE
            .query("SELECT * FROM plugin_registry WHERE plugin_id == $pid LIMIT 1")
            .bind(("pid", plugin_id.to_string()))
            .await?
            .take(0)?;
        Ok(entry)
    }

    pub async fn upsert(entry: &Self) -> anyhow::Result<()> {
        DATABASE
            .query(
                "IF (SELECT id FROM plugin_registry WHERE plugin_id == $pid) != NONE THEN \
                   UPDATE plugin_registry SET \
                     name = $name, description = $desc, version = $ver, author = $author, \
                     tools = $tools, tags = $tags, wasm_bucket_path = $wasm_path, \
                     source_code = $source, updated_at = time::now() \
                   WHERE plugin_id == $pid \
                 ELSE \
                   CREATE plugin_registry SET \
                     plugin_id = $pid, name = $name, description = $desc, version = $ver, \
                     author = $author, tools = $tools, tags = $tags, \
                     wasm_bucket_path = $wasm_path, source_code = $source, \
                     created_at = time::now(), updated_at = time::now() \
                 END"
            )
            .bind(("pid", entry.plugin_id.clone()))
            .bind(("name", entry.name.clone()))
            .bind(("desc", entry.description.clone()))
            .bind(("ver", entry.version.clone()))
            .bind(("author", entry.author.clone()))
            .bind(("tools", entry.tools.clone()))
            .bind(("tags", entry.tags.clone()))
            .bind(("wasm_path", entry.wasm_bucket_path.clone()))
            .bind(("source", entry.source_code.clone()))
            .await?;
        Ok(())
    }

    pub async fn list_all() -> anyhow::Result<Vec<Self>> {
        let entries: Vec<Self> = DATABASE
            .query("SELECT * FROM plugin_registry ORDER BY updated_at DESC LIMIT 50")
            .await?
            .take(0)?;
        Ok(entries)
    }
}
