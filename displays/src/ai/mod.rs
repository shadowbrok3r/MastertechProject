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

/// The user's own OpenAI-compatible endpoint from mcp_settings; None means ZeroClaw serves the call.
pub fn custom_api_base() -> Option<String> {
    mcp_override(|o| o.endpoint.clone())
}

/// Model name: the user's mcp_settings model, else the supplied default.
pub fn effective_model(default: &str) -> String {
    mcp_override(|o| o.model.clone()).unwrap_or_else(|| default.to_string())
}

// region:    --- Modules

#[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
pub mod mcp_chat;
pub mod gpts;

// endregion: --- Modules
