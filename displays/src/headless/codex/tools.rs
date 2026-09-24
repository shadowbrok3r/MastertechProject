//! The Mastertech tools as Codex sees them: dynamic tool specs built from the
//! plugin MCP catalogue, executed in process through an rmcp client wired to a
//! `PluginToolProvider` over an in-memory duplex, so logging, consent gates and
//! provenance behave exactly as for any other MCP caller.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use base64::Engine;
use database::schema::NEVER_REMEMBER_TOOLS;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientInfo, ContentBlock, Implementation, ResourceContents, Tool,
};
use rmcp::service::RunningService;
use rmcp::{RoleClient, ServiceExt};
use serde_json::{json, Value};

use crate::plugins::image_fit;
use crate::plugins::mcp_bridge::PluginToolProvider;
use crate::plugins::PluginManager;

/// Seconds a `remote_exec_wait` keeps between its own timeout and the tool timeout.
const WAIT_MARGIN_SECS: u64 = 15;

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

    /// Runs one tool and renders its result as the text and images the model receives.
    pub async fn call(&self, name: &str, mut arguments: Value, general: bool) -> ToolOutcome {
        if !self.offered(general).iter().any(|t| t == name) {
            return ToolOutcome::failure(format!("tool `{name}` is not available in this session"));
        }
        if !arguments.is_object() {
            arguments = json!({});
        }
        cap_blocking_wait(name, &mut arguments, self.timeout);
        let params: CallToolRequestParams = match serde_json::from_value(json!({ "name": name, "arguments": arguments })) {
            Ok(p) => p,
            Err(e) => return ToolOutcome::failure(format!("bad tool arguments: {e}")),
        };
        match tokio::time::timeout(self.timeout, self.client.call_tool(params)).await {
            Ok(Ok(result)) => self.render(result).await,
            Ok(Err(e)) => ToolOutcome::failure(format!("tool `{name}` failed: {e}")),
            Err(_) => ToolOutcome::failure(format!(
                "tool `{name}` did not finish within {}s; it may still be running",
                self.timeout.as_secs()
            )),
        }
    }

    async fn render(&self, result: CallToolResult) -> ToolOutcome {
        let CallToolResult { content, structured_content, is_error, .. } = result;
        let success = !is_error.unwrap_or(false);
        let (mut parts, images) = split_content(content);
        if parts.is_empty() {
            if let Some(structured) = structured_content.filter(|v| !v.is_null()) {
                parts.push(serde_json::to_string_pretty(&structured).unwrap_or_default());
            }
        }
        let mut text = parts.join("\n");
        let mut fitted = Vec::new();
        for image in fit_images(images).await {
            match image {
                Ok(image) => fitted.push(image),
                Err(note) => text.push_str(&format!("\n[{note}]")),
            }
        }
        if text.trim().is_empty() {
            text = match (success, fitted.is_empty()) {
                (false, _) => "tool returned an error with no message".into(),
                (true, true) => "(no output)".into(),
                (true, false) => "(image output)".into(),
            };
        }
        if text.chars().count() > self.output_chars {
            let kept: String = text.chars().take(self.output_chars).collect();
            text = format!(
                "{kept}\n\n[output truncated at {} characters; ask for a narrower query]",
                self.output_chars
            );
        }
        ToolOutcome { success, text, images: fitted }
    }
}

/// Text blocks, and images as `(base64, mime)`, from a tool result's content.
fn split_content(content: Vec<ContentBlock>) -> (Vec<String>, Vec<(String, String)>) {
    let mut parts = Vec::new();
    let mut images = Vec::new();
    for block in content {
        match block {
            ContentBlock::Text(t) => parts.push(t.text),
            ContentBlock::Image(image) => images.push((image.data, image.mime_type)),
            ContentBlock::Resource(embedded) => match embedded.resource {
                ResourceContents::BlobResourceContents { blob, mime_type: Some(mime), .. }
                    if mime.starts_with("image/") =>
                {
                    images.push((blob, mime))
                }
                other => parts.push(serde_json::to_string(&other).unwrap_or_default()),
            },
            other => parts.push(serde_json::to_string(&other).unwrap_or_default()),
        }
    }
    (parts, images)
}

/// Caps `remote_exec_wait`'s `timeout_secs` below the tool timeout.
fn cap_blocking_wait(name: &str, arguments: &mut Value, budget: Duration) {
    if name != "remote_exec_wait" {
        return;
    }
    let cap = budget.as_secs().saturating_sub(WAIT_MARGIN_SECS).max(1);
    let Some(args) = arguments.as_object_mut() else { return };
    if args.get("timeout_secs").and_then(Value::as_u64).unwrap_or(300) > cap {
        args.insert("timeout_secs".into(), json!(cap));
    }
}

/// Decodes, fits and labels tool images off the async runtime.
async fn fit_images(raw: Vec<(String, String)>) -> Vec<Result<ToolImage, String>> {
    if raw.is_empty() {
        return Vec::new();
    }
    tokio::task::spawn_blocking(move || {
        raw.into_iter()
            .enumerate()
            .map(|(i, (data, mime))| prepare_image(i + 1, &data, &mime))
            .collect()
    })
    .await
    .unwrap_or_else(|e| vec![Err(format!("images could not be prepared: {e}"))])
}

fn prepare_image(n: usize, data: &str, mime: &str) -> Result<ToolImage, String> {
    let engine = base64::engine::general_purpose::STANDARD;
    let bytes = engine.decode(data.trim()).map_err(|e| format!("image {n} is not valid base64: {e}"))?;
    let fitted = image_fit::fit(bytes, mime).map_err(|e| format!("image {n} could not be prepared: {e}"))?;
    Ok(ToolImage {
        label: format!(
            "[image {n}: {}x{} {}, {} KB]",
            fitted.width,
            fitted.height,
            fitted.mime,
            fitted.bytes.len().div_ceil(1024)
        ),
        url: format!("data:{};base64,{}", fitted.mime, engine.encode(&fitted.bytes)),
    })
}

/// One image a tool returned, fitted and encoded for the model.
#[derive(Debug, Clone)]
pub struct ToolImage {
    /// `data:` URL carrying the encoded image.
    pub url: String,
    /// Short text sent next to the image.
    pub label: String,
}

/// What one tool call produced, ready for the codex reply and the transcript.
#[derive(Debug, Clone)]
pub struct ToolOutcome {
    pub success: bool,
    pub text: String,
    pub images: Vec<ToolImage>,
}

impl ToolOutcome {
    pub fn ok(text: String) -> Self {
        Self { success: true, text, images: Vec::new() }
    }

    pub fn failure(text: String) -> Self {
        Self { success: false, text, images: Vec::new() }
    }

    /// The `item/tool/call` response body.
    pub fn response(&self) -> Value {
        self.body(true)
    }

    /// The response as stored in the database, each image reduced to its label.
    pub fn record(&self) -> Value {
        self.body(false)
    }

    fn body(&self, with_images: bool) -> Value {
        let mut items = vec![json!({ "type": "inputText", "text": self.text })];
        for image in &self.images {
            items.push(json!({ "type": "inputText", "text": image.label }));
            if with_images {
                items.push(json!({ "type": "inputImage", "imageUrl": image.url }));
            }
        }
        json!({ "contentItems": items, "success": self.success })
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

    fn tiny_png() -> String {
        use image::{DynamicImage, ImageFormat, RgbImage};
        let mut out = Vec::new();
        DynamicImage::ImageRgb8(RgbImage::new(64, 32))
            .write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)
            .expect("png encodes");
        base64::engine::general_purpose::STANDARD.encode(out)
    }

    #[test]
    fn image_blocks_become_images_and_text_stays_text() {
        let content = vec![ContentBlock::text("{\"image_width\":64}"), ContentBlock::image(tiny_png(), "image/png")];
        let (parts, images) = split_content(content);
        assert_eq!(parts, vec!["{\"image_width\":64}".to_string()]);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].1, "image/png");
    }

    #[test]
    fn a_response_carries_each_image_after_its_label_and_the_record_keeps_only_labels() {
        let image = prepare_image(1, &tiny_png(), "image/png").expect("prepares");
        assert_eq!(image.label, "[image 1: 64x32 image/png, 1 KB]");
        assert!(image.url.starts_with("data:image/png;base64,"));
        let outcome = ToolOutcome { success: true, text: "{\"image_width\":64}".into(), images: vec![image] };
        let response = outcome.response();
        let items = response["contentItems"].as_array().expect("items");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0]["type"], "inputText");
        assert_eq!(items[1]["text"], "[image 1: 64x32 image/png, 1 KB]");
        assert_eq!(items[2]["type"], "inputImage");
        assert!(items[2]["imageUrl"].as_str().is_some_and(|u| u.starts_with("data:image/png;base64,")));
        let record = outcome.record();
        assert_eq!(record["contentItems"].as_array().map(Vec::len), Some(2));
        assert!(!record.to_string().contains("base64"));
    }

    #[test]
    fn a_bad_image_is_reported_instead_of_sent() {
        let err = prepare_image(2, "not base64!", "image/png").unwrap_err();
        assert!(err.starts_with("image 2 is not valid base64"), "{err}");
    }

    #[test]
    fn remote_exec_wait_is_capped_under_the_tool_timeout() {
        let budget = Duration::from_secs(320);
        let mut args = json!({ "job_id": "job-1", "timeout_secs": 900 });
        cap_blocking_wait("remote_exec_wait", &mut args, budget);
        assert_eq!(args["timeout_secs"], 305);
        let mut defaulted = json!({ "job_id": "job-1" });
        cap_blocking_wait("remote_exec_wait", &mut defaulted, Duration::from_secs(120));
        assert_eq!(defaulted["timeout_secs"], 105);
        let mut short = json!({ "timeout_secs": 60 });
        cap_blocking_wait("remote_exec_wait", &mut short, budget);
        assert_eq!(short["timeout_secs"], 60);
        let mut other = json!({ "timeout_secs": 900 });
        cap_blocking_wait("scripts_run_remote", &mut other, budget);
        assert_eq!(other["timeout_secs"], 900);
    }

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
