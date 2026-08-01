//! Display safety for text and numbers decoded out of crash dumps.
//!
//! A misresolved `DUMP_STRING` yields arbitrary bytes, not text, so no font can
//! render it. Runs of codepoints outside the bundled faces collapse into one
//! counted `<?n>` marker instead of a wall of replacement boxes.

use serde_json::Value;

/// Marker prefix written in place of unrenderable runs.
const MARKER_PREFIX: &str = "<?";
/// Legacy lossy addresses are all above this; below it nothing is at risk.
const WIDE_INT_FLOOR: u64 = 1 << 53;
const PAGE: u64 = 4096;

/// Text with unrenderable runs replaced, plus how many codepoints went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitizedText {
    pub text: String,
    pub replaced: usize,
}

impl SanitizedText {
    pub fn is_clean(&self) -> bool {
        self.replaced == 0
    }
}

/// True for codepoints the bundled faces actually cover.
fn renderable(c: char) -> bool {
    let o = c as u32;
    matches!(o, 0x20..=0x7E | 0xA0..=0xFF | 0x100..=0x17F | 0x2010..=0x2026 | 0x20AC) || c == '\n'
}

/// Collapse each run of unrenderable codepoints into one `<?n>` marker.
pub fn sanitize_dump_text_report(s: &str) -> SanitizedText {
    let mut text = String::with_capacity(s.len());
    let mut replaced = 0usize;
    let mut run = 0usize;
    let flush = |run: &mut usize, text: &mut String| {
        if *run == 1 {
            text.push_str("<?>");
        } else if *run > 1 {
            text.push_str(&format!("<?{run}>"));
        }
        *run = 0;
    };
    for c in s.chars() {
        let c = if c == '\t' { ' ' } else { c };
        if c == '\r' {
            continue;
        }
        if renderable(c) {
            flush(&mut run, &mut text);
            text.push(c);
        } else {
            run += 1;
            replaced += 1;
        }
    }
    flush(&mut run, &mut text);
    SanitizedText { text, replaced }
}

pub fn sanitize_dump_text(s: &str) -> String {
    sanitize_dump_text_report(s).text
}

/// True when `s` carries a sanitizer marker.
pub fn contains_marker(s: &str) -> bool {
    s.contains(MARKER_PREFIX)
}

/// Debug-escaped form, for showing what the raw bytes were.
pub fn escaped_bytes(s: &str) -> String {
    s.escape_debug().to_string()
}

/// A module name that renders, flagged when it is not a valid module name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleName {
    pub text: String,
    pub suspect: bool,
}

/// Loaded-module list split by whether each entry can be displayed at all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleSplit {
    pub readable: Vec<ModuleName>,
    pub unreadable: Vec<SanitizedText>,
}

impl ModuleSplit {
    pub fn total(&self) -> usize {
        self.readable.len() + self.unreadable.len()
    }
}

/// Split modules on renderability; readable-but-invalid names stay visible
/// and are marked `suspect` rather than hidden.
pub fn split_modules(modules: &[String]) -> ModuleSplit {
    let mut split = ModuleSplit::default();
    for m in modules {
        let report = sanitize_dump_text_report(m);
        if report.is_clean() {
            split.readable.push(ModuleName {
                suspect: !dump_triage::is_plausible_module_name(m),
                text: report.text,
            });
        } else {
            split.unreadable.push(report);
        }
    }
    split
}

/// Sanitize every string value in a JSON tree; keys are left alone.
pub fn sanitize_json_strings(value: &Value) -> Value {
    match value {
        Value::String(s) => Value::String(sanitize_dump_text(s)),
        Value::Array(items) => Value::Array(items.iter().map(sanitize_json_strings).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), sanitize_json_strings(v)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

/// How faithfully a wide integer survived storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WideInt {
    /// Stored exactly.
    Exact(u64),
    /// Recovered from a float and page-aligned, so bit-exact.
    FloatPageAligned(u64),
    /// Recovered from a float that lost its low bits.
    FloatRounded(u64),
}

impl WideInt {
    pub fn value(self) -> u64 {
        match self {
            Self::Exact(v) | Self::FloatPageAligned(v) | Self::FloatRounded(v) => v,
        }
    }

    pub fn hex(self) -> String {
        format!("{:#x}", self.value())
    }

    pub fn is_approximate(self) -> bool {
        matches!(self, Self::FloatRounded(_))
    }
}

/// Classify a JSON number that is wide enough to have been at risk.
pub fn wide_int(value: &Value) -> Option<WideInt> {
    let n = value.as_number()?;
    if let Some(v) = n.as_u64() {
        return (v >= WIDE_INT_FLOOR).then_some(WideInt::Exact(v));
    }
    let f = n.as_f64()?;
    if !f.is_finite() || f < 0.0 || f.fract() != 0.0 || f >= 18_446_744_073_709_551_616.0 {
        return None;
    }
    let v = f as u64;
    if v < WIDE_INT_FLOOR {
        return None;
    }
    Some(if v % PAGE == 0 {
        WideInt::FloatPageAligned(v)
    } else {
        WideInt::FloatRounded(v)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pinned in both raw and escaped form so a re-encode of this file fails
    /// loudly instead of weakening the test.
    const MOJIBAKE: &str = "저&氀1堀昀椀渀椀琀礀 䴀漀戀椀氀攀";

    #[test]
    fn the_mojibake_literal_is_intact() {
        assert_eq!(
            MOJIBAKE,
            "\u{c800}&\u{6c00}1\u{5800}\u{6600}\u{6900}\u{6e00}\u{6900}\u{7400}\u{7900} \u{4d00}\u{6f00}\u{6200}\u{6900}\u{6c00}\u{6500}"
        );
    }

    #[test]
    fn field_mojibake_collapses_to_counted_markers() {
        let r = sanitize_dump_text_report(MOJIBAKE);
        assert_eq!(r.text, "<?>&<?>1<?7> <?6>");
        assert_eq!(r.replaced, 15);
        assert!(r.text.is_ascii());
        assert!(!r.is_clean());
    }

    #[test]
    fn control_byte_soup_collapses() {
        let r = sanitize_dump_text_report("b\u{0}\u{a68c}\u{3}\u{1}");
        assert_eq!(r.text, "b<?4>");
        assert_eq!(r.replaced, 4);
    }

    #[test]
    fn a_clean_nt_path_is_untouched() {
        let p = r"\SystemRoot\system32\drivers\Ntfs.sys";
        let r = sanitize_dump_text_report(p);
        assert_eq!(r.text, p);
        assert!(r.is_clean());
    }

    #[test]
    fn the_ellipsis_truncate_chars_appends_is_renderable() {
        assert!(renderable('…'));
        assert!(sanitize_dump_text_report("abc…").is_clean());
    }

    #[test]
    fn split_keeps_truncated_names_visible_and_marks_them() {
        let mods = vec![
            "ntoskr".to_string(),
            "Ntfs.sys".to_string(),
            MOJIBAKE.to_string(),
        ];
        let split = split_modules(&mods);
        assert_eq!(split.total(), 3, "nothing may be silently dropped");
        assert_eq!(split.readable.len(), 2);
        assert_eq!(split.unreadable.len(), 1);
        let truncated = &split.readable[0];
        assert_eq!(truncated.text, "ntoskr");
        assert!(truncated.suspect, "a halved name is readable but invalid");
        assert!(!split.readable[1].suspect);
    }

    #[test]
    fn json_strings_are_sanitized_and_numbers_are_not() {
        let v = serde_json::json!({ "name": MOJIBAKE, "size": 652_238_848u64 });
        let out = sanitize_json_strings(&v);
        assert_eq!(out["name"], serde_json::json!("<?>&<?>1<?7> <?6>"));
        assert_eq!(out["size"], serde_json::json!(652_238_848u64));
    }

    #[test]
    fn a_page_aligned_legacy_float_is_exact() {
        let w = wide_int(&serde_json::json!(1.8446735295012733e19)).unwrap();
        assert_eq!(w, WideInt::FloatPageAligned(0xFFFF_F804_0CE5_0000));
        assert_eq!(w.hex(), "0xfffff8040ce50000");
        assert!(!w.is_approximate());
    }

    #[test]
    fn an_unaligned_legacy_float_is_flagged() {
        let raw = 0xFFFF_F804_0CE5_0800u64;
        let w = wide_int(&serde_json::json!(raw as f64)).unwrap();
        assert!(w.is_approximate(), "unaligned addresses lost their low bits");
    }

    #[test]
    fn small_numbers_and_strings_are_not_wide() {
        assert!(wide_int(&serde_json::json!(652_238_848u64)).is_none());
        assert!(wide_int(&serde_json::json!(1_781_679_513i64)).is_none());
        assert!(wide_int(&serde_json::json!("0xfffff80310000000")).is_none());
    }

    #[test]
    fn marker_detection() {
        assert!(contains_marker("b<?4>"));
        assert!(!contains_marker(r"\SystemRoot\ntoskrnl.exe"));
    }
}
