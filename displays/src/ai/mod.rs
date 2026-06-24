pub(crate) const DEFAULT_API_BASE: &str = "https://openrouter.ai/api/v1";

use std::sync::RwLock;

#[derive(Default, Clone)]
struct McpOverride {
    endpoint: Option<String>,
    api_key: Option<String>,
    model: Option<String>,
}

static MCP_OVERRIDE: RwLock<Option<McpOverride>> = RwLock::new(None);

/// Loads the current user's OpenAI-compatible MCP endpoint settings as the active override.
pub fn apply_mcp_settings(user: &database::schema::User) {
    let clean = |s: Option<String>| s.filter(|v| !v.trim().is_empty());
    let over = McpOverride {
        endpoint: clean(user.get_mcp_endpoint()),
        api_key: clean(user.get_mcp_api_key()),
        model: clean(user.get_mcp_model()),
    };
    if let Ok(mut guard) = MCP_OVERRIDE.write() {
        *guard = Some(over);
    }
}

fn mcp_override(pick: impl Fn(&McpOverride) -> Option<String>) -> Option<String> {
    MCP_OVERRIDE.read().ok().and_then(|g| g.as_ref().and_then(&pick))
}

/// API key for OpenAI-compatible calls, from the current user's mcp_settings.
pub fn effective_api_key() -> String {
    mcp_override(|o| o.api_key.clone()).unwrap_or_default()
}

/// API base URL: the user's mcp_settings endpoint, else the default.
pub fn effective_api_base() -> String {
    mcp_override(|o| o.endpoint.clone()).unwrap_or_else(|| DEFAULT_API_BASE.to_string())
}

/// Model name: the user's mcp_settings model, else the supplied default.
pub fn effective_model(default: &str) -> String {
    mcp_override(|o| o.model.clone()).unwrap_or_else(|| default.to_string())
}

// region:    --- Modules

pub mod chat;
pub mod conv;
#[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
pub mod mcp_chat;
pub mod gpts;
pub mod model;
pub mod oa_client;
pub mod tool_call;
pub mod tools;
pub mod utils;

// endregion: --- Modules
