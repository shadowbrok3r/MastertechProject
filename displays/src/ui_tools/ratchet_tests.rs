//! Ratchets on legacy styling in `displays/src` and `Mastertech4.0/src`: each count may only go down.

use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

/// Most `add_space` calls whose argument starts with a number literal.
const ADD_SPACE_LITERALS: usize = 119;
/// Most `Color32` constructors and named colours outside the theme files.
const COLOR_LITERALS: usize = 59;
/// Most `style_mut().visuals.widgets` overrides.
const WIDGET_STYLE_OVERRIDES: usize = 1;
/// Most `Button::new(..)` chains that set `.min_size(..)`.
const BUTTON_MIN_SIZES: usize = 2;

/// Paths under the scanned roots that are never counted.
const SKIPPED: &[&str] = &["tabs/session_board/", "ui_tools/ratchet_tests.rs"];

/// Files that define theme or syntax palettes.
const THEME_FILES: &[&str] = &[
    "file_viewer/themes/",
    "ui_tools/carl_dark.rs",
    "ui_tools/galactic_glass.rs",
    "ui_tools/glass_backdrop.rs",
    "ui_tools/mtech_glass.rs",
    "ui_tools/mtech_studio.rs",
    "ui_tools/neon_glass.rs",
    "ui_tools/rerun_mtech.rs",
    "ui_tools/soft_glass.rs",
    "ui_tools/theme_chrome.rs",
    "ui_tools/theme_config.rs",
    "ui_tools/tokyo_dark.rs",
    "ui_tools/tui_theme.rs",
];

/// One scanned source file: its path relative to its root, with `/` separators, and its text.
struct Source {
    path: String,
    text: String,
}

/// Pattern counters shared by the ratchets and their mutation control.
struct Patterns {
    add_space: Regex,
    color: Regex,
    widgets: Regex,
    button: Regex,
}

impl Patterns {
    fn new() -> Self {
        let re = |pattern: &str| Regex::new(pattern).expect("ratchet pattern compiles");
        Self {
            add_space: re(r"add_space\(\s*[-+]?\d[\d_.]*(?:_?f32)?\s*\)"),
            color: re(r"Color32::(?:from_\w+\(|[A-Z][A-Z0-9_]*\b)"),
            widgets: re(r"style_mut\(\)\s*\.visuals\s*\.widgets"),
            button: re(r"Button::new\("),
        }
    }

    fn add_space(&self, text: &str) -> usize {
        self.add_space.find_iter(text).count()
    }

    fn colors(&self, text: &str) -> usize {
        self.color
            .find_iter(text)
            .filter(|m| !matches!(m.as_str(), "Color32::TRANSPARENT" | "Color32::PLACEHOLDER"))
            .count()
    }

    fn widget_overrides(&self, text: &str) -> usize {
        self.widgets.find_iter(text).count()
    }

    fn button_min_sizes(&self, text: &str) -> usize {
        self.button
            .find_iter(text)
            .filter(|m| {
                let rest = &text[m.start()..];
                let statement = rest.split(';').next().unwrap_or(rest);
                statement.contains(".min_size(")
            })
            .count()
    }
}

/// Every `.rs` file under `dir`, recursively.
fn collect(root: &Path, dir: &Path, out: &mut Vec<Source>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if SKIPPED.iter().any(|s| rel.starts_with(s)) {
                continue;
            }
            if let Ok(text) = fs::read_to_string(&path) {
                out.push(Source { path: rel, text });
            }
        }
    }
}

fn sources() -> Vec<Source> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    for root in [manifest.join("src"), manifest.join("../Mastertech4.0/src")] {
        assert!(root.is_dir(), "missing source root {}", root.display());
        collect(&root, &root, &mut out);
    }
    assert!(out.len() > 100, "only {} source files found", out.len());
    out
}

/// Fails when the total of `count` over `sources` differs from `ceiling`.
fn ratchet(name: &str, ceiling: usize, sources: &[Source], count: impl Fn(&str) -> usize) {
    let mut per_file: Vec<(usize, &str)> = sources
        .iter()
        .map(|s| (count(&s.text), s.path.as_str()))
        .filter(|(n, _)| *n > 0)
        .collect();
    let total: usize = per_file.iter().map(|(n, _)| n).sum();
    per_file.sort_unstable_by(|a, b| b.cmp(a));
    let top: Vec<String> = per_file
        .iter()
        .take(8)
        .map(|(n, p)| format!("{n} {p}"))
        .collect();
    assert!(
        total <= ceiling,
        "{name}: {total} found, ceiling {ceiling}; use kit controls and tokens instead. Top files: {top:#?}"
    );
    assert!(
        total == ceiling,
        "{name}: down to {total} from {ceiling}; lower the ceiling in ratchet_tests.rs to {total}"
    );
}

#[test]
fn add_space_literals_only_go_down() {
    let p = Patterns::new();
    ratchet(
        "add_space(<literal>)",
        ADD_SPACE_LITERALS,
        &sources(),
        |t| p.add_space(t),
    );
}

#[test]
fn color_literals_outside_themes_only_go_down() {
    let p = Patterns::new();
    let sources: Vec<Source> = sources()
        .into_iter()
        .filter(|s| !THEME_FILES.iter().any(|t| s.path.starts_with(t)))
        .collect();
    ratchet("Color32 literals", COLOR_LITERALS, &sources, |t| {
        p.colors(t)
    });
}

#[test]
fn widget_style_overrides_only_go_down() {
    let p = Patterns::new();
    ratchet(
        "style_mut().visuals.widgets",
        WIDGET_STYLE_OVERRIDES,
        &sources(),
        |t| p.widget_overrides(t),
    );
}

#[test]
fn button_min_sizes_only_go_down() {
    let p = Patterns::new();
    ratchet(
        "Button::new(..).min_size",
        BUTTON_MIN_SIZES,
        &sources(),
        |t| p.button_min_sizes(t),
    );
}

#[test]
fn the_counters_see_each_pattern_and_skip_lookalikes() {
    let p = Patterns::new();
    let text = r#"
        ui.add_space(4.0);
        ui.add_space(m.gap.y);
        ui.add_space(2.0 * m.gap.y);
        let a = Color32::from_rgb(1, 2, 3);
        let b = egui::Color32::RED;
        let c = Color32::TRANSPARENT;
        let d = Color32::PLACEHOLDER;
        ui.style_mut().visuals.widgets.inactive.bg_fill = a;
        ui.add(Button::new("Go").min_size(vec2(80.0, 24.0)));
        ui.add(Button::new("Stop"));
        let size = vec2(1.0, 2.0).min_size;
    "#;
    assert_eq!(p.add_space(text), 1);
    assert_eq!(p.colors(text), 2);
    assert_eq!(p.widget_overrides(text), 1);
    assert_eq!(p.button_min_sizes(text), 1);
}
