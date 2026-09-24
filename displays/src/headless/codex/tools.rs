//! The Mastertech tools as Codex sees them: dynamic tool specs built from the
//! plugin MCP catalogue, executed in process through an rmcp client wired to a
//! `PluginToolProvider` over an in-memory duplex, so logging, consent gates and
//! provenance behave exactly as for any other MCP caller.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use database::schema::NEVER_REMEMBER_TOOLS;
use rmcp::model::{CallToolRequestParams, CallToolResult, ClientInfo, Implementation, Tool};
use rmcp::service::RunningService;
use rmcp::{RoleClient, ServiceExt};
use serde_json::{json, Value};

use crate::plugins::mcp_bridge::PluginToolProvider;
use crate::plugins::PluginManager;

/// Tools the diagnostician role may call; the env `MTECH_CODEX_TOOLS` replaces the list.
pub const DIAGNOSTICIAN_TOOLS: &[&str] = &[
    "query_surrealdb",
    "search_diagnostics",
    "get_diagnostic_session",
    "get_computer_details",
    "get_service_order",
    "search_service_orders",
    "get_customer_details",
    "search_prestashop_orders",
    "search_odoo_inventory",
    "remote_channel_health",
    "telemetry_snapshot_remote",
    "minidump_analyze",
    "crash_intel_search",
    "crash_intel_signature",
    "known_bad_driver_list",
    "driver_snapshots_list",
    "driver_snapshot_diff",
    "driver_snapshot_take",
    "validate_connection_links",
    "scripts_list",
    "list_registry_plugins",
    "fetch_plugin",
    "get_ai_task_status",
    "benchmark_results_query",
    "remote_exec_capabilities",
    "remote_exec_arm",
    "remote_exec_disarm",
    "remote_exec_start",
    "remote_exec_tail",
    "remote_exec_wait",
    "remote_exec_signal",
    "remote_exec_list",
    "create_diagnostic_session",
    "log_diagnostic_entry",
    "set_current_theory",
    "mark_diagnosed",
    "close_diagnostic_session",
    "crash_verdict_record",
    "create_ai_task",
    "add_ai_task_steps",
    "edit_ai_task_item",
    "remove_ai_task_item",
    "repair_entity_links",
    "scripts_run_remote",
    "scripts_run_stress_suite_remote",
    "stress_scenario_run_remote",
    "plugin_deploy_remote",
    "call_remote_plugin_tool",
    "remote_reboot_client",
    "desktop_list_monitors",
    "desktop_focus",
    "desktop_list_windows",
    "desktop_screenshot",
    "desktop_activate_window",
    "desktop_click",
    "desktop_type",
    "desktop_key",
    "desktop_scroll",
];

/// Tools of a session with no machine in scope: records only; `MTECH_CODEX_GENERAL_TOOLS` replaces the list.
pub const GENERAL_TOOLS: &[&str] = &[
    "query_surrealdb",
    "search_diagnostics",
    "get_diagnostic_session",
    "get_computer_details",
    "get_service_order",
    "search_service_orders",
    "get_customer_details",
    "search_prestashop_orders",
    "search_odoo_inventory",
    "crash_intel_search",
    "crash_intel_signature",
    "known_bad_driver_list",
    "driver_snapshots_list",
    "driver_snapshot_diff",
    "validate_connection_links",
    "scripts_list",
    "list_registry_plugins",
    "get_ai_task_status",
    "benchmark_results_query",
    "create_ai_task",
    "add_ai_task_steps",
    "edit_ai_task_item",
    "remove_ai_task_item",
    "repair_entity_links",
];

/// Tools a technician must approve each time; `MTECH_CODEX_PROMPT_TOOLS` replaces the list.
pub const PROMPT_TOOLS: &[&str] = &[
    "remote_exec_start",
    "remote_exec_signal",
    "remote_reboot_client",
    "desktop_screenshot",
    "desktop_click",
    "desktop_type",
    "desktop_key",
    "desktop_scroll",
    "desktop_activate_window",
];

fn env_list(key: &str, default: &[&str]) -> Vec<String> {
    match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => {
            v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
        }
        _ => default.iter().map(|s| s.to_string()).collect(),
    }
}

/// Which tools are offered and which need a human.
#[derive(Debug, Clone)]
pub struct ToolPolicy {
    pub allowed: Vec<String>,
    pub general: Vec<String>,
    pub prompt: Vec<String>,
}

impl ToolPolicy {
    pub fn from_env() -> Self {
        Self {
            allowed: env_list("MTECH_CODEX_TOOLS", DIAGNOSTICIAN_TOOLS),
            general: env_list("MTECH_CODEX_GENERAL_TOOLS", GENERAL_TOOLS),
            prompt: env_list("MTECH_CODEX_PROMPT_TOOLS", PROMPT_TOOLS),
        }
    }

    /// The list a session draws from: machine tools, or records only when no machine is in scope.
    pub fn list_for(&self, general: bool) -> &[String] {
        if general { &self.general } else { &self.allowed }
    }

    pub fn needs_approval(&self, tool: &str) -> bool {
        self.prompt.iter().any(|t| t == tool)
    }

    pub fn may_remember(&self, tool: &str) -> bool {
        !NEVER_REMEMBER_TOOLS.contains(&tool)
    }
}

/// An in-process MCP client bound to its own `PluginToolProvider`.
pub struct ToolHost {
    client: RunningService<RoleClient, ClientInfo>,
    catalog: Vec<Tool>,
    pub policy: ToolPolicy,
    output_chars: usize,
    timeout: Duration,
}

impl ToolHost {
    pub async fn start(
        manager: Arc<RwLock<PluginManager>>,
        policy: ToolPolicy,
        output_chars: usize,
        timeout: Duration,
    ) -> anyhow::Result<Self> {
        let (a, b) = tokio::io::duplex(1 << 20);
        let (ar, aw) = tokio::io::split(a);
        let (br, bw) = tokio::io::split(b);
        let provider = PluginToolProvider::new(manager);
        tokio::spawn(async move {
            match provider.serve((ar, aw)).await {
                Ok(running) => {
                    let _ = running.waiting().await;
                }
                Err(e) => log::warn!("codex: in-process tool server failed to start: {e}"),
            }
        });
        let mut info = ClientInfo::default();
        // The provenance code maps a client name containing `codex` to the codex harness.
        info.client_info = Implementation::new("codex-broker", env!("CARGO_PKG_VERSION"));
        let client = info
            .serve((br, bw))
            .await
            .map_err(|e| anyhow::anyhow!("in-process tool client handshake: {e}"))?;
        let catalog = client
            .list_all_tools()
            .await
            .map_err(|e| anyhow::anyhow!("listing tools: {e}"))?;
        let missing: Vec<&String> = policy
            .allowed
            .iter()
            .chain(policy.general.iter())
            .filter(|name| !catalog.iter().any(|t| t.name == name.as_str()))
            .collect();
        if !missing.is_empty() {
            log::warn!("codex: {} allowed tools are not served by the MCP bridge: {missing:?}", missing.len());
        }
        Ok(Self { client, catalog, policy, output_chars, timeout })
    }

    /// Names actually offered to the model, in policy order.
    pub fn offered(&self, general: bool) -> Vec<String> {
        self.policy
            .list_for(general)
            .iter()
            .filter(|name| self.catalog.iter().any(|t| t.name == name.as_str()))
            .cloned()
            .collect()
    }

    /// `thread/start.dynamicTools` entries for the offered tools.
    pub fn dynamic_specs(&self, general: bool) -> Vec<Value> {
        self.offered(general)
            .into_iter()
            .filter_map(|name| self.catalog.iter().find(|t| t.name == name.as_str()))
            .map(|tool| {
                json!({
                    "type": "function",
                    "name": tool.name,
                    "description": tool.description.as_deref().unwrap_or(""),
                    "inputSchema": Value::Object((*tool.input_schema).clone()),
                    "deferLoading": false,
                })
            })
            .collect()
    }

    /// Runs one tool and renders its result as the text the model receives.
    pub async fn call(&self, name: &str, arguments: Value, general: bool) -> ToolOutcome {
        if !self.offered(general).iter().any(|t| t == name) {
            return ToolOutcome::failure(format!("tool `{name}` is not available in this session"));
        }
        let params: CallToolRequestParams = match serde_json::from_value(json!({
            "name": name,
            "arguments": if arguments.is_object() { arguments } else { json!({}) },
        })) {
            Ok(p) => p,
            Err(e) => return ToolOutcome::failure(format!("bad tool arguments: {e}")),
        };
        match tokio::time::timeout(self.timeout, self.client.call_tool(params)).await {
            Ok(Ok(result)) => self.render(result),
            Ok(Err(e)) => ToolOutcome::failure(format!("tool `{name}` failed: {e}")),
            Err(_) => ToolOutcome::failure(format!(
                "tool `{name}` did not finish within {}s; it may still be running",
                self.timeout.as_secs()
            )),
        }
    }

    fn render(&self, result: CallToolResult) -> ToolOutcome {
        let is_error = result.is_error.unwrap_or(false);
        let raw = serde_json::to_value(&result).unwrap_or(Value::Null);
        let mut parts: Vec<String> = Vec::new();
        if let Some(blocks) = raw.get("content").and_then(Value::as_array) {
            for block in blocks {
                match block.get("text").and_then(Value::as_str) {
                    Some(text) => parts.push(text.to_string()),
                    None => parts.push(serde_json::to_string(block).unwrap_or_default()),
                }
            }
        }
        if let Some(structured) = raw.get("structuredContent").filter(|v| !v.is_null()) {
            if parts.is_empty() {
                parts.push(serde_json::to_string_pretty(structured).unwrap_or_default());
            }
        }
        let mut text = parts.join("\n");
        if text.is_empty() {
            text = if is_error { "tool returned an error with no message".into() } else { "(no output)".into() };
        }
        if text.chars().count() > self.output_chars {
            let kept: String = text.chars().take(self.output_chars).collect();
            text = format!(
                "{kept}\n\n[output truncated at {} characters; ask for a narrower query]",
                self.output_chars
            );
        }
        ToolOutcome { success: !is_error, text, raw: Some(raw) }
    }
}

/// What one tool call produced, ready for the codex reply and the transcript.
#[derive(Debug, Clone)]
pub struct ToolOutcome {
    pub success: bool,
    pub text: String,
    pub raw: Option<Value>,
}

impl ToolOutcome {
    pub fn failure(text: String) -> Self {
        Self { success: false, text, raw: None }
    }

    /// The `item/tool/call` response body.
    pub fn response(&self) -> Value {
        json!({
            "contentItems": [{ "type": "inputText", "text": self.text }],
            "success": self.success,
        })
    }
}

/// Refuses a machine-scoped call aimed at another client; a general session has no
/// machine to guard and relies on its records-only tool list instead.
pub fn scope_violation(arguments: &Value, connection_string: &str) -> Option<String> {
    if super::is_general(connection_string) {
        return None;
    }
    let target = arguments.get("connection_string").and_then(Value::as_str)?;
    if target.trim().eq_ignore_ascii_case(connection_string.trim()) {
        None
    } else {
        Some(format!(
            "this session is scoped to {connection_string}; a call for {target} was refused"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_order_and_inventory_searches_are_offered_without_approval_or_machine_scope() {
        for tool in ["search_prestashop_orders", "search_odoo_inventory"] {
            assert!(DIAGNOSTICIAN_TOOLS.contains(&tool), "{tool} missing from machine sessions");
            assert!(GENERAL_TOOLS.contains(&tool), "{tool} missing from general sessions");
            assert!(!PROMPT_TOOLS.contains(&tool), "{tool} must not need approval");
        }
        let args = json!({ "query": "brendt@example.com", "limit": 5 });
        assert_eq!(scope_violation(&args, "DESKTOP-EOA4FR0:3a1e473a3"), None);
        assert_eq!(scope_violation(&json!({ "query": "RTX 4070" }), "DESKTOP-EOA4FR0:3a1e473a3"), None);
        assert!(scope_violation(&json!({ "connection_string": "OTHER:1" }), "DESKTOP-EOA4FR0:3a1e473a3").is_some());
    }
}
