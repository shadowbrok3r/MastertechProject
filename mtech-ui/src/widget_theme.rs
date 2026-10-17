//! Kit colours and geometry for stock egui widgets through egui's experimental theme system.

use std::sync::Arc;

use eframe::egui::{
    Button, Color32, ComboBox, Context, CornerRadius, Frame, Id, Margin, Stroke, Style, TextEdit,
    TextStyle, Ui, Vec2, Visuals,
    class::{ClassName, Classes, HasClasses as _},
    plugin::Plugin,
    style::WidgetVisuals,
    theme::{DefaultStyle, StyleProvider},
    widget_style::{
        ButtonStyle, CheckboxStyle, SeparatorStyle, StyleArgs, TextEditStyle, TextVisuals,
        WidgetState,
    },
};

use crate::controls::{
    ButtonKind, CHECK_INSET, LABEL_GAP, Metrics, State, Tokens, box_radius, box_stroke,
    button_colors, checkbox_colors, dropdown_colors, field_border, medium_font_in, selected_colors,
};

/// A framed primary kit button, or the kit's selected look while `override_text_color` is set.
pub const PRIMARY: ClassName = ClassName::from_static("mt-primary");
/// A framed danger kit button, whatever the local visuals.
pub const DANGER: ClassName = ClassName::from_static("mt-danger");
/// A framed ghost kit button, whatever the local visuals.
pub const GHOST: ClassName = ClassName::from_static("mt-ghost");
/// A framed button at [`Metrics::button_h`].
pub const PROMINENT: ClassName = ClassName::from_static("mt-prominent");
/// egui's default style for the widget or a Ui's widgets; not popups, menus or tooltips they open.
pub const STOCK: ClassName = ClassName::from_static("mt-stock");

/// Width of the stock check mark's stroke.
const TICK_W: f32 = 2.0;
/// Inset of the stock check mark's polyline inside its box.
const TICK_INSET: f32 = CHECK_INSET + 1.0;

/// Styles stock buttons, checkboxes, text edits and separators with the kit's tokens and metrics.
#[derive(Clone, Copy, Debug, Default)]
pub struct KitTheme;

/// Registers [`KitTheme`] for every built-in widget style and records each pass's starting style, once.
pub fn install(ctx: &Context) {
    ctx.add_widget_theme::<ButtonStyle>(KitTheme);
    ctx.add_widget_theme::<CheckboxStyle>(KitTheme);
    ctx.add_widget_theme::<TextEditStyle>(KitTheme);
    ctx.add_widget_theme::<SeparatorStyle>(KitTheme);
    ctx.add_plugin(PassStart);
}

fn enabled_id() -> Id {
    Id::unique("mtech.widget_theme.enabled")
}

/// Whether stock widgets take the kit look; on unless [`set_enabled`] turned it off.
pub fn enabled(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(enabled_id()))
        .unwrap_or(true)
}

/// Turns the kit look for stock widgets on or off until the context's memory is reset.
pub fn set_enabled(ctx: &Context, enabled: bool) {
    ctx.data_mut(|d| d.insert_temp(enabled_id(), enabled));
    ctx.request_repaint();
}

/// Records the global style each pass starts with.
struct PassStart;

/// The global style pass number `pass` started with.
#[derive(Clone)]
struct PassStyle {
    pass: u64,
    style: Arc<Style>,
}

impl Plugin for PassStart {
    fn debug_name(&self) -> &'static str {
        "mtech_ui::widget_theme::PassStart"
    }

    fn on_begin_pass(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx();
        let start = PassStyle {
            pass: ctx.cumulative_pass_nr(),
            style: Arc::clone(ui.style()),
        };
        let id = pass_style_id(ctx);
        ctx.data_mut(|d| d.insert_temp(id, start));
    }
}

fn pass_style_id(ctx: &Context) -> Id {
    Id::unique(("mtech.widget_theme.pass_start", ctx.viewport_id()))
}

/// The global style the current pass started with, when recorded.
fn pass_start(ctx: &Context) -> Option<Arc<Style>> {
    let id = pass_style_id(ctx);
    let start = ctx.data(|d| d.get_temp::<PassStyle>(id))?;
    (start.pass == ctx.cumulative_pass_nr()).then_some(start.style)
}

/// Whether the switch is on and no [`STOCK`] class applies.
fn active(args: &StyleArgs<'_>) -> bool {
    !args.has_class(&STOCK) && enabled(args.ctx)
}

/// Whether `pick` matches between the widget's style and the global style, now or at pass start.
fn unmodified<T: PartialEq>(args: &StyleArgs<'_>, pick: impl Fn(&Visuals) -> T) -> bool {
    let styles = [Some(args.ctx.global_style()), pass_start(args.ctx)];
    let mut styles = styles.iter().flatten();
    if styles.clone().any(|s| std::ptr::eq(args.style, &**s)) {
        return true;
    }
    let local = pick(&args.style.visuals);
    styles.any(|s| pick(&s.visuals) == local)
}

/// Every interactive state's visuals, plus the noninteractive ones for a noninteractive widget.
fn state_visuals(v: &Visuals, state: WidgetState) -> ([WidgetVisuals; 4], Option<WidgetVisuals>) {
    let w = &v.widgets;
    let noninteractive = (state == WidgetState::Noninteractive).then_some(w.noninteractive);
    ([w.inactive, w.hovered, w.active, w.open], noninteractive)
}

fn kit_state(state: WidgetState) -> State {
    match state {
        WidgetState::Hovered => State::Hovered,
        WidgetState::Active => State::Pressed,
        WidgetState::Inactive | WidgetState::Noninteractive => State::Rest,
    }
}

/// The kind a class names.
fn kind_of(class: &ClassName) -> Option<ButtonKind> {
    [ButtonKind::Primary, ButtonKind::Danger, ButtonKind::Ghost]
        .into_iter()
        .find(|kind| kind.class().as_ref() == Some(class))
}

/// The last kind class on the widget, else the one on its nearest ancestor.
fn class_kind(args: &StyleArgs<'_>) -> Option<ButtonKind> {
    let last = |classes: &Classes| classes.iter().rev().find_map(kind_of);
    last(args.classes).or_else(|| args.stack.iter().find_map(|node| last(&node.classes)))
}

/// Whether the button paints a frame: as [`DefaultStyle`] decides, or by default with a kit class.
fn has_frame(args: &StyleArgs<'_>, kit_class: bool) -> bool {
    let classes = args.classes;
    let by_default = kit_class || args.style.visuals.button_frame;
    classes.has_class(&Button::CLASS_FRAME)
        || (!classes.has_class(&Button::CLASS_NO_FRAME) && by_default)
}

impl StyleProvider<ButtonStyle> for KitTheme {
    fn style(&mut self, args: &StyleArgs<'_>) -> ButtonStyle {
        let mut out = DefaultStyle.style(args);
        if !active(args) {
            return out;
        }
        let kind = class_kind(args);
        let prominent = args.has_class(&PROMINENT);
        if !has_frame(args, kind.is_some() || prominent) {
            return out;
        }
        let StyleArgs {
            classes,
            state,
            style,
            ctx,
            ..
        } = *args;
        let small = classes.has_class(&Button::CLASS_SMALL);
        let layout = &mut out.atom_layout;
        if !small {
            let height = if prominent {
                Metrics::get(ctx).button_h
            } else {
                style.spacing.interact_size.y
            };
            layout.min_size = Vec2::new(0.0, height);
        }
        let open = classes.has_class(&ComboBox::CLASS_OPEN);
        let selected = classes.has_class(&Button::CLASS_SELECTED);
        let kit_colours = kind.is_some()
            || unmodified(args, |v| {
                (state_visuals(v, state), selected.then_some(v.selection))
            });
        if !kit_colours {
            return out;
        }

        let t = Tokens::get(ctx);
        let combo = open || classes.has_class(&ComboBox::CLASS);
        let forced_ink = style.visuals.override_text_color;
        let (fill, edge, ink) = match kind {
            _ if selected => selected_colors(&t),
            None if combo => {
                let (fill, edge) = dropdown_colors(&t, kit_state(state), open);
                (fill, edge, t.text)
            }
            Some(ButtonKind::Primary) if forced_ink.is_some() => selected_colors(&t),
            kind => button_colors(&t, kind.unwrap_or_default(), kit_state(state)),
        };
        let ink = forced_ink.unwrap_or(ink);
        let mut inner_margin = Margin::from(style.spacing.button_padding);
        if small {
            inner_margin.top = 0;
            inner_margin.bottom = 0;
        }
        let frame = Frame::new()
            .fill(fill)
            .corner_radius(t.radius_sm)
            .inner_margin(inner_margin)
            .apply_stroke_and_expansion_without_layout_shift(box_stroke(edge), 0.0);
        let hidden = classes.has_class(&Button::CLASS_HIDE_FRAME_WHEN_INACTIVE)
            && state == WidgetState::Inactive;
        layout.frame = if hidden { frame.invisible() } else { frame };
        let font = if combo && kind.is_none() {
            TextStyle::Button.resolve(style)
        } else {
            medium_font_in(ctx, style, TextStyle::Button)
        };
        layout.text_style = TextVisuals::new(style, font, ink);
        layout.image_tint = if classes.has_class(&Button::CLASS_IMAGE_TINT_FOLLOWS_TEXT_COLOR) {
            ink
        } else {
            Color32::WHITE
        };
        out
    }
}

impl StyleProvider<TextEditStyle> for KitTheme {
    fn style(&mut self, args: &StyleArgs<'_>) -> TextEditStyle {
        let mut out = DefaultStyle.style(args);
        let state = args.state;
        let read = |v: &Visuals| {
            (
                state_visuals(v, state),
                v.selection.stroke,
                v.text_edit_bg_color(),
            )
        };
        if !active(args) || !unmodified(args, read) {
            return out;
        }
        let style = args.style;
        let t = Tokens::get(args.ctx);
        let (fill, edge) = if args.classes.has_class(&TextEdit::CLASS_READ_ONLY) {
            (Color32::TRANSPARENT, field_border(&t, State::Rest, false))
        } else {
            let focused = state == WidgetState::Active;
            (t.field, field_border(&t, kit_state(state), focused))
        };
        let ink = style.visuals.override_text_color.unwrap_or(t.text);
        let layout = &mut out.atom_layout;
        layout.frame = Frame::new()
            .fill(fill)
            .corner_radius(t.radius_sm)
            .inner_margin(Margin::from(style.spacing.button_padding))
            .apply_stroke_and_expansion_without_layout_shift(box_stroke(edge), 0.0);
        layout.text_style = TextVisuals::new(style, TextStyle::Body, ink);
        out.prefix_suffix_color = ink;
        out
    }
}

impl StyleProvider<CheckboxStyle> for KitTheme {
    fn style(&mut self, args: &StyleArgs<'_>) -> CheckboxStyle {
        let mut out = DefaultStyle.style(args);
        if !active(args) {
            return out;
        }
        let (style, state) = (args.style, args.state);
        let side = Metrics::get(args.ctx).icon_md;
        out.checkbox_size = side;
        out.check_size = side - 2.0 * TICK_INSET;
        out.atom_layout.gap = LABEL_GAP;
        out.atom_layout.min_size = Vec2::new(0.0, style.spacing.interact_size.y);
        if !unmodified(args, |v| state_visuals(v, state)) {
            return out;
        }
        let t = Tokens::get(args.ctx);
        let (fill, edge) = checkbox_colors(&t, false, kit_state(state));
        out.checkbox_frame = Frame::new()
            .fill(fill)
            .corner_radius(CornerRadius::same(box_radius(&t)))
            .stroke(box_stroke(edge));
        out.check_stroke = Stroke::new(TICK_W, t.primary);
        let ink = style.visuals.override_text_color.unwrap_or(t.text);
        out.atom_layout.text_style = TextVisuals::new(style, TextStyle::Body, ink);
        out
    }
}

impl StyleProvider<SeparatorStyle> for KitTheme {
    fn style(&mut self, args: &StyleArgs<'_>) -> SeparatorStyle {
        let mut out = DefaultStyle.style(args);
        if !active(args) {
            return out;
        }
        out.spacing = Metrics::get(args.ctx).gap.y;
        if unmodified(args, |v| v.widgets.noninteractive.bg_stroke) {
            out.stroke = Stroke::new(1.0, Tokens::get(args.ctx).separator);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;

    use eframe::egui::{
        CentralPanel, CompletionPopup, DragValue, FontDefinitions, Pos2, RawInput, Rect, Shape, Ui,
        UiBuilder, containers::menu, epaint::ClippedShape, vec2,
    };

    use super::*;
    use crate::controls::{self, TextButton, box_stroke};
    use crate::metrics::{self, Density};

    const KINDS: [ButtonKind; 4] = [
        ButtonKind::Neutral,
        ButtonKind::Primary,
        ButtonKind::Danger,
        ButtonKind::Ghost,
    ];

    const STATES: [(WidgetState, State); 4] = [
        (WidgetState::Noninteractive, State::Rest),
        (WidgetState::Inactive, State::Rest),
        (WidgetState::Hovered, State::Hovered),
        (WidgetState::Active, State::Pressed),
    ];

    fn context_with(density: Density, style: Style) -> Context {
        let ctx = Context::default();
        let mut fonts = FontDefinitions::default();
        crate::icons::install_fonts(&mut fonts);
        crate::fonts::install(&mut fonts);
        ctx.set_fonts(fonts);
        metrics::set_density(&ctx, density);
        metrics::install_overlaid_style(&ctx, style);
        ctx
    }

    fn context_at(density: Density) -> Context {
        context_with(density, Style::default())
    }

    fn context() -> Context {
        context_at(Density::Regular)
    }

    /// Runs `f` in a central panel for `passes` frames; returns its last result and shapes.
    fn run<R>(
        ctx: &Context,
        passes: usize,
        mut f: impl FnMut(&mut Ui) -> R,
    ) -> (R, Vec<ClippedShape>) {
        let mut out = None;
        let mut shapes = Vec::new();
        for _ in 0..passes {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 600.0))),
                ..Default::default()
            };
            let mut full = ctx.run_ui(input, |ui| {
                CentralPanel::default().show(ui, |ui| out = Some(f(ui)));
            });
            full.textures_delta.clear();
            shapes = std::mem::take(&mut full.shapes);
        }
        (out.expect("the ui closure ran"), shapes)
    }

    fn classes(list: &[ClassName]) -> Classes {
        list.iter()
            .fold(Classes::default(), |c, class| c.with_class(class))
    }

    fn kind_classes(kind: ButtonKind) -> Classes {
        classes(kind.class().as_slice())
    }

    fn args<'a>(ui: &'a Ui, classes: &'a Classes, state: WidgetState) -> StyleArgs<'a> {
        StyleArgs {
            classes,
            state,
            stack: ui.stack(),
            style: ui.style(),
            ctx: ui.ctx(),
        }
    }

    fn kit<S>(ui: &Ui, classes: &Classes, state: WidgetState) -> S
    where
        KitTheme: StyleProvider<S>,
    {
        KitTheme.style(&args(ui, classes, state))
    }

    fn stock<S>(ui: &Ui, classes: &Classes, state: WidgetState) -> S
    where
        DefaultStyle: StyleProvider<S>,
    {
        DefaultStyle.style(&args(ui, classes, state))
    }

    fn debug<T: Debug>(value: &T) -> String {
        format!("{value:?}")
    }

    /// Debug text of all four styles the kit theme computes for `classes` in `state`.
    fn kit_all(ui: &Ui, classes: &Classes, state: WidgetState) -> [String; 4] {
        [
            debug(&kit::<ButtonStyle>(ui, classes, state)),
            debug(&kit::<TextEditStyle>(ui, classes, state)),
            debug(&kit::<CheckboxStyle>(ui, classes, state)),
            debug(&kit::<SeparatorStyle>(ui, classes, state)),
        ]
    }

    /// Debug text of all four styles egui's default theme computes for `classes` in `state`.
    fn stock_all(ui: &Ui, classes: &Classes, state: WidgetState) -> [String; 4] {
        [
            debug(&stock::<ButtonStyle>(ui, classes, state)),
            debug(&stock::<TextEditStyle>(ui, classes, state)),
            debug(&stock::<CheckboxStyle>(ui, classes, state)),
            debug(&stock::<SeparatorStyle>(ui, classes, state)),
        ]
    }

    /// Debug text of a checkbox style's colours and text, without its geometry.
    fn checkbox_paint(style: &CheckboxStyle) -> String {
        debug(&(
            &style.checkbox_frame,
            style.check_stroke,
            &style.atom_layout.text_style,
        ))
    }

    /// Fill, stroke and text colour of a button style.
    fn look(style: &ButtonStyle) -> (Color32, Stroke, Color32) {
        let layout = &style.atom_layout;
        (
            layout.frame.fill,
            layout.frame.stroke,
            layout.text_style.color,
        )
    }

    fn kit_look(fill: Color32, edge: Color32, ink: Color32) -> (Color32, Stroke, Color32) {
        (fill, box_stroke(edge), ink)
    }

    #[test]
    fn every_kind_and_state_paints_the_kit_button_colours() {
        let ctx = context();
        let ((t, styles), _) = run(&ctx, 1, |ui| {
            let mut styles = Vec::new();
            for kind in KINDS {
                for (widget, state) in STATES {
                    let style = kit::<ButtonStyle>(ui, &kind_classes(kind), widget);
                    styles.push((kind, state, style));
                }
            }
            (Tokens::get(ui.ctx()), styles)
        });
        let margin = styles[0].2.atom_layout.frame.total_margin();
        for (kind, state, style) in &styles {
            let (fill, edge, ink) = controls::button_colors(&t, *kind, *state);
            assert_eq!(look(style), kit_look(fill, edge, ink), "{kind:?} {state:?}");
            let frame = &style.atom_layout.frame;
            assert_eq!(frame.corner_radius, CornerRadius::same(t.radius_sm));
            assert_eq!(frame.total_margin(), margin, "{kind:?} {state:?} moves");
        }
    }

    #[test]
    fn a_kind_class_round_trips_and_neutral_has_none() {
        assert_eq!(ButtonKind::Neutral.class(), None);
        for kind in KINDS.into_iter().skip(1) {
            let class = kind.class().expect("a coloured kind has a class");
            assert_eq!(kind_of(&class), Some(kind));
        }
        assert_eq!(kind_of(&STOCK), None);
        assert_eq!(kind_of(&PROMINENT), None);
    }

    #[test]
    fn the_stock_class_and_the_switch_give_the_default_style() {
        let ctx = context();
        let sets = [
            classes(&[]),
            classes(&[PRIMARY, PROMINENT]),
            classes(&[Button::CLASS_SMALL]),
            classes(&[Button::CLASS_SELECTED]),
            classes(&[ComboBox::CLASS, ComboBox::CLASS_OPEN]),
            classes(&[TextEdit::CLASS_READ_ONLY]),
        ];
        let (pairs, _) = run(&ctx, 1, |ui| {
            let mut pairs = Vec::new();
            for set in &sets {
                for (state, _) in STATES {
                    let own = set.clone().with_class(STOCK);
                    pairs.push((
                        format!("own {set} {state:?}"),
                        kit_all(ui, &own, state),
                        stock_all(ui, &own, state),
                    ));
                    let scoped = ui
                        .scope_builder(UiBuilder::new().with_class(STOCK), |ui| {
                            (kit_all(ui, set, state), stock_all(ui, set, state))
                        })
                        .inner;
                    pairs.push((format!("ancestor {set} {state:?}"), scoped.0, scoped.1));
                    set_enabled(ui.ctx(), false);
                    pairs.push((
                        format!("switched off {set} {state:?}"),
                        kit_all(ui, set, state),
                        stock_all(ui, set, state),
                    ));
                    set_enabled(ui.ctx(), true);
                }
            }
            pairs
        });
        for (what, kit, stock) in pairs {
            assert_eq!(kit, stock, "{what}");
        }
        assert!(enabled(&ctx));
    }

    #[test]
    fn the_switch_is_on_by_default_and_flips() {
        let ctx = Context::default();
        assert!(enabled(&ctx));
        set_enabled(&ctx, false);
        assert!(!enabled(&ctx));
        set_enabled(&ctx, true);
        assert!(enabled(&ctx));
    }

    /// The kit and default button styles for `list` in `state`.
    fn pair(ui: &Ui, list: &[ClassName], state: WidgetState) -> (ButtonStyle, ButtonStyle) {
        let set = classes(list);
        (
            kit::<ButtonStyle>(ui, &set, state),
            stock::<ButtonStyle>(ui, &set, state),
        )
    }

    #[test]
    fn built_in_button_classes_keep_their_default_meaning() {
        let ctx = context();
        let ((t, got), _) = run(&ctx, 1, |ui| {
            let small = pair(ui, &[Button::CLASS_SMALL], WidgetState::Inactive);
            let no_frame = pair(ui, &[Button::CLASS_NO_FRAME], WidgetState::Hovered);
            let hidden = pair(
                ui,
                &[Button::CLASS_HIDE_FRAME_WHEN_INACTIVE],
                WidgetState::Inactive,
            );
            let shown = pair(
                ui,
                &[Button::CLASS_HIDE_FRAME_WHEN_INACTIVE],
                WidgetState::Hovered,
            );
            let tinted = pair(
                ui,
                &[Button::CLASS_IMAGE_TINT_FOLLOWS_TEXT_COLOR],
                WidgetState::Inactive,
            );
            let untinted = pair(ui, &[], WidgetState::Inactive);
            let selected = pair(ui, &[Button::CLASS_SELECTED], WidgetState::Hovered);
            let suggestion = pair(
                ui,
                &[Button::CLASS_SELECTED, CompletionPopup::CLASS_SUGGESTION],
                WidgetState::Hovered,
            );
            let combo = pair(ui, &[ComboBox::CLASS], WidgetState::Hovered);
            let open = pair(
                ui,
                &[ComboBox::CLASS, ComboBox::CLASS_OPEN],
                WidgetState::Inactive,
            );
            let drag = pair(ui, &[DragValue::CLASS], WidgetState::Hovered);
            let plain = pair(ui, &[], WidgetState::Hovered);
            let frameless = ui
                .scope(|ui| {
                    ui.visuals_mut().button_frame = false;
                    (
                        pair(ui, &[], WidgetState::Inactive),
                        pair(ui, &[Button::CLASS_FRAME], WidgetState::Inactive),
                    )
                })
                .inner;
            (
                Tokens::get(ui.ctx()),
                [
                    small,
                    no_frame,
                    hidden,
                    shown,
                    tinted,
                    untinted,
                    selected,
                    suggestion,
                    combo,
                    open,
                    drag,
                    plain,
                    frameless.0,
                    frameless.1,
                ],
            )
        });
        let [
            small,
            no_frame,
            hidden,
            shown,
            tinted,
            untinted,
            selected,
            suggestion,
            combo,
            open,
            drag,
            plain,
            unframed,
            framed,
        ] = got;

        let (kit, stock) = small;
        let (k, s) = (
            kit.atom_layout.frame.total_margin(),
            stock.atom_layout.frame.total_margin(),
        );
        assert_eq!(
            (k.top, k.bottom),
            (0.0, 0.0),
            "small keeps no vertical margin"
        );
        assert_eq!((k.top, k.bottom, k.left), (s.top, s.bottom, s.left));
        assert_eq!(kit.atom_layout.min_size, Vec2::ZERO);
        assert_eq!(stock.atom_layout.min_size, Vec2::ZERO);

        assert_eq!(debug(&no_frame.0), debug(&no_frame.1), "no frame is stock");
        assert_eq!(
            debug(&unframed.0),
            debug(&unframed.1),
            "button_frame off is stock"
        );
        assert_eq!(
            framed.0.atom_layout.frame.fill, t.field,
            "CLASS_FRAME draws the kit frame"
        );

        let frame = &hidden.0.atom_layout.frame;
        assert_eq!(frame.fill, Color32::TRANSPARENT);
        assert_eq!(frame.stroke.color, Color32::TRANSPARENT);
        assert_eq!(
            frame.total_margin(),
            shown.0.atom_layout.frame.total_margin()
        );
        assert_eq!(shown.0.atom_layout.frame.fill, t.hover);

        assert_eq!(tinted.0.atom_layout.image_tint, t.text);
        assert_eq!(untinted.0.atom_layout.image_tint, Color32::WHITE);

        let (fill, edge, ink) = controls::selected_colors(&t);
        assert_eq!(look(&selected.0), kit_look(fill, edge, ink));
        assert_eq!(
            debug(&suggestion.0),
            debug(&selected.0),
            "suggestions are selectables"
        );

        let (fill, edge) = controls::dropdown_colors(&t, State::Hovered, false);
        assert_eq!(look(&combo.0), kit_look(fill, edge, t.text));
        let (fill, edge) = controls::dropdown_colors(&t, State::Rest, true);
        assert_eq!(look(&open.0), kit_look(fill, edge, t.text));

        assert_eq!(
            debug(&drag.0),
            debug(&plain.0),
            "a drag value is a neutral button"
        );
    }

    #[test]
    fn a_kit_class_frames_a_button_in_a_frameless_theme_unless_it_opts_out() {
        let ctx = context();
        let ((t, got), _) = run(&ctx, 1, |ui| {
            ui.scope(|ui| {
                ui.visuals_mut().button_frame = false;
                let state = WidgetState::Inactive;
                (
                    Tokens::get(ui.ctx()),
                    [
                        pair(ui, &[], state),
                        pair(ui, &[PRIMARY], state),
                        pair(ui, &[PROMINENT], state),
                        pair(ui, &[PRIMARY, Button::CLASS_NO_FRAME], state),
                    ],
                )
            })
            .inner
        });
        let [plain, primary, prominent, opted_out] = got;
        assert_eq!(
            debug(&plain.0),
            debug(&plain.1),
            "a plain button follows the theme"
        );
        assert_eq!(primary.0.atom_layout.frame.fill, t.primary);
        assert_eq!(prominent.0.atom_layout.frame.fill, t.field);
        let m = Metrics::get(&ctx);
        assert_eq!(prominent.0.atom_layout.min_size.y, m.button_h);
        assert_eq!(
            debug(&opted_out.0),
            debug(&opted_out.1),
            "frame(false) wins"
        );
    }

    #[test]
    fn text_edits_take_the_field_look_and_keep_read_only() {
        let ctx = context();
        let ((t, got), _) = run(&ctx, 1, |ui| {
            let none = classes(&[]);
            let fixed = classes(&[TextEdit::CLASS_READ_ONLY]);
            (
                Tokens::get(ui.ctx()),
                [
                    kit::<TextEditStyle>(ui, &none, WidgetState::Inactive),
                    kit::<TextEditStyle>(ui, &none, WidgetState::Hovered),
                    kit::<TextEditStyle>(ui, &none, WidgetState::Active),
                    kit::<TextEditStyle>(ui, &fixed, WidgetState::Hovered),
                    stock::<TextEditStyle>(ui, &none, WidgetState::Inactive),
                ],
            )
        });
        let [rest, hovered, focused, fixed, default] = got;
        let edge = |style: &TextEditStyle| style.atom_layout.frame.stroke;
        assert_eq!(rest.atom_layout.frame.fill, t.field);
        assert_eq!(
            edge(&rest),
            box_stroke(controls::field_border(&t, State::Rest, false))
        );
        assert_eq!(
            edge(&hovered),
            box_stroke(controls::field_border(&t, State::Hovered, false))
        );
        assert_eq!(edge(&focused), box_stroke(t.primary_border));
        assert_eq!(fixed.atom_layout.frame.fill, Color32::TRANSPARENT);
        assert_eq!(edge(&fixed), box_stroke(t.field_border));
        assert_eq!(
            rest.hint_text_color, default.hint_text_color,
            "the hint keeps the colour kit fields paint"
        );
        assert_eq!(rest.atom_layout.text_style.color, t.text);
        assert_eq!(
            rest.atom_layout.frame.total_margin(),
            default.atom_layout.frame.total_margin()
        );
        assert_eq!(rest.atom_layout.min_size, default.atom_layout.min_size);
        assert_eq!(rest.atom_layout.align2, default.atom_layout.align2);
    }

    /// Distance from each edit's top to its text, for an empty edit with a hint and a one-line edit.
    fn multiline_text_offsets(ctx: &Context) -> ([f32; 2], f32) {
        let (got, _) = run(ctx, 2, |ui| {
            let mut empty = String::new();
            let mut line = "One line".to_owned();
            let hinted = TextEdit::multiline(&mut empty)
                .hint_text("Describe the fault")
                .desired_rows(4)
                .desired_width(130.0)
                .show(ui);
            let typed = TextEdit::multiline(&mut line)
                .desired_rows(4)
                .desired_width(130.0)
                .show(ui);
            let margin = kit::<TextEditStyle>(ui, &classes(&[]), WidgetState::Inactive)
                .atom_layout
                .frame
                .total_margin()
                .top;
            let offset = |out: &eframe::egui::text_edit::TextEditOutput| {
                out.galley_pos.y - out.response.response.rect.top()
            };
            ([offset(&hinted), offset(&typed)], margin)
        });
        got
    }

    #[test]
    fn multiline_text_edits_keep_their_text_at_the_top_margin() {
        let ctx = context();
        let (on, margin) = multiline_text_offsets(&ctx);
        set_enabled(&ctx, false);
        let (off, _) = multiline_text_offsets(&ctx);
        assert_eq!(on, [margin; 2], "switch on");
        assert_eq!(on, off, "the switch does not move the text");
    }

    #[test]
    fn checkboxes_and_separators_take_the_kit_look() {
        let ctx = context();
        let ((t, m, boxes, line, default_line), _) = run(&ctx, 1, |ui| {
            let none = classes(&[]);
            let boxes =
                STATES.map(|(widget, state)| (state, kit::<CheckboxStyle>(ui, &none, widget)));
            (
                Tokens::get(ui.ctx()),
                Metrics::get(ui.ctx()),
                boxes,
                kit::<SeparatorStyle>(ui, &none, WidgetState::Noninteractive),
                stock::<SeparatorStyle>(ui, &none, WidgetState::Noninteractive),
            )
        });
        for (state, style) in &boxes {
            let (fill, edge) = controls::checkbox_colors(&t, false, *state);
            let frame = &style.checkbox_frame;
            assert_eq!(
                (frame.fill, frame.stroke),
                (fill, box_stroke(edge)),
                "{state:?}"
            );
            assert_eq!(
                frame.corner_radius,
                CornerRadius::same(controls::box_radius(&t))
            );
            assert_eq!(style.check_stroke, Stroke::new(TICK_W, t.primary));
            assert_eq!(style.checkbox_size, m.icon_md);
            assert_eq!(style.atom_layout.gap, controls::LABEL_GAP);
            assert_eq!(style.atom_layout.text_style.color, t.text);
        }
        assert_eq!(line.stroke, Stroke::new(1.0, t.separator));
        assert_eq!(line.spacing, m.gap.y);
        assert_eq!(
            line.spacing, default_line.spacing,
            "Regular spacing matches egui"
        );
    }

    #[test]
    fn the_stock_tick_stays_clear_of_the_box_border_at_every_density() {
        for density in Density::ALL {
            let ctx = context_at(density);
            let (styles, _) = run(&ctx, 1, |ui| {
                STATES.map(|(state, _)| kit::<CheckboxStyle>(ui, &classes(&[]), state))
            });
            for style in &styles {
                let inset = (style.checkbox_size - style.check_size) / 2.0;
                let border = style.checkbox_frame.stroke.width;
                let clear = inset - border - style.check_stroke.width;
                assert!(
                    clear >= 1.0,
                    "{density:?}: {clear} pt between tick and border"
                );
            }
        }
    }

    #[test]
    fn locally_mutated_visuals_keep_the_default_colours() {
        let ctx = context();
        let ((t, got), _) = run(&ctx, 1, |ui| {
            let none = classes(&[]);
            let primary = classes(&[PRIMARY]);
            let out = ui
                .scope(|ui| {
                    ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::RED;
                    ui.visuals_mut().widgets.inactive.bg_fill = Color32::RED;
                    ui.visuals_mut().widgets.noninteractive.bg_stroke =
                        Stroke::new(1.0, Color32::RED);
                    (
                        kit::<ButtonStyle>(ui, &none, WidgetState::Inactive),
                        stock::<ButtonStyle>(ui, &none, WidgetState::Inactive),
                        debug(&kit::<ButtonStyle>(ui, &none, WidgetState::Hovered)),
                        debug(&stock::<ButtonStyle>(ui, &none, WidgetState::Hovered)),
                        kit::<ButtonStyle>(ui, &primary, WidgetState::Inactive),
                        debug(&kit::<TextEditStyle>(ui, &none, WidgetState::Hovered)),
                        debug(&stock::<TextEditStyle>(ui, &none, WidgetState::Hovered)),
                        kit::<CheckboxStyle>(ui, &none, WidgetState::Inactive),
                        stock::<CheckboxStyle>(ui, &none, WidgetState::Inactive),
                        kit::<SeparatorStyle>(ui, &none, WidgetState::Noninteractive),
                    )
                })
                .inner;
            (Tokens::get(ui.ctx()), out)
        });
        let (
            rest,
            stock_rest,
            hovered,
            stock_hovered,
            primary,
            edit,
            stock_edit,
            check,
            stock_check,
            line,
        ) = got;
        assert_eq!(rest.atom_layout.frame.fill, Color32::RED);
        assert_eq!(
            debug(&rest.atom_layout.frame),
            debug(&stock_rest.atom_layout.frame)
        );
        assert_eq!(
            rest.atom_layout.text_style.color,
            stock_rest.atom_layout.text_style.color
        );
        assert_eq!(
            hovered, stock_hovered,
            "the other states keep the default look too"
        );
        assert_eq!(
            primary.atom_layout.frame.fill, t.primary,
            "a kind class always wins"
        );
        assert_eq!(
            edit, stock_edit,
            "a text edit reads the mutated inactive visuals"
        );
        assert_eq!(check.checkbox_frame.fill, Color32::RED);
        assert_eq!(checkbox_paint(&check), checkbox_paint(&stock_check));
        assert_eq!(line.stroke, Stroke::new(1.0, Color32::RED));
    }

    #[test]
    fn overriding_one_state_keeps_the_default_look_in_every_state() {
        let ctx = context();
        let overrides: [fn(&mut Visuals); 3] = [
            |v| v.widgets.inactive.bg_stroke = Stroke::new(0.5, Color32::GOLD),
            |v| v.widgets.hovered.weak_bg_fill = Color32::RED,
            |v| v.widgets.active.bg_stroke = Stroke::new(2.0, Color32::RED),
        ];
        let (pairs, _) = run(&ctx, 1, |ui| {
            let none = classes(&[]);
            let mut pairs = Vec::new();
            for (i, apply) in overrides.iter().enumerate() {
                ui.scope(|ui| {
                    apply(ui.visuals_mut());
                    for (state, _) in STATES {
                        let at = format!("override {i} in {state:?}");
                        pairs.push((
                            format!("button, {at}"),
                            debug(&kit::<ButtonStyle>(ui, &none, state)),
                            debug(&stock::<ButtonStyle>(ui, &none, state)),
                        ));
                        pairs.push((
                            format!("text edit, {at}"),
                            debug(&kit::<TextEditStyle>(ui, &none, state)),
                            debug(&stock::<TextEditStyle>(ui, &none, state)),
                        ));
                        pairs.push((
                            format!("checkbox, {at}"),
                            checkbox_paint(&kit::<CheckboxStyle>(ui, &none, state)),
                            checkbox_paint(&stock::<CheckboxStyle>(ui, &none, state)),
                        ));
                    }
                });
            }
            pairs
        });
        for (what, kit, stock) in pairs {
            assert_eq!(kit, stock, "{what}");
        }
    }

    #[test]
    fn a_noninteractive_override_keeps_interactive_widgets_kit() {
        let ctx = context();
        let ((t, got), _) = run(&ctx, 1, |ui| {
            ui.scope(|ui| {
                ui.visuals_mut().widgets.noninteractive.fg_stroke.color = Color32::RED;
                let none = classes(&[]);
                (
                    Tokens::get(ui.ctx()),
                    (
                        kit::<ButtonStyle>(ui, &none, WidgetState::Hovered),
                        kit::<TextEditStyle>(ui, &none, WidgetState::Active),
                        debug(&kit::<ButtonStyle>(ui, &none, WidgetState::Noninteractive)),
                        debug(&stock::<ButtonStyle>(
                            ui,
                            &none,
                            WidgetState::Noninteractive,
                        )),
                    ),
                )
            })
            .inner
        });
        let (hovered, focused, disabled, stock_disabled) = got;
        let (fill, edge, ink) = controls::button_colors(&t, ButtonKind::Neutral, State::Hovered);
        assert_eq!(look(&hovered), kit_look(fill, edge, ink));
        assert_eq!(focused.atom_layout.frame.fill, t.field);
        assert_eq!(
            disabled, stock_disabled,
            "a noninteractive widget reads the override"
        );
    }

    #[test]
    fn a_menu_styled_ui_keeps_the_default_look_in_every_state() {
        let mut global = Style::default();
        let w = &mut global.visuals.widgets;
        for state in [&mut w.hovered, &mut w.active, &mut w.open] {
            state.bg_stroke = Stroke::NONE;
        }
        let ctx = context_with(Density::Regular, global);
        let (got, _) = run(&ctx, 1, |ui| {
            ui.scope(|ui| {
                menu::menu_style(ui.style_mut());
                let rows = [classes(&[]), classes(&[Button::CLASS_FRAME])];
                let mut got = Vec::new();
                for row in &rows {
                    for (state, _) in STATES {
                        got.push((
                            format!("{row} in {state:?}"),
                            kit::<ButtonStyle>(ui, row, state),
                            stock::<ButtonStyle>(ui, row, state),
                        ));
                    }
                }
                got
            })
            .inner
        });
        for (what, kit, stock) in &got {
            assert_eq!(debug(kit), debug(stock), "{what}");
        }
        let (_, rest, _) = &got[1];
        assert_eq!(rest.atom_layout.frame.fill, Color32::TRANSPARENT);
        assert_eq!(rest.atom_layout.frame.stroke, Stroke::NONE);
    }

    #[test]
    fn a_style_installed_mid_pass_keeps_the_kit_look_in_existing_uis() {
        let ctx = context();
        for step in 0..3_u8 {
            let (got, _) = run(&ctx, 1, |ui| {
                let none = classes(&[]);
                let before = ui.button("Apply").rect;
                let mut style = (*ui.ctx().global_style()).clone();
                style.visuals.widgets.inactive.weak_bg_fill = Color32::from_gray(40 + step);
                style.visuals.selection.stroke.color = Color32::from_gray(90 + step);
                metrics::install_overlaid_style(ui.ctx(), style);
                let after = ui.button("Apply").rect;
                let child = ui
                    .scope(|ui| kit::<ButtonStyle>(ui, &none, WidgetState::Inactive))
                    .inner;
                let local = ui
                    .scope(|ui| {
                        ui.visuals_mut().widgets.inactive.bg_fill = Color32::RED;
                        (
                            debug(&kit::<ButtonStyle>(ui, &none, WidgetState::Inactive)),
                            debug(&stock::<ButtonStyle>(ui, &none, WidgetState::Inactive)),
                        )
                    })
                    .inner;
                (
                    Tokens::get(ui.ctx()),
                    (before, after),
                    kit::<ButtonStyle>(ui, &none, WidgetState::Inactive),
                    child,
                    kit::<TextEditStyle>(ui, &none, WidgetState::Hovered),
                    kit::<CheckboxStyle>(ui, &none, WidgetState::Inactive),
                    local,
                )
            });
            let (t, (before, after), button, child, edit, check, local) = got;
            let at = format!("pass {step}");
            let (fill, edge, ink) = controls::button_colors(&t, ButtonKind::Neutral, State::Rest);
            assert_eq!(look(&button), kit_look(fill, edge, ink), "{at}");
            assert_eq!(look(&child), kit_look(fill, edge, ink), "{at}: child scope");
            assert_eq!(edit.atom_layout.frame.fill, t.field, "{at}: text edit");
            let (fill, edge) = controls::checkbox_colors(&t, false, State::Rest);
            assert_eq!(
                (check.checkbox_frame.fill, check.checkbox_frame.stroke),
                (fill, box_stroke(edge)),
                "{at}: checkbox"
            );
            assert_eq!(before.size(), after.size(), "{at}: a later button resizes");
            assert_eq!(
                local.0, local.1,
                "{at}: a local override keeps the default look"
            );
        }
    }

    #[test]
    fn override_text_color_sets_the_ink_and_primary_takes_the_selected_look() {
        let ctx = context();
        let ((t, got), _) = run(&ctx, 1, |ui| {
            ui.scope(|ui| {
                ui.visuals_mut().override_text_color = Some(Color32::GREEN);
                let none = classes(&[]);
                (
                    Tokens::get(ui.ctx()),
                    (
                        kit::<ButtonStyle>(ui, &none, WidgetState::Inactive),
                        kit::<ButtonStyle>(ui, &classes(&[PRIMARY]), WidgetState::Hovered),
                        kit::<ButtonStyle>(ui, &classes(&[DANGER]), WidgetState::Inactive),
                        kit::<TextEditStyle>(ui, &none, WidgetState::Active)
                            .atom_layout
                            .text_style
                            .color,
                        kit::<CheckboxStyle>(ui, &none, WidgetState::Inactive)
                            .atom_layout
                            .text_style
                            .color,
                    ),
                )
            })
            .inner
        });
        let (plain, primary, danger, edit_ink, check_ink) = got;
        let green = Color32::GREEN;
        let (fill, edge, _) = controls::button_colors(&t, ButtonKind::Neutral, State::Rest);
        assert_eq!(look(&plain), kit_look(fill, edge, green));
        let (fill, edge, _) = controls::selected_colors(&t);
        assert_eq!(look(&primary), kit_look(fill, edge, green));
        let (fill, edge, _) = controls::button_colors(&t, ButtonKind::Danger, State::Rest);
        assert_eq!(look(&danger), kit_look(fill, edge, green));
        assert_eq!([edit_ink, check_ink], [green; 2]);
    }

    #[test]
    fn installing_twice_keeps_one_kit_theme() {
        let ctx = context();
        install(&ctx);
        metrics::install_overlaid_style(&ctx, Style::default());
        install(&ctx);
        assert!(ctx.plugin_opt::<PassStart>().is_some());
        let (got, _) = run(&ctx, 1, |ui| {
            let none = classes(&[]);
            let a = args(ui, &none, WidgetState::Inactive);
            (
                [
                    debug(&ui.ctx().get_widget_style::<ButtonStyle>(&a)),
                    debug(&ui.ctx().get_widget_style::<TextEditStyle>(&a)),
                    debug(&ui.ctx().get_widget_style::<CheckboxStyle>(&a)),
                    debug(&ui.ctx().get_widget_style::<SeparatorStyle>(&a)),
                ],
                kit_all(ui, &none, WidgetState::Inactive),
                stock_all(ui, &none, WidgetState::Inactive),
            )
        });
        let (registered, kit, stock) = got;
        assert_eq!(registered, kit);
        assert_ne!(
            registered[0], stock[0],
            "the default button style was replaced"
        );
    }

    /// Every rect shape in `shapes`, flattened.
    fn rects(shapes: &[ClippedShape]) -> Vec<eframe::egui::epaint::RectShape> {
        fn walk(shape: &Shape, out: &mut Vec<eframe::egui::epaint::RectShape>) {
            match shape {
                Shape::Rect(rect) => out.push(rect.clone()),
                Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        shapes.iter().for_each(|c| walk(&c.shape, &mut out));
        out
    }

    #[test]
    fn explicit_button_overrides_win_over_the_theme() {
        let ctx = context();
        let edge = Stroke::new(2.0, Color32::BLUE);
        let (rect, shapes) = run(&ctx, 2, |ui| {
            let button = Button::new("Go")
                .with_class(PRIMARY)
                .fill(Color32::RED)
                .stroke(edge)
                .corner_radius(3)
                .min_size(vec2(120.0, 40.0));
            ui.add(button).rect
        });
        let painted = rects(&shapes);
        let frame = painted
            .iter()
            .find(|r| r.fill == Color32::RED)
            .expect("the explicit fill is painted");
        assert_eq!(frame.stroke, edge);
        assert_eq!(frame.corner_radius, CornerRadius::same(3));
        assert!(rect.width() >= 120.0 && rect.height() >= 40.0, "{rect:?}");
    }

    #[test]
    fn stock_buttons_match_kit_button_geometry_at_every_density() {
        for density in Density::ALL {
            let ctx = context_at(density);
            let m = Metrics::for_density(density);
            let (got, _) = run(&ctx, 2, |ui| {
                [
                    ui.button("Apply").rect,
                    TextButton::new("Apply").show(ui).rect,
                    ui.add(Button::new("Save").with_class(PROMINENT)).rect,
                ]
            });
            let [stock, kit, prominent] = got;
            assert_eq!(stock.height(), kit.height(), "{density:?}");
            assert!(
                (stock.width() - kit.width()).abs() <= 1.0,
                "{density:?} {stock:?} {kit:?}"
            );
            assert_eq!(prominent.height(), m.button_h, "{density:?}");
        }
    }
}
