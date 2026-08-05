//! Walks the BIOSLove firmware share and emits the model index the UEFI app
//! reads. See `uefi/src/bioslove.rs` for the consuming side of the schema.

mod diff;
mod dirindex;
mod entry;
mod identity;
mod model;
mod script;
mod tokens;

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::{SecondsFormat, Utc};
use clap::Parser;

use crate::identity::{read_launchers, Launcher};
use crate::model::{Index, Lane, Side, SCHEMA_VERSION};

const DEFAULT_SHARE: &str = r"\\opk-riv\winbits\Drivers\Thumb\multiboot\BiosLove";

#[derive(Parser)]
#[command(
    name = "bioslove-index",
    about = "Index the BIOSLove firmware share into index.json"
)]
struct Cli {
    /// Share root holding the laptop and Desktop trees.
    #[arg(long, default_value = DEFAULT_SHARE)]
    share: PathBuf,

    /// Where to write the index.
    #[arg(long, default_value = "index.json")]
    out: PathBuf,

    /// Previous index.json to compare the fresh walk against.
    #[arg(long)]
    diff: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if !cli.share.is_dir() {
        bail!("share {} is not reachable", cli.share.display());
    }

    let index = walk(&cli.share)?;
    let json = serde_json::to_string_pretty(&index)?;
    if let Some(parent) = cli.out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&cli.out, json).with_context(|| format!("write {}", cli.out.display()))?;

    summary(&index, &cli.out);

    if let Some(path) = &cli.diff {
        let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
        let previous: Index = serde_json::from_slice(&bytes)
            .with_context(|| format!("{} is not an index document", path.display()))?;
        let mut stdout = std::io::stdout().lock();
        diff::report(&previous, &index, &mut stdout)?;
    }
    Ok(())
}

fn walk(share: &Path) -> Result<Index> {
    let mut entries = Vec::new();
    for side in Side::ALL {
        let Some(side_dir) = resolve_side_dir(share, side.dir_name())? else {
            eprintln!("warning: no {} tree under the share", side.dir_name());
            continue;
        };
        let launchers = read_launchers(&side_dir)?;
        let mut folders: Vec<(String, PathBuf)> = Vec::new();
        for e in std::fs::read_dir(&side_dir)? {
            let e = e?;
            if e.file_type()?.is_dir() {
                folders.push((e.file_name().to_string_lossy().into_owned(), e.path()));
            }
        }
        folders.sort_by_key(|(name, _)| name.to_ascii_lowercase());

        let empty: Vec<Launcher> = Vec::new();
        for (folder, path) in &folders {
            let mine = launchers
                .get(&folder.to_ascii_lowercase())
                .unwrap_or(&empty)
                .as_slice();
            match entry::build(side, path, folder, mine)? {
                Some(entry) => entries.push(entry),
                None => eprintln!(
                    "skipped {}/{folder}: no firmware in it, or no model behind it",
                    side.label()
                ),
            }
        }
        report_orphan_launchers(side, &launchers, &folders);
    }

    Ok(Index {
        schema_version: SCHEMA_VERSION,
        generated_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        source: share.display().to_string(),
        entries,
    })
}

/// The share spells the two trees differently; match either case.
fn resolve_side_dir(share: &Path, name: &str) -> Result<Option<PathBuf>> {
    for e in std::fs::read_dir(share).with_context(|| format!("read {}", share.display()))? {
        let e = e?;
        if e.file_type()?.is_dir() && e.file_name().to_string_lossy().eq_ignore_ascii_case(name) {
            return Ok(Some(e.path()));
        }
    }
    Ok(None)
}

/// Root launchers whose model directory was pruned in a later drop.
fn report_orphan_launchers(
    side: Side,
    launchers: &HashMap<String, Vec<Launcher>>,
    folders: &[(String, PathBuf)],
) {
    let present: Vec<String> = folders.iter().map(|(f, _)| f.to_ascii_lowercase()).collect();
    let orphans = launchers
        .iter()
        .filter(|(k, _)| !present.contains(k))
        .flat_map(|(_, v)| v.iter())
        .count();
    if orphans > 0 {
        eprintln!("{}: {orphans} root launchers point at folders that are not on the share", side.label());
    }
}

fn summary(index: &Index, out: &Path) {
    let mut err = std::io::stderr().lock();
    let _ = writeln!(err, "\nwrote {}", out.display());
    let _ = writeln!(err, "source {}", index.source);
    let _ = writeln!(err, "entries {}", index.entries.len());

    for side in Side::ALL {
        let of_side: Vec<_> = index.entries.iter().filter(|e| e.side == side).collect();
        if of_side.is_empty() {
            continue;
        }
        let reachable = of_side.iter().filter(|e| e.reachable).count();
        let _ = writeln!(
            err,
            "  {:<8} {:>4} entries, {reachable} reachable",
            side.label(),
            of_side.len()
        );
        for lane in Lane::ALL {
            let n = of_side.iter().filter(|e| e.lane == lane).count();
            if n > 0 {
                let _ = writeln!(err, "      {:<13} {n}", lane.label());
            }
        }
        let dirty: Vec<_> = of_side
            .iter()
            .filter(|e| e.dangling().next().is_some())
            .collect();
        let refs: usize = dirty.iter().map(|e| e.dangling().count()).sum();
        let _ = writeln!(
            err,
            "      {refs} dangling references across {} folders",
            dirty.len()
        );
    }

    let reachable = index.entries.iter().filter(|e| e.reachable).count();
    let refs: usize = index.entries.iter().map(|e| e.dangling().count()).sum();
    let warned = index.entries.iter().filter(|e| !e.warnings.is_empty()).count();
    let _ = writeln!(
        err,
        "total {reachable} reachable, {refs} dangling references, {warned} entries with warnings"
    );
}
