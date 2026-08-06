//! What the firmware's matcher would do with the tokens this index emits.
//!
//! `normalize` and `pattern_matches` mirror `uefi/src/bioslove.rs`; the firmware
//! crate only builds for x86_64-unknown-uefi and cannot run tests, so the shared
//! matching rules are exercised here.

use std::collections::BTreeMap;

use crate::model::{Index, Side};

/// Uppercase alphanumerics only. Mirrors `normalize` in `uefi/src/bioslove.rs`.
pub fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Keeps `?` alongside alphanumerics. Mirrors `normalize_pattern` in the firmware.
pub fn normalize_pattern(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '?')
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// `?` matches any one character. Mirrors `pattern_matches` in the firmware.
pub fn pattern_matches(pattern: &str, value: &str) -> bool {
    if pattern.len() != value.len() {
        return false;
    }
    pattern
        .bytes()
        .zip(value.bytes())
        .all(|(p, v)| p == b'?' || p == v)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// One chassis token claimed by several folders on the same side.
    SharedExact,
    /// A family pattern that also matches another folder's exact identity.
    PatternSwallowsExact,
}

#[derive(Debug, Clone)]
pub struct Collision {
    pub kind: Kind,
    pub side: Side,
    pub token: String,
    /// Folders the firmware would return for that token.
    pub folders: Vec<String>,
}

impl Collision {
    /// Exactly one folder is named for the token, so the firmware's own-name
    /// tiebreak ranks it first without a human deciding.
    pub fn settled_by_name(&self) -> bool {
        self.folders
            .iter()
            .filter(|f| normalize(f) == self.token)
            .count()
            == 1
    }
}

/// Every exact token an entry answers to, normalized and deduplicated.
fn exact_tokens(e: &crate::model::Entry) -> Vec<String> {
    let mut v: Vec<String> = std::iter::once(e.folder.as_str())
        .chain(e.aliases.iter().map(String::as_str))
        .map(normalize)
        .filter(|t| t.len() >= 3)
        .collect();
    v.sort();
    v.dedup();
    v
}

/// Tokens that would leave the firmware unable to pick a single entry.
pub fn collisions(index: &Index) -> Vec<Collision> {
    let mut out = Vec::new();

    for side in Side::ALL {
        let of_side: Vec<_> = index.entries.iter().filter(|e| e.side == side).collect();

        let mut by_token: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for e in &of_side {
            for t in exact_tokens(e) {
                by_token.entry(t).or_default().push(e.folder.clone());
            }
        }
        for (token, mut folders) in by_token.clone() {
            if folders.len() > 1 {
                folders.sort();
                out.push(Collision {
                    kind: Kind::SharedExact,
                    side,
                    token,
                    folders,
                });
            }
        }

        for e in &of_side {
            for p in &e.patterns {
                let pat = normalize_pattern(p);
                if pat.is_empty() {
                    continue;
                }
                let mut swallowed: Vec<String> = by_token
                    .iter()
                    .filter(|(token, _)| pattern_matches(&pat, token))
                    .flat_map(|(_, folders)| folders.iter().cloned())
                    .filter(|f| *f != e.folder)
                    .collect();
                swallowed.sort();
                swallowed.dedup();
                if !swallowed.is_empty() {
                    out.push(Collision {
                        kind: Kind::PatternSwallowsExact,
                        side,
                        token: p.clone(),
                        folders: swallowed,
                    });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Entry, Lane, Versions};

    fn entry(folder: &str, side: Side, aliases: &[&str], patterns: &[&str]) -> Entry {
        Entry {
            folder: folder.to_string(),
            side,
            aliases: aliases.iter().map(|s| s.to_string()).collect(),
            patterns: patterns.iter().map(|s| s.to_string()).collect(),
            modelstring: String::new(),
            versions: Versions::default(),
            lane: Lane::Uefi,
            reachable: true,
            steps: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn index(entries: Vec<Entry>) -> Index {
        Index {
            schema_version: crate::model::SCHEMA_VERSION,
            generated_at: String::new(),
            source: String::new(),
            payload_root: crate::model::DEFAULT_PAYLOAD_ROOT.to_string(),
            entries,
        }
    }

    #[test]
    fn normalize_strips_vendor_punctuation() {
        assert_eq!(normalize("MS-16H5"), "MS16H5");
        assert_eq!(normalize(" nh58dcq "), "NH58DCQ");
        assert_eq!(normalize("P870TM(1)G"), "P870TM1G");
    }

    #[test]
    fn patterns_match_only_same_length_families() {
        assert!(pattern_matches("GM?IX7?", "GM6IX7N"));
        assert!(!pattern_matches("GM?IX7?", "GM6IX9N"));
        assert!(!pattern_matches("GM?IX7?", "GM6IX7NX"));
    }

    #[test]
    fn a_token_on_two_folders_is_a_collision() {
        let i = index(vec![
            entry("NH58DCQ", Side::Laptop, &["SM6"], &[]),
            entry("NH70DCQ", Side::Laptop, &["SM6"], &[]),
        ]);
        let c = collisions(&i);
        let shared: Vec<_> = c.iter().filter(|c| c.kind == Kind::SharedExact).collect();
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0].token, "SM6");
        assert_eq!(shared[0].folders, vec!["NH58DCQ", "NH70DCQ"]);
    }

    #[test]
    fn the_same_token_on_two_sides_is_not_a_collision() {
        let i = index(vec![
            entry("A", Side::Laptop, &["SHARED"], &[]),
            entry("B", Side::Desktop, &["SHARED"], &[]),
        ]);
        assert!(collisions(&i).is_empty());
    }

    #[test]
    fn a_pattern_covering_another_model_is_reported() {
        let i = index(vec![
            entry("PD50SNx", Side::Laptop, &[], &["PD?0SN?"]),
            entry("PD50SNE", Side::Laptop, &[], &[]),
        ]);
        let c = collisions(&i);
        let swallow: Vec<_> = c
            .iter()
            .filter(|c| c.kind == Kind::PatternSwallowsExact)
            .collect();
        assert_eq!(swallow.len(), 1);
        assert_eq!(swallow[0].folders, vec!["PD50SNE"]);
    }

    #[test]
    fn a_pattern_matching_only_its_own_folder_is_clean() {
        let i = index(vec![entry("GM6IX7N", Side::Laptop, &[], &["GM?IX7?"])]);
        assert!(collisions(&i).is_empty());
    }
}
