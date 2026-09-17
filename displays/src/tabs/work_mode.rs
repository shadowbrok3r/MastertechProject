//! The job a technician picked at launch, and the curated tab set it opens.

use egui_dock::{NodeIndex, SurfaceIndex};
use serde::{Deserialize, Serialize};

use crate::ui_tools::icons::p;

use super::dock_session::{DockSession, default_dock_session_native};
use super::tab_id::{TabContext, TabId};

/// What the technician is doing. `Full` is the uncurated app.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkMode {
    TuneupQc,
    Diagnostic,
    Admin,
    #[default]
    Full,
}

const TUNEUP_QC_TABS: &[TabId] = &[
    TabId::TurSheet,
    TabId::StressTest,
    TabId::Scripts,
    TabId::Ai,
];

const DIAGNOSTIC_TABS: &[TabId] = &[
    TabId::TurSheet,
    TabId::StressTest,
    TabId::MinidumpAnalysis,
    TabId::ResourceMonitor,
    TabId::Ai,
];

const ADMIN_TABS: &[TabId] = &[TabId::AdminConsole];

/// Tabs another tab may open by button, absent from the View menu.
const STRESS_ON_DEMAND: &[TabId] = &[TabId::StressLab];

impl WorkMode {
    pub const ALL: &'static [WorkMode] = &[
        WorkMode::TuneupQc,
        WorkMode::Diagnostic,
        WorkMode::Admin,
        WorkMode::Full,
    ];

    /// The persisted vocabulary. Changing a slug orphans every saved preference.
    pub fn slug(self) -> &'static str {
        match self {
            Self::TuneupQc => "tuneup_qc",
            Self::Diagnostic => "diagnostic",
            Self::Admin => "admin",
            Self::Full => "full",
        }
    }

    pub fn from_slug(s: &str) -> Option<Self> {
        match s {
            "tuneup_qc" => Some(Self::TuneupQc),
            "diagnostic" => Some(Self::Diagnostic),
            "admin" => Some(Self::Admin),
            "full" => Some(Self::Full),
            _ => None,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::TuneupQc => "Tune-up / QC",
            Self::Diagnostic => "Diagnostic",
            Self::Admin => "Admin",
            Self::Full => "Everything",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Self::TuneupQc => {
                "Clean up and verify a machine: run the tune-up scripts, stress it, fill the TUR sheet."
            }
            Self::Diagnostic => {
                "Work out what is wrong: crash dumps and live telemetry, with the agent alongside."
            }
            Self::Admin => "Work several machines at once, with the agent driving diagnostics.",
            Self::Full => "Every tab, arranged the way you saved it.",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Self::TuneupQc => p::WRENCH,
            Self::Diagnostic => p::STETHOSCOPE,
            Self::Admin => p::MONITOR,
            Self::Full => p::SQUARES_FOUR,
        }
    }

    /// The tabs this mode opens and lists. `None` means uncurated.
    pub fn curated_tabs(self) -> Option<&'static [TabId]> {
        match self {
            Self::TuneupQc => Some(TUNEUP_QC_TABS),
            Self::Diagnostic => Some(DIAGNOSTIC_TABS),
            Self::Admin => Some(ADMIN_TABS),
            Self::Full => None,
        }
    }

    pub fn on_demand_tabs(self) -> &'static [TabId] {
        match self {
            Self::TuneupQc | Self::Diagnostic => STRESS_ON_DEMAND,
            Self::Admin | Self::Full => &[],
        }
    }

    /// Whether this mode may hold `tab` at all, including on-demand opens.
    pub fn allows(self, tab: TabId) -> bool {
        match self.curated_tabs() {
            None => true,
            Some(curated) => curated.contains(&tab) || self.on_demand_tabs().contains(&tab),
        }
    }

    /// What the View menu lists: the curated set only, clamped by context and the root gate.
    pub fn visible_tabs(self, ctx: TabContext, is_root: bool) -> Vec<TabId> {
        match self.curated_tabs() {
            None => TabId::visible_for_user(ctx, is_root),
            Some(curated) => {
                let in_context = TabId::visible_for(ctx);
                curated
                    .iter()
                    .copied()
                    .filter(|t| (is_root || !t.requires_root()) && in_context.contains(t))
                    .collect()
            }
        }
    }

    /// The layout a preset opens every time. Preset layouts are never restored from disk.
    pub fn canonical_session(self) -> DockSession {
        match self {
            Self::TuneupQc => {
                let mut session =
                    DockSession::new(vec![TabId::TurSheet, TabId::StressTest, TabId::Scripts]);
                let [work, _chat] = session.tree.main_surface_mut().split_right(
                    NodeIndex::root(),
                    0.70,
                    vec![TabId::Ai],
                );
                finish(session, work)
            }
            Self::Diagnostic => {
                let mut session = DockSession::new(vec![
                    TabId::TurSheet,
                    TabId::StressTest,
                    TabId::MinidumpAnalysis,
                ]);
                // Splitting below first leaves the chat spanning the full height beside both.
                session.tree.main_surface_mut().split_below(
                    NodeIndex::root(),
                    0.68,
                    vec![TabId::ResourceMonitor],
                );
                let [work, _chat] = session.tree.main_surface_mut().split_right(
                    NodeIndex::root(),
                    0.72,
                    vec![TabId::Ai],
                );
                finish(session, work)
            }
            Self::Admin => finish(
                DockSession::new(vec![TabId::AdminConsole]),
                NodeIndex::root(),
            ),
            Self::Full => default_dock_session_native(),
        }
    }
}

/// Focuses the work leaf so a View-menu re-open lands there, not beside the chat.
fn finish(mut session: DockSession, work: NodeIndex) -> DockSession {
    session.tree.translations.tab_context_menu.eject_button = "Undock".to_owned();
    session
        .tree
        .set_focused_node_and_surface((SurfaceIndex::main(), work));
    session
}

#[cfg(test)]
mod work_mode_tests {
    use super::*;

    #[test]
    fn every_mode_slug_round_trips() {
        for mode in WorkMode::ALL {
            assert_eq!(
                WorkMode::from_slug(mode.slug()),
                Some(*mode),
                "slug {:?} does not round-trip",
                mode.slug()
            );
        }
    }

    #[test]
    fn a_preset_opens_exactly_its_curated_tabs() {
        for mode in WorkMode::ALL {
            let Some(curated) = mode.curated_tabs() else {
                continue;
            };
            let opened = mode.canonical_session().open_set();
            let expected: std::collections::HashSet<TabId> = curated.iter().copied().collect();
            assert_eq!(
                opened,
                expected,
                "{} opens a different set than it lists",
                mode.title()
            );
        }
    }

    #[test]
    fn every_curated_tab_is_reachable_in_the_native_app() {
        let native = TabId::visible_for(TabContext::MastertechNative);
        for mode in WorkMode::ALL {
            for tab in mode.curated_tabs().unwrap_or(&[]) {
                assert!(
                    native.contains(tab),
                    "{} lists {:?}, which the native app never shows",
                    mode.title(),
                    tab
                );
            }
            for tab in mode.on_demand_tabs() {
                assert!(
                    native.contains(tab),
                    "{} can open {:?}, which the native app never shows",
                    mode.title(),
                    tab
                );
            }
        }
    }

    #[test]
    fn the_chat_sits_in_its_own_pane() {
        for mode in [WorkMode::TuneupQc, WorkMode::Diagnostic] {
            let session = mode.canonical_session();
            let chat = session.tree.find_tab(&TabId::Ai).expect("chat is open");
            let work = session
                .tree
                .find_tab(&TabId::TurSheet)
                .expect("work tab is open");
            assert_ne!(
                chat.1,
                work.1,
                "{} docked the chat into the work pane",
                mode.title()
            );
        }
    }

    #[test]
    fn full_mode_is_uncurated() {
        assert!(WorkMode::Full.curated_tabs().is_none());
        for tab in TabId::visible_for(TabContext::MastertechNative) {
            assert!(WorkMode::Full.allows(*tab), "Full must allow {tab:?}");
        }
    }

    #[test]
    fn a_preset_allows_its_on_demand_tabs_but_does_not_list_them() {
        for mode in [WorkMode::TuneupQc, WorkMode::Diagnostic] {
            assert!(
                mode.allows(TabId::StressLab),
                "{} must let the History button open Stress Lab",
                mode.title()
            );
            assert!(
                !mode
                    .visible_tabs(TabContext::MastertechNative, false)
                    .contains(&TabId::StressLab),
                "{} must not list Stress Lab in the View menu",
                mode.title()
            );
            assert!(
                !mode.allows(TabId::Koth),
                "{} must refuse a tab outside its set",
                mode.title()
            );
        }
    }

    #[test]
    fn a_non_root_operator_never_sees_a_root_only_tab() {
        for mode in WorkMode::ALL {
            assert!(
                !mode
                    .visible_tabs(TabContext::MastertechNative, false)
                    .iter()
                    .any(|t| t.requires_root()),
                "{} leaked a root-only tab",
                mode.title()
            );
        }
    }
}
