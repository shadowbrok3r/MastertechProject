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
                serde_json::from_value(value).ok()
            }) {
            Some(tree) => Self { tree },
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
