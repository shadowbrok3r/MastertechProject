//! Painted input boxes: text, multiline text, numeric with unit suffix, and dropdown.

use std::ops::RangeInclusive;

use eframe::egui::{
    Align, AsIdSalt, Atom, Color32, CornerRadius, DragValue, EventFilter, Frame, Id, IdSalt, Key,
    Layout, Margin, Popup, PopupCloseBehavior, Rect, Response, ScrollArea, Sense, Shape, Stroke,
    StrokeKind, TextBuffer, TextEdit, TextStyle, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType,
    emath::Numeric, epaint::RectShape, pos2, text_edit::TextEditOutput, vec2,
};

use super::{
    Icon, State, Tokens, elided, fill_width, focus_ring, kit, menu_rows, mix, paint_box,
    preview_focused, state_of, text_galley,
};
use crate::fonts;
use crate::icon_pack;

/// Horizontal inset of text inside a field box.
const FIELD_PAD_X: f32 = 8.0;
/// Vertical inset of text inside a text area.
const AREA_PAD_Y: f32 = 6.0;
/// Rows a text area shows by default.
const AREA_ROWS: usize = 4;
/// Gap between a leading icon and the text.
const ICON_GAP: f32 = 6.0;
/// Share of the primary role in a field's text-selection wash.
const SELECTION_MIX: f32 = 0.38;
/// Share of the text colour mixed into a hovered field border.
const HOVER_EDGE: f32 = 0.18;
/// Digit size in a value field.
const DIGITS_PT: f32 = 12.0;
/// Unit-suffix size in a value field.
const SUFFIX_PT: f32 = 11.0;
/// Inset of the unit suffix from the field's right edge.
const SUFFIX_INSET: f32 = 6.0;
/// Gap between the number and its suffix.
const SUFFIX_GAP: f32 = 3.0;
/// Upward nudge of the digits.
const DIGIT_LIFT: f32 = 1.0;
/// Width of a value field built without one.
const VALUE_W: f32 = 76.0;
/// Width of a dropdown built without one.
const DROPDOWN_W: f32 = 120.0;
/// Tallest a dropdown list grows before it scrolls.
const LIST_MAX_H: f32 = 320.0;
/// Label a dropdown shows when its value is not among the options.
const NO_OPTION: &str = "—";

/// Border of a field box in the given interaction state.
fn field_border(t: &Tokens, state: State, focused: bool) -> Color32 {
    if focused {
        t.primary_border
    } else if state == State::Rest {
        t.field_border
    } else {
        mix(t.field_border, t.text, HOVER_EDGE)
    }
}

/// Border of a field box, in the error colour while `invalid`.
fn border_for(t: &Tokens, state: State, focused: bool, invalid: bool) -> Color32 {
    if invalid {
        t.error
    } else {
        field_border(t, state, focused)
    }
}

/// Text-selection wash used inside fields only.
fn selection_wash(t: &Tokens) -> Color32 {
    mix(t.field, t.primary, SELECTION_MIX)
}

/// Field-only selection colours on `ui`.
fn field_selection(ui: &mut Ui, t: &Tokens) {
    ui.style_mut().visuals.selection.bg_fill = selection_wash(t);
    ui.style_mut().visuals.selection.stroke = Stroke::NONE;
}

/// A single-line text field: a painted box around a frameless [`TextEdit`]; `&mut &str` is read-only.
#[must_use = "call `show` to draw the field"]
pub struct TextField<'a> {
    text: &'a mut dyn TextBuffer,
    hint: &'a str,
    width: f32,
    icon: Option<Icon>,
    mono: bool,
    password: bool,
    clear: bool,
    invalid: bool,
    char_limit: Option<usize>,
    cursor_at_end: bool,
    id: Option<Id>,
    enabled: bool,
}

impl<'a> TextField<'a> {
    pub fn new(text: &'a mut dyn TextBuffer) -> Self {
        Self {
            text,
            hint: "",
            width: 160.0,
            icon: None,
            mono: false,
            password: false,
            clear: false,
            invalid: false,
            char_limit: None,
            cursor_at_end: true,
            id: None,
            enabled: true,
        }
    }

    /// Placeholder shown while empty.
    pub fn hint(mut self, hint: &'a str) -> Self {
        self.hint = hint;
        self
    }

    /// Outer width of the box.
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Icon drawn at the left edge of the box.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Monospace text.
    pub fn mono(mut self, mono: bool) -> Self {
        self.mono = mono;
        self
    }

    pub fn password(mut self, password: bool) -> Self {
        self.password = password;
        self
    }

    /// Shows a clear button while the field holds text.
    pub fn clear_button(mut self, clear: bool) -> Self {
        self.clear = clear;
        self
    }

    /// Draws the border in the error colour.
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// Most characters the field accepts.
    pub fn char_limit(mut self, limit: usize) -> Self {
        self.char_limit = Some(limit);
        self
    }

    /// Whether focusing the field puts the cursor after the text; on by default.
    pub fn cursor_at_end(mut self, at_end: bool) -> Self {
        self.cursor_at_end = at_end;
        self
    }

    /// Stable id for the text and its focus.
    pub fn id_salt(mut self, salt: impl AsIdSalt) -> Self {
        self.id = Some(Id::new(salt));
        self
    }

    /// Draws the field disabled and ignores input when `false`.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        self.show_output(ui).response.response
    }

    /// [`Self::show`] returning the editor's full output.
    pub fn show_output(self, ui: &mut Ui) -> TextEditOutput {
        if !self.enabled {
            return ui
                .add_enabled_ui(false, |ui| self.enabled(true).show_output(ui))
                .inner;
        }
        let (t, m) = kit(ui);
        let id = self.id.unwrap_or_else(|| ui.next_auto_id());
        ui.skip_ahead_auto_ids(1);
        let size = vec2(self.width.max(m.control_h), m.control_h);
        let (rect, frame) = ui.allocate_exact_size(size, Sense::CLICK);
        let focused = preview_focused(ui) || ui.memory(|mem| mem.has_focus(id));
        let (state, _) = state_of(ui, &frame);
        if ui.is_rect_visible(rect) {
            let border = border_for(&t, state, focused, self.invalid);
            paint_box(ui, rect, t.radius_sm, t.field, border);
        }

        let mut inner = rect.shrink2(vec2(FIELD_PAD_X, 0.0));
        if let Some(icon) = self.icon {
            let center = pos2(inner.left() + m.icon_sm / 2.0, rect.center().y);
            let ink = if focused { t.text_dim } else { t.text_faint };
            icon.paint(
                ui,
                Rect::from_center_size(center, Vec2::splat(m.icon_sm)),
                m.icon_sm,
                ink,
            );
            inner.min.x += m.icon_sm + ICON_GAP;
        }
        let mut cleared = false;
        if self.clear && !self.text.as_str().is_empty() {
            let center = pos2(rect.right() - m.control_h / 2.0, rect.center().y);
            let slot = Rect::from_center_size(center, Vec2::splat(m.control_h - 8.0));
            inner.max.x = slot.left() - 2.0;
            let clear = ui.interact(slot, id.with("clear"), Sense::click());
            let ink = if clear.hovered() {
                t.text
            } else {
                t.text_faint
            };
            icon_pack::X.paint(ui, slot, m.icon_sm - 2.0, ink);
            if clear.on_hover_text("Clear").clicked() {
                self.text.clear();
                cleared = true;
            }
        }

        let font = if self.mono {
            fonts::mono(TextStyle::Monospace.resolve(ui.style()).size)
        } else {
            TextStyle::Body.resolve(ui.style())
        };
        let layout = Layout::left_to_right(Align::Center);
        let mut child = ui.new_child(UiBuilder::new().max_rect(inner).layout(layout));
        field_selection(&mut child, &t);
        let mut edit = TextEdit::singleline(self.text)
            .id(id)
            .hint_text(self.hint)
            .frame(Frame::NONE)
            .font(font)
            .text_color(t.text)
            .password(self.password)
            .cursor_at_end(self.cursor_at_end)
            .desired_width(inner.width())
            .min_size(inner.size())
            .vertical_align(Align::Center);
        if let Some(limit) = self.char_limit {
            edit = edit.char_limit(limit);
        }
        let mut output = edit.show(&mut child);
        if cleared {
            output.response.mark_changed();
            output.response.request_focus();
        }
        if frame.clicked() {
            output.response.request_focus();
        }
        output.response.response = output.response.response.union(frame);
        output
    }
}

/// Text field `width` wide at [`super::Metrics::control_h`].
pub fn text_field(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> Response {
    TextField::new(text).hint(hint).width(width).show(ui)
}

/// Text field with a leading magnifier and a clear button.
pub fn search_field(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> Response {
    TextField::new(text)
        .hint(hint)
        .width(width)
        .icon(icon_pack::MAGNIFYING_GLASS)
        .clear_button(true)
        .show(ui)
}

/// A painted box around a frameless multiline [`TextEdit`]; `&mut &str` is read-only.
#[must_use = "call `show` to draw the area"]
pub struct TextArea<'a> {
    text: &'a mut dyn TextBuffer,
    hint: &'a str,
    width: Option<f32>,
    rows: usize,
    mono: bool,
    invalid: bool,
    tab_indents: bool,
    char_limit: Option<usize>,
    id: Option<Id>,
    enabled: bool,
}

impl<'a> TextArea<'a> {
    pub fn new(text: &'a mut dyn TextBuffer) -> Self {
        Self {
            text,
            hint: "",
            width: None,
            rows: AREA_ROWS,
            mono: false,
            invalid: false,
            tab_indents: false,
            char_limit: None,
            id: None,
            enabled: true,
        }
    }

    /// Placeholder shown while empty.
    pub fn hint(mut self, hint: &'a str) -> Self {
        self.hint = hint;
        self
    }

    /// Outer width of the box; the available width by default.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Text rows the box shows before it grows.
    pub fn rows(mut self, rows: usize) -> Self {
        self.rows = rows.max(1);
        self
    }

    /// Monospace text.
    pub fn mono(mut self, mono: bool) -> Self {
        self.mono = mono;
        self
    }

    /// Draws the border in the error colour.
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// Tab inserts an indent instead of moving focus.
    pub fn tab_indents(mut self, tab_indents: bool) -> Self {
        self.tab_indents = tab_indents;
        self
    }

    /// Most characters the box accepts.
    pub fn char_limit(mut self, limit: usize) -> Self {
        self.char_limit = Some(limit);
        self
    }

    /// Stable id for the text and its focus.
    pub fn id_salt(mut self, salt: impl AsIdSalt) -> Self {
        self.id = Some(Id::new(salt));
        self
    }

    /// Draws the box disabled and ignores input when `false`.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The [`TextEdit`] response unioned with the box, so `rect` is the whole box.
    pub fn show(self, ui: &mut Ui) -> Response {
        if !self.enabled {
            return ui
                .add_enabled_ui(false, |ui| self.enabled(true).show(ui))
                .inner;
        }
        let (t, _) = kit(ui);
        let id = self.id.unwrap_or_else(|| ui.next_auto_id());
        ui.skip_ahead_auto_ids(1);
        let width = self.width.unwrap_or_else(|| ui.available_width());
        let inner_w = (width - 2.0 * FIELD_PAD_X).max(1.0);
        let focused = preview_focused(ui) || ui.memory(|mem| mem.has_focus(id));
        let font = if self.mono {
            fonts::mono(TextStyle::Monospace.resolve(ui.style()).size)
        } else {
            TextStyle::Body.resolve(ui.style())
        };

        let mut prepared = Frame::new()
            .fill(t.field)
            .stroke(Stroke::new(1.0, Color32::TRANSPARENT))
            .corner_radius(t.radius_sm)
            .inner_margin(Margin::symmetric(
                FIELD_PAD_X as i8 - 1,
                AREA_PAD_Y as i8 - 1,
            ))
            .begin(ui);
        let response = {
            let child = &mut prepared.content_ui;
            field_selection(child, &t);
            let mut edit = TextEdit::multiline(self.text)
                .id(id)
                .hint_text(self.hint)
                .frame(Frame::NONE)
                .margin(Margin::ZERO)
                .font(font)
                .text_color(t.text)
                .desired_width(inner_w)
                .desired_rows(self.rows)
                .lock_focus(self.tab_indents);
            if let Some(limit) = self.char_limit {
                edit = edit.char_limit(limit);
            }
            child.add(edit)
        };
        let content = prepared.content_ui.min_rect();
        let outer = content + prepared.frame.total_margin();
        let frame = ui.interact(outer, id.with("frame"), Sense::hover());
        let (state, _) = state_of(ui, &frame);
        let border = border_for(&t, state, focused, self.invalid);
        prepared.frame.stroke = Stroke::new(1.0, border);
        let allocated = prepared.end(ui);
        let margin_clicked =
            frame.hovered() && !response.hovered() && ui.input(|i| i.pointer.primary_clicked());
        if margin_clicked && ui.is_enabled() {
            response.request_focus();
        }
        response.union(allocated)
    }
}

/// A text area as wide as the row, `rows` lines tall.
pub fn text_area(ui: &mut Ui, text: &mut String, hint: &str, rows: usize) -> Response {
    TextArea::new(text).hint(hint).rows(rows).show(ui)
}

/// Numbers with no trailing zeros: `100`, `12.5`, `0.75`.
fn trim_number(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    };
    if text == "-0" { "0".to_owned() } else { text }
}

type Formatter<'a> = Box<dyn 'a + Fn(f64) -> String>;
type Parser<'a> = Box<dyn 'a + Fn(&str) -> Option<f64>>;

/// Numeric field with a dim unit suffix; drag to scrub, click to type.
#[must_use = "call `show` to draw the field"]
pub struct ValueField<'a, N: Numeric> {
    value: &'a mut N,
    range: RangeInclusive<N>,
    suffix: &'a str,
    width: f32,
    speed: Option<f64>,
    formatter: Option<Formatter<'a>>,
    parser: Option<Parser<'a>>,
}

impl<'a, N: Numeric> ValueField<'a, N> {
    pub fn new(value: &'a mut N, range: RangeInclusive<N>) -> Self {
        Self {
            value,
            range,
            suffix: "",
            width: VALUE_W,
            speed: None,
            formatter: None,
            parser: None,
        }
    }

    /// Dim unit text after the number.
    pub fn suffix(mut self, suffix: &'a str) -> Self {
        self.suffix = suffix;
        self
    }

    /// Outer width of the box.
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Value change per point dragged.
    pub fn speed(mut self, speed: f64) -> Self {
        self.speed = Some(speed);
        self
    }

    /// Text shown for a value, in place of the trimmed number.
    pub fn formatter(mut self, formatter: impl 'a + Fn(f64) -> String) -> Self {
        self.formatter = Some(Box::new(formatter));
        self
    }

    /// Reads typed text back into a value; pair with [`Self::formatter`].
    pub fn parser(mut self, parser: impl 'a + Fn(&str) -> Option<f64>) -> Self {
        self.parser = Some(Box::new(parser));
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let (t, m) = kit(ui);
        let size = vec2(self.width.max(m.control_h), m.control_h);
        let (rect, frame) = ui.allocate_exact_size(size, Sense::hover());
        let background = ui.painter().add(Shape::Noop);
        let suffix_galley =
            (!self.suffix.is_empty()).then(|| text_galley(ui, self.suffix, fonts::mono(SUFFIX_PT)));
        let suffix_w = suffix_galley
            .as_ref()
            .map_or(0.0, |g| g.size().x + SUFFIX_GAP);

        let digits = fonts::mono(DIGITS_PT);
        let row_h = text_galley(ui, "0", digits.clone()).size().y;
        let slot_h = (rect.height() - 4.0).max(row_h).min(rect.height() - 2.0);
        let cy = rect.center().y - DIGIT_LIFT;
        let slot = Rect::from_min_max(
            pos2(rect.left() + 4.0, cy - slot_h / 2.0),
            pos2(rect.right() - SUFFIX_INSET - suffix_w, cy + slot_h / 2.0),
        );

        let span = self.range.end().to_f64() - self.range.start().to_f64();
        let fine = !N::INTEGRAL && span <= 10.0;
        let decimals = if N::INTEGRAL {
            0
        } else if fine {
            2
        } else {
            1
        };

        let layout = Layout::right_to_left(Align::Center);
        let mut child = ui.new_child(UiBuilder::new().max_rect(slot).layout(layout));
        {
            let style = child.style_mut();
            let w = &mut style.visuals.widgets;
            for state in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
                state.bg_fill = Color32::TRANSPARENT;
                state.weak_bg_fill = Color32::TRANSPARENT;
                state.bg_stroke = Stroke::NONE;
                state.fg_stroke.color = t.text;
            }
            style.visuals.text_edit_bg_color = Some(Color32::TRANSPARENT);
            style.visuals.selection.bg_fill = selection_wash(&t);
            style.visuals.selection.stroke = Stroke::NONE;
            style.override_font_id = Some(digits);
            style.spacing.interact_size = slot.size();
            style.spacing.button_padding = Vec2::ZERO;
        }
        let speed = self.speed.unwrap_or(if fine { 0.01 } else { 0.5 });
        let mut drag = DragValue::new(self.value)
            .prefix(Atom::grow())
            .range(self.range)
            .speed(speed);
        drag = match self.formatter {
            Some(format) => drag.custom_formatter(move |v, _| format(v)),
            None => drag.custom_formatter(move |v, _| trim_number(v, decimals)),
        };
        if let Some(parse) = self.parser {
            drag = drag.custom_parser(move |s| parse(s));
        }
        let response = child.add(drag);

        if ui.is_rect_visible(rect) {
            let (state, _) = state_of(ui, &frame);
            let state = if response.hovered() && ui.is_enabled() {
                State::Hovered
            } else {
                state
            };
            let focused = preview_focused(ui) || response.has_focus();
            let border = field_border(&t, state, focused);
            let shape = RectShape::new(
                rect,
                CornerRadius::same(t.radius_sm),
                t.field,
                Stroke::new(1.0, border),
                StrokeKind::Inside,
            );
            ui.painter().set(background, shape);
            if let Some(g) = suffix_galley {
                let x = rect.right() - SUFFIX_INSET - g.size().x;
                let y = (rect.center().y - g.size().y / 2.0).round();
                ui.painter().galley(pos2(x, y), g, t.text_faint);
            }
        }
        response.union(frame)
    }
}

/// Numeric field with a dim unit suffix; drag to scrub, click to type.
pub fn value_field<N: Numeric>(
    ui: &mut Ui,
    value: &mut N,
    range: RangeInclusive<N>,
    suffix: &str,
    width: f32,
) -> Response {
    ValueField::new(value, range)
        .suffix(suffix)
        .width(width)
        .show(ui)
}

/// A dropdown showing the label of its value; its list opens below it.
#[must_use = "call `show` to draw the dropdown"]
pub struct Dropdown<'a, V> {
    salt: IdSalt,
    value: &'a mut V,
    options: &'a [(V, &'a str)],
    width: f32,
    placeholder: &'a str,
    enabled: bool,
}

impl<'a, V: PartialEq + Clone> Dropdown<'a, V> {
    pub fn new(id_salt: impl AsIdSalt, value: &'a mut V, options: &'a [(V, &'a str)]) -> Self {
        Self {
            salt: IdSalt::new(id_salt),
            value,
            options,
            width: DROPDOWN_W,
            placeholder: NO_OPTION,
            enabled: true,
        }
    }

    /// Outer width of the box.
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Faint text shown while the value is not among the options.
    pub fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// Draws the dropdown disabled and ignores input when `false`.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// `changed()` is set when a pick or an arrow key changes the value.
    pub fn show(self, ui: &mut Ui) -> Response {
        let id = ui.make_persistent_id(self.salt);
        if self.enabled {
            self.show_as(ui, id)
        } else {
            ui.add_enabled_ui(false, |ui| self.show_as(ui, id)).inner
        }
    }

    fn show_as(self, ui: &mut Ui, id: Id) -> Response {
        let Self {
            value,
            options,
            width,
            placeholder,
            ..
        } = self;
        let (t, m) = kit(ui);
        let size = vec2(width.max(m.control_h), m.control_h);
        let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
        let current = options.iter().position(|(v, _)| v == value);
        let label = current.map_or(placeholder, |i| options[i].1);
        response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, ui.is_enabled(), label));

        let popup_id = id.with("list");
        if response.has_focus() {
            let filter = EventFilter {
                vertical_arrows: true,
                ..Default::default()
            };
            ui.memory_mut(|mem| mem.set_focus_lock_filter(response.id, filter));
            let step = ui.input(|i| {
                i.key_pressed(Key::ArrowDown) as isize - i.key_pressed(Key::ArrowUp) as isize
            });
            if step != 0 && !options.is_empty() {
                let last = options.len() as isize - 1;
                let next = current.map_or(0, |i| (i as isize + step).clamp(0, last) as usize);
                if Some(next) != current {
                    *value = options[next].0.clone();
                    response.mark_changed();
                }
            }
            if ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Space)) {
                Popup::open_id(ui.ctx(), popup_id);
            }
        }

        let open = Popup::is_id_open(ui.ctx(), popup_id);
        if ui.is_rect_visible(rect) {
            let (state, focused) = state_of(ui, &response);
            let fill = match state {
                State::Rest => t.field,
                State::Hovered => t.hover,
                State::Pressed => t.pressed,
            };
            let border = if open {
                t.primary_border
            } else {
                t.field_border
            };
            paint_box(ui, rect, t.radius_sm, fill, border);

            let center = pos2(rect.right() - m.control_h / 2.0, rect.center().y);
            let chevron = Rect::from_center_size(center, Vec2::splat(m.icon_sm));
            let caret = if open {
                icon_pack::CARET_UP
            } else {
                icon_pack::CARET_DOWN
            };
            let caret_ink = if state == State::Rest {
                t.text_dim
            } else {
                t.text
            };
            caret.paint(ui, chevron, m.icon_sm - 2.0, caret_ink);

            let max_w = chevron.left() - rect.left() - FIELD_PAD_X - 2.0;
            let galley = elided(ui, label, TextStyle::Button.resolve(ui.style()), max_w);
            let y = (rect.center().y - galley.size().y / 2.0).round();
            let ink = if current.is_some() {
                t.text
            } else {
                t.text_faint
            };
            ui.painter()
                .galley(pos2(rect.left() + FIELD_PAD_X, y), galley, ink);
            if focused {
                focus_ring(ui, rect, t.radius_sm, &t);
            }
        }

        let mut chosen = None;
        Popup::menu(&response)
            .id(popup_id)
            .width(rect.width())
            .close_behavior(PopupCloseBehavior::CloseOnClick)
            .show(|ui| {
                ui.set_min_width(rect.width());
                ScrollArea::vertical()
                    .max_height(LIST_MAX_H)
                    .id_salt(popup_id.with("scroll"))
                    .show(ui, |ui| {
                        menu_rows(ui);
                        for (i, (_, text)) in options.iter().enumerate() {
                            if option_row(ui, text, Some(i) == current).clicked() {
                                chosen = Some(i);
                            }
                        }
                    });
            });
        if let Some(i) = chosen
            && Some(i) != current
        {
            *value = options[i].0.clone();
            response.mark_changed();
        }
        response
    }
}

/// A dropdown `width` wide; `changed()` is set on a pick.
pub fn dropdown<V: PartialEq + Clone>(
    ui: &mut Ui,
    id_salt: impl AsIdSalt,
    value: &mut V,
    options: &[(V, &str)],
    width: f32,
) -> Response {
    Dropdown::new(id_salt, value, options).width(width).show(ui)
}

/// One row of a dropdown list; the current option carries a check.
fn option_row(ui: &mut Ui, text: &str, selected: bool) -> Response {
    let (t, m) = kit(ui);
    let galley = text_galley(ui, text, TextStyle::Button.resolve(ui.style()));
    let min_w = galley.size().x + 2.0 * FIELD_PAD_X + m.icon_sm + ICON_GAP;
    let size = vec2(fill_width(ui).max(min_w), m.row_h);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        WidgetInfo::selected(WidgetType::SelectableLabel, ui.is_enabled(), selected, text)
    });
    if ui.is_rect_visible(rect) {
        let (state, _) = state_of(ui, &response);
        if state != State::Rest {
            ui.painter().rect_filled(rect, t.radius_sm, t.hover);
        }
        let ink = if selected || state != State::Rest {
            t.text
        } else {
            t.text_dim
        };
        let y = (rect.center().y - galley.size().y / 2.0).round();
        ui.painter()
            .galley(pos2(rect.left() + FIELD_PAD_X, y), galley, ink);
        if selected {
            let center = pos2(
                rect.right() - FIELD_PAD_X - m.icon_sm / 2.0,
                rect.center().y,
            );
            let slot = Rect::from_center_size(center, Vec2::splat(m.icon_sm));
            icon_pack::CHECK.paint(ui, slot, m.icon_sm, t.primary);
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::trim_number;

    #[test]
    fn numbers_drop_trailing_zeros() {
        assert_eq!(trim_number(100.0, 1), "100");
        assert_eq!(trim_number(12.5, 1), "12.5");
        assert_eq!(trim_number(0.75, 2), "0.75");
        assert_eq!(trim_number(2.0, 2), "2");
        assert_eq!(trim_number(-0.01, 1), "0");
        assert_eq!(trim_number(42.0, 0), "42");
    }
}
