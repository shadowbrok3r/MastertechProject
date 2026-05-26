use eframe::egui::{self, Color32, RichText, Ui};
use std::sync::{Arc, RwLock};

#[cfg(not(target_arch = "wasm32"))]
use crate::ui_tools::theme;

#[cfg(not(target_arch = "wasm32"))]
use crate::plugins::PluginManager;

#[cfg(not(target_arch = "wasm32"))]
pub fn plugins_tab_ui(ui: &mut Ui, plugin_manager: &Arc<RwLock<PluginManager>>) {
    ui.heading("Plugin Manager");
    ui.separator();

    ui.collapsing(
    RichText::new("MCP :9004/mcp — Cursor & remote egui").strong(),
    |ui| {
        ui.label(
            RichText::new("URL: http://127.0.0.1:9004/mcp — after initialize, send notifications/initialized on the same session before tools/call.")
                .small(),
        );
        ui.add_space(4.0);
        ui.label(RichText::new("Remote client UI (admin Web Console connected): list_targets → list_widget_anchors → click_anchor / type, or remote_egui_perform_steps (use sleep_ms between steps).").small());
        ui.label(RichText::new("Switch dock tab via View menu: click_anchor nav.menu.view, sleep ~450ms, then nav.tab.<slug> (e.g. nav.tab.koth, nav.tab.tur_sheet). Slug = lowercase tab name with non-alphanumeric → underscore.").small());
        ui.label(RichText::new("TUR Sheet fields: tur.service_number, tur.customer_name, tur.phone_number, tur.customer_email, tur.salesman, tur.tech, tur.checkin_notes, tur.recommendations.").small());
        ui.add_space(4.0);
        ui.label(
            RichText::new("Full tool list, every View tab with a one-line description, and pitfalls are in the MCP server instructions (initialize response → instructions).")
                .italics()
                .small()
                .color(theme::weak_text(ui)),
        );
    });
    
    ui.add_space(6.0);

    let mut mgr = match plugin_manager.write() {
        Ok(g) => g,
        Err(e) => {
            ui.colored_label(ui.style().visuals.error_fg_color, format!("Plugin manager lock poisoned: {e}"));
            return;
        }
    };

    let plugins = mgr.list_plugins();

    if plugins.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(RichText::new("No plugins loaded").color(Color32::GRAY).size(16.0));
            ui.add_space(10.0);
            ui.label(RichText::new("Enable egui frame capture on the machine being viewed; use Plugins MCP tools to author WASM plugins (plugin_emit_clock_wasm / plugin_compile_wat, plugin_deploy).").small());
        });
        return;
    }

    ui.label(format!("{} plugin(s) registered", plugins.len()));
    ui.add_space(8.0);

    let toggle_ids: Vec<(String, bool)> = plugins.iter().map(|p| (p.id.to_string(), p.enabled)).collect();

    egui::ScrollArea::vertical().show(ui, |ui| {
        for info in &plugins {
            let frame_color = if info.enabled {
                Color32::from_rgba_premultiplied(20, 40, 30, 180)
            } else {
                Color32::from_rgba_premultiplied(40, 20, 20, 180)
            };

            egui::Frame::new()
                .fill(frame_color)
                .corner_radius(6.0)
                .inner_margin(12.0)
                .outer_margin(egui::Margin::symmetric(0, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let status = if info.enabled {
                            RichText::new("●").color(Color32::LIGHT_GREEN)
                        } else {
                            RichText::new("●").color(ui.style().visuals.error_fg_color)
                        };
                        ui.label(status);

                        ui.label(
                            RichText::new(&info.name)
                                .strong()
                                .size(15.0),
                        );
                        ui.label(
                            RichText::new(format!("v{}", info.version))
                                .color(Color32::GRAY)
                                .small(),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if info.tool_count > 0 {
                                ui.label(
                                    RichText::new(format!("{} tools", info.tool_count))
                                        .color(theme::info(ui))
                                        .small(),
                                );
                            }
                        });
                    });

                    if !info.description.is_empty() {
                        ui.label(
                            RichText::new(&info.description)
                                .color(theme::weak_text(ui))
                                .small(),
                        );
                    }

                    ui.label(
                        RichText::new(format!("ID: {}", info.id))
                            .color(theme::weak_text(ui))
                            .small()
                            .monospace(),
                    );
                });
        }
    });

    drop(plugins);

    let mut changes = Vec::new();
    for (id, was_enabled) in &toggle_ids {
        let new_enabled = !was_enabled;
        let btn_text = if *was_enabled { "Disable" } else { "Enable" };
        let _ = (btn_text, id, new_enabled);
    }

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.label(RichText::new("Actions").strong());
    });

    ui.add_space(4.0);

    ui.horizontal(|ui| {
        for (id, was_enabled) in &toggle_ids {
            let (btn_text, btn_color) = if *was_enabled {
                ("Disable", theme::error(ui))
            } else {
                ("Enable", theme::success(ui))
            };
            let label = format!("{btn_text} {id}");
            if ui
                .button(RichText::new(&label).color(btn_color).small())
                .clicked()
            {
                changes.push((id.clone(), !was_enabled));
            }
        }
    });

    for (id, enable) in changes {
        mgr.set_plugin_enabled(&id, enable);
    }
}

#[cfg(target_arch = "wasm32")]
pub fn plugins_tab_ui_unavailable(ui: &mut Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(60.0);
        ui.label(RichText::new("Plugin management is not available in web mode").size(16.0));
    });
}
