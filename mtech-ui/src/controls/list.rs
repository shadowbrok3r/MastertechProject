//! Selectable list and tree rows with a leading icon.

use eframe::egui::{
    AsIdSalt, Color32, IdSalt, Rect, Response, Sense, TextStyle, Ui, Vec2, WidgetInfo, WidgetType,
    collapsing_header::CollapsingState, pos2, vec2,
};

use super::labels::paint_small_spinner;
use super::{
    Icon, IconBox, State, elided, focus_ring, kit, semibold_font_sized, state_of, text_galley,
};
use crate::icon_pack;

/// Size of a tree row's caret.
const CARET: f32 = 12.0;
/// Width of the caret column.
const CARET_W: f32 = 16.0;
/// Gap between a row's icon and label.
const ICON_GAP: f32 = 6.0;
/// Inset of a row's content from its left edge.
const PAD_X: f32 = 6.0;
/// Gap between the label and the trailing detail.
const DETAIL_GAP: f32 = 12.0;
/// Width of the unread-dot column.
const DOT_W: f32 = 12.0;
const DOT_RADIUS: f32 = 3.5;
/// Gap between the label and the subtitle.
const SUBTITLE_GAP: f32 = 1.0;
/// Height of the progress bar.
pub(super) const BAR_H: f32 = 3.0;
/// Gap between the text and the progress bar.
pub(super) const BAR_GAP: f32 = 3.0;

/// A selectable full-width row with optional caret, icon, detail, subtitle, unread dot, action and bar.
#[must_use = "call `show`, `show_with_action` or `show_node` to draw the row"]
pub struct ListItem<'a> {
    label: &'a str,
    icon: Option<Icon>,
    icon_tone: Option<Color32>,
    tone: Option<Color32>,
    selected: bool,
    detail: &'a str,
    width: Option<f32>,
    reserve_caret: bool,
    subtitle: &'a str,
    unread: bool,
    action: Option<(Icon, &'a str)>,
    progress: Option<f32>,
    spinner: Option<Option<Color32>>,
    trailing: Option<(Icon, Color32)>,
}

impl<'a> ListItem<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            icon: None,
            icon_tone: None,
            tone: None,
            selected: false,
            detail: "",
            width: None,
            reserve_caret: false,
            subtitle: "",
            unread: false,
            action: None,
            progress: None,
            spinner: None,
            trailing: None,
        }
    }

    pub fn icon(mut self, icon: impl Into<Option<Icon>>) -> Self {
        self.icon = icon.into();
        self
    }

    /// Icon colour in place of the icon token, or primary while selected.
    pub fn icon_tone(mut self, tone: Color32) -> Self {
        self.icon_tone = Some(tone);
        self
    }

    /// Label colour.
    pub fn tone(mut self, tone: Color32) -> Self {
        self.tone = Some(tone);
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Faint text at the right edge.
    pub fn detail(mut self, detail: &'a str) -> Self {
        self.detail = detail;
        self
    }

    /// Row width; the available width by default.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Leaves an empty caret column so a leaf lines up with sibling tree nodes.
    pub fn reserve_caret(mut self, reserve: bool) -> Self {
        self.reserve_caret = reserve;
        self
    }

    /// A small dim second line under the label; the row grows to hold it.
    pub fn subtitle(mut self, subtitle: &'a str) -> Self {
        self.subtitle = subtitle;
        self
    }

    /// Semibold label and a primary dot at the right edge.
    pub fn unread(mut self, unread: bool) -> Self {
        self.unread = unread;
        self
    }

    /// An icon button at the right edge, shown while the row is hovered or selected.
    pub fn action(mut self, icon: Icon, tip: &'a str) -> Self {
        self.action = Some((icon, tip));
        self
    }

    /// A thin bar under the text with `share`, 0.0 to 1.0, of it filled.
    pub fn progress(mut self, share: f32) -> Self {
        self.progress = Some(share.clamp(0.0, 1.0));
        self
    }

    /// An icon in `tone` at the right edge of the text.
    pub fn trailing(mut self, icon: Icon, tone: Color32) -> Self {
        self.trailing = Some((icon, tone));
        self
    }

    /// A spinner in `tone`, or primary, in place of the icon.
    pub fn spinner(mut self, tone: impl Into<Option<Color32>>) -> Self {
        self.spinner = Some(tone.into());
        self
    }

    /// A flat row.
    pub fn show(self, ui: &mut Ui) -> Response {
        self.show_with_action(ui).0
    }

    /// A flat row; the second response is the action button's while it shows.
    pub fn show_with_action(self, ui: &mut Ui) -> (Response, Option<Response>) {
        let caret = self.reserve_caret.then_some(None);
        let (row, _, action) = self.paint(ui, caret);
        (row, action)
    }

    /// A tree node whose open state persists under `id_salt`; `children` runs indented while open.
    pub fn show_node<R>(
        self,
        ui: &mut Ui,
        id_salt: impl AsIdSalt,
        default_open: bool,
        children: impl FnOnce(&mut Ui) -> R,
    ) -> (Response, Option<R>) {
        let id = ui.make_persistent_id(IdSalt::new(id_salt));
        let mut state = CollapsingState::load_with_default_open(ui.ctx(), id, default_open);
        let (row, caret, _) = self.paint(ui, Some(Some(state.is_open())));
        if let Some(caret) = caret
            && caret.clicked()
        {
            state.toggle(ui);
        }
        state.store(ui.ctx());
        let inner = state.is_open().then(|| ui.indent(id, children).inner);
        (row, inner)
    }

    /// Draws the row; `caret` is `None` for no column, `Some(None)` for an empty one.
    fn paint(
        self,
        ui: &mut Ui,
        caret: Option<Option<bool>>,
    ) -> (Response, Option<Response>, Option<Response>) {
        let (t, m) = kit(ui);
        let width = self.width.unwrap_or_else(|| ui.available_width());
        let lead_w = if self.icon.is_some() || self.spinner.is_some() {
            m.icon_md + ICON_GAP
        } else {
            0.0
        };
        let text_left = PAD_X + caret.map_or(0.0, |_| CARET_W) + lead_w;
        let action_side = IconBox::Sm.side(&m);
        let action_w = self.action.map_or(0.0, |_| action_side + ICON_GAP);
        let dot_w = if self.unread { DOT_W } else { 0.0 };
        let trailing_w = self.trailing.map_or(0.0, |_| m.icon_sm + ICON_GAP);
        let text_right = width - PAD_X - dot_w - action_w - trailing_w;

        let body = TextStyle::Body.resolve(ui.style());
        let font = if self.unread {
            semibold_font_sized(ui, body.size)
        } else {
            body
        };
        let detail = (!self.detail.is_empty())
            .then(|| text_galley(ui, self.detail, TextStyle::Small.resolve(ui.style())))
            .filter(|g| text_right - g.size().x > text_left + DETAIL_GAP);
        let label_right = detail
            .as_ref()
            .map_or(text_right, |g| text_right - g.size().x - DETAIL_GAP);
        let label = elided(ui, self.label, font, label_right - text_left);
        let subtitle = (!self.subtitle.is_empty()).then(|| {
            let small = TextStyle::Small.resolve(ui.style());
            elided(ui, self.subtitle, small, text_right - text_left)
        });
        let line_h = label.size().y;
        let text_h = line_h + subtitle.as_ref().map_or(0.0, |g| SUBTITLE_GAP + g.size().y);
        let bar_h = self.progress.map_or(0.0, |_| BAR_GAP + BAR_H);
        let height = if subtitle.is_none() && self.progress.is_none() {
            m.row_h
        } else {
            (text_h + bar_h + 2.0 * m.pad.y).max(m.row_h).round()
        };

        let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
        response.widget_info(|| {
            WidgetInfo::selected(
                WidgetType::SelectableLabel,
                ui.is_enabled(),
                self.selected,
                self.label,
            )
        });
        let top = (rect.center().y - (text_h + bar_h) / 2.0).round();
        let line = Rect::from_min_size(pos2(rect.left(), top), vec2(width, line_h));
        let caret_slot = caret.map(|open| {
            let slot =
                Rect::from_min_size(pos2(rect.left() + PAD_X, line.top()), vec2(CARET_W, line_h));
            (slot, open)
        });
        let caret_response = caret_slot.and_then(|(slot, open)| {
            open.map(|_| ui.interact(slot, response.id.with("caret"), Sense::click()))
        });
        let action = self
            .action
            .filter(|_| self.selected || response.contains_pointer())
            .map(|(icon, tip)| {
                let center = pos2(
                    rect.right() - PAD_X - dot_w - action_side / 2.0,
                    rect.center().y,
                );
                let spot = Rect::from_center_size(center, Vec2::splat(action_side));
                let hit = ui
                    .interact(spot, response.id.with("action"), Sense::click())
                    .on_hover_text(tip);
                (icon, hit)
            });
        if !ui.is_rect_visible(rect) {
            return (response, caret_response, action.map(|(_, hit)| hit));
        }

        let (state, focused) = state_of(ui, &response);
        let action_hot = action.as_ref().is_some_and(|(_, hit)| hit.hovered());
        let fill = match (self.selected, state, action_hot) {
            (true, _, _) => t.primary_soft,
            (false, _, true) | (false, State::Hovered, _) => t.hover,
            (false, State::Pressed, _) => t.pressed,
            (false, State::Rest, false) => Color32::TRANSPARENT,
        };
        ui.painter().rect_filled(rect, t.radius_sm, fill);
        if let (Some((slot, Some(open))), Some(hit)) = (caret_slot, &caret_response) {
            let glyph = if open {
                icon_pack::CARET_DOWN
            } else {
                icon_pack::CARET_RIGHT
            };
            let ink = if hit.hovered() { t.text } else { t.text_dim };
            glyph.paint(ui, slot, CARET - 1.0, ink);
        }
        let lead = Rect::from_center_size(
            pos2(
                rect.left() + text_left - ICON_GAP - m.icon_md / 2.0,
                line.center().y,
            ),
            Vec2::splat(m.icon_md),
        );
        match (self.spinner, self.icon) {
            (Some(tone), _) => paint_small_spinner(ui, lead, tone),
            (None, Some(icon)) => {
                let tint = self
                    .icon_tone
                    .unwrap_or(if self.selected { t.primary } else { t.icon });
                icon.paint(ui, lead, m.icon_md, tint);
            }
            (None, None) => {}
        }
        let x = rect.left() + text_left;
        let center_y = |h: f32| (line.center().y - h / 2.0).round();
        if let Some(detail) = detail {
            let dx = rect.left() + text_right - detail.size().x;
            ui.painter()
                .galley(pos2(dx, center_y(detail.size().y)), detail, t.text_faint);
        }
        let ink = self.tone.unwrap_or(t.text);
        ui.painter().galley(pos2(x, center_y(line_h)), label, ink);
        if let Some(subtitle) = subtitle {
            let y = line.bottom() + SUBTITLE_GAP;
            ui.painter().galley(pos2(x, y), subtitle, t.text_dim);
        }
        if let Some(share) = self.progress {
            let track = Rect::from_min_size(
                pos2(x, top + text_h + BAR_GAP),
                vec2(text_right - text_left, BAR_H),
            );
            let radius = BAR_H / 2.0;
            ui.painter().rect_filled(track, radius, t.field_border);
            if share > 0.0 {
                let tone = if share >= 1.0 { t.success } else { t.primary };
                let done = Rect::from_min_size(track.min, vec2(track.width() * share, BAR_H));
                ui.painter().rect_filled(done, radius, tone);
            }
        }
        if let Some((icon, tone)) = self.trailing {
            let center = pos2(
                rect.left() + text_right + ICON_GAP + m.icon_sm / 2.0,
                line.center().y,
            );
            icon.paint(
                ui,
                Rect::from_center_size(center, Vec2::splat(m.icon_sm)),
                m.icon_sm,
                tone,
            );
        }
        if self.unread {
            let center = pos2(rect.right() - PAD_X - DOT_W / 2.0, line.center().y);
            ui.painter().circle_filled(center, DOT_RADIUS, t.primary);
        }
        if let Some((icon, hit)) = &action {
            let ink = if hit.hovered() {
                ui.painter().rect_filled(hit.rect, t.radius_sm, t.hover);
                t.text
            } else {
                t.text_dim
            };
            icon.paint(ui, hit.rect, m.icon_sm, ink);
        }
        if focused {
            focus_ring(ui, rect, t.radius_sm, &t);
        }
        (response, caret_response, action.map(|(_, hit)| hit))
    }
}
