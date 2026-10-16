//! App-drawn window top strip: mark, caller content, title, captions, drag area and resize edges.

use eframe::egui::{
    Align, AsIdSalt, Color32, Context, CursorIcon, Frame, Id, LayerId, Layout, Order, Panel,
    PointerButton, Rect, ResizeDirection, Role, Sense, Stroke, StrokeKind, TextWrapMode, Ui,
    UiBuilder, Vec2, ViewportCommand, WidgetInfo, pos2, vec2,
};

use crate::controls::{
    Icon, Metrics, State, Tokens, kit, medium_font_sized, state_of, text_galley,
};
use crate::icon_pack;
use crate::metrics::type_scale;

/// Width of one caption button.
pub const CAPTION_W: f32 = 46.0;
/// Depth of an edge resize zone.
pub const EDGE: f32 = 8.0;
/// Side of a corner resize zone.
pub const CORNER: f32 = 12.0;
/// Space kept between the title and the content beside it.
const TITLE_GAP: f32 = 16.0;
/// Brand-mark size.
const MARK: f32 = 20.0;
/// Opacity of the brand mark's duotone back layer.
const MARK_DUO: f32 = 0.35;
/// Caption glyph size.
const CAPTION_GLYPH: f32 = 14.0;
/// Close-button fill while hovered.
const CLOSE_HOVER: Color32 = Color32::from_rgb(196, 43, 28);
/// Close-button fill while pressed.
const CLOSE_PRESS: Color32 = Color32::from_rgb(160, 36, 24);

/// One window caption button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Caption {
    Minimize,
    Maximize,
    Close,
}

impl Caption {
    /// Left to right.
    pub const ALL: [Self; 3] = [Self::Minimize, Self::Maximize, Self::Close];

    /// Stable widget id.
    pub fn id(self) -> Id {
        Id::unique(("mtech.title_bar.caption", self as u8))
    }

    fn icon(self, maximized: bool) -> Icon {
        match self {
            Self::Minimize => icon_pack::MINUS,
            Self::Maximize if maximized => icon_pack::COPY,
            Self::Maximize => icon_pack::SQUARE,
            Self::Close => icon_pack::X,
        }
    }

    fn tip(self, maximized: bool) -> &'static str {
        match self {
            Self::Minimize => "Minimize",
            Self::Maximize if maximized => "Restore down",
            Self::Maximize => "Maximize",
            Self::Close => "Close",
        }
    }
}

/// Which end of the strip [`TitleBar::show`] is filling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    /// Laid out left to right after the mark.
    Left,
    /// Laid out right to left before the caption buttons; filled first.
    Right,
}

/// What a [`TitleBar`] laid out this frame.
#[derive(Clone, Copy, Debug)]
pub struct TitleBarOutput<R> {
    pub left: R,
    pub right: R,
    /// The whole strip.
    pub rect: Rect,
    /// Free space between the two ends; drags the window when frameless.
    pub gap: Rect,
}

/// The window's top strip, drawn as a top panel.
#[must_use = "call `show` to draw the strip"]
pub struct TitleBar<'a> {
    id: Id,
    title: &'a str,
    mark: Option<Icon>,
    frameless: bool,
}

impl<'a> TitleBar<'a> {
    pub fn new(id_salt: impl AsIdSalt, title: &'a str) -> Self {
        Self {
            id: Id::unique(id_salt),
            title,
            mark: None,
            frameless: false,
        }
    }

    /// Brand mark at the left end.
    pub fn mark(mut self, icon: Icon) -> Self {
        self.mark = Some(icon);
        self
    }

    /// Adds caption buttons, and lets empty space drag and maximize the window.
    pub fn frameless(mut self, frameless: bool) -> Self {
        self.frameless = frameless;
        self
    }

    /// Height of the strip at the current density.
    pub fn height(m: &Metrics) -> f32 {
        m.bar_h
    }

    /// Draws the strip; `add` fills [`Side::Right`] and then [`Side::Left`].
    pub fn show<R>(self, ui: &mut Ui, add: impl FnMut(&mut Ui, Side) -> R) -> TitleBarOutput<R> {
        let (t, m) = kit(ui);
        Panel::top(self.id)
            .exact_size(Self::height(&m))
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::NONE.fill(t.base))
            .show(ui, |ui| self.contents(ui, &t, &m, add))
            .inner
    }

    fn contents<R>(
        &self,
        ui: &mut Ui,
        t: &Tokens,
        m: &Metrics,
        mut add: impl FnMut(&mut Ui, Side) -> R,
    ) -> TitleBarOutput<R> {
        let full = ui.max_rect();
        let line_y = ui.painter().round_to_pixel_center(full.bottom() - 0.5);
        ui.painter()
            .hline(full.x_range(), line_y, Stroke::new(1.0, t.separator));
        let ends = if self.frameless {
            full.right() - Caption::ALL.len() as f32 * CAPTION_W
        } else {
            full.right() - m.pad.x
        };

        let mut start = full.left() + m.pad.x;
        if let Some(mark) = self.mark {
            let rect = Rect::from_center_size(
                pos2(start + MARK / 2.0, full.center().y),
                Vec2::splat(MARK),
            );
            mark.paint_duo(
                ui,
                rect,
                MARK,
                t.primary,
                t.primary.gamma_multiply(MARK_DUO),
            );
            start = rect.right() + m.gap.x;
        }

        let right_rect = Rect::from_x_y_ranges(start..=(ends - m.gap.x).max(start), full.y_range());
        let (right, right_used) = side_ui(ui, right_rect, Side::Right, m, &mut add);
        let right_start = right_used.left().min(right_rect.right());

        let left_rect =
            Rect::from_x_y_ranges(start..=(right_start - TITLE_GAP).max(start), full.y_range());
        let (left, left_used) = side_ui(ui, left_rect, Side::Left, m, &mut add);
        let left_end = left_used.right().max(start);

        let gap = Rect::from_x_y_ranges(left_end..=right_start.max(left_end), full.y_range());
        self.paint_title(ui, t, full, gap);
        if self.frameless {
            drag_area(ui, self.id.with("drag"), gap);
            let lead = Rect::from_x_y_ranges(full.left()..=start, full.y_range());
            drag_area(ui, self.id.with("lead"), lead);
            caption_buttons(ui, full, t);
        }
        TitleBarOutput {
            left,
            right,
            rect: full,
            gap,
        }
    }

    /// The title centred on the strip, slid to stay inside `gap`; omitted when it does not fit.
    fn paint_title(&self, ui: &Ui, t: &Tokens, full: Rect, gap: Rect) {
        if self.title.is_empty() {
            return;
        }
        let galley = text_galley(ui, self.title, medium_font_sized(ui, type_scale::BODY));
        let width = galley.size().x;
        let Some(x) = title_x(full.center().x, gap.left(), gap.right(), width) else {
            return;
        };
        let y = (full.center().y - galley.size().y / 2.0).round();
        ui.painter().galley(pos2(x.round(), y), galley, t.text_dim);
    }
}

/// One end of the strip in its own child Ui; returns `add`'s result and the space used.
fn side_ui<R>(
    ui: &mut Ui,
    rect: Rect,
    side: Side,
    m: &Metrics,
    add: &mut impl FnMut(&mut Ui, Side) -> R,
) -> (R, Rect) {
    let layout = match side {
        Side::Left => Layout::left_to_right(Align::Center),
        Side::Right => Layout::right_to_left(Align::Center),
    };
    let salt = match side {
        Side::Left => "left",
        Side::Right => "right",
    };
    let mut child = ui.new_child(UiBuilder::new().id_salt(salt).max_rect(rect).layout(layout));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child.spacing_mut().item_spacing.x = m.gap.x;
    child.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
    let out = add(&mut child, side);
    (out, child.min_rect())
}

/// Left edge of a title `width` wide centred on `center` within `left..right`; `None` if too wide.
fn title_x(center: f32, left: f32, right: f32, width: f32) -> Option<f32> {
    let (lo, hi) = (left + TITLE_GAP, right - TITLE_GAP - width);
    if !(lo.is_finite() && hi.is_finite()) || hi < lo {
        return None;
    }
    Some((center - width / 2.0).clamp(lo, hi))
}

fn is_maximized(ctx: &Context) -> bool {
    ctx.input(|i| i.viewport().maximized.unwrap_or(false))
}

fn is_fullscreen(ctx: &Context) -> bool {
    ctx.input(|i| i.viewport().fullscreen.unwrap_or(false))
}

/// Empty strip space: a drag moves the window and a double-click maximizes or restores it.
fn drag_area(ui: &Ui, id: Id, rect: Rect) {
    if rect.width() <= 0.0 {
        return;
    }
    let response = ui.interact(rect, id, Sense::click_and_drag());
    if response.drag_started_by(PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if response.double_clicked() {
        let maximized = is_maximized(ui.ctx());
        ui.ctx()
            .send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    }
}

/// Minimize, maximize or restore, and close, flush with the strip's right end.
fn caption_buttons(ui: &Ui, full: Rect, t: &Tokens) {
    let maximized = is_maximized(ui.ctx());
    let left = full.right() - Caption::ALL.len() as f32 * CAPTION_W;
    let mut clicked = None;
    for (i, caption) in Caption::ALL.into_iter().enumerate() {
        let rect = Rect::from_min_size(
            pos2(left + i as f32 * CAPTION_W, full.top()),
            vec2(CAPTION_W, full.height()),
        );
        let response = ui.interact(rect, caption.id(), Sense::click());
        let tip = caption.tip(maximized);
        response.widget_info(|| WidgetInfo::labeled(Role::Button, true, tip));
        let (state, _) = state_of(ui, &response);
        let close = caption == Caption::Close;
        let (fill, ink) = match state {
            State::Rest => (Color32::TRANSPARENT, t.icon),
            State::Hovered if close => (CLOSE_HOVER, Color32::WHITE),
            State::Pressed if close => (CLOSE_PRESS, Color32::WHITE),
            State::Hovered => (t.hover, t.text),
            State::Pressed => (t.pressed, t.text),
        };
        ui.painter().rect_filled(rect, 0, fill);
        caption.icon(maximized).paint(ui, rect, CAPTION_GLYPH, ink);
        if response.on_hover_text(tip).clicked() {
            clicked = Some(caption);
        }
    }
    let command = match clicked {
        Some(Caption::Minimize) => ViewportCommand::Minimized(true),
        Some(Caption::Maximize) => ViewportCommand::Maximized(!maximized),
        Some(Caption::Close) => ViewportCommand::Close,
        None => return,
    };
    ui.ctx().send_viewport_cmd(command);
}

/// Edge then corner zones of `window`: (zone, direction, cursor).
fn zones(window: Rect) -> [(Rect, ResizeDirection, CursorIcon); 8] {
    use ResizeDirection::*;
    let w = window;
    [
        (
            Rect::from_min_max(w.left_top(), pos2(w.right(), w.top() + EDGE)),
            North,
            CursorIcon::ResizeNorth,
        ),
        (
            Rect::from_min_max(pos2(w.left(), w.bottom() - EDGE), w.right_bottom()),
            South,
            CursorIcon::ResizeSouth,
        ),
        (
            Rect::from_min_max(w.left_top(), pos2(w.left() + EDGE, w.bottom())),
            West,
            CursorIcon::ResizeWest,
        ),
        (
            Rect::from_min_max(pos2(w.right() - EDGE, w.top()), w.right_bottom()),
            East,
            CursorIcon::ResizeEast,
        ),
        (
            Rect::from_min_size(w.left_top(), Vec2::splat(CORNER)),
            NorthWest,
            CursorIcon::ResizeNwSe,
        ),
        (
            Rect::from_min_size(w.right_top() - vec2(CORNER, 0.0), Vec2::splat(CORNER)),
            NorthEast,
            CursorIcon::ResizeNeSw,
        ),
        (
            Rect::from_min_size(w.left_bottom() - vec2(0.0, CORNER), Vec2::splat(CORNER)),
            SouthWest,
            CursorIcon::ResizeNeSw,
        ),
        (
            Rect::from_min_size(w.right_bottom() - Vec2::splat(CORNER), Vec2::splat(CORNER)),
            SouthEast,
            CursorIcon::ResizeNwSe,
        ),
    ]
}

/// Resize zones and a one-pixel outline on a frameless window's edges unless maximized.
pub fn window_edges(ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    if is_maximized(&ctx) || is_fullscreen(&ctx) {
        return;
    }
    let window = ctx.content_rect();
    let layer = LayerId::new(Order::Foreground, Id::unique("mtech.title_bar.edges"));
    let edges = ui.new_child(UiBuilder::new().layer_id(layer).max_rect(window));
    for (i, (rect, direction, cursor)) in zones(window).into_iter().enumerate() {
        let response = edges
            .interact(rect, Id::unique(("mtech.title_bar.resize", i)), Sense::drag())
            .on_hover_cursor(cursor);
        if response.drag_started_by(PointerButton::Primary) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
        }
    }
    let (t, _) = kit(ui);
    edges.painter().rect_stroke(
        window,
        0,
        Stroke::new(1.0, t.card_border),
        StrokeKind::Inside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{self, Density};
    use eframe::egui::{
        Event, FontDefinitions, Modifiers, Pos2, RawInput, Style, ViewportId, ViewportInfo,
    };

    /// A headless window `width` points wide showing a title strip and the edge zones.
    struct Win {
        ctx: Context,
        width: f32,
        time: f64,
        maximized: bool,
        frameless: bool,
    }

    impl Win {
        fn new(width: f32) -> Self {
            let ctx = Context::default();
            let mut fonts = FontDefinitions::default();
            crate::icons::install_fonts(&mut fonts);
            crate::fonts::install(&mut fonts);
            ctx.set_fonts(fonts);
            metrics::set_density(&ctx, Density::Regular);
            metrics::install_overlaid_style(&ctx, Style::default());
            Self {
                ctx,
                width,
                time: 0.0,
                maximized: false,
                frameless: true,
            }
        }

        fn frame(&mut self, events: Vec<Event>) -> Vec<ViewportCommand> {
            self.time += 0.05;
            let mut input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(self.width, 600.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            input.viewports.insert(
                ViewportId::ROOT,
                ViewportInfo {
                    maximized: Some(self.maximized),
                    ..Default::default()
                },
            );
            let frameless = self.frameless;
            let mut out = self.ctx.run_ui(input, |ui| {
                let _ = TitleBar::new("test.title_bar", "MasterTech 4.8.12")
                    .mark(icon_pack::MT_LOGO)
                    .frameless(frameless)
                    .show(ui, |ui, side| match side {
                        Side::Left => {
                            let _ = ui.button("View");
                            let _ = ui.button("Mode");
                        }
                        Side::Right => {
                            let _ = ui.button("Account");
                        }
                    });
                if frameless {
                    window_edges(ui);
                }
            });
            out.textures_delta.clear();
            out.viewport_output
                .remove(&ViewportId::ROOT)
                .map(|v| v.commands)
                .unwrap_or_default()
        }

        /// Move to `path[0]`, press, move along `path`, release; the commands sent.
        fn gesture(&mut self, path: &[Pos2]) -> Vec<ViewportCommand> {
            let mut steps = vec![
                vec![Event::PointerMoved(path[0])],
                vec![button(path[0], true)],
            ];
            steps.extend(path[1..].iter().map(|p| vec![Event::PointerMoved(*p)]));
            steps.push(vec![button(*path.last().unwrap_or(&path[0]), false)]);
            steps.into_iter().flat_map(|e| self.frame(e)).collect()
        }

        fn click(&mut self, p: Pos2) -> Vec<ViewportCommand> {
            self.gesture(&[p])
        }

        fn rect_of(&self, id: Id) -> Rect {
            self.ctx.read_response(id).map_or(Rect::NOTHING, |r| r.rect)
        }
    }

    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::default(),
        }
    }

    #[test]
    fn caption_buttons_sit_flush_in_the_top_right_corner() {
        for width in [760.0, 1000.0, 1440.0] {
            let mut w = Win::new(width);
            w.frame(vec![]);
            w.frame(vec![]);
            let [min, max, close] = Caption::ALL.map(|c| w.rect_of(c.id()));
            let bar_h = Metrics::for_density(Density::Regular).bar_h;
            assert_eq!((close.right(), close.top()), (width, 0.0), "at {width}");
            for r in [min, max, close] {
                assert_eq!((r.width(), r.height()), (CAPTION_W, bar_h), "at {width}");
            }
            assert_eq!((min.right(), max.right()), (max.left(), close.left()));
        }
    }

    #[test]
    fn caption_buttons_minimize_maximize_restore_and_close() {
        let mut w = Win::new(1000.0);
        w.frame(vec![]);
        let [min, max, close] = Caption::ALL.map(|c| w.rect_of(c.id()).center());
        assert!(w.click(min).contains(&ViewportCommand::Minimized(true)));
        assert!(w.click(max).contains(&ViewportCommand::Maximized(true)));
        w.maximized = true;
        assert!(w.click(max).contains(&ViewportCommand::Maximized(false)));
        assert!(w.click(close).contains(&ViewportCommand::Close));
    }

    #[test]
    fn empty_strip_drags_and_double_click_maximizes_but_widgets_keep_their_clicks() {
        let mut w = Win::new(1440.0);
        w.frame(vec![]);
        let empty = pos2(1000.0, 18.0);
        let cmds = w.gesture(&[empty, empty + vec2(12.0, 4.0), empty + vec2(30.0, 8.0)]);
        let drags = cmds
            .iter()
            .filter(|c| matches!(c, ViewportCommand::StartDrag))
            .count();
        assert_eq!(drags, 1, "{cmds:?}");
        let mut cmds = w.click(empty);
        cmds.extend(w.click(empty));
        assert!(cmds.contains(&ViewportCommand::Maximized(true)), "{cmds:?}");

        w.time += 1.0;
        let view = pos2(60.0, 18.0);
        let cmds = w.gesture(&[view, view + vec2(12.0, 4.0), view + vec2(30.0, 8.0)]);
        assert!(
            !cmds.iter().any(|c| matches!(c, ViewportCommand::StartDrag)),
            "a press on a widget moved the window: {cmds:?}"
        );
    }

    #[test]
    fn edges_resize_unless_maximized() {
        use ResizeDirection::*;
        let mut w = Win::new(1000.0);
        w.frame(vec![]);
        for (p, direction) in [
            (pos2(1.0, 300.0), West),
            (pos2(998.0, 300.0), East),
            (pos2(500.0, 1.0), North),
            (pos2(500.0, 598.0), South),
            (pos2(2.0, 2.0), NorthWest),
            (pos2(997.0, 3.0), NorthEast),
            (pos2(3.0, 597.0), SouthWest),
            (pos2(998.0, 598.0), SouthEast),
        ] {
            let cmds = w.gesture(&[p, p + vec2(6.0, 6.0)]);
            assert_eq!(
                cmds,
                vec![ViewportCommand::BeginResize(direction)],
                "at {p:?}"
            );
        }
        w.maximized = true;
        let cmds = w.gesture(&[pos2(1.0, 300.0), pos2(8.0, 300.0)]);
        assert!(
            !cmds
                .iter()
                .any(|c| matches!(c, ViewportCommand::BeginResize(_))),
            "{cmds:?}"
        );
    }

    #[test]
    fn a_framed_window_gets_no_captions_drag_or_edges() {
        let mut w = Win::new(1000.0);
        w.frameless = false;
        w.frame(vec![]);
        assert!(w.ctx.read_response(Caption::Close.id()).is_none());
        let empty = pos2(600.0, 18.0);
        let cmds = w.gesture(&[empty, empty + vec2(30.0, 8.0)]);
        assert!(cmds.is_empty(), "{cmds:?}");
    }

    #[test]
    fn title_stays_inside_the_free_gap() {
        assert_eq!(title_x(800.0, 420.0, 1300.0, 60.0), Some(770.0));
        assert_eq!(title_x(400.0, 420.0, 1300.0, 60.0), Some(436.0));
        assert_eq!(title_x(1280.0, 420.0, 1300.0, 60.0), Some(1224.0));
        assert_eq!(title_x(400.0, 420.0, 480.0, 60.0), None);
    }
}
