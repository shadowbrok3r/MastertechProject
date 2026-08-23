//! Overview of the whole graph with the current viewport drawn on it.

use eframe::egui::{
    emath::TSTransform, Color32, Id, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2,
};
use egui_snarl::{NodeId, Snarl};

use super::theme;
use super::viewer::BoardNode;

/// Minimum board area before the map is worth the space it takes.
const MIN_VIEW: f32 = 200.0;
const MARGIN: f32 = 12.0;
/// Graph-space padding so edge nodes are not flush against the frame.
const PAD: f32 = 60.0;

/// Draws the map and returns the graph point a click asked to centre on.
pub fn show(
    ui: &Ui,
    view: Rect,
    to_global: TSTransform,
    nodes: &[(NodeId, Rect)],
    snarl: &Snarl<BoardNode>,
) -> Option<Pos2> {
    if view.width() < MIN_VIEW || view.height() < MIN_VIEW || nodes.is_empty() {
        return None;
    }
    let bounds = nodes
        .iter()
        .map(|(_, r)| *r)
        .reduce(|a, b| a.union(b))?
        .expand(PAD);
    if !bounds.is_finite() || bounds.width() <= 0.0 || bounds.height() <= 0.0 {
        return None;
    }

    // Snarl leaves the ui expanded and its clip rect in graph space, so neither is a
    // usable anchor after `show`; the window rect always is.
    let host = view.intersect(ui.ctx().content_rect());
    if host.width() < MIN_VIEW || host.height() < MIN_VIEW {
        return None;
    }
    let w = (host.width() * 0.22).clamp(120.0, 240.0);
    let h = (w * (bounds.height() / bounds.width()).clamp(0.35, 1.4))
        .clamp(80.0, (host.height() * 0.4).max(80.0));
    let corner = Pos2::new(host.right() - w - MARGIN, host.bottom() - h - MARGIN);

    let mut clicked = None;
    eframe::egui::Area::new(Id::new("session-board-minimap"))
        .order(eframe::egui::Order::Foreground)
        .fixed_pos(corner)
        .constrain_to(host)
        .show(ui.ctx(), |aui| {
            let (resp, p) = aui.allocate_painter(Vec2::new(w, h), Sense::click_and_drag());
            let rect = resp.rect;
            // Marks are derived from graph-space rects; clipping guarantees none of it
            // paints outside the panel whatever the transform does.
            let p = p.with_clip_rect(rect);
            let scale = (rect.size() / bounds.size()).min_elem();
            let tf = TSTransform::new(
                rect.center().to_vec2() - bounds.center().to_vec2() * scale,
                scale,
            );

            p.rect_filled(rect, 4.0, Color32::from_black_alpha(190));

            for (id, node_rect) in nodes {
                let mut m = tf * *node_rect;
                // A node smaller than this is invisible; floor it so the map stays readable.
                if m.width() < 2.0 || m.height() < 2.0 {
                    m = Rect::from_center_size(m.center(), m.size().max(Vec2::new(2.0, 2.0)));
                }
                p.rect_filled(m, 1.0, mark_color(snarl, *id));
            }

            // Screen viewport back into graph space, then into map space.
            let seen = tf * (to_global.inverse() * host);
            p.rect_stroke(
                seen.intersect(rect),
                0.0,
                Stroke::new(1.0, Color32::WHITE),
                StrokeKind::Inside,
            );
            p.rect_stroke(
                rect,
                4.0,
                Stroke::new(1.0, theme::RIM_BRIGHT),
                StrokeKind::Inside,
            );

            // A drag that leaves the map would centre on a wild graph coordinate.
            if resp.clicked() || resp.dragged() {
                if let Some(pointer) = resp.interact_pointer_pos() {
                    if rect.contains(pointer) {
                        clicked = Some(tf.inverse() * pointer);
                    }
                }
            }
        });
    clicked
}

/// Groups read as the structure; open items are the thing you are looking for.
fn mark_color(snarl: &Snarl<BoardNode>, id: NodeId) -> Color32 {
    match snarl.get_node(id) {
        Some(BoardNode::Group(_)) => theme::AQUA.gamma_multiply(0.8),
        Some(BoardNode::Item(i)) if i.status == super::ItemStatus::Open => theme::PINK,
        Some(BoardNode::Item(_)) => Color32::from_gray(110),
        None => Color32::from_gray(90),
    }
}
