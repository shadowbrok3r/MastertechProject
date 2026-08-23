//! Snarl node rendering for the session board.

use std::collections::{HashMap, HashSet};

use eframe::egui::{Color32, RichText, Ui};
use egui_snarl::ui::{PinInfo, SnarlViewer};
use egui_snarl::{InPin, NodeId, OutPin, Snarl};

use super::data::{Context, GroupKind, Item, ItemKind, ItemStatus};
use super::theme;
use crate::ui_tools::icons;

#[derive(Clone, Debug)]
pub struct GroupNode {
    pub id: String,
    pub kind: GroupKind,
    pub title: String,
    pub project: String,
    pub lane: String,
    pub last_active: String,
    pub has_transcript: bool,
    pub open: usize,
    pub total: usize,
}

#[derive(Clone, Debug)]
pub enum BoardNode {
    Group(GroupNode),
    Item(Item),
}

pub enum BoardAction {
    ToggleContext(String),
    SetStatus(Item, ItemStatus),
    CloseGroup(String),
}

pub struct BoardViewer<'a> {
    pub contexts: &'a HashMap<String, Context>,
    pub expanded: &'a HashSet<String>,
    pub actions: Vec<BoardAction>,
    /// Current graph zoom, fed back by `current_transform` for grid coarsening.
    pub scale: f32,
    /// Wheel delta claimed by the board this frame, applied as zoom.
    pub wheel: f32,
    pub pointer: Option<eframe::egui::Pos2>,
    /// Node rects in graph space. `draw_background` runs before the nodes lay out, so
    /// the frost uses last frame's rects and a resize costs one frame of staleness.
    pub node_rects: &'a mut Vec<(NodeId, eframe::egui::Rect)>,
    /// Screen point graph (0,0) should sit at, when the layout just changed.
    pub recenter: Option<eframe::egui::Pos2>,
    /// Graph point to centre the view on, from a minimap click.
    pub center_on: Option<eframe::egui::Pos2>,
    /// Screen centre of the graph area, for `center_on`.
    pub view_center: eframe::egui::Pos2,
    /// The live graph transform, read back so the minimap can place the viewport box.
    pub to_global: eframe::egui::emath::TSTransform,
}

fn status_color(status: ItemStatus) -> Color32 {
    match status {
        ItemStatus::Open => theme::PINK,
        ItemStatus::Archived => theme::AQUA,
        ItemStatus::Dismissed => Color32::from_gray(110),
        ItemStatus::Filed => theme::VIOLET,
    }
}

fn status_icon(status: ItemStatus) -> &'static str {
    match status {
        ItemStatus::Open => icons::STATUS_QUEUED,
        ItemStatus::Archived => icons::ARCHIVE,
        ItemStatus::Dismissed => icons::STATUS_OFF,
        ItemStatus::Filed => icons::STATUS_ON,
    }
}

/// Wheel units to e-fold of zoom; tuned so one notch is a comfortable step.
const ZOOM_PER_WHEEL_UNIT: f32 = 0.0025;

/// Node body width; without a bound, one long line stretches the whole graph.
pub const NODE_W: f32 = 300.0;

/// Adds a label that wraps instead of widening its node.
fn wrapped(ui: &mut Ui, text: RichText) {
    ui.add(eframe::egui::Label::new(text).wrap());
}

/// Date portion of an RFC3339-ish stamp.
fn day(ts: &str) -> &str {
    ts.split('T').next().unwrap_or(ts)
}

impl SnarlViewer<BoardNode> for BoardViewer<'_> {
    fn title(&mut self, node: &BoardNode) -> String {
        match node {
            BoardNode::Group(g) => g.title.clone(),
            BoardNode::Item(i) => i.subject.clone(),
        }
    }

    fn inputs(&mut self, node: &BoardNode) -> usize {
        match node {
            BoardNode::Group(_) => 0,
            BoardNode::Item(_) => 1,
        }
    }

    fn outputs(&mut self, node: &BoardNode) -> usize {
        match node {
            BoardNode::Group(_) => 1,
            BoardNode::Item(_) => 0,
        }
    }

    fn show_input(
        &mut self,
        pin: &InPin,
        _ui: &mut Ui,
        snarl: &mut Snarl<BoardNode>,
    ) -> impl egui_snarl::ui::SnarlPin + 'static {
        let color = match &snarl[pin.id.node] {
            BoardNode::Item(i) => status_color(i.status),
            BoardNode::Group(_) => Color32::GRAY,
        };
        PinInfo::circle().with_fill(color)
    }

    fn show_output(
        &mut self,
        pin: &OutPin,
        _ui: &mut Ui,
        snarl: &mut Snarl<BoardNode>,
    ) -> impl egui_snarl::ui::SnarlPin + 'static {
        let color = match &snarl[pin.id.node] {
            BoardNode::Group(g) if g.open > 0 => status_color(ItemStatus::Open),
            _ => Color32::GRAY,
        };
        PinInfo::circle().with_fill(color)
    }

    fn show_header(
        &mut self,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<BoardNode>,
    ) {
        match snarl[node].clone() {
            BoardNode::Group(g) => self.group_header(&g, ui),
            BoardNode::Item(i) => {
                ui.horizontal(|ui| {
                    let c = status_color(i.status);
                    ui.label(RichText::new(status_icon(i.status)).color(c));
                    let kind = match i.kind {
                        ItemKind::Task => "task",
                        ItemKind::Suggestion => "follow-up",
                    };
                    ui.label(RichText::new(kind).small().color(theme::AQUA));
                });
            }
        }
    }

    fn node_frame(
        &mut self,
        default: eframe::egui::Frame,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        snarl: &Snarl<BoardNode>,
    ) -> eframe::egui::Frame {
        let frame = default
            .fill(theme::NODE_FILL)
            .corner_radius(theme::NODE_CORNER);
        match &snarl[node] {
            // An open item carries the primary signal on its rim.
            BoardNode::Item(i) if i.status == ItemStatus::Open => {
                frame.stroke(eframe::egui::Stroke::new(1.6, theme::PINK))
            }
            BoardNode::Item(i) => {
                frame.stroke(eframe::egui::Stroke::new(1.0, status_color(i.status).gamma_multiply(0.5)))
            }
            BoardNode::Group(g) if g.open > 0 => {
                frame.stroke(eframe::egui::Stroke::new(1.4, theme::PINK_BRIGHT.gamma_multiply(0.7)))
            }
            BoardNode::Group(_) => frame.stroke(eframe::egui::Stroke::new(1.0, theme::RIM_BRIGHT)),
        }
    }

    fn draw_background(
        &mut self,
        _background: Option<&egui_snarl::ui::BackgroundPattern>,
        viewport: &eframe::egui::Rect,
        _snarl_style: &egui_snarl::ui::SnarlStyle,
        _style: &eframe::egui::Style,
        painter: &eframe::egui::Painter,
        _snarl: &Snarl<BoardNode>,
    ) {
        theme::ambience(painter, *viewport, 4);

        // Dot grid anchored in graph units so it scales 1:1 with the nodes; coarsened by
        // powers of two when zoomed out to bound the dot count.
        let mut spacing = theme::DOT_SPACING;
        while spacing * self.scale.max(0.001) < 26.0 {
            spacing *= 2.0;
        }
        let min_x = (viewport.min.x / spacing).floor() as i64;
        let max_x = (viewport.max.x / spacing).ceil() as i64;
        let min_y = (viewport.min.y / spacing).floor() as i64;
        let max_y = (viewport.max.y / spacing).ceil() as i64;
        if (max_x - min_x).saturating_mul(max_y - min_y) > 6500 {
            return;
        }
        for xi in min_x..=max_x {
            for yi in min_y..=max_y {
                let p = eframe::egui::pos2(xi as f32 * spacing, yi as f32 * spacing);
                painter.circle_filled(p, theme::DOT_RADIUS, theme::DOT_COLOR);
            }
        }

        self.frost_nodes(painter, viewport);
    }

    fn current_transform(
        &mut self,
        to_global: &mut eframe::egui::emath::TSTransform,
        _snarl: &mut Snarl<BoardNode>,
    ) {
        if let Some(anchor) = self.recenter {
            *to_global = eframe::egui::emath::TSTransform::from_translation(anchor.to_vec2());
        }
        if let (true, Some(p)) = (self.wheel != 0.0, self.pointer) {
            let factor = (self.wheel * ZOOM_PER_WHEEL_UNIT).exp();
            let scaling = (to_global.scaling * factor).clamp(theme::MIN_SCALE, theme::MAX_SCALE);
            let factor = scaling / to_global.scaling;
            // Zoom about the cursor: graph -> screen, recentre on p, scale, put p back.
            to_global.translation = (to_global.translation - p.to_vec2()) * factor + p.to_vec2();
            to_global.scaling = scaling;
        }
        if let Some(g) = self.center_on {
            to_global.translation =
                self.view_center.to_vec2() - g.to_vec2() * to_global.scaling;
        }
        self.scale = to_global.scaling;
        self.to_global = *to_global;
    }

    fn final_node_rect(
        &mut self,
        node: NodeId,
        rect: eframe::egui::Rect,
        _ui: &mut Ui,
        _snarl: &mut Snarl<BoardNode>,
    ) {
        self.node_rects.push((node, rect));
    }

    fn has_body(&mut self, _node: &BoardNode) -> bool {
        true
    }

    fn show_body(
        &mut self,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<BoardNode>,
    ) {
        match snarl[node].clone() {
            BoardNode::Group(g) => self.group_body(&g, ui),
            BoardNode::Item(i) => self.item_body(&i, ui),
        }
    }

    fn has_node_menu(&mut self, _node: &BoardNode) -> bool {
        true
    }

    fn show_node_menu(
        &mut self,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<BoardNode>,
    ) {
        match snarl[node].clone() {
            BoardNode::Group(g) => {
                if ui.button(format!("{} Close out all open", icons::ARCHIVE)).clicked() {
                    self.actions.push(BoardAction::CloseGroup(g.id));
                    ui.close();
                }
            }
            BoardNode::Item(i) => {
                if i.status == ItemStatus::Open {
                    if ui.button(format!("{} Archive (done)", icons::CHECK)).clicked() {
                        self.actions
                            .push(BoardAction::SetStatus(i.clone(), ItemStatus::Archived));
                        ui.close();
                    }
                    if ui.button(format!("{} Dismiss", icons::TRASH)).clicked() {
                        self.actions
                            .push(BoardAction::SetStatus(i, ItemStatus::Dismissed));
                        ui.close();
                    }
                } else if ui.button(format!("{} Reopen", icons::UNDO)).clicked() {
                    self.actions
                        .push(BoardAction::SetStatus(i, ItemStatus::Open));
                    ui.close();
                }
            }
        }
    }
}

impl BoardViewer<'_> {
    /// Blurs the lit canvas behind each node body, so the glass has something to reveal.
    fn frost_nodes(&mut self, painter: &eframe::egui::Painter, viewport: &eframe::egui::Rect) {
        let rects = std::mem::take(self.node_rects);
        if self.scale < theme::MIN_FROST_SCALE || rects.is_empty() {
            return;
        }
        // A Ui on the snarl layer, so egui applies the graph transform to the callback rect.
        let ui = Ui::new(
            painter.ctx().clone(),
            eframe::egui::Id::new("session-board-frost"),
            eframe::egui::UiBuilder::new()
                .layer_id(painter.layer_id())
                .max_rect(*viewport),
        );
        let mut ui = ui;
        ui.set_clip_rect(*viewport);
        let mut params = theme::node_glass();
        // Corner radius is in screen points and the transform does not scale it.
        params.corner_radius = theme::NODE_CORNER * self.scale;
        for (_, rect) in rects.iter().take(theme::MAX_FROST_PANES) {
            crate::ui_tools::glass_backdrop::frost_with(&ui, *rect, params);
        }
    }

    fn group_header(&mut self, g: &GroupNode, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let icon = match g.kind {
                GroupKind::Session => icons::CHAT,
                GroupKind::Lane => icons::LIST,
            };
            ui.label(RichText::new(icon).color(ui.visuals().weak_text_color()));
            ui.label(RichText::new(&g.title).size(theme::TITLE_SIZE).strong());
            if g.open > 0 {
                ui.label(
                    RichText::new(format!("{} open", g.open))
                        .small()
                        .color(status_color(ItemStatus::Open)),
                );
            }
        });
    }

    fn group_body(&mut self, g: &GroupNode, ui: &mut Ui) {
        ui.set_width(NODE_W);
        ui.vertical(|ui| self.group_rows(g, ui));
    }

    fn group_rows(&mut self, g: &GroupNode, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            let weak = ui.visuals().weak_text_color();
            if !g.project.is_empty() {
                ui.label(RichText::new(format!("{} {}", icons::FOLDER, g.project)).small().color(weak));
            }
            if !g.lane.is_empty() && g.lane != g.id {
                ui.label(RichText::new(format!("{} {}", icons::TAG, g.lane)).small().color(weak));
            }
            ui.label(RichText::new(day(&g.last_active)).small().color(weak));
            ui.label(
                RichText::new(format!("{}/{} done", g.total - g.open, g.total))
                    .small()
                    .color(weak),
            );
        });

        if !g.has_transcript {
            ui.label(
                RichText::new("no transcript on disk")
                    .small()
                    .italics()
                    .color(ui.visuals().weak_text_color()),
            );
            return;
        }

        let showing = self.expanded.contains(&g.id);
        let label = if showing {
            format!("{} Hide context", icons::CHEV_OPEN)
        } else {
            format!("{} What was this about?", icons::CHEV_CLOSED)
        };
        if ui.button(label).clicked() {
            self.actions.push(BoardAction::ToggleContext(g.id.clone()));
        }

        if !showing {
            return;
        }
        match self.contexts.get(&g.id) {
            Some(ctx) => {
                ui.separator();
                ui.label(
                    RichText::new(format!("{} turns", ctx.turns))
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
                context_block(ui, "Opened with", &ctx.opening);
                context_block(ui, "Ended with", &ctx.closing);
            }
            None => {
                ui.spinner();
            }
        }
    }

    fn item_body(&mut self, i: &Item, ui: &mut Ui) {
        ui.set_width(NODE_W);
        ui.vertical(|ui| self.item_rows(i, ui));
    }

    fn item_rows(&mut self, i: &Item, ui: &mut Ui) {
        wrapped(
            ui,
            RichText::new(&i.subject)
                .size(theme::SUBJECT_SIZE)
                .strong()
                .color(theme::INK),
        );
        if !i.detail.is_empty() && i.detail != i.subject {
            wrapped(
                ui,
                RichText::new(&i.detail)
                    .small()
                    .color(ui.visuals().weak_text_color()),
            );
        }
        ui.horizontal(|ui| {
            if ui
                .small_button(icons::COPY)
                .on_hover_text("Copy this item's text")
                .clicked()
            {
                let text = if i.detail.is_empty() || i.detail == i.subject {
                    i.subject.clone()
                } else {
                    format!("{}

{}", i.subject, i.detail)
                };
                ui.ctx().copy_text(text);
            }
            if i.status == ItemStatus::Open {
                if ui
                    .small_button(icons::CHECK)
                    .on_hover_text("Archive as done")
                    .clicked()
                {
                    self.actions
                        .push(BoardAction::SetStatus(i.clone(), ItemStatus::Archived));
                }
                if ui
                    .small_button(icons::TRASH)
                    .on_hover_text("Dismiss, not worth doing")
                    .clicked()
                {
                    self.actions
                        .push(BoardAction::SetStatus(i.clone(), ItemStatus::Dismissed));
                }
            } else {
                ui.label(
                    RichText::new(i.status.label())
                        .small()
                        .color(status_color(i.status)),
                );
                if ui.small_button(icons::UNDO).on_hover_text("Reopen").clicked() {
                    self.actions
                        .push(BoardAction::SetStatus(i.clone(), ItemStatus::Open));
                }
            }
        });
    }
}

fn context_block(ui: &mut Ui, label: &str, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    ui.add_space(4.0);
    ui.label(
        RichText::new(label)
            .small()
            .strong()
            .color(ui.visuals().weak_text_color()),
    );
    eframe::egui::ScrollArea::vertical()
        .max_height(140.0)
        .max_width(NODE_W)
        .id_salt(label)
        .show(ui, |ui| {
            ui.add(eframe::egui::Label::new(RichText::new(text).small()).wrap());
        });
}
