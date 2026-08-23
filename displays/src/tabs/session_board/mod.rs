//! Node-graph board of Claude Code sessions and the work they left outstanding.

pub mod data;
pub mod minimap;
pub mod theme;
pub mod viewer;

use std::collections::{HashMap, HashSet};

use eframe::egui::{Pos2, RichText, Ui};
use egui_snarl::ui::SnarlStyle;
use egui_snarl::Snarl;

use crate::ui_tools::icons;
use data::{Board, Context, ItemKind, ItemStatus};
use viewer::{BoardAction, BoardNode, BoardViewer, GroupNode};

const COLS: usize = 6;
/// Column pitch: node width plus room for the wire elbow.
const COL_W: f32 = 400.0;
/// Items sit slightly right of their group so the wire reads as a branch.
const ITEM_INDENT: f32 = 40.0;
const BAND_GAP: f32 = 70.0;
const NODE_GAP: f32 = 16.0;

/// Node sizes are in style points, so the layout needs no extra factor.
const LAYOUT_SCALE: f32 = 1.0;

/// Rough node height in unscaled points, from the text each node will wrap.
fn estimated_item_height(item: &data::Item) -> f32 {
    const HEADER: f32 = 26.0;
    const ACTIONS: f32 = 28.0;
    const PAD: f32 = 16.0;
    HEADER + PAD + ACTIONS
        + wrapped_height(&item.subject, 40.0, theme::SUBJECT_SIZE + 4.0)
        + wrapped_height(&item.detail, 52.0, theme::META_SIZE + 3.0)
}

fn estimated_group_height(g: &data::Group, expanded: bool) -> f32 {
    const HEADER: f32 = 26.0;
    const META: f32 = 40.0;
    const BUTTON: f32 = 30.0;
    let context = if expanded { 320.0 } else { 0.0 };
    HEADER + META + BUTTON + context
        + wrapped_height(&g.title, 34.0, theme::TITLE_SIZE + 4.0)
}

/// Height of `text` once wrapped at `chars_per_line`, at `line_h` per line.
fn wrapped_height(text: &str, chars_per_line: f32, line_h: f32) -> f32 {
    if text.trim().is_empty() {
        return 0.0;
    }
    let lines = (text.chars().count() as f32 / chars_per_line).ceil().max(1.0);
    lines * line_h
}

/// Which node kinds the board shows.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum KindFilter {
    #[default]
    All,
    Tasks,
    FollowUps,
}

impl KindFilter {
    fn label(self) -> &'static str {
        match self {
            Self::All => "All kinds",
            Self::Tasks => "Tasks",
            Self::FollowUps => "Follow-ups",
        }
    }

    fn accepts(self, kind: ItemKind) -> bool {
        match self {
            Self::All => true,
            Self::Tasks => kind == ItemKind::Task,
            Self::FollowUps => kind == ItemKind::Suggestion,
        }
    }
}

/// Which item statuses the board shows.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum StatusFilter {
    #[default]
    Open,
    All,
    Archived,
    Dismissed,
    Filed,
}

impl StatusFilter {
    fn label(self) -> &'static str {
        match self {
            Self::Open => "Open only",
            Self::All => "Any status",
            Self::Archived => "Archived",
            Self::Dismissed => "Dismissed",
            Self::Filed => "Filed",
        }
    }

    fn accepts(self, status: ItemStatus) -> bool {
        match self {
            Self::All => true,
            Self::Open => status == ItemStatus::Open,
            Self::Archived => status == ItemStatus::Archived,
            Self::Dismissed => status == ItemStatus::Dismissed,
            Self::Filed => status == ItemStatus::Filed,
        }
    }
}

pub struct SessionBoard {
    snarl: Snarl<BoardNode>,
    style: SnarlStyle,
    board: Board,
    contexts: HashMap<String, Context>,
    expanded: HashSet<String>,
    filter: String,
    kind: KindFilter,
    status_filter: StatusFilter,
    /// `None` shows every lane.
    lane: Option<String>,
    status: String,
    built: bool,
    scale: f32,
    /// Node rects from the previous frame, consumed by the frost pass.
    node_rects: Vec<(egui_snarl::NodeId, eframe::egui::Rect)>,
    /// Set when the layout changed, so the view returns to the first band.
    recenter: bool,
    /// Graph point a minimap click asked to centre on, applied next frame.
    center_on: Option<Pos2>,
    to_global: eframe::egui::emath::TSTransform,
    show_map: bool,
}

impl Default for SessionBoard {
    fn default() -> Self {
        Self {
            snarl: Snarl::new(),
            style: theme::style(),
            board: Board::default(),
            contexts: HashMap::new(),
            expanded: HashSet::new(),
            filter: String::new(),
            kind: KindFilter::default(),
            status_filter: StatusFilter::default(),
            lane: None,
            status: String::new(),
            built: false,
            scale: 1.0,
            node_rects: Vec::new(),
            recenter: true,
            center_on: None,
            to_global: eframe::egui::emath::TSTransform::IDENTITY,
            show_map: true,
        }
    }
}

impl SessionBoard {
    pub fn ui(&mut self, ui: &mut Ui) {
        theme::apply(ui);
        if !self.built {
            self.reload();
        }
        self.toolbar(ui);
        ui.separator();

        // Plain wheel zooms the graph. Taking the delta here and zeroing it stops
        // egui's Scene from also panning by the same amount.
        let wheel = if ui.rect_contains_pointer(ui.available_rect_before_wrap()) {
            ui.input_mut(|i| {
                let d = i.smooth_scroll_delta.y;
                if d != 0.0 && !i.modifiers.any() {
                    i.smooth_scroll_delta = eframe::egui::Vec2::ZERO;
                    d
                } else {
                    0.0
                }
            })
        } else {
            0.0
        };
        let pointer = ui.input(|i| i.pointer.latest_pos());
        let view = ui.available_rect_before_wrap();
        let anchor = view.min + eframe::egui::vec2(24.0, 24.0);

        let mut viewer = BoardViewer {
            contexts: &self.contexts,
            expanded: &self.expanded,
            actions: Vec::new(),
            scale: self.scale,
            wheel,
            pointer,
            node_rects: &mut self.node_rects,
            recenter: self.recenter.then_some(anchor),
            center_on: self.center_on.take(),
            view_center: view.center(),
            to_global: self.to_global,
        };
        self.snarl.show(&mut viewer, &self.style, "session_board", ui);
        self.scale = viewer.scale;
        self.to_global = viewer.to_global;
        let actions = std::mem::take(&mut viewer.actions);
        // Releases the borrow on `node_rects` so the map can read them.
        drop(viewer);

        self.recenter = false;
        if self.show_map {
            self.center_on =
                minimap::show(ui, view, self.to_global, &self.node_rects, &self.snarl);
        }
        for action in actions {
            self.apply(action);
        }
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(format!("{} Reload", icons::REFRESH))
                .on_hover_text("Re-read task lists and the captured inbox")
                .clicked()
            {
                self.reload();
            }
            ui.separator();
            ui.label(format!("{} Filter", icons::SEARCH));
            let mut changed = ui
                .add(
                    eframe::egui::TextEdit::singleline(&mut self.filter)
                        .desired_width(170.0)
                        .hint_text("session, project, or text"),
                )
                .changed();
            changed |= self.kind_combo(ui);
            changed |= self.status_combo(ui);
            changed |= self.lane_combo(ui);
            if changed {
                self.rebuild();
            }
            ui.separator();
            let open = self.board.open_total();
            ui.label(
                RichText::new(format!("{} {open} open", icons::STATUS_QUEUED))
                    .color(ui.visuals().warn_fg_color),
            );
            ui.label(
                RichText::new(format!("across {} sessions", self.board.groups.len()))
                    .small()
                    .color(ui.visuals().weak_text_color()),
            );
            if !self.status.is_empty() {
                ui.separator();
                ui.label(
                    RichText::new(&self.status)
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            }
        });
    }

    /// Counts every item matching the current status filter, by kind.
    fn kind_counts(&self) -> (usize, usize) {
        let mut tasks = 0;
        let mut follow_ups = 0;
        for item in self.board.groups.iter().flat_map(|g| g.items.iter()) {
            if !self.status_filter.accepts(item.status) {
                continue;
            }
            match item.kind {
                ItemKind::Task => tasks += 1,
                ItemKind::Suggestion => follow_ups += 1,
            }
        }
        (tasks, follow_ups)
    }

    fn kind_combo(&mut self, ui: &mut Ui) -> bool {
        let (tasks, follow_ups) = self.kind_counts();
        let mut next = self.kind;
        eframe::egui::ComboBox::from_id_salt("board-kind")
            .selected_text(format!("{} {}", icons::LIST, self.kind.label()))
            .width(140.0)
            .show_ui(ui, |ui| {
                for (variant, count) in [
                    (KindFilter::All, tasks + follow_ups),
                    (KindFilter::Tasks, tasks),
                    (KindFilter::FollowUps, follow_ups),
                ] {
                    ui.selectable_value(
                        &mut next,
                        variant,
                        format!("{} ({count})", variant.label()),
                    );
                }
            })
            .response
            .on_hover_text("Show only tasks you wrote, or only mined follow-ups");
        std::mem::replace(&mut self.kind, next) != next
    }

    fn status_combo(&mut self, ui: &mut Ui) -> bool {
        let mut next = self.status_filter;
        eframe::egui::ComboBox::from_id_salt("board-status")
            .selected_text(format!("{} {}", icons::STATUS_QUEUED, self.status_filter.label()))
            .width(140.0)
            .show_ui(ui, |ui| {
                for variant in [
                    StatusFilter::Open,
                    StatusFilter::All,
                    StatusFilter::Archived,
                    StatusFilter::Dismissed,
                    StatusFilter::Filed,
                ] {
                    ui.selectable_value(&mut next, variant, variant.label());
                }
            })
            .response
            .on_hover_text("Open items only, or bring back what you already closed out");
        std::mem::replace(&mut self.status_filter, next) != next
    }

    fn lane_combo(&mut self, ui: &mut Ui) -> bool {
        let mut lanes: Vec<&str> = self
            .board
            .groups
            .iter()
            .map(|g| g.lane.as_str())
            .filter(|l| !l.is_empty())
            .collect();
        lanes.sort_unstable();
        lanes.dedup();

        let mut next = self.lane.clone();
        let selected = next.as_deref().unwrap_or("All lanes");
        eframe::egui::ComboBox::from_id_salt("board-lane")
            .selected_text(format!("{} {}", icons::TAG, shorten_lane(selected)))
            .width(160.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut next, None, "All lanes");
                for lane in lanes {
                    ui.selectable_value(
                        &mut next,
                        Some(lane.to_string()),
                        shorten_lane(lane),
                    );
                }
            })
            .response
            .on_hover_text("A lane is a shared task list; session ids are one-off lists");
        if next != self.lane {
            self.lane = next;
            return true;
        }
        false
    }

    fn reload(&mut self) {
        self.board = data::load();
        self.contexts.clear();
        self.rebuild();
        self.built = true;
    }

    /// Lays each group in its own column with its items stacked beneath, wrapping
    /// into a grid so the graph stays roughly screen-shaped instead of one tall strip.
    fn rebuild(&mut self) {
        self.snarl = Snarl::new();
        self.recenter = true;
        let needle = self.filter.trim().to_lowercase();
        let mut col = 0usize;
        let mut band_top = 0.0f32;
        let mut band_height = 0.0f32;

        for g in &self.board.groups {
            if self.lane.as_deref().is_some_and(|l| l != g.lane) {
                continue;
            }
            let items: Vec<_> = g
                .items
                .iter()
                .filter(|i| self.status_filter.accepts(i.status))
                .filter(|i| self.kind.accepts(i.kind))
                .filter(|i| {
                    needle.is_empty()
                        || i.subject.to_lowercase().contains(&needle)
                        || i.detail.to_lowercase().contains(&needle)
                        || g.title.to_lowercase().contains(&needle)
                        || g.project.to_lowercase().contains(&needle)
                        || g.lane.to_lowercase().contains(&needle)
                })
                .cloned()
                .collect();
            if items.is_empty() {
                continue;
            }

            let node = GroupNode {
                id: g.id.clone(),
                kind: g.kind,
                title: g.title.clone(),
                project: g.project.clone(),
                lane: g.lane.clone(),
                last_active: g.last_active.clone(),
                has_transcript: g.transcript.is_some(),
                open: g.open_count(),
                total: g.items.len(),
            };
            let x = col as f32 * COL_W * LAYOUT_SCALE;
            let gid = self
                .snarl
                .insert_node(Pos2::new(x, band_top), BoardNode::Group(node));

            // Stack items by their own estimated heights; a fixed pitch overlaps as
            // soon as one item wraps to more lines than another.
            let mut y = band_top
                + (estimated_group_height(g, self.expanded.contains(&g.id)) + NODE_GAP)
                    * LAYOUT_SCALE;
            for item in &items {
                let iid = self.snarl.insert_node(
                    Pos2::new(x + ITEM_INDENT * LAYOUT_SCALE, y),
                    BoardNode::Item(item.clone()),
                );
                self.snarl.connect(
                    egui_snarl::OutPinId { node: gid, output: 0 },
                    egui_snarl::InPinId { node: iid, input: 0 },
                );
                y += (estimated_item_height(item) + NODE_GAP) * LAYOUT_SCALE;
            }

            band_height = band_height.max(y - band_top);
            col += 1;
            if col >= COLS {
                col = 0;
                band_top += band_height + BAND_GAP * LAYOUT_SCALE;
                band_height = 0.0;
            }
        }
    }

    fn apply(&mut self, action: BoardAction) {
        match action {
            BoardAction::ToggleContext(id) => {
                if self.expanded.remove(&id) {
                    return;
                }
                self.expanded.insert(id.clone());
                self.load_context(&id);
            }
            BoardAction::SetStatus(item, status) => {
                match data::set_status(&item, status) {
                    Ok(()) => {
                        self.status = format!("{} {}", status.label(), truncate(&item.subject, 40));
                        self.reload();
                    }
                    Err(e) => self.status = format!("write failed: {e}"),
                }
            }
            BoardAction::CloseGroup(id) => {
                let items: Vec<_> = self
                    .board
                    .groups
                    .iter()
                    .filter(|g| g.id == id)
                    .flat_map(|g| g.items.iter())
                    .filter(|i| i.status == ItemStatus::Open)
                    .cloned()
                    .collect();
                let n = items.len();
                let mut failed = 0;
                for item in items {
                    if data::set_status(&item, ItemStatus::Archived).is_err() {
                        failed += 1;
                    }
                }
                self.status = if failed == 0 {
                    format!("archived {n} item(s)")
                } else {
                    format!("archived {} of {n}, {failed} failed", n - failed)
                };
                self.reload();
            }
        }
    }

    fn load_context(&mut self, id: &str) {
        if self.contexts.contains_key(id) {
            return;
        }
        let Some(path) = self
            .board
            .groups
            .iter()
            .find(|g| g.id == id)
            .and_then(|g| g.transcript.clone())
        else {
            return;
        };
        self.contexts.insert(id.to_string(), data::load_context(&path));
    }
}

/// A session-id lane is 36 hex characters; name it instead of showing the id.
fn shorten_lane(lane: &str) -> String {
    if data::is_session_lane(lane) {
        format!("session {}", &lane[..8])
    } else {
        lane.to_string()
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    s.chars().take(n).collect::<String>() + "…"
}


#[cfg(test)]
mod tests {
    use super::*;
    use data::{Group, GroupKind, Item};

    fn item(key: &str, kind: ItemKind, status: ItemStatus) -> Item {
        Item {
            key: key.into(),
            kind,
            status,
            subject: format!("subject {key}"),
            detail: String::new(),
            ts: "2026-08-22T00:00:00Z".into(),
            task_file: None,
        }
    }

    fn board() -> Board {
        Board {
            groups: vec![Group {
                id: "s1".into(),
                kind: GroupKind::Session,
                title: "s1".into(),
                project: "proj".into(),
                lane: "mtech-dev".into(),
                last_active: "2026-08-22T00:00:00Z".into(),
                transcript: None,
                items: vec![
                    item("a", ItemKind::Task, ItemStatus::Open),
                    item("b", ItemKind::Suggestion, ItemStatus::Open),
                    item("c", ItemKind::Task, ItemStatus::Archived),
                ],
            }],
        }
    }

    /// Item nodes surviving the current filters, by kind.
    fn shown(b: &SessionBoard) -> Vec<ItemKind> {
        b.snarl
            .nodes_ids_data()
            .filter_map(|(_, n)| match &n.value {
                BoardNode::Item(i) => Some(i.kind),
                BoardNode::Group(_) => None,
            })
            .collect()
    }

    fn with(kind: KindFilter, status: StatusFilter, lane: Option<&str>) -> SessionBoard {
        let mut b = SessionBoard {
            board: board(),
            kind,
            status_filter: status,
            lane: lane.map(str::to_string),
            ..Default::default()
        };
        b.rebuild();
        b
    }

    #[test]
    fn kind_filter_selects_node_kinds() {
        let all = with(KindFilter::All, StatusFilter::Open, None);
        assert_eq!(shown(&all).len(), 2, "both open items");

        let tasks = with(KindFilter::Tasks, StatusFilter::Open, None);
        assert_eq!(shown(&tasks), vec![ItemKind::Task]);

        let follow = with(KindFilter::FollowUps, StatusFilter::Open, None);
        assert_eq!(shown(&follow), vec![ItemKind::Suggestion]);
    }

    #[test]
    fn status_filter_reaches_closed_items() {
        let open = with(KindFilter::All, StatusFilter::Open, None);
        assert_eq!(shown(&open).len(), 2);

        let any = with(KindFilter::All, StatusFilter::All, None);
        assert_eq!(shown(&any).len(), 3, "archived item comes back");

        let archived = with(KindFilter::All, StatusFilter::Archived, None);
        assert_eq!(shown(&archived), vec![ItemKind::Task]);
    }

    #[test]
    fn lane_filter_drops_other_lanes() {
        let matching = with(KindFilter::All, StatusFilter::Open, Some("mtech-dev"));
        assert_eq!(shown(&matching).len(), 2);

        let other = with(KindFilter::All, StatusFilter::Open, Some("backfill"));
        assert!(shown(&other).is_empty(), "no group is in that lane");
    }

    /// A filter change re-lays out from the origin, so the view has to follow.
    #[test]
    fn rebuild_requests_a_recenter() {
        let mut b = with(KindFilter::All, StatusFilter::Open, None);
        b.recenter = false;
        b.rebuild();
        assert!(b.recenter);
    }
}
