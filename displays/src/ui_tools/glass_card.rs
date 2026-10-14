//! Themed card surfaces for content *inside* a panel.
//!
//! Floating surfaces get their pane from [`glass_backdrop::frost_open_windows`]. A card in a panel
//! body has no such sweep, so every call site used to invent its own fill — which is why the admin
//! console had a dozen hand-picked greys that ignored the active theme.
//!
//! [`card`] and [`titled_card`] take their fill, outline and rounding from the theme, and frost
//! their own backdrop through [`glass_backdrop::glass_frame`], so a card is a translucent pane on a
//! glass theme and a raised opaque surface on a flat one, without the call site choosing.
//!
//! # What the frost reaches
//!
//! A frost grabs the framebuffer where the card sits, so it blurs whatever was painted before it:
//! the panel fill, and any content already laid out above or behind the card. It cannot blur a
//! sibling that paints later. On a flat panel there is nothing to smear and the card reads as a
//! plain tinted pane — which is what [`glass_backdrop::GlassParams::tint`] is for.

use eframe::egui::{
    Align, AsIdSalt, Color32, CornerRadius, Frame, InnerResponse, Layout, Margin, Rangef, Rect,
    Response, RichText, Sense, Shape, Stroke, Ui, UiBuilder, UiKind, UiStackInfo,
    epaint::MarginF32, layers::ShapeIdx, pos2, vec2,
};

use super::glass_backdrop;
use super::{
    controls, icons,
    icons::IconRef,
    icons::svg::Icon,
    theme,
    tokens::{self, Tokens},
};

/// Band the theme's window margin is clamped into for card padding: below the floor content
/// touches the outline, above the ceiling a card of key/value rows turns mostly into margin.
const CARD_PAD_MIN: i8 = 6;
const CARD_PAD_MAX: i8 = 10;
/// Vertical gap between stacked cards.
const CARD_GAP: i8 = 3;
/// Corner radius when the theme asks for square windows; a card is never a hard rectangle.
const MIN_RADIUS: u8 = 3;

/// Card padding, tracking the theme's own window margin so a compact theme gets compact cards.
fn card_padding(ui: &Ui) -> i8 {
    ui.spacing()
        .window_margin
        .left
        .clamp(CARD_PAD_MIN, CARD_PAD_MAX)
}

/// The card surface tone for the active theme.
///
/// A glass theme's `window_fill` is translucent, and that translucency is the point — it is the
/// film the frost reads through, and it is the same tone every window and popup already paints,
/// so cards match them. A flat theme's is opaque and often equal to `panel_fill`, which would make
/// a card invisible; those get the faint tint composited over the panel instead.
pub fn card_fill(ui: &Ui) -> Color32 {
    let v = ui.visuals();
    if v.window_fill.a() < 255 {
        return v.window_fill;
    }
    let lifted = tokens::composite(v.faint_bg_color, v.panel_fill);
    if lifted == v.panel_fill {
        tokens::composite(v.widgets.noninteractive.weak_bg_fill, v.panel_fill)
    } else {
        lifted
    }
}

/// Fill for a panel that holds cards — a sidebar or a rail — pushed away from `panel_fill` so the
/// cards drawn on it with [`card_fill`] still read as raised.
pub fn recessed_fill(ui: &Ui) -> Color32 {
    let v = ui.visuals();
    let [r, g, b, a] = v.panel_fill.to_srgba_unmultiplied();
    // Toward black on a dark theme, toward white on a light one: either way, away from the cards.
    let shift = |c: u8| {
        if v.dark_mode {
            (c as f32 * 0.72).round() as u8
        } else {
            (c as f32 + (255.0 - c as f32) * 0.28).round() as u8
        }
    };
    Color32::from_rgba_unmultiplied(shift(r), shift(g), shift(b), a)
}

/// The card outline for the active theme, falling back to the noninteractive widget edge when the
/// theme leaves `window_stroke` invisible.
pub fn card_stroke(ui: &Ui) -> Stroke {
    let v = ui.visuals();
    if v.window_stroke.width > 0.0 && v.window_stroke.color.a() > 0 {
        v.window_stroke
    } else {
        v.widgets.noninteractive.bg_stroke
    }
}

/// A [`Frame`] shaped like a card in the active theme. Use [`card`] instead unless you need to
/// adjust the frame before showing it — a bare frame does not frost.
pub fn card_frame(ui: &Ui) -> Frame {
    let radius = ui.visuals().window_corner_radius.nw.max(MIN_RADIUS);
    Frame::new()
        .fill(card_fill(ui))
        .stroke(card_stroke(ui))
        .corner_radius(CornerRadius::same(radius))
        .inner_margin(Margin::same(card_padding(ui)))
        .outer_margin(Margin::symmetric(0, CARD_GAP))
}

/// A themed card that frosts its own backdrop.
///
/// `id_salt` must be unique among the cards of one `Ui`; it keys the last-frame rect the frost is
/// enqueued against, so two cards sharing a salt would frost each other's position.
///
/// Each call costs one grab-pass, so this is for the handful of standing surfaces on a page. Use
/// [`group`] for anything that repeats per row.
pub fn card<R>(
    ui: &mut Ui,
    id_salt: impl AsIdSalt,
    contents: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    let frame = card_frame(ui);
    glass_backdrop::glass_frame(ui, id_salt, frame, contents)
}

/// Drop-in for [`Ui::group`] that draws the theme's card instead of egui's default group frame.
///
/// No frost: this is what a per-row card uses, and a grab-pass per row would cost far more than
/// blurring a flat panel is worth. The translucent fill still lets the panel read through.
///
/// [`Ui::group`]: eframe::egui::Ui::group
pub fn group<R>(ui: &mut Ui, contents: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    card_frame(ui).show(ui, contents)
}

/// A [`group`] exactly `ui.available_width()` wide whatever it holds; plain while unbounded.
pub fn row_card<R>(ui: &mut Ui, contents: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    let width = ui.available_width();
    if ui.is_sizing_pass() || !width.is_finite() {
        return group(ui, contents);
    }
    let frame = card_frame(ui);
    let mut card = FixedWidthFrame::begin(ui, frame, width);
    let inner = contents(&mut card.content_ui);
    InnerResponse::new(inner, card.end(ui))
}

/// A [`Frame`] of fixed outer width; wider content is clipped instead of widening it or its parent.
pub struct FixedWidthFrame {
    /// Fill, stroke and shadow are read when painting; margins are fixed by [`Self::begin`].
    pub frame: Frame,
    /// The `Ui` the frame's contents go in.
    pub content_ui: Ui,
    background: ShapeIdx,
    outer_left: f32,
    outer_top: f32,
    outer_width: f32,
    margin: MarginF32,
    outer_margin: Margin,
}

impl FixedWidthFrame {
    /// Starts `frame` at the cursor, `width` wide including its outer margin.
    pub fn begin(ui: &mut Ui, frame: Frame, width: f32) -> Self {
        debug_assert!(
            width.is_finite(),
            "a fixed-width frame needs a finite width"
        );
        let background = ui.painter().add(Shape::Noop);
        let available = ui.available_rect_before_wrap();
        let outer = Rect::from_min_size(available.min, vec2(width.max(0.0), available.height()));
        let margin = frame.total_margin();
        let mut content = outer - margin;
        content.max.x = content.max.x.max(content.min.x);
        content.max.y = content.max.y.max(content.min.y);

        let mut content_ui = ui.new_child(
            UiBuilder::new()
                .ui_stack_info(UiStackInfo::new(UiKind::Frame).with_frame(frame))
                .max_rect(content),
        );
        content_ui.set_width(content.width());
        let visible = outer - frame.outer_margin;
        content_ui.shrink_clip_rect(Rect::from_x_y_ranges(visible.x_range(), Rangef::EVERYTHING));

        Self {
            frame,
            content_ui,
            background,
            outer_left: outer.left(),
            outer_top: outer.top(),
            outer_width: outer.width(),
            margin,
            outer_margin: frame.outer_margin,
        }
    }

    /// The locked width by the content's height plus the margins.
    pub fn outer_rect(&self) -> Rect {
        let content_top = self.outer_top + self.margin.top;
        let bottom = self.content_ui.min_rect().bottom().max(content_top) + self.margin.bottom;
        Rect::from_min_max(
            pos2(self.outer_left, self.outer_top),
            pos2(self.outer_left + self.outer_width, bottom),
        )
    }

    /// Allocates [`Self::outer_rect`] in `ui`.
    pub fn allocate_space(&self, ui: &mut Ui) -> Response {
        ui.allocate_rect(self.outer_rect(), Sense::hover())
    }

    /// Paints the frame behind the contents; its edge stays put whatever the stroke width.
    pub fn paint(&self, ui: &Ui) {
        let visible = self.outer_rect() - self.outer_margin;
        if ui.is_rect_visible(visible) {
            let content = (visible - self.frame.inner_margin).shrink(self.frame.stroke.width);
            ui.painter().set(self.background, self.frame.paint(content));
        }
    }

    /// Paints and allocates the frame.
    pub fn end(self, ui: &mut Ui) -> Response {
        self.paint(ui);
        self.allocate_space(ui)
    }
}

/// A themed card with an accent glyph, a title, and an optional right-aligned subtitle.
///
/// Salts itself from the `Ui`'s auto-id sequence rather than from `title`, so a loop that emits
/// the same title twice still gives each card its own stored rect.
pub fn titled_card<'a, R>(
    ui: &mut Ui,
    icon: impl Into<IconRef<'a>>,
    title: &str,
    subtitle: Option<&str>,
    contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    let icon = icon.into();
    let salt = ui.next_auto_id();
    ui.skip_ahead_auto_ids(1);
    card(ui, salt, |ui| {
        ui.set_min_width(ui.available_width());
        card_header(ui, icon, title, subtitle);
        contents(ui)
    })
    .inner
}

/// A non-frosting [`group`] card that opens with an icon, a title and an optional subtitle.
pub fn titled_group<R>(
    ui: &mut Ui,
    icon: Icon,
    title: &str,
    subtitle: Option<&str>,
    contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    group(ui, |ui| {
        ui.set_min_width(ui.available_width());
        icon_header(ui, Some(icon), title, subtitle);
        contents(ui)
    })
    .inner
}

/// The icon + title row a [`titled_card`] opens with, for call sites that build their own frame.
pub fn card_header<'a>(
    ui: &mut Ui,
    icon: impl Into<IconRef<'a>>,
    title: &str,
    subtitle: Option<&str>,
) {
    let icon = icon.into();
    match icon.icon() {
        Some(icon) => icon_header(ui, Some(icon), title, subtitle),
        None if icon.glyph().is_empty() => icon_header(ui, None, title, subtitle),
        None => {
            let glyph = icon.glyph();
            header_row(ui, subtitle, |ui| {
                let ink = Tokens::get(ui.ctx()).icon;
                ui.label(icons::icon_colored(glyph, ink));
                controls::subheading(ui, None, title);
            });
        }
    }
}

/// The SVG icon + title row a titled card opens with.
pub fn icon_header(ui: &mut Ui, icon: Option<Icon>, title: &str, subtitle: Option<&str>) {
    header_row(ui, subtitle, |ui| {
        controls::subheading(ui, icon, title);
    });
}

/// `lead` and the subtitle on one row, then a hairline.
fn header_row(ui: &mut Ui, subtitle: Option<&str>, lead: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        lead(ui);
        if let Some(subtitle) = subtitle {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(subtitle).small().color(theme::weak_text(ui)));
            });
        }
    });
    controls::hairline(ui);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_tools::theme_config::{PresetStyles, style_for_preset};
    use eframe::egui::Context;

    /// Runs `f` against a `Ui` carrying `preset`'s style.
    fn with_preset<R>(preset: PresetStyles, f: impl FnOnce(&mut Ui) -> R) -> R {
        let ctx = Context::default();
        ctx.set_global_style(std::sync::Arc::new(style_for_preset(preset)));
        // `run_ui` wants `FnMut`, so the one-shot closure is handed over through a slot.
        let mut once = Some(f);
        let mut out = None;
        let mut full = ctx.run_ui(Default::default(), |ui| {
            if let Some(f) = once.take() {
                out = Some(f(ui));
            }
        });
        // epaint 0.36 panics on a dropped `TexturesDelta` no renderer consumed.
        full.textures_delta.clear();
        out.expect("the ui closure ran")
    }

    // The bug the helper exists to prevent: a card whose fill equals the panel it sits on is
    // invisible, and every flat theme in the picker has at least one slot that collapses that way.
    #[test]
    fn a_card_never_disappears_into_its_panel() {
        for preset in [
            PresetStyles::ShippedClassic,
            PresetStyles::MtechStudio,
            PresetStyles::LegacyClassic,
            PresetStyles::DefaultEgui,
            PresetStyles::MtechNoir,
            PresetStyles::MtechNoirGlass,
            PresetStyles::NebulaGlass,
            PresetStyles::ObsidianGlass,
            PresetStyles::VelvetGlass,
            PresetStyles::TwilightGlass,
            PresetStyles::QuartzGlass,
            PresetStyles::MtechGlassFull,
            PresetStyles::CarlDarkFull,
            PresetStyles::TokyoNightFull,
            PresetStyles::RerunMtechOledFull,
        ] {
            let (fill, panel, stroke) = with_preset(preset, |ui| {
                (card_fill(ui), ui.visuals().panel_fill, card_stroke(ui))
            });
            let separated = fill != panel
                || (stroke.width > 0.0 && stroke.color.a() > 0 && stroke.color != panel);
            assert!(
                separated,
                "{}: a card is indistinguishable from its panel (fill {fill:?}, panel {panel:?})",
                preset.as_str(),
            );
        }
    }

    #[test]
    fn a_fixed_width_frame_keeps_its_width_whatever_it_holds() {
        let (parent, rects) = with_preset(PresetStyles::MtechStudio, |ui| {
            let mut column = ui.new_child(
                UiBuilder::new()
                    .max_rect(Rect::from_min_size(pos2(10.0, 10.0), vec2(300.0, 600.0))),
            );
            let rects: Vec<Rect> = [40.0, 900.0, 120.0]
                .into_iter()
                .map(|content_w| {
                    let frame =
                        card_frame(&column).stroke(Stroke::new(1.0, card_stroke(&column).color));
                    let width = column.available_width();
                    let mut card = FixedWidthFrame::begin(&mut column, frame, width);
                    card.content_ui
                        .allocate_exact_size(vec2(content_w, 20.0), Sense::hover());
                    card.frame.stroke = Stroke::new(3.0, card.frame.stroke.color);
                    card.end(&mut column).rect
                })
                .collect();
            (column.min_rect(), rects)
        });
        for rect in &rects {
            assert_eq!((rect.left(), rect.right()), (10.0, 310.0), "card {rect:?}");
        }
        assert_eq!(parent.right(), 310.0, "a wide card widened its column");
    }

    // A glass theme's card has to stay translucent, or the frost behind it is wasted work.
    #[test]
    fn a_glass_theme_keeps_its_cards_translucent() {
        for preset in [
            PresetStyles::ObsidianGlass,
            PresetStyles::VelvetGlass,
            PresetStyles::TwilightGlass,
            PresetStyles::QuartzGlass,
            PresetStyles::NebulaGlass,
        ] {
            let fill = with_preset(preset, |ui| card_fill(ui));
            assert!(fill.a() < 255, "{}: card fill is opaque", preset.as_str());
        }
    }
}
