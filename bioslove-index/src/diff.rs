//! Compare two index documents: the staleness signal for the share.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{Entry, Index};

type Key = (String, String);

fn key(e: &Entry) -> Key {
    (e.side.label().to_string(), e.folder.to_ascii_lowercase())
}

fn by_key(index: &Index) -> BTreeMap<Key, &Entry> {
    index.entries.iter().map(|e| (key(e), e)).collect()
}

fn dangling(e: &Entry) -> BTreeSet<String> {
    e.dangling().map(|s| s.to_string()).collect()
}

/// Write the differences between `old` and `new` to `out`.
pub fn report(old: &Index, new: &Index, out: &mut impl std::io::Write) -> std::io::Result<()> {
    let previous = by_key(old);
    let current = by_key(new);

    writeln!(out, "diff {} -> {}", old.generated_at, new.generated_at)?;

    let added: Vec<&Entry> = current
        .iter()
        .filter(|(k, _)| !previous.contains_key(*k))
        .map(|(_, e)| *e)
        .collect();
    section(out, "new folders", added.len())?;
    for e in &added {
        writeln!(out, "  + {}/{} [{}]", e.side.label(), e.folder, e.lane.label())?;
    }

    let removed: Vec<&Entry> = previous
        .iter()
        .filter(|(k, _)| !current.contains_key(*k))
        .map(|(_, e)| *e)
        .collect();
    section(out, "disappeared folders", removed.len())?;
    for e in &removed {
        writeln!(out, "  - {}/{}", e.side.label(), e.folder)?;
    }

    let mut version_changes = Vec::new();
    let mut new_dangling = Vec::new();
    let mut fixed = Vec::new();
    for (k, now) in &current {
        let Some(before) = previous.get(k) else {
            continue;
        };
        if before.versions.bios != now.versions.bios {
            version_changes.push(format!(
                "  ~ {}/{} BIOS {} -> {}",
                now.side.label(),
                now.folder,
                shown(&before.versions.bios),
                shown(&now.versions.bios)
            ));
        }
        if before.versions.ec != now.versions.ec {
            version_changes.push(format!(
                "  ~ {}/{} EC {} -> {}",
                now.side.label(),
                now.folder,
                shown(&before.versions.ec),
                shown(&now.versions.ec)
            ));
        }
        let was = dangling(before);
        let is = dangling(now);
        for w in is.difference(&was) {
            new_dangling.push(format!("  ! {}/{}: {w}", now.side.label(), now.folder));
        }
        for w in was.difference(&is) {
            fixed.push(format!("  * {}/{}: resolved, {w}", now.side.label(), now.folder));
        }
    }

    section(out, "changed BIOS/EC versions", version_changes.len())?;
    for line in &version_changes {
        writeln!(out, "{line}")?;
    }
    section(out, "newly dangling references", new_dangling.len())?;
    for line in &new_dangling {
        writeln!(out, "{line}")?;
    }
    section(out, "references restored", fixed.len())?;
    for line in &fixed {
        writeln!(out, "{line}")?;
    }
    Ok(())
}

fn shown(v: &str) -> &str {
    if v.is_empty() {
        "(none)"
    } else {
        v
    }
}

fn section(out: &mut impl std::io::Write, title: &str, n: usize) -> std::io::Result<()> {
    writeln!(out, "\n{title}: {n}")
}
