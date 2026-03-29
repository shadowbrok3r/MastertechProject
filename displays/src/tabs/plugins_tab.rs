use eframe::egui::{self, Color32, RichText, Ui};
use std::sync::{Arc, Mutex};

#[cfg(not(target_arch = "wasm32"))]
use crate::plugins::PluginManager;

#[cfg(not(target_arch = "wasm32"))]
pub fn plugins_tab_ui(ui: &mut Ui, plugin_manager: &Arc<Mutex<PluginManager>>) {
    ui.heading("Plugin Manager");
    ui.separator();

    let Ok(mut mgr) = plugin_manager.lock() else {
        ui.colored_label(Color32::RED, "Failed to acquire plugin manager lock");
        return;
    };

    let plugins = mgr.list_plugins();

    if plugins.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(RichText::new("No plugins loaded").color(Color32::GRAY).size(16.0));
            ui.add_space(10.0);
            ui.label("Plugins: MCP :9004/mcp — remote_egui_list_widget_anchors, click_anchor, perform_steps, list_targets; plugin_emit_clock_wasm / plugin_compile_wat.");
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
                            RichText::new("●").color(Color32::GREEN)
                        } else {
                            RichText::new("●").color(Color32::RED)
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
                                        .color(Color32::from_rgb(120, 160, 200))
                                        .small(),
                                );
                            }
                        });
                    });

                    if !info.description.is_empty() {
                        ui.label(
                            RichText::new(&info.description)
                                .color(Color32::from_rgb(180, 180, 180))
                                .small(),
                        );
                    }

                    ui.label(
                        RichText::new(format!("ID: {}", info.id))
                            .color(Color32::from_rgb(120, 120, 140))
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
                ("Disable", Color32::from_rgb(200, 100, 100))
            } else {
                ("Enable", Color32::from_rgb(100, 200, 100))
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
