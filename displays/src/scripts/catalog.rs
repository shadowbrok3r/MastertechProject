//! The script catalog: one declarative entry per script, embedded at compile time.
//!
//! The files are `include_str!`'d rather than read from disk so the catalog is
//! present on a customer machine with no filesystem or network, mirroring the
//! pattern `stress-runner`'s cert presets already use.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

use super::ScriptCategory;
use super::id::ScriptId;

const EMBEDDED: &[(&str, &str)] = &[
    ("tuneup", include_str!("../../scripts_catalog/tuneup.toml")),
    (
        "informational",
        include_str!("../../scripts_catalog/informational.toml"),
    ),
    ("junkware", include_str!("../../scripts_catalog/junkware.toml")),
    ("stress", include_str!("../../scripts_catalog/stress.toml")),
    (
        "benchmarks",
        include_str!("../../scripts_catalog/benchmarks.toml"),
    ),
];

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

pub struct ScriptCatalog {
    defs: Vec<ScriptDef>,
    by_id: HashMap<ScriptId, usize>,
    by_legacy_name: HashMap<String, ScriptId>,
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

        Self {
            defs: kept,
            by_id,
            by_legacy_name,
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
            let file: CatalogFile = toml::from_str(raw)
                .unwrap_or_else(|e| panic!("{name}.toml does not parse: {e}"));
            assert!(!file.script.is_empty(), "{name}.toml is empty");
        }
        assert_eq!(CATALOG.len(), 95, "every catalog entry must survive load");
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

    /// Renaming a script is a breaking change in six places, so the set is pinned.
    #[test]
    fn display_names_are_unchanged() {
        let catalog: std::collections::BTreeSet<&str> =
            CATALOG.iter().map(|d| d.name.as_str()).collect();
        let legacy: std::collections::BTreeSet<String> = crate::scripts::categories::get_all_categories()
            .values()
            .flatten()
            .map(|s| s.name.clone())
            .collect();
        let legacy: std::collections::BTreeSet<&str> = legacy.iter().map(|s| s.as_str()).collect();
        assert_eq!(catalog, legacy, "the catalog no longer matches the shipped names");
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
}
