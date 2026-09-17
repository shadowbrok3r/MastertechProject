use std::collections::HashSet;

use egui_dock::{DockState, Node, NodeIndex, SurfaceIndex};
use log::warn;
use serde::{Deserialize, Serialize};

use super::tab_id::TabId;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DockSession {
    pub tree: DockState<TabId>,
}

impl DockSession {
    pub fn new(initial: Vec<TabId>) -> Self {
        Self {
            tree: DockState::new(initial),
        }
    }

    pub fn is_open(&self, tab: TabId) -> bool {
        self.tree.find_tab(&tab).is_some()
    }

    pub fn open(&mut self, tab: TabId) {
        if !self.is_open(tab) {
            self.tree.push_to_focused_leaf(tab);
        }
    }

    pub fn close(&mut self, tab: TabId) {
        if let Some(index) = self.tree.find_tab(&tab) {
            self.tree.remove_tab(index);
        }
    }

    pub fn toggle(&mut self, tab: TabId) {
        if self.is_open(tab) {
            self.close(tab);
        } else {
            self.open(tab);
        }
    }

    pub fn open_set(&self) -> HashSet<TabId> {
        let mut set = HashSet::new();
        for node in self.tree[SurfaceIndex::main()].iter() {
            if let Node::Leaf(leaf) = node {
                for tab in &leaf.tabs {
                    set.insert(*tab);
                }
            }
        }
        set
    }

    pub fn from_legacy_tree(old: DockState<String>) -> Self {
        match serde_json::to_value(&old)
            .ok()
            .and_then(|mut value| {
                remap_legacy_tabs(&mut value);
                normalize_null_floats(&mut value);
                serde_json::from_value::<DockState<TabId>>(value).ok()
            }) {
            Some(mut tree) => {
                // Drops leaves the remap emptied of retired tabs.
                tree.retain_tabs(|_| true);
                Self { tree }
            }
            None => {
                warn!("DockSession: legacy layout migration failed; using defaults");
                if cfg!(target_arch = "wasm32") {
                    default_dock_session_wasm()
                } else {
                    default_dock_session_native()
                }
            }
        }
    }
}

fn remap_legacy_tabs(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if key == "tabs" {
                    if let serde_json::Value::Array(items) = child {
                        *items = items
                            .iter()
                            .filter_map(|item| {
                                item.as_str()
                                    .and_then(TabId::from_legacy_title)
                                    .map(|id| serde_json::Value::String(id.slug().to_string()))
                            })
                            .collect();
                    }
                } else {
                    remap_legacy_tabs(child);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items.iter_mut() {
                remap_legacy_tabs(item);
            }
        }
        _ => {}
    }
}

/// Rewrites `null` numbers to `0.0`; serde_json writes a NaN rect coordinate as `null`,
/// which no longer deserializes into `f32`.
fn normalize_null_floats(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if matches!(key.as_str(), "x" | "y" | "fraction" | "scroll") && child.is_null() {
                    *child = serde_json::json!(0.0);
                } else {
                    normalize_null_floats(child);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items.iter_mut() {
                normalize_null_floats(item);
            }
        }
        _ => {}
    }
}

pub fn default_dock_session_wasm() -> DockSession {
    let mut session = DockSession::new(vec![
        TabId::StoreTasks,
        TabId::CompletedTasks,
        TabId::Inventory,
        TabId::Logs,
    ]);

    let [_, _] = session.tree.main_surface_mut().split_below(
        NodeIndex::root(),
        0.6,
        vec![TabId::MyTasks, TabId::BugReport, TabId::TaskAudit],
    );

    session.tree.translations.tab_context_menu.eject_button = "Undock".to_owned();
    session
}

pub fn default_dock_session_native() -> DockSession {
    let mut session = DockSession::new(vec![
        TabId::TurSheet,
        TabId::MyTasks,
        TabId::StoreTasks,
        TabId::CompletedTasks,
        TabId::Downloads,
        TabId::Inventory,
    ]);
    session.tree.translations.tab_context_menu.eject_button = "Undock".to_owned();

    let [_a, _b] = session.tree.main_surface_mut().split_left(
        NodeIndex::root(),
        0.30,
        vec![TabId::FileBrowser, TabId::Logs],
    );
    let [_a, _b] = session.tree.main_surface_mut().split_below(
        NodeIndex::root(),
        0.65,
        vec![TabId::BugReport, TabId::ResourceMonitor, TabId::Scripts],
    );

    session
}


#[cfg(test)]
mod dock_session_tests {
    use super::*;

    fn empty_leaves(session: &DockSession) -> usize {
        session.tree[SurfaceIndex::main()]
            .iter()
            .filter(|node| matches!(node, Node::Leaf(leaf) if leaf.tabs.is_empty()))
            .count()
    }

    /// A leaf holding only a retired tab must be removed, not left as an empty pane.
    #[test]
    fn a_leaf_of_only_retired_tabs_is_dropped() {
        let mut legacy = DockState::new(vec!["tur_sheet".to_owned(), "logs".to_owned()]);
        legacy
            .main_surface_mut()
            .split_below(NodeIndex::root(), 0.5, vec!["qc".to_owned()]);

        let session = DockSession::from_legacy_tree(legacy);
        let open = session.open_set();

        assert!(
            !open.iter().any(|t| t.slug() == "qc"),
            "the retired tab must not survive the migration"
        );
        assert!(
            open.contains(&TabId::TurSheet) && open.contains(&TabId::Logs),
            "the surviving tabs must be kept, got {open:?}"
        );
        assert_eq!(empty_leaves(&session), 0, "no empty leaf may be left behind");
    }

    /// An un-laid-out tree serializes its rects as `null`; the migration must still recover it.
    #[test]
    fn a_layout_with_null_rects_still_migrates() {
        let legacy = DockState::new(vec!["tur_sheet".to_owned(), "scripts".to_owned()]);
        let raw = serde_json::to_string(&legacy).expect("serialize");
        assert!(raw.contains("null"), "this test is meaningless without a null rect");

        let session = DockSession::from_legacy_tree(legacy);

        assert_eq!(
            session.open_set(),
            [TabId::TurSheet, TabId::Scripts].into_iter().collect(),
            "the migration fell back to defaults instead of recovering the layout"
        );
    }
}
