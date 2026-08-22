//! Node-graph board of Claude Code sessions and the work they left outstanding.

pub mod data;
pub mod theme;
pub mod viewer;

use std::collections::{HashMap, HashSet};

use eframe::egui::{Pos2, RichText, Ui};
use egui_snarl::ui::SnarlStyle;
use egui_snarl::Snarl;

use crate::ui_tools::icons;
use data::{Board, Context, GroupKind, ItemStatus};
use viewer::{BoardAction, BoardNode, BoardViewer, GroupNode};

const GROUP_X: f32 = 0.0;
const ITEM_X: f32 = 360.0;
const ROW_H: f32 = 150.0;
const ITEM_H: f32 = 150.0;

pub struct SessionBoard {
    snarl: Snarl<BoardNode>,
    style: SnarlStyle,
    board: Board,
    contexts: HashMap<String, Context>,
    expanded: HashSet<String>,
    filter: String,
    show_closed: bool,
    status: String,
    built: bool,
    scale: f32,
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
            show_closed: false,
            status: String::new(),
            built: false,
            scale: 1.0,
        }
    }
}

impl SessionBoard {
    pub fn ui(&mut self, ui: &mut Ui) {
        if !self.built {
            self.reload();
        }
        self.toolbar(ui);
        ui.separator();

        let mut viewer = BoardViewer {
            contexts: &self.contexts,
            expanded: &self.expanded,
            actions: Vec::new(),
            scale: self.scale,
        };
        self.snarl.show(&mut viewer, &self.style, "session_board", ui);
        self.scale = viewer.scale;
        let actions = std::mem::take(&mut viewer.actions);
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
            let changed = ui
                .add(
                    eframe::egui::TextEdit::singleline(&mut self.filter)
                        .desired_width(190.0)
                        .hint_text("session, project, or text"),
                )
                .changed();
            if ui
                .checkbox(&mut self.show_closed, "Show closed")
                .on_hover_text("Include archived, dismissed, and filed items")
                .changed()
                || changed
            {
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

    fn reload(&mut self) {
        self.board = data::load();
        self.contexts.clear();
        self.rebuild();
        self.built = true;
    }

    /// Lays groups down the left edge with their items to the right.
    fn rebuild(&mut self) {
        self.snarl = Snarl::new();
        let needle = self.filter.trim().to_lowercase();
        let mut y = 0.0f32;

        for g in &self.board.groups {
            let items: Vec<_> = g
                .items
                .iter()
                .filter(|i| self.show_closed || i.status == ItemStatus::Open)
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
            let gid = self
                .snarl
                .insert_node(Pos2::new(GROUP_X, y), BoardNode::Group(node));

            for (n, item) in items.iter().enumerate() {
                let iid = self.snarl.insert_node(
                    Pos2::new(ITEM_X, y + n as f32 * ITEM_H),
                    BoardNode::Item(item.clone()),
                );
                self.snarl.connect(
                    egui_snarl::OutPinId { node: gid, output: 0 },
                    egui_snarl::InPinId { node: iid, input: 0 },
                );
            }
            y += ROW_H.max(items.len() as f32 * ITEM_H) + 40.0;
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

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    s.chars().take(n).collect::<String>() + "…"
}

/// Kept so a lane-only group still reports a kind in debug output.
pub fn group_kind_label(kind: GroupKind) -> &'static str {
    match kind {
        GroupKind::Session => "session",
        GroupKind::Lane => "lane",
    }
}
