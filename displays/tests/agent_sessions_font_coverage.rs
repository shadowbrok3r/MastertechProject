//! Transcript text the agent sessions tab paints must resolve in both families
//! it renders with: `Proportional` for reply bodies and `Monospace` for tool
//! payloads. Model output arrives with fullwidth token delimiters and emoji
//! that no face in either family carries.
//!
//! Coverage is probed with `glyph_width`, which is 0.0 for a char no face
//! carries — the same technique `tui_font_coverage` uses, and for the same
//! reason: `Fonts::has_glyph` misreports single-face families.

use std::collections::HashMap;

use eframe::egui::{FontFamily, FontId};
use eframe::epaint::text::{Fonts, TextOptions};
use serde_json::json;

/// Every codepoint class observed in live `/api/logs` output.
const SAMPLE: &str = "<｜tool_search｜> ok ✅\u{fe0f} warn ⚠ fail ❌ huh ❓ next → em — en – ellipsis …";

#[test]
fn transcript_text_resolves_in_the_families_the_tab_paints() {
    let logs = json!({"events": [{
        "trace_id": "t1",
        "@timestamp": "2026-09-08T20:00:00Z",
        "message": "turn_final_response",
        "attributes": { "text": SAMPLE },
    }]});
    let sessions = displays::ai::zeroclaw_sessions::sessions_from(&logs, &HashMap::new());
    let text = sessions
        .first()
        .and_then(|s| s.outcome())
        .expect("the sample produced a final answer")
        .to_string();

    assert!(!text.contains('｜'), "fullwidth delimiters survived: {text}");

    let mut fonts = Fonts::new(TextOptions::default(), displays::app_state::font_definitions());
    let mut view = fonts.with_pixels_per_point(1.0);
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        let font_id = FontId::new(14.0, family.clone());
        assert!(view.glyph_width(&font_id, ' ') > 0.0, "{family:?} did not resolve");
        for ch in text.chars().filter(|c| !c.is_whitespace()) {
            assert!(
                view.glyph_width(&font_id, ch) > 0.0,
                "no glyph for U+{:04X} ({ch:?}) in {family:?}; add a fold in \
                 ai::zeroclaw_sessions::renderable",
                ch as u32
            );
        }
    }
}
