//! Text buttons in four tones and fixed-box icon buttons.

use std::sync::Arc;

use eframe::egui::{
    Color32, Galley, Rect, Response, Sense, TextStyle, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};

use super::{
    Icon, Metrics, State, Tokens, focus_ring, kit, medium_font, mix, paint_box, state_of,
    text_galley, whole,
};

/// Gap between a button's icon and its label.
const ICON_GAP: f32 = 6.0;
/// Share of white mixed into a primary fill on hover.
const PRIMARY_HOVER_LIFT: f32 = 0.10;
/// Share of black mixed into a primary fill while pressed.
const PRIMARY_PRESS_SHADE: f32 = 0.18;
/// Share of the error hue in a danger button's fill, at rest, hovered and pressed.
const DANGER_WASH: [f32; 3] = [0.14, 0.22, 0.30];
/// Share of the error hue in a danger button's border.
const DANGER_EDGE: f32 = 0.55;
/// Opacity of a selected icon's duotone back layer.
const DUO_ALPHA: f32 = 0.30;

/// What a [`TextButton`] is for, which decides its colours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ButtonKind {
    /// Field fill with a neutral border.
    #[default]
    Neutral,
    /// Filled with the primary role.
    Primary,
    /// Error-tinted wash for destructive actions.
    Danger,
    /// Transparent until hovered.
    Ghost,
}

/// A text button with an optional leading icon.
#[must_use = "call `show` to draw the button"]
pub struct TextButton<'a> {
    label: &'a str,
    icon: Option<Icon>,
    kind: ButtonKind,
    prominent: bool,
    min_width: f32,
    ink: Option<Color32>,
    enabled: bool,
    mono: bool,
}

impl<'a> TextButton<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            icon: None,
            kind: ButtonKind::Neutral,
            prominent: false,
            min_width: 0.0,
            ink: None,
            enabled: true,
            mono: false,
        }
    }

    pub fn kind(mut self, kind: ButtonKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Uses [`Metrics::button_h`] instead of [`Metrics::control_h`].
    pub fn prominent(mut self, prominent: bool) -> Self {
        self.prominent = prominent;
        self
    }

    pub fn min_width(mut self, min_width: f32) -> Self {
        self.min_width = min_width;
        self
    }

    /// Label and icon colour in every state, in place of the kind's ink.
    pub fn ink(mut self, ink: Color32) -> Self {
        self.ink = Some(ink);
        self
    }

    /// Draws the button disabled and ignores clicks when `false`.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Monospace label.
    pub fn mono(mut self, mono: bool) -> Self {
        self.mono = mono;
        self
    }

    /// Size this button takes at the current density.
    pub fn size(&self, ui: &Ui) -> Vec2 {
        let (_, m) = kit(ui);
        self.measure(&m, self.label_galley(ui).size().x)
    }

    /// Size for a label `text_w` wide.
    pub(crate) fn measure(&self, m: &Metrics, text_w: f32) -> Vec2 {
        let icon_w = self.icon.map_or(0.0, |_| m.icon_md + ICON_GAP);
        let h = if self.prominent {
            m.button_h
        } else {
            m.control_h
        };
        vec2(
            whole(text_w + icon_w + 2.0 * m.pad.x).max(self.min_width),
            h,
        )
    }

    /// The label laid out in the button's face.
    pub(crate) fn label_galley(&self, ui: &Ui) -> Arc<Galley> {
        galley(ui, self.label, self.mono)
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let text = self.label_galley(ui);
        self.show_galley(ui, text)
    }

    /// Draws the button around a label galley from [`Self::label_galley`].
    pub(crate) fn show_galley(self, ui: &mut Ui, text: Arc<Galley>) -> Response {
        if !self.enabled {
            return ui
                .add_enabled_ui(false, |ui| self.enabled(true).show_galley(ui, text))
                .inner;
        }
        let (t, m) = kit(ui);
        let size = self.measure(&m, text.size().x);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        response
            .widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), self.label));
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let (state, focused) = state_of(ui, &response);
        let (fill, stroke, ink) = colors(&t, self.kind, state);
        let ink = self.ink.unwrap_or(ink);
        paint_box(ui, rect, t.radius_sm, fill, stroke);

        let icon_w = self.icon.map_or(0.0, |_| m.icon_md + ICON_GAP);
        let left = (rect.center().x - (text.size().x + icon_w) / 2.0).round();
        if let Some(icon) = self.icon {
            let slot = Rect::from_center_size(
                pos2(left + m.icon_md / 2.0, rect.center().y),
                Vec2::splat(m.icon_md),
            );
            icon.paint(ui, slot, m.icon_md, ink);
        }
        let pos = pos2(
            left + icon_w,
            (rect.center().y - text.size().y / 2.0).round(),
        );
        ui.painter().galley(pos, text, ink);
        if focused {
            focus_ring(ui, rect, t.radius_sm, &t);
        }
        response
    }
}

/// Button label on one row in the theme's medium button face, or monospace.
fn galley(ui: &Ui, label: &str, mono: bool) -> Arc<Galley> {
    let font = if mono {
        crate::fonts::mono(TextStyle::Monospace.resolve(ui.style()).size)
    } else {
        medium_font(ui, TextStyle::Button)
    };
    text_galley(ui, label, font)
}

/// Fill, border and ink for a button of `kind` in `state`.
fn colors(t: &Tokens, kind: ButtonKind, state: State) -> (Color32, Color32, Color32) {
    match kind {
        ButtonKind::Neutral => match state {
            State::Rest => (t.field, t.field_border, t.text),
            State::Hovered => (t.hover, t.field_border, t.text),
            State::Pressed => (t.pressed, t.primary_border, t.text),
        },
        ButtonKind::Primary => match state {
            State::Rest => (t.primary, Color32::TRANSPARENT, t.on_primary),
            State::Hovered => (
                mix(t.primary, Color32::WHITE, PRIMARY_HOVER_LIFT),
                Color32::TRANSPARENT,
                t.on_primary,
            ),
            State::Pressed => (
                mix(t.primary, Color32::BLACK, PRIMARY_PRESS_SHADE),
                Color32::TRANSPARENT,
                t.on_primary,
            ),
        },
        ButtonKind::Danger => {
            let wash = match state {
                State::Rest => DANGER_WASH[0],
                State::Hovered => DANGER_WASH[1],
                State::Pressed => DANGER_WASH[2],
            };
            (
                mix(t.field, t.error, wash),
                mix(t.field, t.error, DANGER_EDGE),
                t.error,
            )
        }
        ButtonKind::Ghost => match state {
            State::Rest => (Color32::TRANSPARENT, Color32::TRANSPARENT, t.text_dim),
            State::Hovered => (t.hover, Color32::TRANSPARENT, t.text),
            State::Pressed => (t.pressed, Color32::TRANSPARENT, t.text),
        },
    }
}

/// Neutral button at [`Metrics::control_h`].
pub fn button(ui: &mut Ui, label: &str) -> Response {
    TextButton::new(label).show(ui)
}

/// Neutral button with a leading icon at [`Metrics::control_h`].
pub fn icon_text_button(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    TextButton::new(label).icon(icon).show(ui)
}

/// Primary-filled button at [`Metrics::button_h`].
pub fn primary_button(ui: &mut Ui, label: &str) -> Response {
    TextButton::new(label)
        .kind(ButtonKind::Primary)
        .prominent(true)
        .show(ui)
}

/// Neutral button at [`Metrics::button_h`].
pub fn secondary_button(ui: &mut Ui, label: &str) -> Response {
    TextButton::new(label).prominent(true).show(ui)
}

/// Error-tinted button at [`Metrics::button_h`].
pub fn danger_button(ui: &mut Ui, label: &str) -> Response {
    TextButton::new(label)
        .kind(ButtonKind::Danger)
        .prominent(true)
        .show(ui)
}

/// Transparent-until-hovered button at [`Metrics::control_h`].
pub fn ghost_button(ui: &mut Ui, label: &str) -> Response {
    TextButton::new(label).kind(ButtonKind::Ghost).show(ui)
}

/// Square icon-button sizes; the discriminant is the side at Regular density.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum IconBox {
    Sm = 20,
    /// Matches [`Metrics::control_h`].
    #[default]
    Md = 24,
    /// Matches [`Metrics::button_h`].
    Lg = 28,
    Tool = 32,
}

impl IconBox {
    /// Side of the box at the density of `m`.
    pub fn side(self, m: &Metrics) -> f32 {
        match self {
            Self::Sm => m.control_h - 4.0,
            Self::Md => m.control_h,
            Self::Lg => m.button_h,
            Self::Tool => m.bar_h - 4.0,
        }
    }

    /// Icon size drawn in the box at the density of `m`.
    pub fn glyph(self, m: &Metrics) -> f32 {
        match self {
            Self::Sm => m.icon_sm,
            Self::Md => m.icon_md,
            Self::Lg => m.icon_md + 2.0,
            Self::Tool => m.icon_lg,
        }
    }
}

/// A square button holding one icon; transparent until hovered.
#[must_use = "call `show` to draw the button"]
pub struct IconButton<'a> {
    icon: Icon,
    size: IconBox,
    selected: bool,
    tip: &'a str,
    tint: Option<Color32>,
    enabled: bool,
}

impl<'a> IconButton<'a> {
    pub fn new(icon: Icon, size: IconBox) -> Self {
        Self {
            icon,
            size,
            selected: false,
            tip: "",
            tint: None,
            enabled: true,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Tooltip and accessible name.
    pub fn tip(mut self, tip: &'a str) -> Self {
        self.tip = tip;
        self
    }

    /// Icon colour while unselected, in place of the icon token.
    pub fn tint(mut self, tint: Color32) -> Self {
        self.tint = Some(tint);
        self
    }

    /// Draws the button disabled and ignores clicks when `false`.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        if !self.enabled {
            return ui
                .add_enabled_ui(false, |ui| self.enabled(true).show(ui))
                .inner;
        }
        let (t, m) = kit(ui);
        let side = self.size.side(&m);
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
        let name = if self.tip.is_empty() {
            self.icon.name()
        } else {
            self.tip
        };
        response.widget_info(|| {
            WidgetInfo::selected(WidgetType::Button, ui.is_enabled(), self.selected, name)
        });
        if ui.is_rect_visible(rect) {
            let (state, focused) = state_of(ui, &response);
            let glyph = self.size.glyph(&m);
            if self.selected {
                paint_box(ui, rect, t.radius_sm, t.primary_soft, t.primary_border);
                self.icon.paint_duo(
                    ui,
                    rect,
                    glyph,
                    t.primary,
                    t.primary.gamma_multiply(DUO_ALPHA),
                );
            } else {
                let rest = self.tint.unwrap_or(t.icon);
                let (fill, ink) = match state {
                    State::Rest => (Color32::TRANSPARENT, rest),
                    State::Hovered => (t.hover, self.tint.unwrap_or(t.text)),
                    State::Pressed => (t.pressed, self.tint.unwrap_or(t.text)),
                };
                ui.painter().rect_filled(rect, t.radius_sm, fill);
                self.icon.paint(ui, rect, glyph, ink);
            }
            if focused {
                focus_ring(ui, rect, t.radius_sm, &t);
            }
        }
        if self.tip.is_empty() {
            response
        } else {
            response.on_hover_text(self.tip)
        }
    }
}

/// Square icon button; `tip` is its tooltip and accessible name.
pub fn icon_button(ui: &mut Ui, icon: Icon, size: IconBox, selected: bool, tip: &str) -> Response {
    IconButton::new(icon, size)
        .selected(selected)
        .tip(tip)
        .show(ui)
}
