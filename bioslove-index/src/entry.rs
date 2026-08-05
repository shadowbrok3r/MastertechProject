//! Turn one model directory into an index entry.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;

use anyhow::Result;

use crate::dirindex::{DirIndex, FileMeta};
use crate::identity::{parse_ver_txt, Launcher};
use crate::model::{After, Entry, Lane, PayloadFile, Side, Step, StepKind, Versions, ABSENT};
use crate::script::{Expander, Invocation};
use crate::tokens::{self, Token};

/// EFI_FIRMWARE_MANAGEMENT_CAPSULE_ID_GUID, mixed-endian as it sits on disk.
const FMP_GUID: [u8; 16] = [
    0xed, 0xd5, 0xcb, 0x6d, 0x2d, 0xe8, 0x44, 0x4c, 0xbd, 0xa1, 0x71, 0x94, 0x19, 0x9a, 0xd9, 0x2a,
];

/// BIOS recipes to try when the directory has no numbered steps, best first.
/// The branding variants are alternatives, so each is scored on its own.
const FALLBACK_BIOS: [&str; 4] = ["FlashPCL", "FlashXDX", "FlashBBX", "flash"];

/// EC recipes, which pair with whichever BIOS recipe is chosen.
const FALLBACK_EC: [&str; 2] = ["FlashEC", "EcFlash"];

/// Rounds a menu script is followed through the siblings it names.
const MENU_ROUNDS: usize = 3;

/// A directory holds firmware if it has a script, a tool or a sizeable payload.
pub fn is_model_dir(dir: &DirIndex) -> bool {
    dir.iter().any(|f| {
        matches!(
            f.ext().to_ascii_lowercase().as_str(),
            "nsh" | "bat" | "efi" | "exe" | "com"
        )
    }) || !dir.bare_payloads().is_empty()
}

/// Index one model directory, or `None` when it holds no firmware.
pub fn build(side: Side, path: &Path, folder: &str, launchers: &[Launcher]) -> Result<Option<Entry>> {
    let dir = DirIndex::read(path)?;
    if !is_model_dir(&dir) {
        return Ok(None);
    }
    let mut warnings: Vec<String> = Vec::new();

    let (aliases, patterns, versions, modelstring) =
        identity(&dir, folder, launchers, &mut warnings);
    if is_tool_dump(&dir, folder, &aliases, &patterns, &modelstring) {
        return Ok(None);
    }

    let invocations = choose_recipe(&dir, folder, launchers, &mut warnings);

    let capsule = dir
        .bare_payloads()
        .iter()
        .any(|f| is_fmp_capsule(&dir, f));

    let (lane, steps) = if invocations.is_empty() {
        let lane = if capsule { Lane::Capsule } else { Lane::InBiosOnly };
        (lane, bare_steps(&dir, lane, &mut warnings))
    } else {
        (
            lane_of(&dir, &invocations),
            real_steps(&dir, &invocations, &mut warnings),
        )
    };

    let mut reachable = !steps.is_empty() && steps.iter().all(|s| s.resolved);
    if steps.is_empty() {
        warnings.push("no flash recipe and no payload found".to_string());
    }
    // A lane the firmware launches must actually write a BIOS image; a probe or
    // an EC-only chain that reports ready would flash nothing.
    if reachable && lane.launchable() && !steps.iter().any(|s| s.kind == StepKind::Bios) {
        warnings.push(
            "this recipe writes no BIOS image; it probes or flashes EC only, so the real flasher \
             is somewhere this index cannot follow"
                .to_string(),
        );
        reachable = false;
    }
    sweep_missing(&dir, &mut warnings);

    Ok(Some(Entry {
        folder: folder.to_string(),
        side,
        aliases,
        patterns,
        modelstring,
        versions,
        lane,
        reachable,
        steps,
        warnings: unique(warnings),
    }))
}

/// A shared tool dump such as `laptop/EFI`: it names no machine, carries no
/// image of its own and its folder name is not a chassis token.
fn is_tool_dump(
    dir: &DirIndex,
    folder: &str,
    aliases: &[String],
    patterns: &[String],
    modelstring: &str,
) -> bool {
    aliases.is_empty()
        && patterns.is_empty()
        && modelstring.trim().is_empty()
        && dir.get("ver.txt").is_none()
        && dir.bare_payloads().is_empty()
        && !tokens::is_model_token(folder)
}

/// Check every filename any script in the folder names, not just the recipe's.
/// Later drops delete payloads without touching the scripts that call for them.
fn sweep_missing(dir: &DirIndex, warnings: &mut Vec<String>) {
    let mut scripts: Vec<String> = dir
        .iter()
        .filter(|f| matches!(f.ext().to_ascii_lowercase().as_str(), "nsh" | "bat"))
        .map(|f| f.name.clone())
        .collect();
    scripts.sort_by_key(|s| s.to_ascii_lowercase());

    for script in &scripts {
        let mut expander = Expander::new(dir);
        expander.expand(script);
        let absent: Vec<String> = expander
            .missing_scripts()
            .iter()
            .map(|(origin, called)| format!("{origin} calls {called}{ABSENT}"))
            .collect();
        warnings.extend(absent);
        for inv in expander.finish() {
            if dir.resolve_exec_in(&inv.cwd, &inv.exec, !inv.from_bat).is_none() {
                warnings.push(format!("{} runs {}{ABSENT}", inv.origin, inv.exec));
            }
            for name in &inv.files {
                if dir.get_in(&inv.cwd, name).is_none() {
                    warnings.push(format!("{} references {name}{ABSENT}", inv.origin));
                }
            }
        }
    }
}

/// Comparison key for a token, wildcards included.
fn key_of(tok: &Token) -> String {
    match tok {
        Token::Exact(t) => tokens::normalize(t),
        Token::Pattern(p) => p
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '?')
            .map(|c| c.to_ascii_uppercase())
            .collect(),
    }
}

/// MSI board numbers named by the files in the folder, plus the title line of
/// its release note.
fn msi_identity(dir: &DirIndex) -> (BTreeSet<String>, Option<String>) {
    let mut boards = BTreeSet::new();
    let mut note = None;
    for f in dir.iter() {
        let upper = f.name.to_ascii_uppercase();
        let Some(code) = msi_board_code(&upper) else {
            continue;
        };
        boards.insert(format!("MS-{code}"));
        if note.is_none() && upper.ends_with(".TXT") && f.size < 16 * 1024 {
            note = dir.read_text(&f.name).and_then(|text| {
                text.lines()
                    .map(str::trim)
                    .find(|l| l.len() > 8 && l.starts_with(|c: char| c.is_ascii_alphanumeric()))
                    .map(str::to_string)
            });
        }
    }
    (boards, note)
}

/// `E7A38AMS.MG2` and `7A38vMx.txt` both name board MS-7A38.
fn msi_board_code(upper_name: &str) -> Option<String> {
    let stem = upper_name.split('.').next()?;
    let bytes = stem.as_bytes();
    let alnum = |s: &str| s.bytes().all(|c| c.is_ascii_alphanumeric());
    if bytes.len() >= 8 && bytes[0] == b'E' && bytes[1].is_ascii_digit() && alnum(&stem[1..5]) {
        let suffix = &stem[5..];
        if ["AMS", "IMS", "IMT"].iter().any(|s| suffix.starts_with(s)) {
            return Some(stem[1..5].to_string());
        }
    }
    if bytes.len() >= 5 && bytes[0].is_ascii_digit() && bytes[4] == b'V' && alnum(&stem[..4]) {
        return Some(stem[..4].to_string());
    }
    None
}

fn push(tok: Token, exact: &mut BTreeSet<String>, patterns: &mut BTreeSet<String>) {
    match tok {
        Token::Exact(t) => {
            exact.insert(t);
        }
        Token::Pattern(p) => {
            patterns.insert(p);
        }
    }
}

fn unique(warnings: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    warnings.into_iter().filter(|w| seen.insert(w.clone())).collect()
}

/// Chassis tokens and versions, from `ver.txt` first and the launchers second.
fn identity(
    dir: &DirIndex,
    folder: &str,
    launchers: &[Launcher],
    warnings: &mut Vec<String>,
) -> (Vec<String>, Vec<String>, Versions, String) {
    let mut exact: BTreeSet<String> = BTreeSet::new();
    let mut patterns: BTreeSet<String> = BTreeSet::new();
    let mut versions = Versions::default();

    // The folder name is matched literally by the firmware; only its wildcard
    // form has to be added.
    if let (Token::Pattern(p), _) = tokens::classify(folder) {
        patterns.insert(p);
    }

    let mut ver_tokens: HashSet<String> = HashSet::new();
    if let Some(text) = dir.read_text("ver.txt") {
        let ver = parse_ver_txt(&text);
        versions = Versions {
            bios: ver.bios,
            ec: ver.ec,
            me: ver.me,
        };
        if !ver.has_versions {
            warnings.push("ver.txt carries release notes, not a B:/E: version header".to_string());
        }
        for line in &ver.tokens {
            let (toks, warns) = tokens::tokens_of(line);
            for t in toks {
                ver_tokens.insert(key_of(&t));
                push(t, &mut exact, &mut patterns);
            }
            warnings.extend(warns);
        }
    }

    // MSI stamps the board number SMBIOS reports into its payload and release
    // note filenames; the folder name is only the shop's nickname for it.
    let (boards, note) = msi_identity(dir);
    if boards.len() > 1 {
        // Two boards' images in one folder: neither number identifies it, and
        // claiming both makes the folder exact-match the wrong machine.
        warnings.push(format!(
            "payloads name {} MSI boards ({}); no board alias was emitted, and one of these \
             images does not belong in this folder",
            boards.len(),
            boards.iter().cloned().collect::<Vec<_>>().join(", ")
        ));
    } else {
        exact.extend(boards);
    }
    let mut modelstring = note.clone().unwrap_or_default();
    if let Some(line) = &note {
        let (toks, warns) = tokens::tokens_of(line);
        for t in toks {
            push(t, &mut exact, &mut patterns);
        }
        warnings.extend(warns);
    }

    for l in launchers {
        for name in [l.model.as_deref(), Some(l.stem.as_str())].into_iter().flatten() {
            if tokens::is_generic_script(name)
                || tokens::is_brand_variant(name)
                || !tokens::is_model_token(name)
            {
                continue;
            }
            let (tok, warn) = tokens::classify(name);
            push(tok, &mut exact, &mut patterns);
            warnings.extend(warn);
        }

        let Some(ms) = l.modelstring.as_deref() else {
            continue;
        };
        let (toks, warns) = tokens::tokens_of(ms);
        // ver.txt wins: a MODELSTRING naming an unrelated family is a copy-paste
        // error and must not widen this folder's match.
        let agrees = ver_tokens.is_empty() || toks.iter().any(|t| ver_tokens.contains(&key_of(t)));
        if agrees {
            for t in toks {
                push(t, &mut exact, &mut patterns);
            }
            warnings.extend(warns);
        } else {
            warnings.push(format!(
                "{} declares MODELSTRING \"{ms}\", which names no model in ver.txt; its tokens were dropped",
                l.file
            ));
        }
        if modelstring.is_empty()
            || l.model
                .as_deref()
                .is_some_and(|m| m.eq_ignore_ascii_case(folder))
        {
            modelstring = ms.to_string();
        }
    }

    let folder_key = tokens::normalize(folder);
    let aliases = exact
        .into_iter()
        .filter(|a| tokens::normalize(a) != folder_key)
        .collect();
    (aliases, patterns.into_iter().collect(), versions, modelstring)
}

/// Flatten one candidate recipe.
fn run(dir: &DirIndex, set: &[String]) -> Vec<Invocation> {
    let mut expander = Expander::new(dir);
    for script in set {
        expander.expand(script);
    }
    dedup(expander.finish())
}

/// Every exec resolves and every payload the recipe names is on disk.
fn fully_resolved(dir: &DirIndex, invocations: &[Invocation]) -> bool {
    invocations.iter().all(|i| {
        i.unresolved_vars.is_empty()
            && dir.resolve_exec_in(&i.cwd, &i.exec, !i.from_bat).is_some()
            && i.files.iter().all(|f| dir.get_in(&i.cwd, f).is_some())
    })
}

fn any_exec_resolves(dir: &DirIndex, invocations: &[Invocation]) -> bool {
    invocations
        .iter()
        .any(|i| dir.resolve_exec_in(&i.cwd, &i.exec, !i.from_bat).is_some())
}

/// Run the recipes in priority order and keep the first one that is complete:
/// every tool and every payload it names is still in the folder. A branding
/// variant whose ROM was never copied in must not bury the sibling recipe that
/// is intact, so an incomplete candidate is only a fallback.
fn choose_recipe(
    dir: &DirIndex,
    folder: &str,
    launchers: &[Launcher],
    warnings: &mut Vec<String>,
) -> Vec<Invocation> {
    let mut partial: Vec<Invocation> = Vec::new();
    let mut fallback: Vec<Invocation> = Vec::new();
    for set in candidate_recipes(dir, folder, launchers) {
        let invocations = run(dir, &set);
        if invocations.is_empty() {
            continue;
        }
        if fully_resolved(dir, &invocations) {
            return invocations;
        }
        if partial.is_empty() && any_exec_resolves(dir, &invocations) {
            partial = invocations;
        } else if fallback.is_empty() {
            fallback = invocations;
        }
    }
    if !partial.is_empty() {
        return partial;
    }
    if !fallback.is_empty() {
        return fallback;
    }
    menu_recipe(dir, folder, launchers, warnings)
}

/// Recipe entry points, best first: numbered steps, then what the root launcher
/// calls, then the conventional script names, one candidate per branding
/// variant. Every UEFI candidate is tried before any DOS one.
fn candidate_recipes(dir: &DirIndex, folder: &str, launchers: &[Launcher]) -> Vec<Vec<String>> {
    let mut sets: Vec<Vec<String>> = Vec::new();
    for ext in ["nsh", "bat"] {
        sets.push(
            (1..=9)
                .map(|n| format!("step{n}.{ext}"))
                .filter(|s| dir.get(s).is_some())
                .collect(),
        );
        let called: Vec<String> = launchers
            .iter()
            .flat_map(|l| l.calls.iter())
            .filter(|c| c.ends_with(ext) && dir.get(c).is_some())
            .cloned()
            .collect();
        sets.push(unique(called));

        let ec: Vec<String> = FALLBACK_EC
            .iter()
            .map(|s| format!("{s}.{ext}"))
            .filter(|s| dir.get(s).is_some())
            .collect();
        let bios: Vec<String> = FALLBACK_BIOS
            .iter()
            .map(|s| format!("{s}.{ext}"))
            .filter(|s| dir.get(s).is_some())
            .collect();
        for one in &bios {
            let mut set = vec![one.clone()];
            set.extend(ec.iter().cloned());
            sets.push(set);
        }
        if bios.is_empty() {
            sets.push(ec);
        }

        let own = format!("{folder}.{ext}");
        sets.push(dir.get(&own).map(|_| own).into_iter().collect());
    }
    sets.retain(|s| !s.is_empty());
    sets
}

/// Every script the ordinary candidates start from, deduplicated.
fn entry_scripts(dir: &DirIndex, folder: &str, launchers: &[Launcher]) -> Vec<String> {
    unique(
        candidate_recipes(dir, folder, launchers)
            .into_iter()
            .flatten()
            .collect(),
    )
}

/// When every candidate came up empty the folder's launcher is a menu, not a
/// recipe: follow the sibling scripts it tells the operator to type.
fn menu_recipe(
    dir: &DirIndex,
    folder: &str,
    launchers: &[Launcher],
    warnings: &mut Vec<String>,
) -> Vec<Invocation> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut frontier = entry_scripts(dir, folder, launchers);
    for name in &frontier {
        seen.insert(name.to_ascii_lowercase());
    }
    for _ in 0..MENU_ROUNDS {
        let mut next: Vec<String> = Vec::new();
        for script in &frontier {
            for target in chooser_targets(dir, script) {
                if seen.insert(target.to_ascii_lowercase()) {
                    next.push(target);
                }
            }
        }
        if next.is_empty() {
            return Vec::new();
        }
        let invocations = run(dir, &next);
        if !invocations.is_empty() {
            let chosen: Vec<&str> = next
                .iter()
                .filter(|s| !run(dir, std::slice::from_ref(s)).is_empty())
                .map(String::as_str)
                .collect();
            if chosen.len() > 1 {
                warnings.push(format!(
                    "the folder's launcher is a menu offering {}; the steps below are all of \
                     them in order, not one recipe",
                    chosen.join(", ")
                ));
            }
            return invocations;
        }
        frontier = next;
    }
    Vec::new()
}

/// Sibling scripts a menu names, the ones it tells the operator to type first.
fn chooser_targets(dir: &DirIndex, script: &str) -> Vec<String> {
    let Some(body) = dir.read_text(script) else {
        return Vec::new();
    };
    // `type help.txt` prints the menu from a note beside the script.
    let mut sources = vec![script.to_string()];
    for line in body.lines() {
        let line = line.trim().trim_start_matches('@').trim();
        if !line.get(..5).is_some_and(|h| h.eq_ignore_ascii_case("type ")) {
            continue;
        }
        let named = line[5..].trim().trim_matches('"');
        if dir.get(named).is_some() && !named.eq_ignore_ascii_case(script) {
            sources.push(named.to_string());
        }
    }

    let mut typed: Vec<String> = Vec::new();
    let mut other: Vec<String> = Vec::new();
    for source in &sources {
        let Some(text) = dir.read_text(source) else {
            continue;
        };
        let words: Vec<&str> = text
            .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')))
            .map(|w| w.trim_matches(['.', '-']))
            .filter(|w| !w.is_empty())
            .collect();
        for (i, word) in words.iter().enumerate() {
            let Some(name) = sibling_script(dir, word) else {
                continue;
            };
            if sources.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
                continue;
            }
            let bucket = if i > 0 && words[i - 1].eq_ignore_ascii_case("type") {
                &mut typed
            } else {
                &mut other
            };
            if !bucket.iter().any(|x| x.eq_ignore_ascii_case(&name)) {
                bucket.push(name);
            }
        }
    }
    for name in other {
        if !typed.iter().any(|x| x.eq_ignore_ascii_case(&name)) {
            typed.push(name);
        }
    }
    typed
}

/// The `.bat` or `.nsh` in this folder that `word` names, if any.
fn sibling_script(dir: &DirIndex, word: &str) -> Option<String> {
    let lower = word.to_ascii_lowercase();
    if lower.ends_with(".bat") || lower.ends_with(".nsh") {
        return dir.get(word).filter(|f| f.is_top()).map(|f| f.name.clone());
    }
    ["bat", "nsh"].iter().find_map(|ext| {
        dir.get(&format!("{word}.{ext}"))
            .filter(|f| f.is_top())
            .map(|f| f.name.clone())
    })
}

/// Drop commands a branchy script reaches twice.
fn dedup(invocations: Vec<Invocation>) -> Vec<Invocation> {
    let mut seen = HashSet::new();
    invocations
        .into_iter()
        .filter(|i| {
            seen.insert((
                i.cwd.to_ascii_lowercase(),
                i.exec.to_ascii_lowercase(),
                i.args.to_ascii_lowercase(),
            ))
        })
        .collect()
}

/// The lane a recipe runs on, judged by the tool each command resolves to: a
/// subdirectory recipe calls its flasher by a bare name.
fn lane_of(dir: &DirIndex, invocations: &[Invocation]) -> Lane {
    let is_efi = |i: &Invocation| {
        if i.exec.to_ascii_lowercase().ends_with(".efi") {
            return true;
        }
        !i.from_bat
            && dir
                .resolve_exec_in(&i.cwd, &i.exec, true)
                .is_some_and(|m| m.ext().eq_ignore_ascii_case("efi"))
    };
    if invocations.iter().any(is_efi) {
        return Lane::Uefi;
    }
    let windows = invocations
        .iter()
        .all(|i| i.from_bat && i.exec.to_ascii_lowercase().contains("win"));
    if windows {
        Lane::WindowsOnly
    } else {
        Lane::DosOnly
    }
}

fn real_steps(dir: &DirIndex, invocations: &[Invocation], warnings: &mut Vec<String>) -> Vec<Step> {
    let mut steps = Vec::with_capacity(invocations.len());
    for (i, inv) in invocations.iter().enumerate() {
        let prefer_efi = !inv.from_bat;
        let resolved_exec = dir.resolve_exec_in(&inv.cwd, &inv.exec, prefer_efi);
        let exec_name = resolved_exec.map_or_else(|| inv.exec.clone(), |m| m.name.clone());
        let exec_sha = match resolved_exec {
            Some(m) => dir.sha256(&m.name).unwrap_or_default(),
            None => {
                warnings.push(format!("{} runs {}{ABSENT}", inv.origin, inv.exec));
                String::new()
            }
        };

        let mut resolved = resolved_exec.is_some();
        let mut files = Vec::with_capacity(inv.files.len());
        for name in &inv.files {
            match dir.get_in(&inv.cwd, name) {
                Some(m) => files.push(PayloadFile {
                    name: m.name.clone(),
                    sha256: dir.sha256(&m.name).unwrap_or_default(),
                    size: m.size,
                }),
                None => {
                    resolved = false;
                    warnings.push(format!("{} references {name}{ABSENT}", inv.origin));
                    files.push(PayloadFile {
                        name: name.clone(),
                        sha256: String::new(),
                        size: 0,
                    });
                }
            }
        }

        let mut note = String::new();
        if !inv.unresolved_vars.is_empty() {
            resolved = false;
            note = format!("unset variable %{}%", inv.unresolved_vars.join("%, %"));
            warnings.push(format!(
                "{} uses {note} so its payload cannot be named",
                inv.origin
            ));
        }
        // A `%NAME%` that survived expansion leaves the real target unnamed, so
        // the command cannot be replayed as written.
        if note.is_empty() && (exec_name.contains('%') || inv.args.contains('%')) {
            resolved = false;
            note = "a variable in this command was never expanded".to_string();
            warnings.push(format!(
                "{} leaves a variable unexpanded in \"{exec_name} {}\"",
                inv.origin,
                inv.args.trim()
            ));
        }

        let size = resolved_exec.map_or(0, |m| m.size);
        steps.push(Step {
            index: i as u32 + 1,
            kind: classify_kind(&exec_name, &inv.args, size, resolved_exec.is_some()),
            exec: exec_name,
            exec_sha256: exec_sha,
            args: inv.args.clone(),
            files,
            after: after_of(&inv.args, &inv.exec, resolved),
            resolved,
            note,
        });
    }
    steps
}

/// A directory whose payload has no launcher still gets its payload indexed so
/// the diff can see it change.
fn bare_steps(dir: &DirIndex, lane: Lane, warnings: &mut Vec<String>) -> Vec<Step> {
    let note = match lane {
        Lane::Capsule => "FMP capsule payload; no launcher script in this folder",
        _ if dir.has_executable() => {
            "the folder has a flasher but no script naming this payload; check the vendor's own \
             instructions before flashing"
        }
        _ => "no launcher script; flash this payload from the vendor's in-BIOS updater",
    };
    let payloads: Vec<FileMeta> = dir.bare_payloads().into_iter().cloned().collect();
    if payloads.len() > 1 {
        warnings.push(format!(
            "{} payloads and no script to choose between them: {}",
            payloads.len(),
            payloads
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    payloads
        .iter()
        .enumerate()
        .map(|(i, f)| Step {
            index: i as u32 + 1,
            kind: StepKind::Bios,
            exec: f.name.clone(),
            exec_sha256: dir.sha256(&f.name).unwrap_or_default(),
            args: String::new(),
            files: vec![PayloadFile {
                name: f.name.clone(),
                sha256: dir.sha256(&f.name).unwrap_or_default(),
                size: f.size,
            }],
            after: After::Unknown,
            resolved: true,
            note: note.to_string(),
        })
        .collect()
}

fn classify_kind(exec: &str, args: &str, size: u64, resolved: bool) -> StepKind {
    let e = exec.rsplit('/').next().unwrap_or(exec).to_ascii_lowercase();
    let a = args.to_ascii_lowercase();
    if e.contains("kbdetectck") || e.contains("ckmever") {
        return StepKind::Gate;
    }
    if e.contains("uecflash") || e.contains("ecflash") || e.starts_with("elash") || e.contains("ifu")
    {
        return StepKind::Ec;
    }
    if e.contains("meset") || e.contains("meinfo") || e.contains("memanuf") || e.contains("fwupdlcl")
    {
        return StepKind::Me;
    }
    if e.starts_with("fpt") {
        return if a.contains("closemnf") {
            StepKind::Gate
        } else {
            StepKind::Me
        };
    }
    if e.contains("afu") || e.contains("efiflash") {
        return StepKind::Bios;
    }
    if e.contains("amide")
        || e.contains("oaid")
        || e.contains("gmsdm")
        || e.contains("chksum")
        || e.contains("checksum")
        || e.contains("forcepoweroff")
        || e.starts_with("open")
    {
        return StepKind::Other;
    }
    // Anything else a recipe launches is the vendor's own BIOS image, but a word
    // that names no file is a typo in the script, not a flasher.
    if !resolved {
        return if e.contains('.') {
            StepKind::Bios
        } else {
            StepKind::Other
        };
    }
    if size >= 1024 * 1024 {
        StepKind::Bios
    } else {
        StepKind::Other
    }
}

fn after_of(args: &str, exec: &str, resolved: bool) -> After {
    if !resolved {
        return After::Unknown;
    }
    let a = args.to_ascii_lowercase();
    let e = exec.to_ascii_lowercase();
    if a.contains("shutdown") || e.contains("forcepoweroff") {
        After::Shutdown
    } else if a.contains("reboot") {
        After::Reboot
    } else {
        After::Returns
    }
}

/// Spec-conformant EFI_CAPSULE_HEADER carrying the FMP GUID.
fn is_fmp_capsule(dir: &DirIndex, file: &FileMeta) -> bool {
    let Some(head) = dir.head(&file.name, 28) else {
        return false;
    };
    if head.len() < 28 || head[..16] != FMP_GUID {
        return false;
    }
    let header_size = u32::from_le_bytes([head[16], head[17], head[18], head[19]]);
    let image_size = u32::from_le_bytes([head[24], head[25], head[26], head[27]]);
    header_size >= 28 && u64::from(image_size) == file.size
}
