//! Font-atlas telemetry for tracking down intermittent text corruption.
//!
//! epaint rebuilds the entire glyph atlas whenever it passes 80% full
//! (`Fonts::begin_pass`); every glyph's UV changes and the full texture must
//! re-upload that frame. A lost or failed upload leaves stale UVs pointing at
//! blank texture regions — text renders with most glyphs missing. This module
//! logs every rebuild and its trigger conditions so a corruption event can be
//! matched to the atlas timeline in the log/browser console, and provides a
//! Ctrl+Shift+F hotkey that force-reinstalls fonts to heal a desynced atlas.

use eframe::egui::{Context, Id, Key, KeyboardShortcut, Modifiers};

#[derive(Clone, Default)]
struct WatchState {
    initialized: bool,
    last_ratio: f32,
    last_size: [usize; 2],
    last_ppp: f32,
    rebuilds: u32,
    warned_almost_full: bool,
}

const HEAL_SHORTCUT: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::F);

/// Call once per frame. Logs atlas growth, imminent-rebuild warnings, rebuild
/// events, and pixels-per-point changes; Ctrl+Shift+F reinstalls fonts.
pub fn watch(ctx: &Context) {
    let id = Id::new("font_atlas_watch");
    let ratio = ctx.fonts(|f| f.font_atlas_fill_ratio());
    let size = ctx.fonts(|f| f.font_image_size());
    let ppp = ctx.pixels_per_point();

    let mut st: WatchState = ctx.data_mut(|d| d.get_temp(id)).unwrap_or_default();

    if !st.initialized {
        let max_side = ctx.input(|i| i.max_texture_side);
        log::info!(
            "font_atlas_watch: start size={size:?} fill={:.1}% max_texture_side={max_side} ppp={ppp}",
            ratio * 100.0
        );
        st.initialized = true;
    }

    if ppp != st.last_ppp && st.last_ppp != 0.0 {
        log::warn!(
            "font_atlas_watch: pixels_per_point {} -> {ppp} — every glyph re-rasterizes at the new size (atlas churn)",
            st.last_ppp
        );
    }

    // Fill dropping sharply or the texture shrinking means begin_pass recreated the atlas.
    let rebuilt = st.last_size != [0, 0]
        && (size[1] < st.last_size[1] || (st.last_ratio - ratio) > 0.25);
    if rebuilt {
        st.rebuilds += 1;
        st.warned_almost_full = false;
        log::warn!(
            "font_atlas_watch: ATLAS REBUILT (#{}) {:?} {:.1}% -> {size:?} {:.1}% — full texture re-upload this frame; corruption appearing now means the upload was lost",
            st.rebuilds,
            st.last_size,
            st.last_ratio * 100.0,
            ratio * 100.0
        );
    } else if size != st.last_size && st.last_size != [0, 0] {
        log::info!(
            "font_atlas_watch: atlas grew {:?} -> {size:?} (fill {:.1}%)",
            st.last_size,
            ratio * 100.0
        );
    }

    if ratio > 0.8 && !st.warned_almost_full {
        st.warned_almost_full = true;
        log::warn!(
            "font_atlas_watch: atlas {:.1}% full at {size:?} — egui rebuilds it next frame",
            ratio * 100.0
        );
    }

    st.last_ratio = ratio;
    st.last_size = size;
    st.last_ppp = ppp;
    ctx.data_mut(|d| d.insert_temp(id, st));

    if ctx.input_mut(|i| i.consume_shortcut(&HEAL_SHORTCUT)) {
        log::warn!("font_atlas_watch: Ctrl+Shift+F — reinstalling fonts to force a clean atlas + full texture upload");
        crate::app_state::reinstall_custom_fonts(ctx);
    }
}
