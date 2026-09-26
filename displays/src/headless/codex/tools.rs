//! The Mastertech tools as Codex sees them: dynamic tool specs built from the
//! plugin MCP catalogue, executed in process through an rmcp client wired to a
//! `PluginToolProvider` over an in-memory duplex, so logging, consent gates and
//! provenance behave exactly as for any other MCP caller.

use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use base64::Engine;
use database::schema::assistant::Person;
use database::schema::NEVER_REMEMBER_TOOLS;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientConfig, ContentBlock, Implementation, ResourceContents, Tool,
};
use rmcp::service::RunningService;
use rmcp::{RoleClient, ServiceExt};
use serde_json::{json, Value};

use crate::plugins::assistant_tools::AssistantCaller;
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
    "ensure_service_task",
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
    "create_task",
    "notify_user",
    "schedule_task",
    "list_task_schedules",
    "cancel_task_schedule",
    "post_ticket_brief",
    "route_part",
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
    "create_task",
    "notify_user",
    "schedule_task",
    "list_task_schedules",
    "cancel_task_schedule",
    "post_ticket_brief",
    "route_part",
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
    "create_task",
    "notify_user",
    "schedule_task",
    "route_part",
];

/// Assistant tools and the argument naming the person they act on.
const PEOPLE_ARGUMENT: &[(&str, &str)] = &[("create_task", "assignee"), ("schedule_task", "assignee"), ("notify_user", "person")];

/// True when an assistant call acts only for the session's requester, or `route_part` only reads stock.
pub fn serves_only_owner(tool: &str, arguments: &Value, owner: Option<&Person>) -> bool {
    if tool == "route_part" {
        return !arguments.get("create_task").and_then(Value::as_bool).unwrap_or(false);
    }
    let Some((_, field)) = PEOPLE_ARGUMENT.iter().find(|(name, _)| *name == tool) else {
        return false;
    };
    let target = arguments.get(*field).and_then(Value::as_str).unwrap_or("").trim().to_lowercase();
    if target.is_empty() || matches!(target.as_str(), "me" | "myself" | "self" | "i") {
        return tool != "notify_user";
    }
    let Some(owner) = owner else { return false };
    let email = owner.email.to_lowercase();
    let username = email.split('@').next().unwrap_or("");
    target == email || target == username || target == owner.name.trim().to_lowercase()
}

/// Approval-card wording for an assistant call.
pub fn assistant_summary(tool: &str, arguments: &Value) -> Option<String> {
    let arg = |k: &str| arguments.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty());
    let who = |k: &str| arg(k).unwrap_or("me").to_string();
    match tool {
        "create_task" => {
            let due = arg("due").map(|d| format!(", due {d}")).unwrap_or_default();
            Some(format!("Create a task for {}: \"{}\"{due}", who("assignee"), arg("title").unwrap_or("")))
        }
        "schedule_task" => {
            let every = arg("every").unwrap_or("once");
            let when = arg("when").map(|w| format!(" {w}")).or_else(|| arg("at").map(|a| format!(" at {a}"))).unwrap_or_default();
            let days = arguments
                .get("weekdays")
                .and_then(Value::as_array)
                .filter(|d| !d.is_empty())
                .map(|d| format!(" on {}", d.iter().map(|v| v.to_string().trim_matches('"').to_string()).collect::<Vec<_>>().join(", ")))
                .unwrap_or_default();
            Some(format!("Schedule for {}: \"{}\" ({every}{days}{when})", who("assignee"), arg("title").unwrap_or("")))
        }
        "notify_user" => Some(format!("Notify {}: \"{}\"", who("person"), arg("message").unwrap_or(""))),
        "route_part" => {
            let qty = arguments.get("quantity").and_then(Value::as_i64).unwrap_or(1);
            let part = arg("part").map(str::to_string).unwrap_or_else(|| {
                arguments.get("product_id").map(|v| format!("product {v}")).unwrap_or_default()
            });
            let sn = arg("service_number").map(|s| format!(" for {s}")).unwrap_or_default();
            Some(format!("Ask another store to send {qty}× {part}{sn}"))
        }
        _ => None,
    }
}

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

    /// [`Self::gate`], except assistant calls that serve only the requester run without asking.
    pub fn gate_for(
        &self,
        tool: &str,
        arguments: &Value,
        approve_all: bool,
        remembered: &HashSet<String>,
        owner: Option<&Person>,
    ) -> Gate {
        if serves_only_owner(tool, arguments, owner) {
            return Gate::Run;
        }
        self.gate(tool, arguments, approve_all, remembered)
    }

    /// How a call gets past the approval gate, given the session's approve-all flag and remembered tools.
    pub fn gate(&self, tool: &str, arguments: &Value, approve_all: bool, remembered: &HashSet<String>) -> Gate {
        if !self.needs_approval(tool) || remembered.contains(tool) {
            Gate::Run
        } else if tool == "remote_exec_start" && declares_read(arguments) {
            Gate::ReadOnlyJob
        } else if approve_all {
            Gate::SessionAllowsAll
        } else {
            Gate::Ask
        }
    }
}

/// How a tool call gets past the approval gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Not gated, or its tool was approved for the session.
    Run,
    /// A `remote_exec_start` whose risk is `read`.
    ReadOnlyJob,
    /// The technician approved every call of the session.
    SessionAllowsAll,
    /// A technician decides.
    Ask,
}

impl Gate {
    pub fn needs_human(self) -> bool {
        self == Self::Ask
    }

    /// Transcript note for a gated call that runs without asking.
    pub fn note(self) -> Option<&'static str> {
        match self {
            Self::ReadOnlyJob => Some("Auto-approved read-only job"),
            Self::SessionAllowsAll => Some("Auto-approved (session allows all)"),
            Self::Run | Self::Ask => None,
        }
    }
}

/// `risk` is `read` in any letter case.
fn declares_read(arguments: &Value) -> bool {
    arguments
        .get("risk")
        .and_then(Value::as_str)
        .is_some_and(|risk| risk.eq_ignore_ascii_case("read"))
}

/// An in-process MCP client bound to its own `PluginToolProvider`.
pub struct ToolHost {
    client: RunningService<RoleClient, ClientConfig>,
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
        caller: Option<AssistantCaller>,
    ) -> anyhow::Result<Self> {
        let (a, b) = tokio::io::duplex(1 << 20);
        let (ar, aw) = tokio::io::split(a);
        let (br, bw) = tokio::io::split(b);
        let provider = PluginToolProvider::for_caller(manager, caller);
        tokio::spawn(async move {
            match provider.serve((ar, aw)).await {
                Ok(running) => {
                    let _ = running.waiting().await;
                }
                Err(e) => log::warn!("codex: in-process tool server failed to start: {e}"),
            }
        });
        let mut info = ClientConfig::default();
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
        let text = fit_text(text, self.output_chars);
        ToolOutcome { success, text, images: fitted }
    }
}

/// `text` within `limit` characters: two-thirds from the start and the rest from the end, around a cut note.
fn fit_text(text: String, limit: usize) -> String {
    let total = text.chars().count();
    if total <= limit {
        return text;
    }
    let head_n = limit * 2 / 3;
    let tail_n = limit - head_n;
    let head: String = text.chars().take(head_n).collect();
    let tail: String = text.chars().skip(total - tail_n).collect();
    format!(
        "{head}\n\n[{} of {total} characters cut from the middle; the start and end are shown. \
         Ask for a narrower query, or have a script save its output to a file and read it in parts.]\n\n{tail}",
        total - head_n - tail_n
    )
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
    fn long_output_keeps_its_start_and_end() {
        assert_eq!(fit_text("short".into(), 100), "short");
        let text = format!("HEAD{}TAIL", "x".repeat(10_000));
        let fitted = fit_text(text, 3_000);
        assert!(fitted.starts_with("HEAD") && fitted.ends_with("TAIL"), "{}", &fitted[..40]);
        assert!(fitted.contains("characters cut from the middle"));
        let kept = fitted.chars().filter(|c| *c == 'x').count();
        assert_eq!(kept, 3_000 - 8, "two-thirds head plus one-third tail fill the limit");
    }

    #[test]
    fn a_machine_session_can_file_the_missing_service_task_without_approval() {
        assert!(DIAGNOSTICIAN_TOOLS.contains(&"ensure_service_task"));
        assert!(!PROMPT_TOOLS.contains(&"ensure_service_task"));
        let args = json!({ "service_number": "2155467", "connection_string": "OTHER:1" });
        assert!(scope_violation(&args, "DESKTOP-787KAB8:8d3db801f").is_some());
    }

    fn policy() -> ToolPolicy {
        let list = |l: &[&str]| l.iter().map(|s| s.to_string()).collect();
        ToolPolicy { allowed: list(DIAGNOSTICIAN_TOOLS), general: list(GENERAL_TOOLS), prompt: list(PROMPT_TOOLS) }
    }

    #[test]
    fn a_read_only_job_runs_without_asking_and_other_jobs_ask() {
        let (p, none) = (policy(), HashSet::new());
        for risk in ["read", "READ", "Read"] {
            let args = json!({ "script": "Get-Date", "risk": risk });
            assert_eq!(p.gate("remote_exec_start", &args, false, &none), Gate::ReadOnlyJob, "{risk}");
        }
        for args in [json!({ "risk": "mutate" }), json!({ "risk": " read" }), json!({ "risk": true }), json!({})] {
            let gate = p.gate("remote_exec_start", &args, false, &none);
            assert!(gate.needs_human(), "{args}");
        }
        assert!(p.gate("remote_reboot_client", &json!({ "risk": "read" }), false, &none).needs_human());
        assert!(p.gate("desktop_click", &json!({ "risk": "read" }), false, &none).needs_human());
    }

    #[test]
    fn approve_all_runs_every_gated_call_and_remembered_tools_run_silently() {
        let p = policy();
        let none = HashSet::new();
        for tool in ["remote_exec_start", "remote_reboot_client", "desktop_click"] {
            let gate = p.gate(tool, &json!({ "risk": "destructive" }), true, &none);
            assert_eq!(gate, Gate::SessionAllowsAll, "{tool}");
            assert!(!gate.needs_human() && gate.note().is_some());
        }
        let remembered: HashSet<String> = ["desktop_click".to_string()].into();
        assert_eq!(p.gate("desktop_click", &json!({}), false, &remembered), Gate::Run);
        assert_eq!(p.gate("query_surrealdb", &json!({}), false, &none), Gate::Run);
        assert_eq!(Gate::Run.note(), None);
        assert_eq!(Gate::ReadOnlyJob.note(), Some("Auto-approved read-only job"));
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

    fn owner() -> Person {
        Person {
            id: database::schema::RecordId::new("user", "sam"),
            name: "Sam Jones".into(),
            email: "sam.jones@pclaptops.com".into(),
            store: "RIV".into(),
            authorization: "User".into(),
            active: true,
        }
    }

    #[test]
    fn assistant_calls_for_the_requester_run_and_others_ask() {
        let (p, none, me) = (policy(), HashSet::new(), owner());
        for tool in ["create_task", "schedule_task", "notify_user", "route_part"] {
            assert!(PROMPT_TOOLS.contains(&tool), "{tool} must be gated");
            assert!(GENERAL_TOOLS.contains(&tool) && DIAGNOSTICIAN_TOOLS.contains(&tool), "{tool} must be offered");
        }
        for target in ["", "me", "Sam Jones", "sam.jones", "SAM.JONES@pclaptops.com"] {
            let args = json!({ "assignee": target, "title": "Count paste" });
            assert_eq!(p.gate_for("create_task", &args, false, &none, Some(&me)), Gate::Run, "{target}");
        }
        let other = json!({ "assignee": "Kim", "title": "Count paste" });
        assert!(p.gate_for("create_task", &other, false, &none, Some(&me)).needs_human());
        assert!(p.gate_for("schedule_task", &json!({ "assignee": "Sam" }), false, &none, Some(&me)).needs_human());
        assert!(p.gate_for("notify_user", &json!({ "person": "me" }), false, &none, Some(&me)).needs_human());
        assert!(p.gate_for("create_task", &json!({ "assignee": "sam.jones" }), false, &none, None).needs_human());
        assert_eq!(p.gate_for("route_part", &json!({ "part": "SSD" }), false, &none, Some(&me)), Gate::Run);
        assert!(p.gate_for("route_part", &json!({ "part": "SSD", "create_task": true }), false, &none, Some(&me)).needs_human());
        for tool in ["post_ticket_brief", "list_task_schedules", "cancel_task_schedule"] {
            assert_eq!(p.gate_for(tool, &json!({}), false, &none, Some(&me)), Gate::Run, "{tool}");
        }
    }

    #[test]
    fn assistant_calls_read_plainly_on_the_approval_card() {
        let task = json!({ "assignee": "Kim", "title": "Count paste", "due": "friday" });
        assert_eq!(assistant_summary("create_task", &task).unwrap(), "Create a task for Kim: \"Count paste\", due friday");
        let weekly = json!({ "assignee": "Kim", "title": "Count paste", "every": "week", "weekdays": ["mon", 4], "at": "10:00" });
        assert_eq!(
            assistant_summary("schedule_task", &weekly).unwrap(),
            "Schedule for Kim: \"Count paste\" (week on mon, 4 at 10:00)"
        );
        let part = json!({ "part": "1TB NVMe", "quantity": 2, "service_number": "2155144", "create_task": true });
        assert_eq!(assistant_summary("route_part", &part).unwrap(), "Ask another store to send 2× 1TB NVMe for 2155144");
        assert_eq!(assistant_summary("query_surrealdb", &json!({})), None);
    }
}
