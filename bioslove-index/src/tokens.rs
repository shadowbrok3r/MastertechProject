//! Chassis-token extraction from vendor strings, and the wildcard-vs-literal
//! call. A missed wildcard costs a match; a false wildcard flashes the wrong
//! machine, so anything ambiguous stays literal and raises a warning.

/// Branding variants of a model name; their trailing letters are not wildcards.
const BRAND_SUFFIX: [&str; 5] = ["bbx", "xdx", "pcl", "std", "factory"];

/// Script names that live in every model directory and name no model.
const GENERIC_SCRIPT: [&str; 50] = [
    "step1",
    "step2",
    "step3",
    "step4",
    "winstep1",
    "winstep2",
    "winstep3",
    "flash",
    "flashme",
    "flashall",
    "flashwinx64",
    "flashwinx86",
    "flashmewinx64",
    "flashmewinx86",
    "flashallwinx64",
    "flashpcl",
    "flashec",
    "flashxdx",
    "flashbbx",
    "ecflash",
    "ec2flash",
    "ecall",
    "ecwinflash",
    "eol",
    "eoltest",
    "eolwin64",
    "eolwinx64",
    "eolwinx86",
    "eoltestwin64",
    "ckme",
    "ckmey",
    "meinfo",
    "meinfowin64",
    "me_shell",
    "me_winx64",
    "me_all",
    "startup",
    "autoexec",
    "menu",
    "mainmenu",
    "restore",
    "factory",
    "f-factory",
    "install",
    "sku01",
    "sku02",
    "sku03",
    "sku04",
    "bios",
    "gui",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// Literal chassis token.
    Exact(String),
    /// Family token; `?` matches any one character.
    Pattern(String),
}

/// Uppercase alphanumerics only, matching `uefi::bioslove::normalize`.
pub fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// True for a directory or script stem that names no model.
pub fn is_generic_script(stem: &str) -> bool {
    let lower = stem.to_ascii_lowercase();
    GENERIC_SCRIPT.contains(&lower.as_str()) || lower.len() <= 2
}

/// True when the stem is a branding variant of another model name.
pub fn is_brand_variant(stem: &str) -> bool {
    let lower = stem.to_ascii_lowercase();
    BRAND_SUFFIX
        .iter()
        .any(|s| lower.len() > s.len() && lower.ends_with(s))
}

/// Split a vendor line into candidate tokens on whitespace, comma and slash.
pub fn split_line(line: &str) -> Vec<String> {
    line.split(|c: char| c.is_whitespace() || c == ',' || c == '/')
        .flat_map(clean)
        .collect()
}

/// Trim decoration off one raw token, expanding an inline `(x)` into both forms.
fn clean(raw: &str) -> Vec<String> {
    let t = raw.trim_matches(|c: char| {
        c.is_whitespace() || matches!(c, '"' | '\'' | ';' | ':' | '.' | ',' | '?' | '!' | '*')
    });
    if t.is_empty() {
        return Vec::new();
    }
    // `P870KM(1)G` covers both P870KMG and P870KM1G.
    if let Some((head, rest)) = t.split_once('(') {
        if let Some((inner, tail)) = rest.split_once(')') {
            if !head.is_empty() && !inner.is_empty() && !inner.contains('(') {
                return vec![format!("{head}{tail}"), format!("{head}{inner}{tail}")];
            }
        }
    }
    vec![t.trim_matches(|c| matches!(c, '(' | ')')).to_string()]
}

/// True when a token has the shape of an OEM chassis name.
pub fn is_model_token(t: &str) -> bool {
    if t.len() < 4 || t.len() > 16 {
        return false;
    }
    if !t.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return false;
    }
    if !t
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '#' | '-' | '_'))
    {
        return false;
    }
    let wildcards = t.chars().filter(|c| matches!(c, 'x' | '#')).count();
    if !t.chars().any(|c| c.is_ascii_digit()) && wildcards < 2 {
        return false;
    }
    !is_part_name(t)
}

/// GPU and CPU part numbers that share the shape of a chassis token.
fn is_part_name(t: &str) -> bool {
    let upper = t.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    for prefix in ["GTX", "RTX", "GTS", "QUADRO", "RADEON", "MX", "RX"] {
        if let Some(rest) = upper.strip_prefix(prefix) {
            if rest.starts_with(|c: char| c.is_ascii_digit()) {
                return true;
            }
        }
    }
    // i9-12900K, R7-5800X.
    matches!(bytes.first(), Some(b'I') | Some(b'R'))
        && matches!(bytes.get(1), Some(b'3') | Some(b'5') | Some(b'7') | Some(b'9'))
        && matches!(bytes.get(2), Some(b'-') | Some(b'_'))
}

/// Wildcard form of a token, or `None` when every character is literal.
///
/// The vendor writes a wildcard as a lowercase `x` between uppercase or digit
/// runs; `#` is a placeholder that can never be a real model character. A token
/// carrying any other lowercase letter is a branding variant, not a family.
fn wildcard_pattern(t: &str) -> Option<String> {
    let chars: Vec<char> = t.chars().collect();
    let branded = chars.iter().any(|c| c.is_ascii_lowercase() && *c != 'x');
    let shouty = |c: &char| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == 'x' || *c == '#';
    let mut out = String::with_capacity(t.len());
    let mut wild = false;
    for (i, c) in chars.iter().enumerate() {
        let is_wild = match c {
            '#' => true,
            'x' if !branded && i > 0 => {
                shouty(&chars[i - 1]) && chars.get(i + 1).is_none_or(shouty)
            }
            _ => false,
        };
        wild |= is_wild;
        out.push(if is_wild { '?' } else { *c });
    }
    wild.then_some(out)
}

/// True for an interior uppercase `X` sitting after a digit, which the vendor
/// sometimes uses as a wildcard and sometimes does not.
fn suspect_uppercase_wildcard(t: &str) -> bool {
    let chars: Vec<char> = t.chars().collect();
    chars
        .iter()
        .enumerate()
        .any(|(i, c)| *c == 'X' && i > 0 && i + 1 < chars.len() && chars[i - 1].is_ascii_digit())
}

/// Classify one already-filtered token, with a warning when the call is close.
pub fn classify(t: &str) -> (Token, Option<String>) {
    match wildcard_pattern(t) {
        Some(p) => (Token::Pattern(p), None),
        None if suspect_uppercase_wildcard(t) => (
            Token::Exact(t.to_string()),
            Some(format!(
                "{t} may be a wildcard family token; kept literal so it cannot match the wrong machine"
            )),
        ),
        None => (Token::Exact(t.to_string()), None),
    }
}

/// Every chassis token on a vendor line, classified.
pub fn tokens_of(line: &str) -> (Vec<Token>, Vec<String>) {
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    for raw in split_line(line) {
        if !is_model_token(&raw) {
            continue;
        }
        let (tok, warn) = classify(&raw);
        out.push(tok);
        warnings.extend(warn);
    }
    (out, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercase_x_between_shouty_runs_is_a_wildcard() {
        assert_eq!(wildcard_pattern("PDxxSNx").as_deref(), Some("PD??SN?"));
        assert_eq!(wildcard_pattern("GMxIX7x").as_deref(), Some("GM?IX7?"));
        assert_eq!(wildcard_pattern("V350WNx").as_deref(), Some("V350WN?"));
        assert_eq!(wildcard_pattern("NPxxRNx").as_deref(), Some("NP??RN?"));
        assert_eq!(wildcard_pattern("X560WNxG").as_deref(), Some("X560WN?G"));
        assert_eq!(wildcard_pattern("PD#0PNR").as_deref(), Some("PD?0PNR"));
    }

    #[test]
    fn real_model_names_stay_literal() {
        assert_eq!(wildcard_pattern("X170KMG"), None);
        assert_eq!(wildcard_pattern("X370SNVG"), None);
        assert_eq!(wildcard_pattern("X6AR57TY"), None);
        // Branding variants carry another lowercase letter.
        assert_eq!(wildcard_pattern("GX5HRXGxdx"), None);
        assert_eq!(wildcard_pattern("LUXGxdx"), None);
    }

    #[test]
    fn part_numbers_and_dates_are_not_chassis_tokens() {
        assert!(!is_model_token("GTX1070"));
        assert!(!is_model_token("RTX2060"));
        assert!(!is_model_token("I9-12900K"));
        assert!(!is_model_token("03-18-2021"));
        assert!(!is_model_token("Microcode"));
        assert!(is_model_token("NH58DCQ"));
        assert!(is_model_token("PDxxSNx"));
        assert!(is_model_token("MS-16K21"));
    }

    #[test]
    fn optional_group_expands_to_both_names() {
        assert_eq!(clean("P870KM(1)G"), ["P870KMG", "P870KM1G"]);
        assert_eq!(clean("(NH58RDQ"), ["NH58RDQ"]);
        assert_eq!(clean("NH70RDQ)"), ["NH70RDQ"]);
    }
}
