//! Plot builders shared across tabs.

use eframe::egui::AsId;
use egui_plot::Plot;

/// A plot with every bounds-mutating interaction disabled.
///
/// Five independent paths in `egui_plot` can move the bounds — pan drag, axis
/// drag-zoom, boxed zoom, ctrl+wheel zoom and wheel scroll — so each needs its
/// own flag. With all five off the plot keeps `auto_bounds`, and the wheel
/// delta reaches the enclosing `ScrollArea` instead of panning the chart.
pub fn pinned<'a>(id: impl AsId) -> Plot<'a> {
    Plot::new(id)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_axis_zoom_drag(false)
}

/// [`pinned`] when `interactive` is false, otherwise a plot the operator can pan and zoom.
pub fn maybe_pinned<'a>(id: impl AsId, interactive: bool) -> Plot<'a> {
    if interactive {
        Plot::new(id)
    } else {
        pinned(id)
    }
}
