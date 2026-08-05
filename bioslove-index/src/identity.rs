//! Model identity: `ver.txt` inside a model directory, and the sibling root
//! launcher scripts that name the same folder.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;

/// Lines past this are release notes, not an identity header.
const HEADER_LINES: usize = 10;

#[derive(Debug, Default)]
pub struct VerTxt {
    pub tokens: Vec<String>,
    pub bios: String,
    pub ec: String,
    pub me: String,
    /// A `B:` line was present.
    pub has_versions: bool,
}

/// Parse the vendor's `ver.txt`: an identity header, then `B:`/`E:`/`ME:`.
pub fn parse_ver_txt(text: &str) -> VerTxt {
    let mut out = VerTxt::default();
    let mut header = 0usize;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_ascii_uppercase();
            let value = value.trim().to_string();
            match key.as_str() {
                "B" | "BIOS" => {
                    out.has_versions = true;
                    if out.bios.is_empty() {
                        out.bios = value;
                    }
                    continue;
                }
                "E" | "EC" => {
                    if out.ec.is_empty() {
                        out.ec = value;
                    }
                    continue;
                }
                "ME" | "ME FW" | "TXE" => {
                    if out.me.is_empty() {
                        out.me = value;
                    }
                    continue;
                }
                _ => {}
            }
        }
        header += 1;
        if header <= HEADER_LINES {
            out.tokens.push(line.to_string());
        }
    }
    out
}

/// A root-level `<name>.nsh` / `<name>.bat` that boots one model directory.
#[derive(Debug)]
pub struct Launcher {
    pub file: String,
    pub stem: String,
    pub model: Option<String>,
    pub basemodel: String,
    pub modelstring: Option<String>,
    /// Scripts the launcher runs inside the model directory, in order.
    pub calls: Vec<String>,
}

/// Read every root launcher on one side, keyed by lowercase target folder.
pub fn read_launchers(side_dir: &Path) -> Result<HashMap<String, Vec<Launcher>>> {
    let mut out: HashMap<String, Vec<Launcher>> = HashMap::new();
    for entry in std::fs::read_dir(side_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let file = entry.file_name().to_string_lossy().into_owned();
        let Some((stem, ext)) = file.rsplit_once('.') else {
            continue;
        };
        let ext = ext.to_ascii_lowercase();
        if ext != "nsh" && ext != "bat" {
            continue;
        }
        let Ok(bytes) = std::fs::read(entry.path()) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        if let Some(l) = parse_launcher(&file, stem, &text) {
            out.entry(l.basemodel.to_ascii_lowercase())
                .or_default()
                .push(l);
        }
    }
    for list in out.values_mut() {
        list.sort_by_key(|l| l.file.to_ascii_lowercase());
    }
    Ok(out)
}

fn parse_launcher(file: &str, stem: &str, text: &str) -> Option<Launcher> {
    let mut model = None;
    let mut basemodel = None;
    let mut modelstring = None;
    let mut calls = Vec::new();
    let mut cd_seen = false;

    for line in text.lines() {
        let line = line.trim().trim_start_matches('@').trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = strip_ci(line, "set -v ") {
            if let Some((name, value)) = rest.split_once(char::is_whitespace) {
                let value = unquote(value);
                match name.to_ascii_uppercase().as_str() {
                    "MODEL" => model = Some(value),
                    "BASEMODEL" => basemodel = Some(value),
                    "MODELSTRING" => modelstring = Some(value),
                    _ => {}
                }
            }
            continue;
        }
        if let Some(rest) = strip_ci(line, "cd ") {
            let target = unquote(rest);
            if !target.is_empty() && !target.starts_with('%') {
                basemodel = Some(target);
            }
            cd_seen = true;
            continue;
        }
        // The .bat launchers only describe the model in the confirmation prompt.
        if let Some(rest) = strip_ci(line, "echo ") {
            if modelstring.is_none() {
                if let Some(m) = prompt_modelstring(rest) {
                    modelstring = Some(m);
                }
            }
            continue;
        }
        if cd_seen {
            let cmd = strip_ci(line, "call ").unwrap_or(line);
            let cmd = cmd.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
            if cmd.ends_with(".nsh") || cmd.ends_with(".bat") {
                calls.push(cmd);
            }
        }
    }

    let basemodel = basemodel.or_else(|| model.clone())?;
    let calls = calls
        .into_iter()
        .map(|c| {
            let c = c.replace("%basemodel%", &basemodel.to_ascii_lowercase());
            match &model {
                Some(m) => c.replace("%model%", &m.to_ascii_lowercase()),
                None => c,
            }
        })
        .collect();
    Some(Launcher {
        file: file.to_string(),
        stem: stem.to_string(),
        model,
        basemodel,
        modelstring,
        calls,
    })
}

fn strip_ci<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    line.get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .map(|_| line[prefix.len()..].trim())
}

/// Pull the model description out of a `Are you sure you want to flash a X?`
/// prompt, keeping vendor parentheses balanced.
fn prompt_modelstring(rest: &str) -> Option<String> {
    let lead = "are you sure you want to flash a";
    let body = match rest.get(..lead.len()) {
        Some(head) if head.eq_ignore_ascii_case(lead) => &rest[lead.len()..],
        _ => return None,
    };
    let mut body = body.trim().trim_end_matches(['?', ' ']).to_string();
    while body.ends_with(')')
        && body.matches(')').count() > body.matches('(').count()
    {
        body.pop();
    }
    let body = unquote(&body);
    (!body.is_empty() && body.len() < 80).then_some(body)
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clevo_ver_txt_yields_tokens_and_versions() {
        let v = parse_ver_txt("NH58DCQ NH70DCQ NH58DDW NH70DDW  03-18-2021\nB: 1.07.06\nE: 1.07.03\nME: 14.0.31.1120\n");
        assert_eq!(v.bios, "1.07.06");
        assert_eq!(v.ec, "1.07.03");
        assert_eq!(v.me, "14.0.31.1120");
        assert!(v.has_versions);
        assert_eq!(v.tokens.len(), 1);
    }

    #[test]
    fn release_notes_ver_txt_has_no_versions() {
        let v = parse_ver_txt("TP202NA_105\nER\n01\tE\tChange Memeory configuration;\n");
        assert!(!v.has_versions);
        assert!(v.bios.is_empty());
    }

    #[test]
    fn nsh_launcher_carries_model_variables() {
        let l = parse_launcher(
            "NH55RCQ.NSH",
            "NH55RCQ",
            "SET -V MODEL NH55RCQ \nSET -V BASEMODEL NH70RCQ \nSET -V MODELSTRING \"SM6/v5  NH70RCQ/NH55RCQ\"\ncd %BASEMODEL%\nflash.nsh\n",
        )
        .expect("launcher parses");
        assert_eq!(l.basemodel, "NH70RCQ");
        assert_eq!(l.model.as_deref(), Some("NH55RCQ"));
        assert_eq!(l.calls, ["flash.nsh"]);
    }
}
