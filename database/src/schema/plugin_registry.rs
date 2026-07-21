use crate::db;
use serde::{Deserialize, Serialize};
use super::{Datetime, RecordId, SurrealValue};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct PluginToolInfo {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub parameters_schema: serde_json::Value,
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
    #[serde(default)]
    pub abi_version: Option<u32>,
    // u64 fingerprint bits stored as i64; SurrealDB numbers reject u64 >= 2^63.
    #[serde(default)]
    pub fingerprint: Option<i64>,
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
            abi_version: None,
            fingerprint: None,
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

impl PluginRegistryEntry {
    /// Search the plugin registry by keyword (case-insensitive, multi-token OR match on name, description, plugin_id, and tags).
    pub async fn search(query: &str, tags: Option<&[String]>) -> anyhow::Result<Vec<Self>> {
        // Split into individual tokens so "hw-diag display bsod" matches plugins
        // that contain any of those words, rather than the exact phrase.
        let tokens: Vec<String> = query
            .split_whitespace()
            .map(|t| t.to_lowercase())
            .filter(|t| !t.is_empty())
            .collect();

        if tokens.is_empty() {
            return Self::list_all().await;
        }

        // Build a WHERE clause: each token must be found in at least one field,
        // then all tokens are ANDed so the result is "contains all of these words".
        // This gives useful intersection semantics: "gpu bsod" returns plugins
        // that mention both, not one that only mentions "gpu bsod" as a phrase.
        let token_clause: String = tokens
            .iter()
            .enumerate()
            .map(|(i, _)| format!(
                "(string::lowercase(name) CONTAINS $t{i} \
                  OR string::lowercase(description) CONTAINS $t{i} \
                  OR string::lowercase(plugin_id) CONTAINS $t{i} \
                  OR tags CONTAINS $t{i})"
            ))
            .collect::<Vec<_>>()
            .join(" AND ");

        let tag_clause = if tags.is_some() {
            " AND tags CONTAINSANY $tags"
        } else {
            ""
        };

        let sql = format!(
            "SELECT * FROM plugin_registry WHERE {token_clause}{tag_clause} LIMIT 25"
        );

        let dbh = db();
        let mut q = dbh.query(sql);
        for (i, token) in tokens.iter().enumerate() {
            q = q.bind((format!("t{i}"), token.clone()));
        }
        if let Some(tag_list) = tags {
            q = q.bind(("tags", tag_list.to_vec()));
        }

        let entries: Vec<Self> = q.await?.take(0)?;
        Ok(entries)
    }

    /// Get a plugin registry entry by plugin_id (uses plugin_id as record key).
    pub async fn get_by_plugin_id(plugin_id: &str) -> anyhow::Result<Option<Self>> {
        let rid = RecordId::new(super::PLUGIN_REGISTRY_TABLE, plugin_id);
        let entry: Option<Self> = db().select(rid).await?;
        Ok(entry)
    }

    /// Upsert a plugin registry entry — uses plugin_id as the record key so duplicates
    /// are impossible and lookups are O(1).
    pub async fn upsert(entry: &Self) -> anyhow::Result<()> {
        let rid = RecordId::new(super::PLUGIN_REGISTRY_TABLE, entry.plugin_id.clone());
        let mut e = entry.clone();
        e.id = rid.clone();
        e.updated_at = chrono::Utc::now().into();
        let _: Option<Self> = db().upsert(rid).content(e).await?;
        Ok(())
    }

    /// List all plugins in the registry.
    pub async fn list_all() -> anyhow::Result<Vec<Self>> {
        let entries: Vec<Self> = db()
            .query("SELECT * FROM plugin_registry ORDER BY updated_at DESC LIMIT 50")
            .await?
            .take(0)?;
        Ok(entries)
    }
}
