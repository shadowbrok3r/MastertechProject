//! The script catalog: one declarative entry per script, embedded at compile time.
//!
//! The files are `include_str!`'d rather than read from disk so the catalog is
//! present on a customer machine with no filesystem or network, mirroring the
//! pattern `stress-runner`'s cert presets already use.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

use super::id::ScriptId;
use super::{ScriptCategory, ScriptItem};

const EMBEDDED: &[(&str, &str)] = &[
    ("tuneup", include_str!("../../scripts_catalog/tuneup.toml")),
    (
        "informational",
        include_str!("../../scripts_catalog/informational.toml"),
    ),
    (
        "junkware",
        include_str!("../../scripts_catalog/junkware.toml"),
    ),
    ("stress", include_str!("../../scripts_catalog/stress.toml")),
    (
        "benchmarks",
        include_str!("../../scripts_catalog/benchmarks.toml"),
    ),
];

const PRESETS: &str = include_str!("../../scripts_catalog/presets.toml");

/// Whether the script needs an elevated process.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Elevation {
    None,
    #[default]
    Admin,
}

/// Whether finishing the script leaves work that only a reboot completes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RebootHint {
    #[default]
    Never,
    Maybe,
    Always,
}

/// Context a script cannot run without.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Requirement {
    ServiceNumber,
    CustomerEmail,
    Internet,
    Gpu,
}

/// Where a script may be offered. An entry with no surfaces stays catalog-known
/// so legacy names still resolve, but is offered nowhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    Egui,
    Terminal,
    Remote,
    Mcp,
}

/// The category as written in the catalog files, decoupled from the runtime enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CatalogCategory {
    Tuneup,
    Informational,
    Junkware,
    Stress,
}

impl From<CatalogCategory> for ScriptCategory {
    fn from(c: CatalogCategory) -> Self {
        match c {
            CatalogCategory::Tuneup => ScriptCategory::Tuneup,
            CatalogCategory::Informational => ScriptCategory::Informational,
            CatalogCategory::Junkware => ScriptCategory::JunkwareRemoval,
            CatalogCategory::Stress => ScriptCategory::StressTests,
        }
    }
}

fn default_timeout() -> u64 {
    600
}

/// One script's immutable definition.
#[derive(Clone, Debug, Deserialize)]
pub struct ScriptDef {
    pub id: ScriptId,
    /// Display name. This is the wire and legacy key; it must not change.
    pub name: String,
    #[serde(rename = "category")]
    raw_category: CatalogCategory,
    pub summary: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub pass: Option<String>,
    #[serde(default)]
    pub warn: Option<String>,
    #[serde(default)]
    pub fail: Option<String>,
    #[serde(default)]
    pub elevation: Elevation,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub requires: Vec<Requirement>,
    #[serde(default)]
    pub reboot: RebootHint,
    #[serde(default)]
    pub surfaces: Vec<Surface>,
    /// Ids this script runs in sequence, for composites.
    #[serde(default)]
    pub runs: Vec<ScriptId>,
    /// Extra legacy names that resolve to this id.
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub order: u32,
}

impl ScriptDef {
    pub fn category(&self) -> ScriptCategory {
        self.raw_category.into()
    }

    pub fn offered_on(&self, surface: Surface) -> bool {
        self.surfaces.contains(&surface)
    }

    pub fn requires(&self, requirement: Requirement) -> bool {
        self.requires.contains(&requirement)
    }
}

#[derive(Deserialize)]
struct CatalogFile {
    #[serde(default)]
    script: Vec<ScriptDef>,
}

/// A named, ordered list of scripts queued together.
#[derive(Clone, Debug, Deserialize)]
pub struct PresetDef {
    pub id: ScriptId,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    pub scripts: Vec<ScriptId>,
}

#[derive(Deserialize)]
struct PresetFile {
    #[serde(default)]
    preset: Vec<PresetDef>,
}

pub struct ScriptCatalog {
    defs: Vec<ScriptDef>,
    by_id: HashMap<ScriptId, usize>,
    by_legacy_name: HashMap<String, ScriptId>,
    presets: Vec<PresetDef>,
}

impl ScriptCatalog {
    /// Parses every embedded file, keeping whatever parsed. A malformed file
    /// must not panic here — a shipped binary would brick on a customer machine.
    /// `catalog_parses` is what keeps the degraded path unreachable in practice.
    fn load() -> Self {
        let mut defs: Vec<ScriptDef> = Vec::new();
        for (name, raw) in EMBEDDED {
            match toml::from_str::<CatalogFile>(raw) {
                Ok(file) => defs.extend(file.script),
                Err(e) => log::error!("script catalog: {name}.toml failed to parse: {e}"),
            }
        }

        let mut by_id = HashMap::new();
        let mut by_legacy_name = HashMap::new();
        let mut kept: Vec<ScriptDef> = Vec::with_capacity(defs.len());
        for def in defs {
            if by_id.contains_key(&def.id) {
                log::error!("script catalog: duplicate id {}, dropping", def.id);
                continue;
            }
            let mut names = vec![def.name.clone()];
            names.extend(def.aliases.iter().cloned());
            for name in names {
                if let Some(existing) = by_legacy_name.insert(name.clone(), def.id.clone()) {
                    log::error!(
                        "script catalog: '{name}' claimed by both {existing} and {}",
                        def.id
                    );
                }
            }
            by_id.insert(def.id.clone(), kept.len());
            kept.push(def);
        }

        let presets = match toml::from_str::<PresetFile>(PRESETS) {
            Ok(file) => file.preset,
            Err(e) => {
                log::error!("script catalog: presets.toml failed to parse: {e}");
                Vec::new()
            }
        };

        Self {
            defs: kept,
            by_id,
            by_legacy_name,
            presets,
        }
    }

    pub fn get(&self, id: &ScriptId) -> Option<&ScriptDef> {
        self.by_id.get(id).map(|i| &self.defs[*i])
    }

    /// Resolves a display name from the wire, a saved layout, or an older peer.
    pub fn id_for_legacy_name(&self, name: &str) -> Option<&ScriptId> {
        self.by_legacy_name
            .get(name)
            .or_else(|| self.by_legacy_name.get(name.trim()))
    }

    pub fn iter(&self) -> impl Iterator<Item = &ScriptDef> {
        self.defs.iter()
    }

    pub fn len(&self) -> usize {
        self.defs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    /// Entries offered on `surface`, in catalog order within each category.
    pub fn for_surface(&self, surface: Surface) -> impl Iterator<Item = &ScriptDef> {
        self.defs.iter().filter(move |d| d.offered_on(surface))
    }

    pub fn presets(&self) -> &[PresetDef] {
        &self.presets
    }

    pub fn preset(&self, id: &str) -> Option<&PresetDef> {
        self.presets.iter().find(|p| p.id.as_str() == id)
    }

    /// The entries offered on `surface`, as list items grouped by category.
    pub fn items_for(&self, surface: Surface) -> HashMap<ScriptCategory, Vec<ScriptItem>> {
        let mut items: HashMap<ScriptCategory, Vec<ScriptItem>> = HashMap::new();
        for def in self.for_surface(surface) {
            let mut item =
                ScriptItem::new(def.name.clone(), def.category()).with_description(&def.summary);
            if let Some(pass) = &def.pass {
                item = item.with_pass_criteria(pass);
            }
            if let Some(warn) = &def.warn {
                item = item.with_warning_criteria(warn);
            }
            if let Some(fail) = &def.fail {
                item = item.with_error_criteria(fail);
            }
            items.entry(def.category()).or_default().push(item);
        }
        items
    }

    pub fn timeout_secs(&self, name: &str) -> Option<u64> {
        self.id_for_legacy_name(name)
            .and_then(|id| self.get(id))
            .map(|d| d.timeout_secs)
    }
}

pub static CATALOG: LazyLock<ScriptCatalog> = LazyLock::new(ScriptCatalog::load);

#[cfg(test)]
mod catalog_tests {
    use super::*;

    /// A parse failure degrades silently at runtime by design, so this test is what
    /// keeps that path unreachable in a shipped binary.
    #[test]
    fn catalog_parses() {
        for (name, raw) in EMBEDDED {
            let file: CatalogFile =
                toml::from_str(raw).unwrap_or_else(|e| panic!("{name}.toml does not parse: {e}"));
            assert!(!file.script.is_empty(), "{name}.toml is empty");
        }
        assert_eq!(CATALOG.len(), 98, "every catalog entry must survive load");
    }

    #[test]
    fn presets_parse() {
        let file: PresetFile = toml::from_str(PRESETS).expect("presets.toml parses");
        assert!(!file.preset.is_empty(), "presets.toml is empty");
        assert!(CATALOG.preset("standard-tuneup").is_some());
    }

    /// A preset naming a script the tab does not offer would queue something the
    /// tech never chose and cannot see in the list.
    #[test]
    fn every_preset_script_is_offered_in_the_tab() {
        for preset in CATALOG.presets() {
            let mut seen = std::collections::HashSet::new();
            for id in &preset.scripts {
                let def = CATALOG
                    .get(id)
                    .unwrap_or_else(|| panic!("{} names {id}, which is not in the catalog", preset.id));
                assert!(def.offered_on(Surface::Egui), "{} queues {id}, which the tab hides", preset.id);
                assert!(seen.insert(id.clone()), "{} lists {id} twice", preset.id);
            }
        }
    }

    /// The tab is built from the catalog, so benchmarks and unimplemented stubs
    /// stay out of it.
    #[test]
    fn the_tab_lists_only_what_the_catalog_offers_it() {
        let items = CATALOG.items_for(Surface::Egui);
        let names: Vec<&str> = items.values().flatten().map(|i| i.name.as_str()).collect();
        assert_eq!(names.len(), CATALOG.for_surface(Surface::Egui).count());
        assert!(!names.iter().any(|n| n.starts_with("Benchmark")), "a benchmark is listed");
        for stub in ["Disable proxy settings", "Change SuperAntiSpyware settings"] {
            assert!(!names.contains(&stub), "{stub} is listed");
        }
    }

    #[test]
    fn ids_are_wellformed_and_unique() {
        let mut seen = std::collections::HashSet::new();
        for def in CATALOG.iter() {
            assert!(def.id.is_wellformed(), "malformed id: {}", def.id);
            assert!(seen.insert(def.id.clone()), "duplicate id: {}", def.id);
            assert!(!def.name.is_empty(), "{} has no display name", def.id);
        }
    }

    #[test]
    fn every_display_name_resolves_to_its_own_id() {
        for def in CATALOG.iter() {
            assert_eq!(
                CATALOG.id_for_legacy_name(&def.name),
                Some(&def.id),
                "'{}' does not resolve back to {}",
                def.name,
                def.id
            );
        }
    }

    /// Renaming a script is a breaking change in six places, so no shipped name may
    /// change or disappear. New entries are allowed only if RECOVERED declares them.
    #[test]
    fn display_names_are_unchanged() {
        let catalog: std::collections::BTreeSet<String> =
            CATALOG.iter().map(|d| d.name.clone()).collect();
        let shipped: std::collections::BTreeSet<String> =
            crate::scripts::categories::get_all_categories()
                .values()
                .flatten()
                .map(|s| s.name.clone())
                .collect();
        let missing: Vec<&String> = shipped.difference(&catalog).collect();
        assert!(
            missing.is_empty(),
            "shipped names dropped from the catalog: {missing:?}"
        );
        let added: std::collections::BTreeSet<&str> =
            catalog.difference(&shipped).map(|s| s.as_str()).collect();
        let declared: std::collections::BTreeSet<&str> = RECOVERED.iter().copied().collect();
        assert_eq!(added, declared, "undeclared additions to the catalog");
    }

    #[test]
    fn composites_terminate() {
        for def in CATALOG.iter() {
            for target in &def.runs {
                assert!(
                    CATALOG.get(target).is_some(),
                    "{} runs {target}, which is not in the catalog",
                    def.id
                );
                assert_ne!(target, &def.id, "{} runs itself", def.id);
            }
        }
    }

    /// Renaming a script silently dropped its budget to the 600s default before.
    #[test]
    fn timeouts_match_legacy() {
        for def in CATALOG.iter() {
            assert_eq!(
                def.timeout_secs,
                crate::scripts::default_remote_script_timeout_secs(&def.name),
                "timeout drifted for '{}'",
                def.name
            );
        }
    }

    /// The runner is the contract; the catalog conforms to it, not the reverse.
    #[test]
    fn every_runner_script_resolves() {
        for name in stress_runner::STRESS_SCRIPT_NAMES {
            assert!(
                CATALOG.id_for_legacy_name(name).is_some(),
                "stress script missing from the catalog: {name}"
            );
        }
        for name in stress_runner::BENCHMARK_SCRIPT_NAMES {
            assert!(
                CATALOG.id_for_legacy_name(name).is_some(),
                "benchmark missing from the catalog: {name}"
            );
        }
    }

    /// The benchmarks only run from terminal/remote/MCP, so the egui tab must not offer them.
    #[test]
    fn benchmarks_are_hidden_from_the_egui_tab() {
        for name in stress_runner::BENCHMARK_SCRIPT_NAMES {
            let def = CATALOG
                .id_for_legacy_name(name)
                .and_then(|id| CATALOG.get(id))
                .expect("benchmark is in the catalog");
            assert!(
                !def.offered_on(Surface::Egui),
                "'{name}' queues in the egui tab but cannot run there"
            );
            assert!(
                def.offered_on(Surface::Mcp),
                "'{name}' must still be reachable over MCP"
            );
        }
    }

    /// Every name the terminal tab's own catalog lists, extracted verbatim from
    /// terminal_mode/tabs/scripts/mod.rs.
    const TERMINAL_CATALOG: &[&str] = &[
        "Activate SEB",
        "Activate SuperAnti",
        "Activate Webroot",
        "Align Taskbar to left",
        "Any Recent Blue Screens?",
        "Are there scheduled tasks for it?",
        "Avast Browser",
        "Change SuperAntiSpyware settings",
        "Change Timezone to Mountain",
        "Check Updates",
        "Clear Browser",
        "Data Transfer",
        "Disable BitLocker",
        "Disable Edge Startup Boost",
        "Disable Notifications",
        "Disable OneDrive Startup",
        "Disable Sleep / Hibernation",
        "Disable Startup Apps",
        "Disable proxy settings",
        "Driver Support",
        "ESET Security",
        "Install LibreOffice",
        "Install Windows Updates",
        "Is Hibernation/Sleep enabled?",
        "Is SuperAntiSpyware installed?",
        "Is SuperEasyBackup installed?",
        "Is Webroot installed?",
        "Is Windows Activated?",
        "Mcaffee Safe",
        "OneLaunch",
        "Remove Browser Hijackers",
        "Run Junkware Category",
        "Run Prechecks",
        "Run SuperAntiSpyware Scan",
        "Run Webroot Scan",
        "Scan For Browser Hijackers",
        "Shift Browser",
        "SuperAnti TEST",
        "Uninstall Microsoft 365",
        "Uninstall OneDrive",
        "Unpin Copilot",
        "Wave Browser",
        "WebNavigator Browser",
        "Webroot TEST",
        "When Was The Last Service Date?",
        "Windows Version",
        "Winzip",
    ];

    /// Every name the remote executor has a match arm for, extracted verbatim from
    /// terminal_mode/websockets/mod.rs.
    const REMOTE_MATCH_ARMS: &[&str] = &[
        "Activate SEB",
        "Activate SuperAnti",
        "Activate Webroot",
        "Align Taskbar to left",
        "Any Recent Blue Screens?",
        "Change SuperAntiSpyware settings",
        "Check Updates",
        "Disable Notifications",
        "Disable Sleep / Hibernation",
        "Disable Startup Apps",
        "Install LibreOffice",
        "Install Windows Updates",
        "Is Hibernation/Sleep enabled?",
        "Is SuperAntiSpyware installed?",
        "Is SuperEasyBackup installed?",
        "Is Webroot installed?",
        "Is Windows Activated?",
        "Remove Browser Hijackers",
        "Run Prechecks",
        "Run SuperAntiSpyware Scan",
        "Run Webroot Scan",
        "Scan For Browser Hijackers",
        "Unpin Copilot",
        "Windows Version",
    ];

    /// Recovered by the three-way diff: implemented somewhere but listed in no
    /// catalog, so unreachable by name. Adding to this list is a deliberate act.
    const RECOVERED: &[&str] = &["Activate Webroot", "Activate SuperAnti", "ESET Security"];

    /// Three catalogs disagreed; this keeps them from drifting apart again. A name
    /// here that stops resolving means a surface can ask for a script nothing can name.
    #[test]
    fn legacy_names_are_exhaustive() {
        let mut unresolved = Vec::new();
        for (source, names) in [
            ("terminal catalog", TERMINAL_CATALOG),
            ("remote match arms", REMOTE_MATCH_ARMS),
        ] {
            for name in names {
                if CATALOG.id_for_legacy_name(name).is_none() {
                    unresolved.push(format!("{source}: {name}"));
                }
            }
        }
        assert!(
            unresolved.is_empty(),
            "names no surface can resolve: {unresolved:?}"
        );
    }
}
