use std::collections::HashMap;
use std::sync::Arc;
use web_time::Duration;
use eframe::egui::epaint::RectShape;
use eframe::egui::{Align2, Area, Context, Direction, Frame, Id, Order, Pos2, Response, CornerRadius, Shape, Stroke, Ui, StrokeKind};
use eframe::egui::{Color32, Margin, RichText, WidgetText};

pub type ToastContents = dyn Fn(&mut Ui, &mut Toast) -> Response + Send + Sync;

pub struct Toasts {
    id: Id,
    align: Align2,
    offset: Pos2,
    direction: Direction,
    custom_toast_contents: HashMap<ToastKind, Arc<ToastContents>>,
    /// Toasts added since the last draw call. These are moved to the
    /// egui context's memory, so you are free to recreate the [`Toasts`] instance every frame.
    added_toasts: Vec<Toast>,
}

impl Default for Toasts {
    fn default() -> Self {
        Self {
            id: Id::new("__toasts"),
            align: Align2::LEFT_TOP,
            offset: Pos2::new(10.0, 10.0),
            direction: Direction::TopDown,
            custom_toast_contents: HashMap::new(),
            added_toasts: Vec::new(),
        }
    }
}

impl Toasts {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new [`Toasts`] instance with a custom id
    ///
    /// This can be useful if you want to have multiple toast groups
    /// in the same UI.
    pub fn with_id(id: Id) -> Self {
        Self {
            id,
            ..Default::default()
        }
    }

    /// Position where the toasts show up.
    ///
    /// The toasts will start from this position and stack up
    /// in the direction specified with [`Self::direction`].
    pub fn position(mut self, position: impl Into<Pos2>) -> Self {
        self.offset = position.into();
        self
    }

    /// Anchor for the toasts.
    ///
    /// For instance, if you set this to (10.0, 10.0) and [`Align2::LEFT_TOP`],
    /// then (10.0, 10.0) will be the top-left corner of the first toast.
    pub fn anchor(mut self, anchor: Align2, offset: impl Into<Pos2>) -> Self {
        self.align = anchor;
        self.offset = offset.into();
        self
    }

    /// Direction where the toasts stack up
    pub fn direction(mut self, direction: impl Into<Direction>) -> Self {
        self.direction = direction.into();
        self
    }

    /// Can be used to specify a custom rendering function for toasts for given kind
    pub fn custom_contents(
        mut self,
        kind: impl Into<ToastKind>,
        add_contents: impl Fn(&mut Ui, &mut Toast) -> Response + Send + Sync + 'static,
    ) -> Self {
        self.custom_toast_contents
            .insert(kind.into(), Arc::new(add_contents));
        self
    }

    /// Add a new toast, or drop it to the log while Do Not Disturb is on.
    pub fn add(&mut self, toast: Toast) -> &mut Self {
        if super::do_not_disturb::silenced() {
            log::debug!("toast silenced by Do Not Disturb: {}", toast.text.text());
            return self;
        }
        self.added_toasts.push(toast);
        self
    }

    /// Show and update all toasts. Returns texts the operator dismissed via close.
    pub fn show(&mut self, ctx: &Context) -> Vec<String> {
        let Self {
            id,
            align,
            mut offset,
            direction,
            ..
        } = *self;

        let dt = ctx.input(|i| i.unstable_dt) as f64;

        let mut toasts: Vec<Toast> = ctx.data_mut(|d| d.get_temp(id).unwrap_or_default());
        toasts.extend(std::mem::take(&mut self.added_toasts));
        toasts.retain(|toast| toast.options.ttl_sec > 0.0);

        // Collapse identical toasts (same kind + text) into one, accumulating an occurrence count.
        let mut deduped: Vec<Toast> = Vec::with_capacity(toasts.len());
        for toast in toasts {
            if let Some(existing) = deduped.iter_mut().find(|t| {
                t.kind == toast.kind
                    && t.text.text() == toast.text.text()
                    && t.payload == toast.payload
            }) {
                existing.options.count = existing.options.count.saturating_add(toast.options.count.max(1));
                existing.options.ttl_sec = existing.options.ttl_sec.max(toast.options.ttl_sec);
            } else {
                deduped.push(toast);
            }
        }
        let mut toasts = deduped;

        for (i, toast) in toasts.iter_mut().enumerate() {
            let response = Area::new(id.with("toast").with(i))
                .anchor(align, offset.to_vec2())
                .order(Order::Foreground)
                .interactable(true)
                .show(ctx, |ui| {
                    if let Some(add_contents) = self.custom_toast_contents.get_mut(&toast.kind) {
                        add_contents(ui, toast)
                    } else {
                        default_toast_contents(ui, toast)
                    };
                })
                .response;

            if !response.hovered() {
                toast.options.ttl_sec -= dt;
                if toast.options.ttl_sec.is_finite() {
                    ctx.request_repaint_after(Duration::from_secs_f64(
                        toast.options.ttl_sec.max(0.0),
                    ));
                }
            }

            if toast.options.show_progress {
                ctx.request_repaint();
            }

            match direction {
                Direction::LeftToRight => {
                    offset.x += response.rect.width() + 10.0;
                }
                Direction::RightToLeft => {
                    offset.x -= response.rect.width() + 10.0;
                }
                Direction::TopDown => {
                    offset.y += response.rect.height() + 10.0;
                }
                Direction::BottomUp => {
                    offset.y -= response.rect.height() + 10.0;
                }
            }
        }

        let mut dismissed = Vec::new();
        toasts.retain(|toast| {
            if toast.options.ttl_sec > 0.0 {
                return true;
            }
            if toast.user_dismissed {
                dismissed.push(toast.text.text().to_string());
            }
            false
        });

        ctx.data_mut(|d| d.insert_temp(id, toasts));
        dismissed
    }
}

fn default_toast_contents(ui: &mut Ui, toast: &mut Toast) -> Response {
    let inner_margin = 10.0;
    let frame = Frame::window(ui.style());
    let response = frame
        .inner_margin(inner_margin)
        .stroke(Stroke::NONE)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let a = |ui: &mut Ui, toast: &mut Toast| {
                    if toast.options.show_icon {
                        ui.label(match toast.kind {
                            ToastKind::Warning => toast.style.warning_icon.clone(),
                            ToastKind::Error => toast.style.error_icon.clone(),
                            ToastKind::Success => toast.style.success_icon.clone(),
                            _ => toast.style.info_icon.clone(),
                        });
                    }
                };
                let b = |ui: &mut Ui, toast: &mut Toast| {
                    let resp = ui.label(toast.text.clone());
                    if toast.options.count > 1 {
                        Frame::new()
                            .fill(Color32::from_rgb(191, 33, 101))
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(Margin::symmetric(6, 1))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(toast.options.count.to_string())
                                        .strong()
                                        .color(Color32::WHITE),
                                );
                            });
                    }
                    resp
                };
                let c = |ui: &mut Ui, toast: &mut Toast| {
                    if ui.button(toast.style.close_button_text.clone()).clicked() {
                        toast.close();
                    }
                };

                // Draw the contents in the reverse order on right-to-left layouts
                // to keep the same look.
                if ui.layout().prefer_right_to_left() {
                    c(ui, toast);
                    b(ui, toast);
                    a(ui, toast);
                } else {
                    a(ui, toast);
                    b(ui, toast);
                    c(ui, toast);
                }
            })
        })
        .response;

    if toast.options.show_progress {
        progress_bar(ui, &response, toast);
    }

    // Draw the frame's stroke last
    let frame_shape = Shape::Rect(RectShape::stroke(
        response.rect,
        frame.corner_radius,
        ui.visuals().window_stroke,
        StrokeKind::Inside
    ));
    ui.painter().add(frame_shape);

    response
}

fn progress_bar(ui: &mut Ui, response: &Response, toast: &Toast) {
    let rounding = CornerRadius {
        nw: 0,
        ne: 0,
        ..ui.visuals().window_corner_radius
    };
    let mut clip_rect = response.rect;
    clip_rect.set_top(clip_rect.bottom() - 2.0);
    clip_rect.set_right(clip_rect.left() + clip_rect.width() * toast.options.progress() as f32);

    ui.painter().with_clip_rect(clip_rect).rect_filled(
        response.rect,
        rounding,
        ui.visuals().text_color(),
    );
}

#[derive(Default, Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub enum ToastKind {
    #[default]
    Info,
    Warning,
    Error,
    Success,
    Custom(u32),
}

impl From<u32> for ToastKind {
    fn from(value: u32) -> ToastKind {
        ToastKind::Custom(value)
    }
}

#[derive(Clone, Default)]
pub struct Toast {
    pub kind: ToastKind,
    pub text: WidgetText,
    pub options: ToastOptions,
    pub style: ToastStyle,
    /// Set when the operator clicks the close button (not on TTL expiry).
    pub user_dismissed: bool,
    /// Opaque key a custom renderer uses to find the state the toast acts on
    /// (e.g. the task id behind an undo button). Also keys dedup, so two
    /// toasts of the same kind and text but different payloads stay separate.
    pub payload: Option<String>,
}

impl Toast {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn kind(mut self, kind: ToastKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn text(mut self, text: impl Into<WidgetText>) -> Self {
        self.text = text.into();
        self
    }

    pub fn options(mut self, options: ToastOptions) -> Self {
        self.options = options;
        self
    }

    pub fn style(mut self, style: ToastStyle) -> Self {
        self.style = style;
        self
    }

    pub fn payload(mut self, payload: impl Into<String>) -> Self {
        self.payload = Some(payload.into());
        self
    }

    /// Restarts the toast's lifetime, e.g. when the action it offers to undo
    /// has been extended by a further edit.
    pub fn refresh(&mut self, ttl: Duration) {
        self.options.ttl_sec = ttl.as_secs_f64();
        self.options.initial_ttl_sec = self.options.ttl_sec;
    }

    /// Close the toast immediately (operator dismiss).
    pub fn close(&mut self) {
        self.user_dismissed = true;
        self.options.ttl_sec = 0.0;
    }
}

#[derive(Clone)]
pub struct ToastStyle {
    pub info_icon: WidgetText,
    pub warning_icon: WidgetText,
    pub error_icon: WidgetText,
    pub success_icon: WidgetText,
    pub close_button_text: WidgetText,
}

impl Default for ToastStyle {
    fn default() -> Self {
        Self {
            info_icon: WidgetText::from("ℹ").color(Color32::from_rgb(0, 155, 255)),
            warning_icon: WidgetText::from("⚠").color(Color32::from_rgb(255, 212, 0)),
            error_icon: WidgetText::from("❗").color(Color32::from_rgb(255, 32, 0)),
            success_icon: WidgetText::from("✔").color(Color32::from_rgb(0, 255, 32)),
            close_button_text: WidgetText::from("🗙"),
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct ToastOptions {
    /// Whether the toast should include an icon.
    pub show_icon: bool,
    /// Whether the toast should visualize the remaining time
    pub show_progress: bool,
    /// The toast is removed when this reaches zero.
    pub(crate) ttl_sec: f64,
    /// Initial value of ttl_sec, used for progress
    pub(crate) initial_ttl_sec: f64,
    /// Number of identical toasts collapsed into this one (shown as a badge when > 1).
    pub(crate) count: u32,
}

impl Default for ToastOptions {
    fn default() -> Self {
        Self {
            show_icon: true,
            show_progress: true,
            ttl_sec: f64::INFINITY,
            initial_ttl_sec: f64::INFINITY,
            count: 1,
        }
    }
}

impl ToastOptions {
    /// Set duration of the toast. [None] duration means the toast never expires.
    pub fn duration(mut self, duration: impl Into<Option<Duration>>) -> Self {
        self.ttl_sec = duration
            .into()
            .map_or(f64::INFINITY, |duration| duration.as_secs_f64());
        self.initial_ttl_sec = self.ttl_sec;
        self
    }

    /// Set duration of the toast in milliseconds.
    pub fn duration_in_millis(self, millis: u64) -> Self {
        self.duration(Duration::from_millis(millis))
    }

    /// Set duration of the toast in seconds.
    pub fn duration_in_seconds(self, secs: f64) -> Self {
        self.duration(Duration::from_secs_f64(secs))
    }

    /// Visualize remaining time using a progress bar.
    pub fn show_progress(mut self, show_progress: bool) -> Self {
        self.show_progress = show_progress;
        self
    }

    /// Show type icon in the toast.
    pub fn show_icon(mut self, show_icon: bool) -> Self {
        self.show_icon = show_icon;
        self
    }

    /// Remaining time of the toast between 1..0
    pub fn progress(self) -> f64 {
        if self.ttl_sec.is_finite() && self.initial_ttl_sec > 0.0 {
            self.ttl_sec / self.initial_ttl_sec
        } else {
            0.0
        }
    }
}