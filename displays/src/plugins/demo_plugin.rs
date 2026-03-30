//! Example compiled-in plugin: demonstrates `MastertechPlugin` lifecycle, UI, and MCP tools.
//!
//! Registered from the desktop app on startup. Verify with plugin MCP (`list_plugins`,
//! `call_plugin_tool` with `plugin_id` = [`HelloMastertechPlugin::ID`]).

use super::{MastertechPlugin, PluginHost, PluginToolDescriptor};
use eframe::egui;

/// Stable id for MCP `call_plugin_tool` and enable/disable.
pub const HELLO_PLUGIN_ID: &str = "com.mastertech.demo.hello";

/// Minimal demo plugin with a window, a logic tick counter, and a `demo_ping` MCP tool.
pub struct HelloMastertechPlugin {
    enabled: bool,
    /// Incremented in [`MastertechPlugin::logic`] each enabled frame.
    pub tick: u64,
    window_open: bool,
}

impl Default for HelloMastertechPlugin {
    fn default() -> Self {
        Self {
            enabled: true,
            tick: 0,
            window_open: true,
        }
    }
}

impl HelloMastertechPlugin {
    pub const ID: &'static str = HELLO_PLUGIN_ID;
}

impl MastertechPlugin for HelloMastertechPlugin {
    fn id(&self) -> &'static str {
        HELLO_PLUGIN_ID
    }

    fn name(&self) -> &str {
        "Hello Demo"
    }

    fn version(&self) -> &str {
        "0.1.0"
    }

    fn description(&self) -> &str {
        "Sample Mastertech plugin: floating window, logic ticks, and demo_ping MCP tool."
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn on_load(&mut self, host: &PluginHost) {
        log::info!("HelloMastertechPlugin loaded");
        host.send_notification(
            "Plugins",
            "Hello Demo plugin loaded (see floating window).",
            super::NotificationKind::Info,
        );
    }

    fn on_unload(&mut self) {
        log::info!("HelloMastertechPlugin unloaded");
    }

    fn logic(&mut self, _host: &PluginHost) {
        self.tick = self.tick.saturating_add(1);
    }

    fn ui(&mut self, ui: &mut egui::Ui, host: &PluginHost) {
        egui::Window::new("Hello Mastertech Plugin")
            .open(&mut self.window_open)
            .default_width(320.0)
            .show(ui.ctx(), |ui| {
                ui.label(egui::RichText::new("Demo plugin is running.").strong());
                ui.add_space(8.0);
                ui.label(format!("Logic tick counter: {}", self.tick));
                ui.separator();
                if ui.button("Request repaint").clicked() {
                    host.request_repaint();
                }
                if ui
                    .button("Toast via PluginHost (demo)")
                    .on_hover_text("Uses the global toast channel")
                    .clicked()
                {
                    host.send_notification(
                        "Demo",
                        format!("Tick at send time: {}", self.tick),
                        super::NotificationKind::Success,
                    );
                }
            });
    }

    fn mcp_tools(&self) -> Vec<PluginToolDescriptor> {
        vec![PluginToolDescriptor {
            name: "demo_ping".to_string(),
            description: "Returns pong, optional echo message, and current logic tick.".to_string(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "Optional string to echo back"
                    }
                }
            }),
        }]
    }

    fn handle_mcp_call(
        &mut self,
        tool: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        if tool != "demo_ping" {
            return Err(format!("unknown tool: {tool}"));
        }
        let message = args
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("ping");
        Ok(serde_json::json!({
            "pong": true,
            "echo": message,
            "tick": self.tick,
            "plugin_id": self.id(),
        }))
    }
}
