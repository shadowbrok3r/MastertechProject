//! Fixed-height bars, the dialog shell and button row, empty states, menu rows and tab titles.

use std::borrow::Cow;
use std::sync::Arc;

use eframe::egui::{
    Align, Color32, Context, FontFamily, FontId, Frame, Galley, Id, Layout, Margin, Modal, Popup,
    PopupCloseBehavior, Rect, Response, Sense, Stroke, TextStyle, TextWrapMode, Ui, UiBuilder,
    UiKind, UiStackInfo, Vec2, WidgetInfo, WidgetText, WidgetType, Window,
    containers::menu::{self, MenuConfig},
    pos2,
    text::{LayoutJob, TextFormat},
    vec2,
};

use super::{
    ButtonKind, Icon, Metrics, SpinnerSize, State, TextButton, Tokens, elided, fill_width,
    focus_ring, kit, medium_font, mix, semibold_font_sized, spinner, state_of, text_galley, whole,
};
use crate::metrics::type_scale;
use crate::{fonts, icon_pack};

/// Smallest width a dialog button takes.
const DIALOG_BUTTON_MIN_W: f32 = 88.0;
/// Inner margin of a modal.
const MODAL_MARGIN: i8 = 16;
/// Narrowest a menu popup gets.
pub(super) const MENU_MIN_W: f32 = 200.0;
/// Icon size in an empty state.
const EMPTY_ICON: f32 = 40.0;
/// Widest an empty state's text runs.
const EMPTY_TEXT_W: f32 = 360.0;
/// Empty-state title size.
const EMPTY_TITLE_PT: f32 = 14.0;
/// Opacity of an empty-state icon's duotone back layer.
const EMPTY_DUO_ALPHA: f32 = 0.35;
/// Tab-title icon size.
const TAB_ICON_PT: f32 = 14.0;
/// Gap between a tab's icon and its title.
const TAB_ICON_GAP: f32 = 6.0;
/// Inset of a menu trigger's content from its left edge.
const TRIGGER_PAD: f32 = 8.0;
/// Inset of a menu trigger's caret from its right edge.
const TRIGGER_PAD_END: f32 = 6.0;
/// Gap between a menu trigger's icon and label.
const TRIGGER_ICON_GAP: f32 = 6.0;
/// Gap before a menu trigger's caret.
const TRIGGER_CARET_GAP: f32 = 4.0;
/// Share of the error hue in a hovered danger menu row.
const DANGER_HOVER: f32 = 0.18;
/// Label a picker shows when its value is not among the options.
const NO_OPTION: &str = "—";
/// Share of the tone in a callout's fill and border.
const CALLOUT_WASH: f32 = 0.12;
const CALLOUT_EDGE: f32 = 0.55;

/// What a [`bar`] holds, which sets its height and where its rule runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BarKind {
    /// Toolbar; rule below.
    Tool,
    /// Section or card header; rule below.
    Header,
    /// Status line; rule above.
    Status,
    /// Dialog or panel footer; rule above.
    Footer,
}

impl BarKind {
    pub fn height(self, m: &Metrics) -> f32 {
        match self {
            Self::Tool => m.bar_h,
            Self::Header | Self::Status => m.header_h,
            Self::Footer => m.footer_h,
        }
    }

    fn rule_above(self) -> bool {
        matches!(self, Self::Status | Self::Footer)
    }
}

/// A full-width bar of fixed height whose contents sit on one vertically centred row.
pub fn bar<R>(ui: &mut Ui, kind: BarKind, add: impl FnOnce(&mut Ui) -> R) -> R {
    let (t, m) = kit(ui);
    let height = kind.height(&m);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let y = if kind.rule_above() {
        rect.top()
    } else {
        rect.bottom() - 1.0
    };
    let y = ui.painter().round_to_pixel_center(y + 0.5);
    ui.painter().hline(
        rect.x_range(),
        y,
        eframe::egui::Stroke::new(1.0, t.separator),
    );
    let inner = rect.shrink2(vec2(m.pad.x, 0.0));
    let layout = Layout::left_to_right(Align::Center);
    let mut child = ui.new_child(UiBuilder::new().max_rect(inner).layout(layout));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child.spacing_mut().item_spacing.x = m.gap.x;
    child.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
    add(&mut child)
}

/// What a dialog button does, which sets its look and its place in the row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// The default answer, drawn primary and placed first.
    Primary,
    /// Another answer that acts.
    Secondary,
    /// A destructive default answer, placed first.
    Danger,
    /// Closes without acting, placed last.
    Cancel,
}

impl Role {
    fn slot(self) -> u8 {
        match self {
            Self::Primary | Self::Danger => 0,
            Self::Secondary => 1,
            Self::Cancel => 2,
        }
    }

    fn button(self, label: &str) -> TextButton<'_> {
        let kind = match self {
            Self::Primary => ButtonKind::Primary,
            Self::Danger => ButtonKind::Danger,
            Self::Secondary | Self::Cancel => ButtonKind::Neutral,
        };
        TextButton::new(label)
            .kind(kind)
            .prominent(true)
            .min_width(DIALOG_BUTTON_MIN_W)
    }
}

/// Indices of `buttons` in row order: default answers, other answers, then cancel.
pub(crate) fn dialog_order(buttons: &[(Role, &str)]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..buttons.len()).collect();
    order.sort_by_key(|&i| buttons[i].0.slot());
    order
}

/// Dialog buttons at the right of the current row, default first; returns the index clicked.
pub fn dialog_buttons(ui: &mut Ui, buttons: &[(Role, &str)]) -> Option<usize> {
    dialog_buttons_enabled(ui, buttons, &[])
}

/// [`dialog_buttons`] where `enabled[i]` gates button `i`; buttons past its end are enabled.
pub fn dialog_buttons_enabled(
    ui: &mut Ui,
    buttons: &[(Role, &str)],
    enabled: &[bool],
) -> Option<usize> {
    let (_, m) = kit(ui);
    let (order, galleys, size) = measure_row(ui, &m, buttons);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = m.gap.x;
            let mut hit = None;
            for (&i, galley) in order.iter().zip(galleys) {
                let (role, label) = buttons[i];
                let on = enabled.get(i).copied().unwrap_or(true);
                if role
                    .button(label)
                    .enabled(on)
                    .show_galley(ui, galley)
                    .clicked()
                {
                    hit = Some(i);
                }
            }
            hit
        })
        .inner
    })
    .inner
}

/// Row order, label galleys and total size of a row of dialog buttons.
fn measure_row(
    ui: &Ui,
    m: &Metrics,
    buttons: &[(Role, &str)],
) -> (Vec<usize>, Vec<Arc<Galley>>, Vec2) {
    let order = dialog_order(buttons);
    let galleys: Vec<Arc<Galley>> = order
        .iter()
        .map(|&i| buttons[i].0.button(buttons[i].1).label_galley(ui))
        .collect();
    let widths: f32 = order
        .iter()
        .zip(&galleys)
        .map(|(&i, g)| buttons[i].0.button(buttons[i].1).measure(m, g.size().x).x)
        .sum();
    let gaps = m.gap.x * order.len().saturating_sub(1) as f32;
    (order, galleys, vec2(widths + gaps, m.button_h))
}

/// Modal widths in points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ModalWidth {
    #[default]
    Narrow = 420,
    Regular = 600,
    Wide = 800,
}

impl ModalWidth {
    pub fn points(self) -> f32 {
        self as u16 as f32
    }
}

/// What a [`modal`] reported this frame.
#[derive(Clone, Debug)]
pub struct ModalOutcome<R> {
    pub inner: R,
    /// Index into the buttons of the one clicked.
    pub clicked: Option<usize>,
    /// Escape, a click outside, or a [`Role::Cancel`] button.
    pub should_close: bool,
}

/// A modal dialog: title, body, and a footer of dialog buttons.
pub fn modal<R>(
    ctx: &Context,
    id: Id,
    icon: impl Into<Option<Icon>>,
    title: &str,
    width: ModalWidth,
    buttons: &[(Role, &str)],
    body: impl FnOnce(&mut Ui) -> R,
) -> ModalOutcome<R> {
    let icon = icon.into();
    let frame = Frame::window(&ctx.global_style()).inner_margin(MODAL_MARGIN);
    let content_w = width.points() - 2.0 * MODAL_MARGIN as f32;
    let shown = Modal::new(id).frame(frame).show(ctx, |ui| {
        dialog_contents(ui, icon, title, content_w, buttons, body)
    });
    let dismissed = shown.should_close();
    let (inner, clicked) = shown.inner;
    let cancelled = clicked.is_some_and(|i| buttons[i].0 == Role::Cancel);
    ModalOutcome {
        inner,
        clicked,
        should_close: dismissed || cancelled,
    }
}

/// Title row, body and button footer `width` wide; returns the body result and button clicked.
pub fn dialog_contents<R>(
    ui: &mut Ui,
    icon: Option<Icon>,
    title: &str,
    width: f32,
    buttons: &[(Role, &str)],
    body: impl FnOnce(&mut Ui) -> R,
) -> (R, Option<usize>) {
    let (t, m) = kit(ui);
    ui.set_width(width);
    let (row, _) = ui.allocate_exact_size(vec2(width, m.header_h), Sense::hover());
    let mut x = row.left();
    if let Some(icon) = icon {
        let slot = Rect::from_min_size(
            pos2(x, row.center().y - m.icon_lg / 2.0),
            Vec2::splat(m.icon_lg),
        );
        icon.paint_duo(
            ui,
            slot,
            m.icon_lg,
            t.primary,
            t.primary.gamma_multiply(EMPTY_DUO_ALPHA),
        );
        x = slot.right() + m.gap.x;
    }
    let galley = elided(
        ui,
        title,
        semibold_font_sized(ui, type_scale::HEADING),
        row.right() - x,
    );
    let y = (row.center().y - galley.size().y / 2.0).round();
    ui.painter().galley(pos2(x, y), galley, t.text);
    super::hairline(ui);
    ui.add_space(m.gap.y);
    let inner = body(ui);
    ui.add_space(m.gap.y);
    let clicked = if buttons.is_empty() {
        None
    } else {
        super::bar(ui, BarKind::Footer, |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                dialog_buttons(ui, buttons)
            })
            .inner
        })
    };
    (inner, clicked)
}

/// A centred placeholder for an empty list or panel; returns the action clicked.
pub fn empty_state(
    ui: &mut Ui,
    icon: Icon,
    title: &str,
    hint: &str,
    actions: &[(Role, &str)],
) -> Option<usize> {
    let (t, m) = kit(ui);
    let width = ui.available_width().min(EMPTY_TEXT_W);
    ui.vertical_centered(|ui| {
        ui.add_space(2.0 * m.gap.y);
        let (slot, _) = ui.allocate_exact_size(Vec2::splat(EMPTY_ICON), Sense::hover());
        icon.paint_duo(
            ui,
            slot,
            EMPTY_ICON,
            t.text_faint,
            t.text_faint.gamma_multiply(EMPTY_DUO_ALPHA),
        );
        ui.add_space(m.gap.y);
        let title_font = super::medium_font_sized(ui, EMPTY_TITLE_PT);
        let title = elided(ui, title, title_font, width);
        let (rect, _) = ui.allocate_exact_size(title.size(), Sense::hover());
        ui.painter().galley(rect.min, title, t.text_dim);
        if !hint.is_empty() {
            ui.add_space(m.gap.y / 2.0);
            let font = TextStyle::Body.resolve(ui.style());
            let job = LayoutJob::simple(hint.to_owned(), font, Color32::PLACEHOLDER, width);
            let hint = ui.painter().layout_job(job);
            let (rect, _) = ui.allocate_exact_size(hint.size(), Sense::hover());
            ui.painter().galley(rect.min, hint, t.text_dim);
        }
        if actions.is_empty() {
            return None;
        }
        ui.add_space(2.0 * m.gap.y);
        let (order, galleys, row) = measure_row(ui, &m, actions);
        ui.allocate_ui_with_layout(row, Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = m.gap.x;
            let mut hit = None;
            for (&i, galley) in order.iter().zip(galleys) {
                let (role, label) = actions[i];
                if role.button(label).show_galley(ui, galley).clicked() {
                    hit = Some(i);
                }
            }
            hit
        })
        .inner
    })
    .inner
}

/// Menu-row spacing: rows touch and never wrap.
pub(crate) fn menu_rows(ui: &mut Ui) {
    let (_, m) = kit(ui);
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.spacing_mut().button_padding = vec2(m.pad.x, 2.0);
    ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
}

/// Runs `add` with menu-row spacing and a minimum menu width.
pub fn menu_body<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    menu_rows(ui);
    ui.set_min_width(MENU_MIN_W);
    add(ui)
}

/// A menu divider: a hairline with half a gap above and below.
pub fn menu_separator(ui: &mut Ui) {
    let (_, m) = kit(ui);
    ui.add_space(m.gap.y / 2.0);
    super::hairline(ui);
    ui.add_space(m.gap.y / 2.0);
}

/// A row-height menu line inset by the menu padding, holding `add`.
pub fn menu_inset_row<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let (_, m) = kit(ui);
    ui.horizontal(|ui| {
        ui.set_min_height(m.row_h);
        ui.add_space(m.pad.x);
        add(ui)
    })
    .inner
}

/// What sits at the end of a menu row.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Trail {
    None,
    Check(bool),
    Submenu { open: bool },
}

/// A full-width menu row: an optional icon, the label, and an optional check mark.
#[must_use = "call `show` or `submenu` to draw the row"]
pub struct MenuItem<'a> {
    label: &'a str,
    icon: Option<Icon>,
    checked: Option<bool>,
    danger: bool,
    tone: Option<Color32>,
    enabled: bool,
}

impl<'a> MenuItem<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            icon: None,
            checked: None,
            danger: false,
            tone: None,
            enabled: true,
        }
    }

    pub fn icon(mut self, icon: impl Into<Option<Icon>>) -> Self {
        self.icon = icon.into();
        self
    }

    /// Trailing check mark while `checked`.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    /// Error colour, for destructive actions.
    pub fn danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }

    /// Label and icon colour.
    pub fn tone(mut self, tone: Color32) -> Self {
        self.tone = Some(tone);
        self
    }

    /// Draws the row disabled and ignores clicks when `false`.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn ink(&self) -> Ink {
        match (self.danger, self.tone) {
            (true, _) => Ink::Danger,
            (false, Some(tone)) => Ink::Tone(tone),
            (false, None) => Ink::Normal,
        }
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        if !self.enabled {
            return ui
                .add_enabled_ui(false, |ui| self.enabled(true).show(ui))
                .inner;
        }
        let trail = self.checked.map_or(Trail::None, Trail::Check);
        menu_row(ui, self.icon, self.label, trail, self.ink())
    }

    /// Draws the row with a caret; its menu opens beside it and returns `add`'s result while open.
    pub fn submenu<R>(self, ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        let ink = self.ink();
        let closed = Trail::Submenu { open: false };
        if !self.enabled {
            ui.add_enabled_ui(false, |ui| menu_row(ui, self.icon, self.label, closed, ink));
            return None;
        }
        if !menu::is_in_menu(ui) {
            let response = menu_row(ui, self.icon, self.label, closed, ink);
            return popup_menu(&response, false, add);
        }
        let row_id = ui.next_auto_id();
        let open = menu::MenuState::from_ui(ui, |state, _| {
            state.open_item == Some(menu::SubMenu::id_from_widget_id(row_id))
        });
        let response = menu_row(ui, self.icon, self.label, Trail::Submenu { open }, ink);
        menu::SubMenu::new()
            .show(ui, &response, |ui| menu_body(ui, add))
            .map(|shown| shown.inner)
    }
}

/// Colour of a menu row's label and icon.
#[derive(Clone, Copy, PartialEq)]
enum Ink {
    Normal,
    Danger,
    Tone(Color32),
}

/// A full-width menu row with an optional leading icon.
pub fn menu_item(ui: &mut Ui, icon: impl Into<Option<Icon>>, label: &str) -> Response {
    MenuItem::new(label).icon(icon).show(ui)
}

/// A full-width menu row with a trailing check mark while `checked`.
pub fn menu_check_item(
    ui: &mut Ui,
    icon: impl Into<Option<Icon>>,
    label: &str,
    checked: bool,
) -> Response {
    MenuItem::new(label).icon(icon).checked(checked).show(ui)
}

/// A full-width menu row in the error colour, for destructive actions.
pub fn menu_danger_item(ui: &mut Ui, icon: impl Into<Option<Icon>>, label: &str) -> Response {
    MenuItem::new(label).icon(icon).danger(true).show(ui)
}

/// A menu row with a trailing caret whose menu opens beside it; returns `add`'s result while open.
pub fn submenu<R>(
    ui: &mut Ui,
    icon: impl Into<Option<Icon>>,
    label: &str,
    add: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    MenuItem::new(label).icon(icon).submenu(ui, add)
}

/// One menu row.
fn menu_row(ui: &mut Ui, icon: Option<Icon>, label: &str, trail: Trail, ink: Ink) -> Response {
    let (t, m) = kit(ui);
    let galley = text_galley(ui, label, TextStyle::Button.resolve(ui.style()));
    let lead = m.pad.x + m.icon_md + m.gap.x;
    let trail_w = match trail {
        Trail::None => m.pad.x,
        Trail::Check(_) | Trail::Submenu { .. } => m.gap.x + m.icon_sm + m.pad.x,
    };
    let min_w = lead + galley.size().x + trail_w;
    let size = vec2(fill_width(ui).max(min_w), m.row_h);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| match trail {
        Trail::Check(on) => WidgetInfo::selected(WidgetType::Checkbox, ui.is_enabled(), on, label),
        _ => WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label),
    });
    if ui.is_rect_visible(rect) {
        let (state, focused) = state_of(ui, &response);
        let hot = state != State::Rest || trail == Trail::Submenu { open: true };
        if hot {
            let fill = if ink == Ink::Danger {
                mix(t.hover, t.error, DANGER_HOVER)
            } else {
                t.hover
            };
            ui.painter().rect_filled(rect, t.radius_sm, fill);
        }
        let (ink, icon_ink) = match (ink, hot) {
            (Ink::Danger, _) => (t.error, t.error),
            (Ink::Tone(tone), _) => (tone, tone),
            (Ink::Normal, true) => (t.text, t.text),
            (Ink::Normal, false) => (t.text, t.icon),
        };
        if let Some(icon) = icon {
            let slot = Rect::from_min_size(
                pos2(rect.left() + m.pad.x, rect.center().y - m.icon_md / 2.0),
                Vec2::splat(m.icon_md),
            );
            icon.paint(ui, slot, m.icon_md, icon_ink);
        }
        let y = (rect.center().y - galley.size().y / 2.0).round();
        ui.painter()
            .galley(pos2(rect.left() + lead, y), galley, ink);
        let end = Rect::from_min_size(
            pos2(
                rect.right() - m.pad.x - m.icon_sm,
                rect.center().y - m.icon_sm / 2.0,
            ),
            Vec2::splat(m.icon_sm),
        );
        match trail {
            Trail::Check(true) => icon_pack::CHECK.paint(ui, end, m.icon_sm, t.primary),
            Trail::Submenu { .. } => {
                let caret_ink = if hot { t.text } else { t.text_dim };
                icon_pack::CARET_RIGHT.paint(ui, end, m.icon_sm - 2.0, caret_ink);
            }
            Trail::None | Trail::Check(false) => {}
        }
        if focused {
            focus_ring(ui, rect, t.radius_sm, &t);
        }
    }
    response
}

/// Menu trigger labelled with `value`'s option; any pick, even the current one, sets `changed()`.
pub fn menu_picker<V: PartialEq + Clone>(
    ui: &mut Ui,
    icon: impl Into<Option<Icon>>,
    tip: &str,
    value: &mut V,
    options: &[(V, &str)],
) -> Response {
    let label = options
        .iter()
        .find(|(v, _)| v == value)
        .map_or(NO_OPTION, |(_, text)| *text);
    let mut button = MenuButton::new(label).tip(tip);
    if let Some(icon) = icon.into() {
        button = button.icon(icon);
    }
    let mut picked = false;
    let (mut response, _) = button.show(ui, |ui| {
        for (option, text) in options {
            if menu_check_item(ui, None, text, option == value).clicked() {
                *value = option.clone();
                picked = true;
            }
        }
    });
    if picked {
        response.mark_changed();
    }
    response
}

/// Kit menu under `response` toggled by its clicks; `keep_open` closes only on outside clicks.
pub fn popup_menu<R>(
    response: &Response,
    keep_open: bool,
    add: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    let close = if keep_open {
        PopupCloseBehavior::CloseOnClickOutside
    } else {
        PopupCloseBehavior::CloseOnClick
    };
    let config = MenuConfig::new().close_behavior(close);
    Popup::menu(response)
        .id(menu_popup_id(response.id))
        .close_behavior(close)
        .style(config.style.clone())
        .info(UiStackInfo::new(UiKind::Menu).with_tag_value(MenuConfig::MENU_CONFIG_TAG, config))
        .show(|ui| menu_body(ui, add))
        .map(|shown| shown.inner)
}

/// Popup id of the menu [`popup_menu`] attaches to the widget `id`.
fn menu_popup_id(id: Id) -> Id {
    id.with("mtech.menu")
}

/// A transparent bar trigger with an optional icon, a label and a caret that opens a menu.
#[must_use = "call `show` to draw the button"]
pub struct MenuButton<'a> {
    label: &'a str,
    icon: Option<Icon>,
    tint: Option<Color32>,
    tip: &'a str,
    keep_open: bool,
}

impl<'a> MenuButton<'a> {
    /// An empty `label` draws the icon and caret only.
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            icon: None,
            tint: None,
            tip: "",
            keep_open: false,
        }
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Icon colour in place of the icon token.
    pub fn tint(mut self, tint: Color32) -> Self {
        self.tint = Some(tint);
        self
    }

    /// Tooltip while closed; the accessible name when the label is empty.
    pub fn tip(mut self, tip: &'a str) -> Self {
        self.tip = tip;
        self
    }

    /// Closes the menu only on a click outside it.
    pub fn keep_open(mut self, keep_open: bool) -> Self {
        self.keep_open = keep_open;
        self
    }

    /// Size the trigger takes at the current density.
    pub fn size(&self, ui: &Ui) -> Vec2 {
        let (_, m) = kit(ui);
        let text_w = self.galley(ui).map_or(0.0, |g| g.size().x);
        vec2(whole(self.content_w(&m, text_w)), m.control_h)
    }

    fn galley(&self, ui: &Ui) -> Option<std::sync::Arc<eframe::egui::Galley>> {
        (!self.label.is_empty())
            .then(|| text_galley(ui, self.label, medium_font(ui, TextStyle::Button)))
    }

    /// Padding, icon, label and caret widths for a label `text_w` wide.
    fn content_w(&self, m: &Metrics, text_w: f32) -> f32 {
        let icon_w = self.icon.map_or(0.0, |_| m.icon_md);
        let gap = if icon_w > 0.0 && text_w > 0.0 {
            TRIGGER_ICON_GAP
        } else {
            0.0
        };
        TRIGGER_PAD + icon_w + gap + text_w + TRIGGER_CARET_GAP + caret_size(m) + TRIGGER_PAD_END
    }

    /// Draws the trigger and its menu; returns the trigger and `add`'s result while open.
    pub fn show<R>(self, ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> (Response, Option<R>) {
        let (t, m) = kit(ui);
        let galley = self.galley(ui);
        let text_w = galley.as_ref().map_or(0.0, |g| g.size().x);
        let size = vec2(whole(self.content_w(&m, text_w)), m.control_h);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let name = if self.label.is_empty() {
            self.tip
        } else {
            self.label
        };
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), name));
        let inner = popup_menu(&response, self.keep_open, add);
        let open = Popup::is_id_open(ui.ctx(), menu_popup_id(response.id));
        if ui.is_rect_visible(rect) {
            let (state, focused) = state_of(ui, &response);
            let fill = match (open, state) {
                (true, _) | (_, State::Pressed) => t.pressed,
                (false, State::Hovered) => t.hover,
                (false, State::Rest) => Color32::TRANSPARENT,
            };
            ui.painter().rect_filled(rect, t.radius_sm, fill);
            let hot = open || state != State::Rest;
            let mut x = rect.left() + TRIGGER_PAD;
            if let Some(icon) = self.icon {
                let slot = Rect::from_min_size(
                    pos2(x, rect.center().y - m.icon_md / 2.0),
                    Vec2::splat(m.icon_md),
                );
                let ink = self.tint.unwrap_or(if hot { t.text } else { t.icon });
                icon.paint(ui, slot, m.icon_md, ink);
                x = slot.right() + if text_w > 0.0 { TRIGGER_ICON_GAP } else { 0.0 };
            }
            if let Some(galley) = galley {
                let y = (rect.center().y - galley.size().y / 2.0).round();
                ui.painter().galley(pos2(x, y), galley, t.text);
                x += text_w;
            }
            let caret = caret_size(&m);
            let slot = Rect::from_min_size(
                pos2(x + TRIGGER_CARET_GAP, rect.center().y - caret / 2.0),
                Vec2::splat(caret),
            );
            let caret_ink = if hot { t.text_dim } else { t.text_faint };
            icon_pack::CARET_DOWN.paint(ui, slot, caret, caret_ink);
            if focused {
                focus_ring(ui, rect, t.radius_sm, &t);
            }
        }
        let response = if self.tip.is_empty() || open {
            response
        } else {
            response.on_hover_text(self.tip)
        };
        (response, inner)
    }
}

/// Size of a menu trigger's caret.
fn caret_size(m: &Metrics) -> f32 {
    m.icon_sm - 2.0
}

/// A dock tab title: the icon's glyph, then the title in the medium face, both in the tab's colour.
pub fn tab_title(icon: Icon, title: &str) -> WidgetText {
    let mut job = LayoutJob::default();
    job.append(
        icon.glyph(),
        0.0,
        TextFormat {
            font_id: FontId::new(TAB_ICON_PT, FontFamily::Proportional),
            color: Color32::PLACEHOLDER,
            valign: Align::Center,
            ..Default::default()
        },
    );
    job.append(
        title,
        TAB_ICON_GAP,
        TextFormat {
            font_id: fonts::medium(type_scale::BODY),
            color: Color32::PLACEHOLDER,
            valign: Align::Center,
            ..Default::default()
        },
    );
    job.into()
}

/// The id `Window::new(title)` derives from a plain-text title.
pub fn window_id(title: &str) -> Id {
    Id::new(Some(Cow::Borrowed(title)))
}

/// A window titled with a primary SVG icon and `title`; its id matches `Window::new(title)`.
pub fn icon_window<'a>(ctx: &Context, icon: Icon, title: &'a str) -> Window<'a> {
    icon_window_tone(ctx, icon, title, Tokens::get(ctx).primary)
}

/// An [`icon_window`] whose icon is `tone`; not collapsible unless the caller opts in.
pub fn icon_window_tone<'a>(
    ctx: &Context,
    icon: Icon,
    title: &'a str,
    tone: Color32,
) -> Window<'a> {
    let m = Metrics::get(ctx);
    Window::new((icon.image(m.icon_md).tint(tone), title))
        .id(window_id(title))
        .collapsible(false)
}

/// A centred large spinner over a dim caption.
pub fn loading_state(ui: &mut Ui, caption: &str) -> Response {
    let (t, m) = kit(ui);
    let width = ui.available_width().min(EMPTY_TEXT_W);
    ui.vertical_centered(|ui| {
        ui.add_space(2.0 * m.gap.y);
        spinner(ui, SpinnerSize::Lg);
        if !caption.is_empty() {
            ui.add_space(m.gap.y);
            let galley = elided(ui, caption, TextStyle::Body.resolve(ui.style()), width);
            let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
            ui.painter().galley(rect.min, galley, t.text_dim);
        }
    })
    .response
}

/// A header bar with a primary icon, a semibold title and right-aligned `actions`.
pub fn panel_header<R>(
    ui: &mut Ui,
    icon: impl Into<Option<Icon>>,
    title: &str,
    actions: impl FnOnce(&mut Ui) -> R,
) -> R {
    let icon = icon.into();
    bar(ui, BarKind::Header, |ui| {
        let (t, m) = kit(ui);
        if let Some(icon) = icon {
            let (slot, _) = ui.allocate_exact_size(Vec2::splat(m.icon_md), Sense::hover());
            icon.paint(ui, slot, m.icon_md, t.primary);
        }
        let galley = text_galley(ui, title, semibold_font_sized(ui, type_scale::HEADING));
        let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
        ui.painter().galley(rect.min, galley, t.text);
        ui.with_layout(Layout::right_to_left(Align::Center), actions)
            .inner
    })
}

/// A panel washed and outlined in `tone`, with an optional leading icon in the tone.
pub fn callout<R>(
    ui: &mut Ui,
    tone: Color32,
    icon: impl Into<Option<Icon>>,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    let (t, m) = kit(ui);
    let icon = icon.into();
    Frame::new()
        .fill(tone.gamma_multiply(CALLOUT_WASH))
        .stroke(Stroke::new(1.0, tone.gamma_multiply(CALLOUT_EDGE)))
        .corner_radius(t.radius_md)
        .inner_margin(Margin::symmetric(m.gap.x as i8, m.gap.y as i8))
        .show(ui, |ui| match icon {
            None => add(ui),
            Some(icon) => {
                ui.horizontal_top(|ui| {
                    let line_h = ui.text_style_height(&TextStyle::Body).max(m.icon_md);
                    let (slot, _) = ui.allocate_exact_size(vec2(m.icon_md, line_h), Sense::hover());
                    icon.paint(ui, slot, m.icon_md, tone);
                    ui.vertical(add).inner
                })
                .inner
            }
        })
        .inner
}

/// A plain card frame: a lifted fill, the card border and medium corners; it never frosts.
pub fn card_frame(ui: &Ui) -> Frame {
    let (t, m) = kit(ui);
    let fill = if t.window == t.raised {
        mix(t.raised, t.hover, 0.5)
    } else {
        t.window
    };
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, t.card_border))
        .corner_radius(t.radius_md)
        .inner_margin(Margin::symmetric(m.pad.x as i8, m.gap.x as i8))
}

/// Runs `add` inside a [`card_frame`].
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    card_frame(ui).show(ui, add).inner
}
