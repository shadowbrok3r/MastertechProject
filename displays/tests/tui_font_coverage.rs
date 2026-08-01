//! The terminal UI grid shears and its `floor(dx / char_width)` mouse mapping
//! drifts unless every glyph it paints resolves in the `CascadiaMono` family at
//! exactly the advance of a space.
//!
//! Coverage is probed with `glyph_width`, which is 0.0 for a char no face in the
//! family carries. `Fonts::has_glyph` cannot be used: it reports a hit only when
//! the char resolves to a face other than the replacement-glyph face, so it
//! returns false for every char of a single-face family.

use eframe::egui::{FontFamily, FontId};
use eframe::epaint::text::{Fonts, TextOptions};

/// Ranges the terminal UI may draw markers from.
const RANGES: &[(&str, u32, u32)] = &[
    ("Arrows", 0x2190, 0x2195),
    ("Box Drawing", 0x2500, 0x257F),
    ("Block Elements", 0x2580, 0x259F),
    ("Geometric Shapes", 0x25A0, 0x25FF),
];

/// Glyphs used outside those ranges.
const EXTRAS: &str = "\u{00a7}\u{00b7}\u{2013}\u{2014}\u{2026}\u{2713}";

#[test]
fn terminal_family_covers_every_glyph_at_cell_width() {
    let font_id = FontId::new(16.0, FontFamily::Name("CascadiaMono".into()));
    let mut fonts = Fonts::new(TextOptions::default(), displays::app_state::font_definitions());
    let mut view = fonts.with_pixels_per_point(1.0);
    let cell = view.glyph_width(&font_id, ' ');
    assert!(cell > 0.0, "CascadiaMono family did not resolve");

    let codepoints = RANGES
        .iter()
        .flat_map(|&(_, lo, hi)| (lo..=hi).filter_map(char::from_u32))
        .chain(EXTRAS.chars());

    let mut bad = Vec::new();
    for c in codepoints {
        let w = view.glyph_width(&font_id, c);
        if w == 0.0 {
            bad.push(format!("U+{:04X} not in the family", c as u32));
        } else if (w - cell).abs() > 0.01 {
            bad.push(format!("U+{:04X} advance {w} != cell {cell}", c as u32));
        }
    }

    assert!(bad.is_empty(), "{} bad glyph(s):\n{}", bad.len(), bad.join("\n"));
}
