use eframe::egui::Ui;

use crate::ui_tools::controls;
use crate::ui_tools::icons::svg;

#[cfg(not(target_arch = "wasm32"))]
use eframe::egui::{self, RichText};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Arc, RwLock};

#[cfg(not(target_arch = "wasm32"))]
use crate::ui_tools::{
    controls::{ButtonKind, Metrics, TextButton},
    fonts,
    glass_card,
    metrics::type_scale,
    theme,
};

#[cfg(not(target_arch = "wasm32"))]
use crate::plugins::PluginManager;

#[cfg(not(target_arch = "wasm32"))]
pub fn plugins_tab_ui(ui: &mut Ui, plugin_manager: &Arc<RwLock<PluginManager>>) {
    let m = Metrics::get(ui.ctx());
    controls::panel_header(ui, svg::MT_PLUGIN, "Plugin Manager", |_| {});

    if controls::section_header(
        ui,
        "plugins_tab_mcp",
        svg::PLUGS_CONNECTED,
        "MCP :9004/mcp — Cursor & remote egui",
    ) {
        ui.label(
            RichText::new("URL: http://127.0.0.1:9004/mcp — after initialize, send notifications/initialized on the same session before tools/call.")
                .small(),
        );
        ui.add_space(m.pad.y);
        ui.label(RichText::new("Remote client UI (admin Web Console connected): list_targets → list_widget_anchors → click_anchor / type, or remote_egui_perform_steps (use sleep_ms between steps).").small());
        ui.label(RichText::new("Switch dock tab via View menu: click_anchor nav.menu.view, sleep ~450ms, then nav.tab.<slug> (e.g. nav.tab.koth, nav.tab.tur_sheet). Slug = lowercase tab name with non-alphanumeric → underscore.").small());
        ui.label(RichText::new("TUR Sheet fields: tur.service_number, tur.customer_name, tur.phone_number, tur.customer_email, tur.salesman, tur.tech, tur.checkin_notes, tur.recommendations.").small());
        ui.add_space(m.pad.y);
        ui.label(
            RichText::new("Full tool list, every View tab with a one-line description, and pitfalls are in the MCP server instructions (initialize response → instructions).")
                .italics()
                .small()
                .color(theme::weak_text(ui)),
        );
    }

    ui.add_space(m.gap.y);

    let mut mgr = match plugin_manager.write() {
        Ok(g) => g,
        Err(e) => {
            let text = format!("Plugin manager lock poisoned: {e}");
            controls::status_label(ui, svg::WARNING_OCTAGON, &text, theme::error(ui));
            return;
        }
    };

    let plugins = mgr.list_plugins();

    if plugins.is_empty() {
        controls::empty_state(
            ui,
            svg::MT_PLUGIN,
            "No plugins loaded",
            "Enable egui frame capture on the machine being viewed; use Plugins MCP tools to author WASM plugins (plugin_emit_clock_wasm / plugin_compile_wat, plugin_deploy).",
            &[],
        );
        return;
    }

    controls::icon_label(
        ui,
        svg::PUZZLE_PIECE,
        &format!("{} plugin(s) registered", plugins.len()),
    );
    ui.add_space(m.gap.y);

    let toggle_ids: Vec<(String, bool)> = plugins.iter().map(|p| (p.id.to_string(), p.enabled)).collect();

    egui::ScrollArea::vertical().show(ui, |ui| {
        for info in &plugins {
            glass_card::group(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (status_tint, status_label) = if info.enabled {
                        (theme::success(ui), "enabled")
                    } else {
                        (theme::error(ui), "disabled")
                    };
                    ui.add(svg::MT_PLUGIN.image(m.icon_md).tint(status_tint));

                    ui.label(
                        RichText::new(&info.name)
                            .font(fonts::semibold(type_scale::HEADING))
                            .color(theme::strong_text(ui)),
                    );
                    ui.label(
                        RichText::new(format!("v{}", info.version))
                            .color(theme::weak_text(ui))
                            .small(),
                    );
                    controls::badge(ui, status_label, status_tint);

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if info.tool_count > 0 {
                            controls::badge(
                                ui,
                                &format!("{} tools", info.tool_count),
                                theme::info(ui),
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

                let id = format!("ID: {}", info.id);
                controls::IconLabel::new(svg::FINGERPRINT, &id)
                    .tone(theme::weak_text(ui))
                    .icon_tone(theme::faint_text(ui))
                    .small(true)
                    .mono(true)
                    .show(ui);
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

    ui.add_space(m.gap.y);
    controls::hairline(ui);
    controls::section_label(ui, "Actions");
    ui.add_space(m.pad.y);

    ui.horizontal_wrapped(|ui| {
        for (id, was_enabled) in &toggle_ids {
            let (btn_text, kind, icon) = if *was_enabled {
                ("Disable", ButtonKind::Danger, svg::STOP)
            } else {
                ("Enable", ButtonKind::Neutral, svg::PLAY)
            };
            let label = format!("{btn_text} {id}");
            if TextButton::new(&label).kind(kind).icon(icon).show(ui).clicked() {
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
    controls::empty_state(
        ui,
        svg::MT_PLUGIN,
        "Plugin management is not available in web mode",
        "",
        &[],
    );
}
