//! Flatten a vendor `.nsh` / `.bat` recipe into the ordered tool invocations it
//! performs, following includes and `cd` into subdirectories and expanding
//! `set` variables.

use std::collections::{HashMap, HashSet};

use crate::dirindex::DirIndex;

/// Shell keywords and built-ins that invoke no vendor tool.
const BUILTIN: [&str; 41] = [
    "echo", "pause", "cls", "goto", "if", "endif", "else", "for", "endfor", "set", "del", "rem",
    "type", "exit", "pushd", "popd", "cd", "copy", "xcopy", "md", "mkdir", "rd", "attrib", "cmd",
    "start", "timeout", "color", "title", "mode", "path", "prompt", "shift", "setlocal",
    "endlocal", "choice", "call", "reset", "map", "mount", "stall", "vol",
];

/// Files the flash scripts create at run time; absent on disk by design.
const RUNTIME_FILE: [&str; 2] = ["msdm.bin", "yes.txt"];

/// Labels that end the script rather than choosing an alternative recipe.
const TERMINAL_LABEL: [&str; 6] = ["end", "exit", "eof", "quit", "done", "lerror"];

/// Substitution rounds before a `%NAME%` chain is called circular.
const EXPAND_PASSES: usize = 8;

#[derive(Debug, Clone)]
pub struct Invocation {
    /// Script the command came from, relative to the model folder.
    pub origin: String,
    /// Directory the command runs in, relative to the model folder.
    pub cwd: String,
    /// Tool as written, after variable expansion.
    pub exec: String,
    pub args: String,
    /// Arguments that name a payload file.
    pub files: Vec<String>,
    /// Variables the script never defined.
    pub unresolved_vars: Vec<String>,
    /// The command came from a `.bat`, so it runs under DOS or Windows.
    pub from_bat: bool,
}

pub struct Expander<'a> {
    dir: &'a DirIndex,
    vars: HashMap<String, String>,
    visited: HashSet<String>,
    out: Vec<Invocation>,
    /// One flag per open `if` block: false means the block does not run.
    blocks: Vec<bool>,
    /// Working directory, relative to the model folder.
    cwd: String,
    /// Label a taken `goto` is skipping forward to.
    pending_goto: Option<String>,
    /// Block depth the running script started at; a `goto` unwinds to it.
    goto_floor: usize,
    /// `(origin, script)` for callees the folder does not have.
    missing: Vec<(String, String)>,
}

impl<'a> Expander<'a> {
    pub fn new(dir: &'a DirIndex) -> Self {
        Self {
            dir,
            vars: HashMap::new(),
            visited: HashSet::new(),
            out: Vec::new(),
            blocks: Vec::new(),
            cwd: String::new(),
            pending_goto: None,
            goto_floor: 0,
            missing: Vec::new(),
        }
    }

    pub fn finish(self) -> Vec<Invocation> {
        self.out
    }

    /// Scripts a recipe called that the folder does not carry.
    pub fn missing_scripts(&self) -> &[(String, String)] {
        &self.missing
    }

    /// Run one script, following the scripts it calls.
    pub fn expand(&mut self, script: &str) {
        let dir: &'a DirIndex = self.dir;
        let Some(meta) = dir.get_in(&self.cwd, script) else {
            return;
        };
        let name = meta.name.clone();
        if !self.visited.insert(name.to_ascii_lowercase()) {
            return;
        }
        let Some(text) = dir.read_text(&name) else {
            return;
        };
        let from_bat = name.to_ascii_lowercase().ends_with(".bat");
        let depth = self.blocks.len();
        let outer_floor = std::mem::replace(&mut self.goto_floor, depth);
        let outer_goto = self.pending_goto.take();
        for line in text.lines() {
            self.line(&name, line, from_bat);
        }
        self.pending_goto = outer_goto;
        self.goto_floor = outer_floor;
        self.blocks.truncate(depth);
    }

    fn line(&mut self, origin: &str, raw: &str, from_bat: bool) {
        let line = clean_line(raw);
        // The UEFI shell writes the running script's own name as a bare `%0`.
        let line = if line.contains("%0") {
            line.replace("%0", origin.rsplit('/').next().unwrap_or(origin))
        } else {
            line
        };
        let line = line.as_str();
        if line.is_empty() {
            return;
        }
        if let Some(label) = line.strip_prefix(':') {
            let label = label.trim().to_ascii_lowercase();
            if self.pending_goto.as_deref() == Some(label.as_str()) {
                self.pending_goto = None;
            }
            return;
        }
        if self.pending_goto.is_some() || starts_skipped(line) {
            return;
        }
        if self.control_flow(line) {
            return;
        }
        if !self.blocks.iter().all(|open| *open) {
            return;
        }
        // A UEFI flasher wraps its default command in a caller-argument variant
        // the bare run never reaches; a DOS launcher instead appends `%1 %2 %3`
        // as pass-through to its one real command.
        let line = if from_bat {
            strip_trailing_positionals(line)
        } else {
            line
        };
        if has_positional(line) {
            return;
        }
        if let Some(rest) = strip_ci(line, "set ") {
            self.set_var(rest);
            return;
        }
        // Only the UEFI shell uses `goto` to dispatch between recipes.
        if !from_bat {
            if let Some(target) = strip_ci(line, "goto ") {
                self.goto(target);
                return;
            }
        }
        if let Some(target) = strip_ci(line, "cd ") {
            self.change_dir(target);
            return;
        }
        let line = strip_ci(line, "call ").unwrap_or(line);
        let first = line.split_whitespace().next().unwrap_or("");
        if is_builtin(first) {
            return;
        }

        let (expanded, unresolved_vars) = self.expand_vars(line);
        let mut parts = split_args(&expanded);
        if parts.is_empty() {
            return;
        }
        let exec = parts.remove(0);
        let exec_lower = exec.to_ascii_lowercase();
        if exec_lower.ends_with(".nsh") || exec_lower.ends_with(".bat") {
            if self.dir.get_in(&self.cwd, &exec).is_none() {
                self.missing.push((origin.to_string(), exec.clone()));
            }
            self.expand(&exec);
            return;
        }
        // A stray sentence in an `@Echo` block is not a command.
        if self.dir.resolve_exec_in(&self.cwd, &exec, !from_bat).is_none()
            && is_prose(&exec, &parts)
        {
            return;
        }

        let files = parts.iter().filter(|a| self.looks_like_payload(a)).cloned().collect();
        self.out.push(Invocation {
            origin: origin.to_string(),
            cwd: self.cwd.clone(),
            exec,
            args: parts.join(" "),
            files,
            unresolved_vars,
            from_bat,
        });
    }

    /// Track `if`/`else`/`endif`, deciding `if [not] exist` against the listing
    /// so a dead branch's payloads are not reported missing. Returns true when
    /// the line was block structure rather than a command.
    fn control_flow(&mut self, line: &str) -> bool {
        if line.eq_ignore_ascii_case("endif") {
            self.blocks.pop();
            return true;
        }
        if line.eq_ignore_ascii_case("else") {
            if let Some(open) = self.blocks.last_mut() {
                *open = !*open;
            }
            return true;
        }
        let Some(cond) = strip_ci(line, "if ") else {
            return false;
        };
        if cond.len() < 4 || !cond[cond.len() - 4..].eq_ignore_ascii_case("then") {
            return false;
        }
        let (cond, _) = self.expand_vars(&cond[..cond.len() - 4]);
        let cond = cond.trim();
        let negated = strip_ci(cond, "not ").is_some();
        let cond = strip_ci(cond, "not ").unwrap_or(cond);
        let open = match strip_ci(cond, "exist ") {
            Some(name) => {
                let name = name.trim().trim_matches('"');
                // A file an earlier step writes is absent from the listing by design.
                if is_runtime_file(name) {
                    !negated
                } else {
                    self.dir.get_in(&self.cwd, name).is_some() != negated
                }
            }
            // A runtime condition such as %Lasterror%; take the branch.
            None => true,
        };
        self.blocks.push(open);
        true
    }

    /// Follow a `goto` that selects a branch. A jump to the end of the script
    /// from inside a runtime `if` is the failure path, so it stays untaken.
    fn goto(&mut self, target: &str) {
        let target = target.trim().trim_start_matches(':').to_ascii_lowercase();
        if target.is_empty() {
            return;
        }
        if self.blocks.len() > self.goto_floor && TERMINAL_LABEL.contains(&target.as_str()) {
            return;
        }
        self.blocks.truncate(self.goto_floor);
        self.pending_goto = Some(target);
    }

    /// Descend into a subdirectory the folder carries; other paths are the
    /// vendor's own and cannot be followed.
    fn change_dir(&mut self, target: &str) {
        let dir: &'a DirIndex = self.dir;
        let (target, _) = self.expand_vars(target);
        let target = target.trim().trim_matches('"').replace('\\', "/");
        if target.starts_with('/') {
            self.cwd.clear();
        }
        for part in target.split('/').filter(|p| !p.is_empty() && *p != ".") {
            if part == ".." {
                self.cwd = self
                    .cwd
                    .rsplit_once('/')
                    .map_or(String::new(), |(parent, _)| parent.to_string());
                continue;
            }
            match dir.subdir_in(&self.cwd, part) {
                Some(path) => self.cwd = path.to_string(),
                None => return,
            }
        }
    }

    fn set_var(&mut self, rest: &str) {
        let rest = strip_ci(rest, "-v ").unwrap_or(rest);
        let (name, value) = match rest.split_once('=') {
            Some(pair) => pair,
            None => match rest.split_once(char::is_whitespace) {
                Some(pair) => pair,
                None => return,
            },
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().trim_matches('"').trim().to_string();
        if !name.is_empty() {
            self.vars.insert(name, value);
        }
    }

    /// Substitute `%NAME%` until nothing changes: a variable's value routinely
    /// names other variables. Reports names the script never set.
    fn expand_vars(&self, line: &str) -> (String, Vec<String>) {
        let mut out = line.to_string();
        let mut unresolved = Vec::new();
        for _ in 0..EXPAND_PASSES {
            let (next, missing) = self.substitute(&out);
            unresolved = missing;
            if next == out {
                break;
            }
            out = next;
        }
        (out, unresolved)
    }

    /// One substitution pass over `%NAME%`.
    fn substitute(&self, line: &str) -> (String, Vec<String>) {
        let mut out = String::with_capacity(line.len());
        let mut unresolved = Vec::new();
        let mut rest = line;
        while let Some(open) = rest.find('%') {
            out.push_str(&rest[..open]);
            let after = &rest[open + 1..];
            let Some(close) = after.find('%') else {
                out.push_str(&rest[open..]);
                return (out, unresolved);
            };
            let name = &after[..close];
            match self.vars.get(&name.to_ascii_lowercase()) {
                Some(value) => out.push_str(value),
                None => {
                    out.push('%');
                    out.push_str(name);
                    out.push('%');
                    unresolved.push(name.to_string());
                }
            }
            rest = &after[close + 1..];
        }
        out.push_str(rest);
        (out, unresolved)
    }

    /// True when an argument names a payload rather than a switch or a number.
    fn looks_like_payload(&self, arg: &str) -> bool {
        if arg.len() < 4 || arg.starts_with(['/', '-', '<', '>', '$', '%']) {
            return false;
        }
        if is_runtime_file(arg) {
            return false;
        }
        if self.dir.get_in(&self.cwd, arg).is_some() {
            return true;
        }
        // Dangling references still have to look like filenames: a version
        // string such as 0014.0000.0031.1120 carries no letters.
        let Some((stem, ext)) = arg.rsplit_once('.') else {
            return false;
        };
        !stem.is_empty()
            && (1..=4).contains(&ext.len())
            && arg.chars().any(|c| c.is_ascii_alphabetic())
    }
}

/// `echo.` and `rem:` are the same built-ins with punctuation stuck on.
fn is_builtin(word: &str) -> bool {
    let w = word.to_ascii_lowercase();
    let w = w.trim_end_matches(['.', ':', ',']);
    BUILTIN.contains(&w) || w.starts_with("echo")
}

fn is_runtime_file(name: &str) -> bool {
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase();
    RUNTIME_FILE.contains(&base.as_str())
}

/// Drop the C0 control bytes a DOS editor leaves behind, then the `@` prefix.
/// `503.BAT` ends in a bare 0x1A, which otherwise parses as a command.
fn clean_line(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .trim_start_matches('@')
        .trim()
        .to_string()
}

fn starts_skipped(line: &str) -> bool {
    let first = line.chars().next().unwrap_or(' ');
    // A command starts with a name, a variable or a path.
    if !(first.is_ascii_alphanumeric() || matches!(first, '_' | '%' | '.' | '\\' | '/')) {
        return true;
    }
    // `fs0:` style drive changes.
    line.ends_with(':') && !line.contains(char::is_whitespace)
}

/// True when a word is followed by plain English rather than switches or
/// filenames, as in `@Test sound in Windows 8.` typed for `@Echo`.
fn is_prose(exec: &str, args: &[String]) -> bool {
    !exec.contains('.')
        && args.len() >= 2
        && args
            .iter()
            .all(|a| !a.starts_with(['/', '-']) && !looks_like_filename(a))
}

fn looks_like_filename(arg: &str) -> bool {
    arg.rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty()
            && (1..=4).contains(&ext.len())
            && ext.chars().all(|c| c.is_ascii_alphanumeric())
    })
}

fn strip_trailing_positionals(line: &str) -> &str {
    let mut end = line.trim_end().len();
    while let Some(space) = line[..end].rfind(char::is_whitespace) {
        let last = &line.as_bytes()[space + 1..end];
        if last.len() == 2 && last[0] == b'%' && last[1].is_ascii_digit() {
            end = line[..space].trim_end().len();
        } else {
            break;
        }
    }
    &line[..end]
}

fn has_positional(line: &str) -> bool {
    let bytes = line.as_bytes();
    bytes
        .iter()
        .enumerate()
        .any(|(i, b)| *b == b'%' && matches!(bytes.get(i + 1), Some(b'1'..=b'9' | b'~')))
}

fn strip_ci<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    line.get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .map(|_| line[prefix.len()..].trim())
}

/// Whitespace split that keeps quoted arguments whole.
fn split_args(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in line.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_arguments_stay_whole() {
        assert_eq!(
            split_args(r#"AMIDEWINx64.EXE /SP "Slim Pro Series""#),
            ["AMIDEWINx64.EXE", "/SP", "Slim Pro Series"]
        );
    }

    #[test]
    fn positional_argument_lines_are_the_untaken_path() {
        assert!(has_positional("%FLASH_TOOL% %EC_ROM% %2 %3 %4"));
        assert!(!has_positional("%FLASH_TOOL% %EC_ROM% /ad /h3 /f2 /l"));
    }

    #[test]
    fn dos_end_of_file_byte_is_not_a_command() {
        assert_eq!(clean_line("\u{1a}"), "");
        assert_eq!(clean_line("@echo off\u{1a}"), "echo off");
    }

    #[test]
    fn punctuation_leading_a_line_is_not_a_tool() {
        assert!(starts_skipped("-e@echo off"));
        assert!(starts_skipped("#comment"));
        assert!(!starts_skipped("AfuEfix64.efi P870TM.8M /p"));
        assert!(!starts_skipped("%BIOSEXE%"));
    }

    #[test]
    fn a_sentence_is_not_an_invocation() {
        let words: Vec<String> = "sound in Windows 8."
            .split(' ')
            .map(str::to_string)
            .collect();
        assert!(is_prose("Test", &words));
        assert!(!is_prose("ecflash", &["ecD900F.4sa".to_string()]));
        assert!(!is_prose(
            "afuefix64",
            &["P870TM.8M".to_string(), "/p".to_string()]
        ));
    }
}
