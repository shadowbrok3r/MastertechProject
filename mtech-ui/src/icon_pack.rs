//! White SVG icons (Phosphor Regular + MasterTech `MT_*`) drawn through egui_extras' SVG loader.

use std::collections::HashMap;
use std::sync::OnceLock;

use eframe::egui::{
    Color32, Image, ImageSource, Pos2, Rect, Response, Sense, Ui, Vec2, load::Bytes,
};

/// One icon of the pack. Constants are generated from `assets/icons/regular/*.svg`.
#[derive(Clone, Copy, Debug)]
pub struct Icon {
    name: &'static str,
    uri: &'static str,
    duo_uri: &'static str,
    svg: &'static [u8],
    duo: Option<&'static [u8]>,
    glyph: &'static str,
}

impl Icon {
    /// File stem, e.g. `"hard-drive"` or `"mt-stress"`.
    pub const fn name(self) -> &'static str {
        self.name
    }

    /// Phosphor font glyph for inline text.
    pub const fn glyph(self) -> &'static str {
        self.glyph
    }

    /// Raw SVG bytes of the front layer.
    pub const fn svg(self) -> &'static [u8] {
        self.svg
    }

    /// Raw SVG bytes of the duotone back layer.
    pub const fn duo_svg(self) -> Option<&'static [u8]> {
        self.duo
    }

    pub const fn has_duo(self) -> bool {
        self.duo.is_some()
    }

    /// Looks up an icon by file stem.
    pub fn find(name: &str) -> Option<Self> {
        ALL.binary_search_by(|icon| icon.name.cmp(name))
            .ok()
            .map(|i| ALL[i])
    }

    /// The Phosphor icon whose font glyph is `glyph`.
    pub fn from_glyph(glyph: &str) -> Option<Self> {
        static BY_GLYPH: OnceLock<HashMap<&'static str, Icon>> = OnceLock::new();
        BY_GLYPH
            .get_or_init(|| {
                ALL.iter()
                    .filter(|icon| !BORROWED.contains(&icon.name))
                    .map(|icon| (icon.glyph, *icon))
                    .collect()
            })
            .get(glyph)
            .copied()
    }

    pub fn source(self) -> ImageSource<'static> {
        ImageSource::Bytes {
            uri: self.uri.into(),
            bytes: Bytes::Static(self.svg),
        }
    }

    pub fn duo_source(self) -> Option<ImageSource<'static>> {
        self.duo.map(|bytes| ImageSource::Bytes {
            uri: self.duo_uri.into(),
            bytes: Bytes::Static(bytes),
        })
    }

    /// Untinted (white) image fixed at `size` points square; chain `.tint(..)` to colour it.
    pub fn image(self, size: f32) -> Image<'static> {
        Image::new(self.source()).fit_to_exact_size(Vec2::splat(size))
    }

    /// Allocates a `size` square and paints the icon in the current text colour.
    pub fn show(self, ui: &mut Ui, size: f32) -> Response {
        let tint = ui.visuals().text_color();
        self.show_tinted(ui, size, tint)
    }

    /// Allocates a `size` square and paints the icon tinted `tint`.
    pub fn show_tinted(self, ui: &mut Ui, size: f32, tint: Color32) -> Response {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
        if ui.is_rect_visible(rect) {
            self.paint(ui, rect, size, tint);
        }
        response
    }

    /// Paints the icon `size` points square, centred in `rect` and snapped to whole pixels.
    pub fn paint(self, ui: &Ui, rect: Rect, size: f32, tint: Color32) {
        let target = snapped_square(rect.center(), size, ui.pixels_per_point());
        Image::new(self.source()).tint(tint).paint_at(ui, target);
    }

    /// Paints the back layer tinted `bg` under the front layer tinted `fg`.
    pub fn paint_duo(self, ui: &Ui, rect: Rect, size: f32, fg: Color32, bg: Color32) {
        let target = snapped_square(rect.center(), size, ui.pixels_per_point());
        if let Some(back) = self.duo_source() {
            Image::new(back).tint(bg).paint_at(ui, target);
        }
        Image::new(self.source()).tint(fg).paint_at(ui, target);
    }
}

impl PartialEq for Icon {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for Icon {}

impl std::hash::Hash for Icon {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

impl From<Icon> for ImageSource<'static> {
    fn from(icon: Icon) -> Self {
        icon.source()
    }
}

/// Square of a whole number of physical pixels whose origin sits on the pixel grid.
fn snapped_square(center: Pos2, size: f32, pixels_per_point: f32) -> Rect {
    let px = (size * pixels_per_point).round().max(1.0);
    let min =
        (center.to_vec2() * pixels_per_point - Vec2::splat(px * 0.5)).round() / pixels_per_point;
    Rect::from_min_size(min.to_pos2(), Vec2::splat(px / pixels_per_point))
}

include!(concat!(env!("OUT_DIR"), "/icon_table.rs"));

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn names_sorted_and_unique() {
        assert!(ALL.windows(2).all(|w| w[0].name < w[1].name));
        let uris: HashSet<_> = ALL.iter().map(|i| i.uri).collect();
        assert_eq!(uris.len(), ALL.len());
        assert!(ALL.len() >= 300, "pack shrank to {}", ALL.len());
    }

    #[test]
    fn find_round_trips() {
        for icon in ALL {
            assert_eq!(Icon::find(icon.name), Some(*icon));
        }
        assert_eq!(Icon::find("cpu"), Some(CPU));
        assert_eq!(Icon::find("no-such-icon"), None);
    }

    #[test]
    fn phosphor_glyphs_map_back_to_their_icon() {
        assert_eq!(Icon::from_glyph(egui_phosphor::regular::CPU), Some(CPU));
        assert_eq!(
            Icon::from_glyph(MT_STRESS.glyph()).map(|i| i.name().starts_with("mt-")),
            Some(false)
        );
        assert_eq!(Icon::from_glyph("x"), None);
        assert_eq!(Icon::from_glyph(CROWN_SIMPLE.glyph()), Some(USER_GEAR));
    }

    #[test]
    fn every_custom_glyph_borrows() {
        for icon in ALL.iter().filter(|i| i.name.starts_with("mt-")) {
            assert!(BORROWED.contains(&icon.name), "{}", icon.name);
        }
    }

    #[test]
    fn uris_end_in_svg() {
        for icon in ALL {
            assert!(icon.uri.starts_with("bytes://mtech-icons/") && icon.uri.ends_with(".svg"));
            assert!(icon.duo_uri.ends_with(".svg") && icon.duo_uri != icon.uri);
        }
    }

    #[test]
    fn glyphs_are_single_phosphor_chars() {
        for icon in ALL {
            let mut chars = icon.glyph.chars();
            let c = chars
                .next()
                .unwrap_or_else(|| panic!("{} has an empty glyph", icon.name));
            assert!(
                chars.next().is_none(),
                "{} glyph is not one char",
                icon.name
            );
            assert!(
                ('\u{E000}'..='\u{F8FF}').contains(&c),
                "{} glyph is outside the PUA",
                icon.name
            );
        }
    }

    fn rasterise(name: &str, bytes: &[u8], px: u32) -> resvg::tiny_skia::Pixmap {
        let text = std::str::from_utf8(bytes).unwrap_or_else(|e| panic!("{name}: not UTF-8: {e}"));
        assert!(
            text.trim_start().starts_with("<svg"),
            "{name}: not an <svg> root"
        );
        assert!(
            !text.contains("currentColor"),
            "{name}: currentColor cannot be tinted"
        );
        let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(tree.size().width(), 256.0, "{name}: not on the 256 grid");
        let mut pixmap = resvg::tiny_skia::Pixmap::new(px, px).expect("non-zero pixmap");
        let scale = px as f32 / 256.0;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        pixmap
    }

    fn coverage(pixmap: &resvg::tiny_skia::Pixmap) -> f32 {
        let lit = pixmap.pixels().iter().filter(|p| p.alpha() > 127).count();
        lit as f32 / pixmap.pixels().len() as f32
    }

    #[test]
    fn every_layer_parses_and_renders_white() {
        for icon in ALL {
            let front = rasterise(icon.name, icon.svg, 48);
            let cov = coverage(&front);
            assert!(
                (0.003..0.75).contains(&cov),
                "{}: front coverage {cov}",
                icon.name
            );
            for p in front.pixels().iter().filter(|p| p.alpha() == 255) {
                assert_eq!(
                    (p.red(), p.green(), p.blue()),
                    (255, 255, 255),
                    "{}: non-white ink",
                    icon.name
                );
            }
            if let Some(duo) = icon.duo {
                assert!(
                    coverage(&rasterise(icon.name, duo, 48)) > 0.003,
                    "{}: empty duo layer",
                    icon.name
                );
            }
        }
    }

    #[test]
    fn mtech_glyphs_present() {
        let custom: Vec<_> = ALL.iter().filter(|i| i.name.starts_with("mt-")).collect();
        assert!(custom.len() >= 6, "only {} MasterTech glyphs", custom.len());
        assert!(custom.iter().all(|i| i.has_duo()));
    }

    #[test]
    fn loads_through_egui_extras_svg_loader() {
        let ctx = eframe::egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        for icon in [CPU, HARD_DRIVE, MT_STRESS, MT_LOGO] {
            let duo = icon.duo_source().expect("duo layer");
            for source in [icon.source(), duo] {
                let image = Image::new(source).fit_to_exact_size(Vec2::splat(24.0));
                match image.load_for_size(&ctx, Vec2::splat(100.0)) {
                    Ok(eframe::egui::load::TexturePoll::Ready { texture }) => {
                        let raster = ctx.tex_manager().read().meta(texture.id).map(|m| m.size);
                        assert_eq!(raster, Some([24, 24]), "{}", icon.name);
                    }
                    Ok(_) => panic!("{}: still loading", icon.name),
                    Err(e) => panic!("{}: {e}", icon.name),
                }
            }
        }
    }

    #[test]
    fn snapped_square_is_pixel_exact() {
        for ppp in [1.0, 1.25, 1.5, 2.0] {
            for x in [10.0, 10.3, 10.5, 10.77] {
                let r = snapped_square(Pos2::new(x, x + 0.2), 16.0, ppp);
                let px = r.size() * ppp;
                assert!((px.x - (16.0 * ppp).round()).abs() < 1e-3);
                assert!(((r.min.x * ppp) - (r.min.x * ppp).round()).abs() < 1e-3);
            }
        }
    }
}
