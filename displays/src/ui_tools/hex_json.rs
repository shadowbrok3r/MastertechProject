//! JSON tree for stored dump payloads: kernel addresses as hex, sanitizer
//! markers highlighted.

use eframe::egui::Ui;
use egui_json_tree::{
    render::{DefaultRender, RenderContext},
    value::BaseValueType,
    DefaultExpand, JsonTree, JsonTreeStyle, JsonTreeVisuals,
};
use serde_json::Value;

use crate::ui_tools::{dump_text, icons, theme};

/// House JSON syntax palette, tracking the active theme preset.
pub fn json_visuals(ui: &Ui) -> JsonTreeVisuals {
    let accent = theme::accent(ui);
    JsonTreeVisuals {
        object_key_color: theme::info(ui),
        array_idx_color: theme::weak_text(ui),
        null_color: theme::accent_secondary(ui),
        bool_color: theme::accent_secondary(ui),
        number_color: theme::warn(ui),
        string_color: theme::success(ui),
        highlight_color: eframe::egui::Color32::from_rgba_unmultiplied(
            accent.r(),
            accent.g(),
            accent.b(),
            80,
        ),
        punctuation_color: theme::weak_text(ui),
    }
}

/// Render `value` as a tree. Wide integers show as hex, flagged when a legacy
/// float write lost their low bits; strings carrying `<?n>` markers are tinted.
pub fn dump_json_tree(ui: &mut Ui, id_salt: &str, value: &Value) {
    let visuals = json_visuals(ui);
    let number_color = visuals.number_color;
    let lost_color = theme::error(ui);
    let marker_color = theme::warn(ui);

    JsonTree::new(id_salt, value)
        .style(JsonTreeStyle::new().visuals(visuals))
        .default_expand(DefaultExpand::ToLevel(1))
        .on_render(move |ui, ctx| {
            if let RenderContext::BaseValue(ref base) = ctx {
                match base.value_type {
                    BaseValueType::Number => {
                        if let Some(wide) = dump_text::wide_int(base.value) {
                            let (color, hint) = if wide.is_approximate() {
                                (
                                    lost_color,
                                    "Stored as a float before addresses were hex-encoded; the low bits are lost, so this is within 2048 bytes of the real address.",
                                )
                            } else {
                                (
                                    number_color,
                                    "Recovered exactly: page-aligned, so the float round-trip was lossless.",
                                )
                            };
                            ui.horizontal(|ui| {
                                ui.colored_label(
                                    color,
                                    eframe::egui::RichText::new(wide.hex()).monospace(),
                                )
                                .on_hover_text(hint);
                                if wide.is_approximate() {
                                    ui.colored_label(lost_color, icons::STATUS_WARN)
                                        .on_hover_text(hint);
                                }
                            });
                            return;
                        }
                    }
                    BaseValueType::String => {
                        let raw = base.display_value.to_string();
                        if dump_text::contains_marker(&raw) {
                            ui.colored_label(marker_color, format!("\"{raw}\""))
                                .on_hover_text(
                                    "<?n> stands for n codepoints no bundled font can render — the dump parser produced bytes that are not text.",
                                );
                            return;
                        }
                    }
                    _ => {}
                }
            }
            ctx.render_default(ui);
        })
        .show(ui);
}
