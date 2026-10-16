//! Rules, headings, collapsible sections, labels, links, chips and the spinner.

use std::sync::Arc;

use eframe::egui::{
    Align, AsIdSalt, Color32, CursorIcon, FontId, Galley, Id, IdSalt, Label, Layout, OpenUrl, Rect,
    Response, RichText, Role, Sense, Shape, Stroke, TextStyle, TextWrapMode, Ui, UiBuilder, Vec2,
    WidgetInfo,
    collapsing_header::CollapsingState,
    pos2,
    text::{LayoutJob, TextFormat},
    vec2,
};

use super::{
    Icon, State, Tokens, elided, fill_width, focus_ring, ink_on, kit, medium_font,
    medium_font_sized, mix, paint_box, semibold_font_sized, state_of, text_galley, whole,
};
use crate::metrics::type_scale;
use crate::{fonts, icon_pack};

/// Width a [`vline`] allocates around its rule.
const VLINE_W: f32 = 9.0;
/// Inset of a section header's chevron centre from the left edge.
const CHEVRON_X: f32 = 7.0;
/// Size of a section header's chevron.
const CHEVRON: f32 = 12.0;
/// Gap between adjacent items inside a header, label or chip.
const INNER_GAP: f32 = 6.0;
/// Opacity of a heading icon's duotone back layer.
const HEADING_DUO: f32 = 0.30;
/// Section-label size.
const SECTION_LABEL_PT: f32 = 11.5;
/// Badge text size.
const BADGE_PT: f32 = 10.5;
/// Horizontal padding inside a badge.
const BADGE_PAD_X: f32 = 6.0;
/// Badge corner radius.
const BADGE_RADIUS: u8 = 4;
/// Opacity of a badge's fill and border tint.
const BADGE_FILL: f32 = 0.16;
const BADGE_EDGE: f32 = 0.55;
/// Horizontal padding inside a pill.
const PILL_PAD_X: f32 = 10.0;
/// Share of the tone in a toned pill's fill and border.
const PILL_TONE_FILL: f32 = 0.20;
const PILL_TONE_EDGE: f32 = 0.55;
/// Count-badge text size.
const COUNT_PT: f32 = 9.5;
/// Count-badge height.
const COUNT_H: f32 = 14.0;
/// Horizontal padding inside a count badge.
const COUNT_PAD_X: f32 = 4.0;
/// How far a count badge sticks out past its anchor's corner.
const COUNT_OVERHANG: f32 = 4.0;
/// Turns per second of the spinner arc.
const SPIN_RATE: f32 = 1.1;
/// Share of a full turn the spinner arc covers.
const SPIN_ARC: f32 = 0.28;
/// Opacity of a tinted spinner's track.
const SPIN_TRACK: f32 = 0.22;

/// A full-width one-pixel rule in the separator colour.
pub fn hairline(ui: &mut Ui) -> Response {
    let (t, _) = kit(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(fill_width(ui), 1.0), Sense::hover());
    let y = ui.painter().round_to_pixel_center(rect.center().y);
    ui.painter()
        .hline(rect.x_range(), y, Stroke::new(1.0, t.separator));
    response
}

/// A vertical one-pixel rule `height` tall.
pub fn vline(ui: &mut Ui, height: f32) -> Response {
    let (t, _) = kit(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(VLINE_W, height), Sense::hover());
    let x = ui.painter().round_to_pixel_center(rect.center().x);
    ui.painter()
        .vline(x, rect.y_range(), Stroke::new(1.0, t.separator));
    response
}

/// Section header drawn `open` or closed; the caller flips its own state on a click.
pub fn fold_header(
    ui: &mut Ui,
    open: bool,
    icon: impl Into<Option<Icon>>,
    title: &str,
) -> Response {
    let look = HeaderLook {
        icon: icon.into(),
        icon_tone: None,
        title,
        title_tone: None,
        suffixes: &[],
    };
    fold_row(ui, open, &look, None)
}

/// What a section header shows besides its chevron.
struct HeaderLook<'s> {
    icon: Option<Icon>,
    icon_tone: Option<Color32>,
    title: &'s str,
    title_tone: Option<Color32>,
    suffixes: &'s [(&'s str, Option<Color32>)],
}

/// Extra widgets a section header lays out after its title.
type Trailing<'a> = Box<dyn FnOnce(&mut Ui) + 'a>;

/// One header row: chevron, icon, title and suffixes, then `trailing` in the room left.
fn fold_row(
    ui: &mut Ui,
    open: bool,
    look: &HeaderLook<'_>,
    trailing: Option<Trailing<'_>>,
) -> Response {
    let (t, m) = kit(ui);
    hairline(ui);
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), m.header_h), Sense::click());
    response.widget_info(|| {
        WidgetInfo::selected(Role::DisclosureTriangle, ui.is_enabled(), open, look.title)
    });
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    let chevron = Rect::from_center_size(
        pos2(rect.left() + CHEVRON_X, rect.center().y),
        Vec2::splat(CHEVRON),
    );
    let mut x = chevron.right() + INNER_GAP;
    let icon_slot = look.icon.map(|icon| {
        let slot = Rect::from_min_size(
            pos2(x, rect.center().y - m.icon_sm / 2.0),
            Vec2::splat(m.icon_sm),
        );
        x = slot.right() + INNER_GAP;
        (icon, slot)
    });
    let title = header_title(ui, &t, look, rect.right() - x);
    let text_right = x + title.size().x;
    if ui.is_rect_visible(rect) {
        let (state, focused) = state_of(ui, &response);
        let ink = if state == State::Rest {
            t.text_dim
        } else {
            t.text
        };
        let caret = if open {
            icon_pack::CARET_DOWN
        } else {
            icon_pack::CARET_RIGHT
        };
        caret.paint(ui, chevron, CHEVRON - 1.0, ink);
        if let Some((icon, slot)) = icon_slot {
            icon.paint(ui, slot, m.icon_sm, look.icon_tone.unwrap_or(t.icon));
        }
        let y = (rect.center().y - title.size().y / 2.0).round();
        ui.painter().galley(pos2(x, y), title, t.text);
        if focused {
            focus_ring(ui, rect, t.radius_sm, &t);
        }
    }
    if let Some(trailing) = trailing {
        let room = Rect::from_min_max(pos2(text_right + m.gap.x, rect.top()), rect.max);
        if room.width() > 0.0 {
            let layout = Layout::left_to_right(Align::Center);
            let mut child = ui.new_child(UiBuilder::new().max_rect(room).layout(layout));
            child.set_clip_rect(room.intersect(ui.clip_rect()));
            child.spacing_mut().item_spacing.x = INNER_GAP;
            child.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
            trailing(&mut child);
        }
    }
    response
}

/// The semibold title and its suffixes on one row no wider than `max_w`.
fn header_title(ui: &Ui, t: &Tokens, look: &HeaderLook<'_>, max_w: f32) -> Arc<Galley> {
    let body = TextStyle::Body.resolve(ui.style());
    let strong = semibold_font_sized(ui, body.size);
    let mut job = LayoutJob::default();
    let title_ink = look.title_tone.unwrap_or(t.text);
    job.append(look.title, 0.0, TextFormat::simple(strong, title_ink));
    for (text, tone) in look.suffixes {
        let format = TextFormat::simple(body.clone(), tone.unwrap_or(t.text_dim));
        job.append(text, INNER_GAP, format);
    }
    job.wrap.max_width = max_w.max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    ui.painter().layout_job(job)
}

/// A collapsible section whose open state shares the id of a same-salt `CollapsingHeader`.
#[must_use = "call `header` or `show` to draw the section"]
pub struct Section<'a> {
    salt: IdSalt,
    id: Option<Id>,
    title: &'a str,
    title_tone: Option<Color32>,
    suffixes: Vec<(&'a str, Option<Color32>)>,
    icon: Option<Icon>,
    icon_tone: Option<Color32>,
    default_open: bool,
    open: Option<bool>,
    trailing: Option<Trailing<'a>>,
}

impl<'a> Section<'a> {
    /// Open by default.
    pub fn new(id_salt: impl AsIdSalt, title: &'a str) -> Self {
        Self {
            salt: IdSalt::new(id_salt),
            id: None,
            title,
            title_tone: None,
            suffixes: Vec::new(),
            icon: None,
            icon_tone: None,
            default_open: true,
            open: None,
            trailing: None,
        }
    }

    /// Exact id of the open state in place of one derived from the salt and the `Ui`.
    pub fn id(mut self, id: Id) -> Self {
        self.id = Some(id);
        self
    }

    pub fn icon(mut self, icon: impl Into<Option<Icon>>) -> Self {
        self.icon = icon.into();
        self
    }

    /// Icon colour in place of the icon token.
    pub fn icon_tone(mut self, tone: Color32) -> Self {
        self.icon_tone = Some(tone);
        self
    }

    /// Title colour in place of the text token.
    pub fn title_tone(mut self, tone: Color32) -> Self {
        self.title_tone = Some(tone);
        self
    }

    /// Body-weight text after the title, in `tone` or the dim text colour.
    pub fn suffix(mut self, text: &'a str, tone: impl Into<Option<Color32>>) -> Self {
        self.suffixes.push((text, tone.into()));
        self
    }

    /// Widgets laid out on the header row after the title.
    pub fn trailing(mut self, add: impl FnOnce(&mut Ui) + 'a) -> Self {
        self.trailing = Some(Box::new(add));
        self
    }

    /// Open state before the user first toggles it.
    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    /// Sets the open state this frame when `Some`.
    pub fn open(mut self, open: Option<bool>) -> Self {
        self.open = open;
        self
    }

    /// Draws the header; returns whether the section is open.
    pub fn header(self, ui: &mut Ui) -> bool {
        self.header_response(ui).1
    }

    /// Draws the header; returns its response and whether the section is open.
    pub fn header_response(self, ui: &mut Ui) -> (Response, bool) {
        let id = self.id.unwrap_or_else(|| ui.make_persistent_id(self.salt));
        let mut state = CollapsingState::load_with_default_open(ui.ctx(), id, self.default_open);
        if let Some(open) = self.open {
            state.set_open(open);
        }
        let look = HeaderLook {
            icon: self.icon,
            icon_tone: self.icon_tone,
            title: self.title,
            title_tone: self.title_tone,
            suffixes: &self.suffixes,
        };
        let response = fold_row(ui, state.is_open(), &look, self.trailing);
        if response.clicked() {
            state.toggle(ui);
        }
        state.store(ui.ctx());
        (response, state.is_open())
    }

    /// Draws the header and, while open, runs `add` indented below it.
    pub fn show<R>(self, ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        self.show_with_header(ui, add).1
    }

    /// [`Self::show`] that also returns the header's response.
    pub fn show_with_header<R>(
        self,
        ui: &mut Ui,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> (Response, Option<R>) {
        let salt = self.salt;
        let (header, open) = self.header_response(ui);
        (header, open.then(|| ui.indent(salt, add).inner))
    }
}

/// Collapsible section header, open by default; returns whether open.
pub fn section_header(
    ui: &mut Ui,
    id_salt: impl AsIdSalt,
    icon: impl Into<Option<Icon>>,
    title: &str,
) -> bool {
    Section::new(id_salt, title).icon(icon).header(ui)
}

/// A heading: duotone primary icon and semibold title.
pub fn heading(ui: &mut Ui, icon: impl Into<Option<Icon>>, title: &str) -> Response {
    let (t, m) = kit(ui);
    let icon = icon.into();
    let galley = text_galley(ui, title, semibold_font_sized(ui, type_scale::HEADING));
    let icon_w = icon.map_or(0.0, |_| m.icon_lg + m.gap.x);
    let height = galley.size().y.max(m.icon_lg);
    let (rect, response) = ui.allocate_exact_size(
        vec2(whole(icon_w + galley.size().x), height),
        Sense::hover(),
    );
    if ui.is_rect_visible(rect) {
        if let Some(icon) = icon {
            let slot = Rect::from_min_size(
                pos2(rect.left(), rect.center().y - m.icon_lg / 2.0),
                Vec2::splat(m.icon_lg),
            );
            let back = t.primary.gamma_multiply(HEADING_DUO);
            icon.paint_duo(ui, slot, m.icon_lg, t.primary, back);
        }
        let y = (rect.center().y - galley.size().y / 2.0).round();
        ui.painter()
            .galley(pos2(rect.left() + icon_w, y), galley, t.text);
    }
    response.widget_info(|| WidgetInfo::labeled(Role::Label, ui.is_enabled(), title));
    response
}

/// A sub-heading: icon and semibold body-size title.
pub fn subheading(ui: &mut Ui, icon: impl Into<Option<Icon>>, title: &str) -> Response {
    let (t, m) = kit(ui);
    let icon = icon.into();
    let size = TextStyle::Body.resolve(ui.style()).size;
    let galley = text_galley(ui, title, semibold_font_sized(ui, size));
    let icon_w = icon.map_or(0.0, |_| m.icon_md + INNER_GAP);
    let height = galley.size().y.max(m.icon_md);
    let (rect, response) = ui.allocate_exact_size(
        vec2(whole(icon_w + galley.size().x), height),
        Sense::hover(),
    );
    if ui.is_rect_visible(rect) {
        if let Some(icon) = icon {
            let slot = Rect::from_min_size(
                pos2(rect.left(), rect.center().y - m.icon_md / 2.0),
                Vec2::splat(m.icon_md),
            );
            icon.paint(ui, slot, m.icon_md, t.icon);
        }
        let y = (rect.center().y - galley.size().y / 2.0).round();
        ui.painter()
            .galley(pos2(rect.left() + icon_w, y), galley, t.text);
    }
    response.widget_info(|| WidgetInfo::labeled(Role::Label, ui.is_enabled(), title));
    response
}

/// A small faint sub-heading.
pub fn section_label(ui: &mut Ui, text: &str) -> Response {
    let (t, _) = kit(ui);
    let galley = text_galley(ui, text, medium_font_sized(ui, SECTION_LABEL_PT));
    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::hover());
    ui.painter().galley(rect.min, galley, t.text_faint);
    response.widget_info(|| WidgetInfo::labeled(Role::Label, ui.is_enabled(), text));
    response
}

/// A [`section_label`] led by a small faint icon.
pub fn section_label_icon(ui: &mut Ui, icon: Icon, text: &str) -> Response {
    let (t, m) = kit(ui);
    ui.horizontal(|ui| {
        icon.show_tinted(ui, m.icon_sm, t.text_faint);
        section_label(ui, text);
    })
    .response
}

/// One row of body text no wider than `max_w`, cut with '…' and shown whole on hover.
pub fn elided_label(ui: &mut Ui, text: &str, max_w: f32) -> Response {
    let (t, _) = kit(ui);
    let galley = elided(ui, text, TextStyle::Body.resolve(ui.style()), max_w);
    let cut = galley.elided;
    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::hover());
    ui.painter().galley(rect.min, galley, t.text);
    response.widget_info(|| WidgetInfo::labeled(Role::Label, ui.is_enabled(), text));
    if cut {
        response.on_hover_text(text)
    } else {
        response
    }
}

/// An icon followed by text, both optionally tinted.
#[must_use = "call `show` to draw the label"]
pub struct IconLabel<'a> {
    icon: Icon,
    text: &'a str,
    tone: Option<Color32>,
    icon_tone: Option<Color32>,
    small: bool,
    wrap: bool,
    selectable: bool,
    strong: bool,
    mono: bool,
    truncate: bool,
}

impl<'a> IconLabel<'a> {
    pub fn new(icon: Icon, text: &'a str) -> Self {
        Self {
            icon,
            text,
            tone: None,
            icon_tone: None,
            small: false,
            wrap: false,
            selectable: false,
            strong: false,
            mono: false,
            truncate: false,
        }
    }

    /// Semibold text.
    pub fn strong(mut self, strong: bool) -> Self {
        self.strong = strong;
        self
    }

    /// Monospace text; wins over [`Self::strong`].
    pub fn mono(mut self, mono: bool) -> Self {
        self.mono = mono;
        self
    }

    /// One row cut with '…' at the available width, shown whole on hover.
    pub fn truncate(mut self, truncate: bool) -> Self {
        self.truncate = truncate;
        self
    }

    /// Text font for the label's size and weight.
    fn font(&self, ui: &Ui, style: &TextStyle) -> FontId {
        let size = style.resolve(ui.style()).size;
        if self.mono {
            let mono = if self.small {
                size
            } else {
                TextStyle::Monospace.resolve(ui.style()).size
            };
            fonts::mono(mono)
        } else if self.strong {
            semibold_font_sized(ui, size)
        } else {
            style.resolve(ui.style())
        }
    }

    /// Icon and text colour.
    pub fn tone(mut self, tone: Color32) -> Self {
        self.tone = Some(tone);
        self
    }

    /// Icon colour only.
    pub fn icon_tone(mut self, tone: Color32) -> Self {
        self.icon_tone = Some(tone);
        self
    }

    /// Small icon and small text.
    pub fn small(mut self, small: bool) -> Self {
        self.small = small;
        self
    }

    /// Wraps the text at the available width; otherwise it follows the `Ui`'s wrap mode.
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// Lets the text be selected and copied.
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    /// The response covers the text.
    pub fn show(self, ui: &mut Ui) -> Response {
        let (t, m) = kit(ui);
        let ink = self.tone.unwrap_or(t.text);
        let icon_ink = self.icon_tone.or(self.tone).unwrap_or(t.icon);
        let (side, style) = if self.small {
            (m.icon_sm, TextStyle::Small)
        } else {
            (m.icon_md, TextStyle::Body)
        };
        let font = self.font(ui, &style);
        if !self.wrap && !self.selectable {
            let galley = if self.truncate {
                let room = ui.available_width() - side - INNER_GAP;
                elided(ui, self.text, font, room)
            } else {
                text_galley(ui, self.text, font)
            };
            let cut = galley.elided;
            let height = galley.size().y.max(side);
            let width = whole(side + INNER_GAP + galley.size().x);
            let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
            if ui.is_rect_visible(rect) {
                let slot = Rect::from_min_size(
                    pos2(rect.left(), rect.center().y - side / 2.0),
                    Vec2::splat(side),
                );
                self.icon.paint(ui, slot, side, icon_ink);
                let y = (rect.center().y - galley.size().y / 2.0).round();
                ui.painter()
                    .galley(pos2(slot.right() + INNER_GAP, y), galley, ink);
            }
            response.widget_info(|| WidgetInfo::labeled(Role::Label, ui.is_enabled(), self.text));
            return if cut {
                response.on_hover_text(self.text)
            } else {
                response
            };
        }
        let line_h = ui.text_style_height(&style);
        let width = ui.available_width();
        let rtl = ui.layout().prefer_right_to_left();
        let layout = if rtl {
            Layout::right_to_left(Align::Min)
        } else {
            Layout::left_to_right(Align::Min)
        };
        ui.allocate_ui_with_layout(vec2(width, line_h), layout, |ui| {
            ui.spacing_mut().item_spacing.x = INNER_GAP;
            let label = Label::new(RichText::new(self.text).font(font).color(ink))
                .selectable(self.selectable);
            let label = if self.truncate {
                label.truncate()
            } else if self.wrap {
                label.wrap()
            } else {
                label
            };
            let icon = |ui: &mut Ui| {
                let (slot, _) = ui.allocate_exact_size(vec2(side, line_h), Sense::hover());
                self.icon.paint(ui, slot, side, icon_ink);
            };
            // Text first in right-to-left rows.
            if rtl {
                let response = ui.add(label);
                icon(ui);
                response
            } else {
                icon(ui);
                ui.add(label)
            }
        })
        .inner
    }
}

/// An icon followed by body text, centred on one row.
pub fn icon_label(ui: &mut Ui, icon: Icon, text: &str) -> Response {
    IconLabel::new(icon, text).show(ui)
}

/// An icon and selectable text, both tinted `tone`; the text follows the `Ui`'s wrap mode.
pub fn status_label(ui: &mut Ui, icon: Icon, text: &str, tone: Color32) -> Response {
    IconLabel::new(icon, text)
        .tone(tone)
        .selectable(true)
        .show(ui)
}

/// An icon and text, both tinted `tone`; the text wraps at the available width.
pub fn note(ui: &mut Ui, icon: Icon, text: &str, tone: Color32) -> Response {
    IconLabel::new(icon, text).tone(tone).wrap(true).show(ui)
}

/// Clickable text with an optional leading icon, underlined on hover.
#[must_use = "call `show` to draw the link"]
pub struct IconLink<'a> {
    text: &'a str,
    icon: Option<Icon>,
    tone: Option<Color32>,
    url: Option<&'a str>,
    small: bool,
    new_tab: bool,
}

impl<'a> IconLink<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            icon: None,
            tone: None,
            url: None,
            small: false,
            new_tab: false,
        }
    }

    /// Small text.
    pub fn small(mut self, small: bool) -> Self {
        self.small = small;
        self
    }

    /// Opens the url in a new tab even without a modifier held.
    pub fn new_tab(mut self, new_tab: bool) -> Self {
        self.new_tab = new_tab;
        self
    }

    pub fn icon(mut self, icon: impl Into<Option<Icon>>) -> Self {
        self.icon = icon.into();
        self
    }

    /// Icon and text colour in place of the info role.
    pub fn tone(mut self, tone: Color32) -> Self {
        self.tone = Some(tone);
        self
    }

    /// Opens `url` when clicked and shows it on hover.
    pub fn url(mut self, url: &'a str) -> Self {
        self.url = Some(url);
        self
    }

    /// The text galley and the width of the icon slot before it.
    fn parts(&self, ui: &Ui, icon_sm: f32) -> (Arc<Galley>, f32) {
        let style = if self.small {
            TextStyle::Small
        } else {
            TextStyle::Body
        };
        let galley = text_galley(ui, self.text, style.resolve(ui.style()));
        (galley, self.icon.map_or(0.0, |_| icon_sm + INNER_GAP))
    }

    /// Width [`Self::show`] allocates.
    pub fn width(&self, ui: &Ui) -> f32 {
        let (galley, icon_w) = self.parts(ui, kit(ui).1.icon_sm);
        whole(icon_w + galley.size().x)
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let (t, m) = kit(ui);
        let ink = self.tone.unwrap_or(t.info);
        let (galley, icon_w) = self.parts(ui, m.icon_sm);
        let height = galley.size().y.max(m.icon_sm);
        let size = vec2(whole(icon_w + galley.size().x), height);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        response.widget_info(|| WidgetInfo::labeled(Role::Link, ui.is_enabled(), self.text));
        if ui.is_rect_visible(rect) {
            let (state, focused) = state_of(ui, &response);
            if let Some(icon) = self.icon {
                let slot = Rect::from_min_size(
                    pos2(rect.left(), rect.center().y - m.icon_sm / 2.0),
                    Vec2::splat(m.icon_sm),
                );
                icon.paint(ui, slot, m.icon_sm, ink);
            }
            let y = (rect.center().y - galley.size().y / 2.0).round();
            let text_rect = Rect::from_min_size(pos2(rect.left() + icon_w, y), galley.size());
            ui.painter().galley(text_rect.min, galley, ink);
            if state != State::Rest {
                let base = ui.painter().round_to_pixel_center(text_rect.bottom() - 1.0);
                ui.painter()
                    .hline(text_rect.x_range(), base, Stroke::new(1.0, ink));
            }
            if focused {
                focus_ring(ui, rect, t.radius_sm, &t);
            }
        }
        let response = response.on_hover_cursor(CursorIcon::PointingHand);
        let Some(url) = self.url else {
            return response;
        };
        if response.clicked() {
            let new_tab = self.new_tab || ui.input(|i| i.modifiers.any());
            ui.ctx().open_url(OpenUrl {
                url: url.to_owned(),
                new_tab,
            });
        }
        response.on_hover_text(url)
    }
}

/// Info-toned clickable text with an optional leading icon.
pub fn icon_link(ui: &mut Ui, icon: impl Into<Option<Icon>>, text: &str) -> Response {
    IconLink::new(text).icon(icon).show(ui)
}

/// Info-toned link that opens `url` when clicked.
pub fn icon_hyperlink(
    ui: &mut Ui,
    icon: impl Into<Option<Icon>>,
    text: &str,
    url: &str,
) -> Response {
    IconLink::new(text).icon(icon).url(url).show(ui)
}

/// A compact status or count chip tinted `tone`.
pub fn badge(ui: &mut Ui, text: &str, tone: Color32) -> Response {
    let (_, m) = kit(ui);
    let galley = text_galley(ui, text, medium_font_sized(ui, BADGE_PT));
    let height = (m.control_h - 6.0).max(galley.size().y);
    let size = vec2(whole(galley.size().x + 2.0 * BADGE_PAD_X), height);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        let fill = tone.gamma_multiply(BADGE_FILL);
        paint_box(
            ui,
            rect,
            BADGE_RADIUS,
            fill,
            tone.gamma_multiply(BADGE_EDGE),
        );
        let pos = (rect.center() - galley.size() / 2.0).round();
        ui.painter().galley(pos, galley, tone);
    }
    response.widget_info(|| WidgetInfo::labeled(Role::Label, ui.is_enabled(), text));
    response
}

/// A count pill over the top-right corner of `anchor`; nothing for zero, "99+" above 99.
pub fn count_badge(ui: &Ui, anchor: Rect, count: usize, tone: Color32) {
    if count == 0 || !ui.is_rect_visible(anchor) {
        return;
    }
    let (t, _) = kit(ui);
    let text = if count > 99 {
        "99+".to_owned()
    } else {
        count.to_string()
    };
    let galley = text_galley(ui, &text, medium_font_sized(ui, COUNT_PT));
    let width = whole(galley.size().x + 2.0 * COUNT_PAD_X).max(COUNT_H);
    let min = pos2(
        anchor.right() - width + COUNT_OVERHANG,
        anchor.top() - COUNT_OVERHANG,
    );
    let rect = Rect::from_min_size(min.round(), vec2(width, COUNT_H));
    ui.painter().rect(
        rect,
        (COUNT_H / 2.0) as u8,
        tone,
        Stroke::new(1.0, t.base),
        eframe::egui::StrokeKind::Outside,
    );
    let pos = (rect.center() - galley.size() / 2.0).round();
    ui.painter().galley(pos, galley, ink_on(tone));
}

/// A rounded, selectable chip.
#[must_use = "call `show` to draw the pill"]
pub struct Pill<'a> {
    label: &'a str,
    icon: Option<Icon>,
    trailing: Option<Icon>,
    selected: bool,
    tone: Option<Color32>,
}

impl<'a> Pill<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            icon: None,
            trailing: None,
            selected: false,
            tone: None,
        }
    }

    pub fn icon(mut self, icon: impl Into<Option<Icon>>) -> Self {
        self.icon = icon.into();
        self
    }

    /// Small icon after the label, such as a caret.
    pub fn trailing(mut self, icon: impl Into<Option<Icon>>) -> Self {
        self.trailing = icon.into();
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Colour of the selected chip's wash, border, icon and text.
    pub fn tone(mut self, tone: Color32) -> Self {
        self.tone = Some(tone);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let (t, m) = kit(ui);
        let galley = text_galley(ui, self.label, medium_font(ui, TextStyle::Button));
        let icon_w = self.icon.map_or(0.0, |_| m.icon_sm + INNER_GAP);
        let tail = m.icon_sm - 2.0;
        let tail_w = self.trailing.map_or(0.0, |_| INNER_GAP + tail);
        let size = vec2(
            whole(galley.size().x + icon_w + tail_w + 2.0 * PILL_PAD_X),
            m.control_h,
        );
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        response.widget_info(|| {
            WidgetInfo::selected(Role::Button, ui.is_enabled(), self.selected, self.label)
        });
        if ui.is_rect_visible(rect) {
            let (state, focused) = state_of(ui, &response);
            let radius = (m.control_h / 2.0) as u8;
            let (fill, edge, ink, tint) = match (self.selected, self.tone) {
                (true, Some(tone)) => (
                    mix(t.field, tone, PILL_TONE_FILL),
                    mix(t.field, tone, PILL_TONE_EDGE),
                    tone,
                    tone,
                ),
                (true, None) => (t.field, t.field_border, t.text, t.primary),
                (false, _) => {
                    let (fill, ink) = match state {
                        State::Rest => (Color32::TRANSPARENT, t.text_dim),
                        State::Hovered => (t.hover, t.text),
                        State::Pressed => (t.pressed, t.text),
                    };
                    (fill, Color32::TRANSPARENT, ink, ink)
                }
            };
            paint_box(ui, rect, radius, fill, edge);
            let text_w = galley.size().x;
            let left = (rect.center().x - (text_w + icon_w + tail_w) / 2.0).round();
            if let Some(icon) = self.icon {
                let slot = Rect::from_min_size(
                    pos2(left, rect.center().y - m.icon_sm / 2.0),
                    Vec2::splat(m.icon_sm),
                );
                icon.paint(ui, slot, m.icon_sm, tint);
            }
            let y = (rect.center().y - galley.size().y / 2.0).round();
            ui.painter().galley(pos2(left + icon_w, y), galley, ink);
            if let Some(icon) = self.trailing {
                let slot = Rect::from_min_size(
                    pos2(
                        left + icon_w + text_w + INNER_GAP,
                        rect.center().y - tail / 2.0,
                    ),
                    Vec2::splat(tail),
                );
                icon.paint(ui, slot, tail, ink);
            }
            if focused {
                focus_ring(ui, rect, radius, &t);
            }
        }
        response
    }
}

/// A rounded, selectable chip.
pub fn pill(ui: &mut Ui, icon: impl Into<Option<Icon>>, label: &str, selected: bool) -> Response {
    Pill::new(label).icon(icon).selected(selected).show(ui)
}

/// Spinner sizes in points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SpinnerSize {
    Sm = 14,
    #[default]
    Md = 20,
    Lg = 32,
}

impl SpinnerSize {
    pub fn side(self) -> f32 {
        self as u8 as f32
    }

    fn stroke(self) -> f32 {
        match self {
            Self::Sm => 1.5,
            Self::Md => 2.0,
            Self::Lg => 3.0,
        }
    }
}

/// A primary-tinted arc turning over a faint track.
pub fn spinner(ui: &mut Ui, size: SpinnerSize) -> Response {
    let (t, _) = kit(ui);
    spin(ui, size, t.primary, t.primary_soft)
}

/// A spinner whose arc is `tint` over a faint track of the same hue.
pub fn tinted_spinner(ui: &mut Ui, size: SpinnerSize, tint: Color32) -> Response {
    spin(ui, size, tint, tint.gamma_multiply(SPIN_TRACK))
}

fn spin(ui: &mut Ui, size: SpinnerSize, arc: Color32, track: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size.side()), Sense::hover());
    response
        .widget_info(|| WidgetInfo::labeled(Role::ProgressIndicator, ui.is_enabled(), "Loading"));
    if ui.is_rect_visible(rect) {
        paint_spin(ui, rect, size, arc, track);
    }
    response
}

/// Paints a small spinner centred in `rect`, tinted `tone` or primary.
pub(crate) fn paint_small_spinner(ui: &Ui, rect: Rect, tone: Option<Color32>) {
    let (t, _) = kit(ui);
    let (arc, track) = match tone {
        Some(tone) => (tone, tone.gamma_multiply(SPIN_TRACK)),
        None => (t.primary, t.primary_soft),
    };
    paint_spin(ui, rect, SpinnerSize::Sm, arc, track);
}

/// Paints a spinner of `size` centred in `rect` and schedules the next frame.
fn paint_spin(ui: &Ui, rect: Rect, size: SpinnerSize, arc: Color32, track: Color32) {
    ui.ctx().request_repaint();
    let width = size.stroke();
    let radius = (size.side() - width) / 2.0;
    let center = rect.center();
    ui.painter()
        .circle_stroke(center, radius, Stroke::new(width, track));
    let time = ui.input(|i| i.time) as f32;
    let start = time * SPIN_RATE * std::f32::consts::TAU;
    let sweep = SPIN_ARC * std::f32::consts::TAU;
    let steps = 24;
    let points = (0..=steps)
        .map(|i| {
            let a = start + sweep * i as f32 / steps as f32;
            center + radius * Vec2::angled(a)
        })
        .collect();
    ui.painter()
        .add(Shape::line(points, Stroke::new(width, arc)));
}

/// A small spinner and a dim caption on one row.
pub fn loading_row(ui: &mut Ui, caption: &str) -> Response {
    let (t, _) = kit(ui);
    let size = SpinnerSize::Sm;
    let side = size.side();
    let galley = text_galley(ui, caption, TextStyle::Body.resolve(ui.style()));
    let text_w = if caption.is_empty() {
        0.0
    } else {
        INNER_GAP + galley.size().x
    };
    let height = galley.size().y.max(side);
    let (rect, response) =
        ui.allocate_exact_size(vec2(whole(side + text_w), height), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(Role::ProgressIndicator, ui.is_enabled(), caption));
    if ui.is_rect_visible(rect) {
        let slot = Rect::from_min_size(
            pos2(rect.left(), rect.center().y - side / 2.0),
            Vec2::splat(side),
        );
        paint_spin(ui, slot, size, t.primary, t.primary_soft);
        let y = (rect.center().y - galley.size().y / 2.0).round();
        ui.painter()
            .galley(pos2(slot.right() + INNER_GAP, y), galley, t.text_dim);
    }
    response
}
