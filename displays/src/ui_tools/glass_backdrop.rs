//! Real GPU backdrop blur behind tinted-glass surfaces, over `backdrop-blur-egui`'s grab-pass path.
//!
//! Two halves that stay separable:
//!
//! - [`GlassParams`] — the theme's glass material (blur radius, film tint, corner radius,
//!   presence), stored on the [`Context`] the same way `theme::set_success_color` stores its
//!   accents. Every preset supplies one; presets without glass supply [`GlassParams::OFF`].
//! - The renderer — a process-global [`GrabPassRenderer`] built once from
//!   `eframe::CreationContext::gl`. Absent on a context that cannot support it (or on mobile,
//!   where the backend is not compiled), in which case every frost is a no-op and glass degrades
//!   to whatever fill the surface already paints.
//!
//! # Draw order
//!
//! A frost grabs the live framebuffer at its rect, so it must be enqueued *before* the surface it
//! sits under.
//!
//! - Floating surfaces (windows, modals) are handled wholesale by [`frost_open_windows`], one call
//!   per frame from the app's root `Ui`. They own their layer and paint their frame and title bar
//!   before any body content, so frosting from inside the body would cover them.
//! - A surface *inside* a panel uses [`glass_frame`]: it frosts last frame's rect into the parent
//!   `Ui`, then shows the [`Frame`], whose fill and stroke land on top of the blurred result.
//!
//! Frosting from inside a panel body blurs that panel's own background, not what is behind it —
//! and nothing is behind a panel anyway.
//!
//! Fade with [`GlassParams::presence`], never `Ui::multiply_opacity` — egui's opacity multiplier
//! does not reach paint callbacks and silently no-ops on the blur.

use eframe::egui::{
    Align2, Area, AsIdSalt, Color32, Context, CornerRadius, FontId, Frame, Id, InnerResponse, Order,
    Rect, Response, Sense, StrokeKind, Ui, Vec2, pos2,
};
use serde::{Deserialize, Serialize};

use crate::ui_tools::theme;

const PARAMS_KEY: &str = "mtech.theme.glass_params";

fn params_id() -> Id {
    Id::new(PARAMS_KEY)
}

/// The glass material a theme asks for: how far the backdrop is smeared, the film painted over it,
/// and how present the whole thing is.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GlassParams {
    /// Whether frosting runs at all. Off means every [`frost`] call is a no-op.
    pub enabled: bool,
    /// Blur radius in logical points. `0` is a plain tinted pane.
    pub blur_radius: f32,
    /// The glass film painted over the blur; its alpha is the mix between film and blurred
    /// backdrop. Alpha `0` is pure blur, alpha `255` hides the blur entirely.
    pub tint: Color32,
    /// Corner rounding of the frosted rect, in logical points.
    pub corner_radius: f32,
    /// Surface-global fade in `[0, 1]` — how present the glass is.
    pub presence: f32,
}

impl GlassParams {
    /// Themes that paint their own opaque chrome.
    pub const OFF: Self = Self {
        enabled: false,
        blur_radius: 0.0,
        tint: Color32::TRANSPARENT,
        corner_radius: 0.0,
        presence: 1.0,
    };

    /// Whether this material would produce anything visible.
    pub fn is_visible(&self) -> bool {
        self.enabled && self.presence > 0.0 && (self.blur_radius > 0.0 || self.tint.a() > 0)
    }
}

impl Default for GlassParams {
    fn default() -> Self {
        Self::OFF
    }
}

/// Stores the active theme's glass material. Live state only — the durable copy travels with the
/// account, inside `SavedTheme`, so a tuned material follows the operator between machines.
pub fn set_params(ctx: &Context, params: GlassParams) {
    ctx.data_mut(|d| d.insert_temp(params_id(), params));
}

/// The active theme's glass material, [`GlassParams::OFF`] when no theme set one.
pub fn params(ctx: &Context) -> GlassParams {
    ctx.data(|d| d.get_temp::<GlassParams>(params_id()))
        .unwrap_or(GlassParams::OFF)
}

/// Whether the GPU backend is built and ready — false on an unsupported context, before
/// [`install`], and on targets with no grab-pass backend.
pub fn is_available() -> bool {
    backend::is_available()
}

/// Build the grab-pass backend from the host's GL context. Call once from the app's
/// `eframe::CreationContext`. Returns whether the backend came up; a failure is logged and leaves
/// every frost a no-op rather than failing the app.
pub fn install(cc: &eframe::CreationContext<'_>) -> bool {
    backend::install(cc)
}

/// Free the backend's GL objects. Call from `eframe::App::on_exit`, where the context is still
/// current. Idempotent.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub fn shutdown(gl: &backdrop_blur_egui::glow::Context) {
    backend::shutdown(gl);
}

/// Enqueue a frost of `rect` using the active theme's material. Returns whether one was enqueued.
///
/// Call this *before* painting the surface that sits over `rect` — the callback grabs whatever is
/// in the framebuffer at its position when it runs.
pub fn frost(ui: &Ui, rect: Rect) -> bool {
    frost_with(ui, rect, params(ui.ctx()))
}

/// [`frost`] with an explicit material, for a surface that overrides the theme (a heavier scrim
/// behind a modal, a tint carrying a status color).
pub fn frost_with(ui: &Ui, rect: Rect, params: GlassParams) -> bool {
    if !params.is_visible() || !rect.is_positive() {
        return false;
    }
    backend::frost(ui, rect, params)
}

/// Frost the backdrop behind every floating surface open this frame. Call once per frame from the
/// app's root `Ui`; returns how many surfaces were frosted.
///
/// A glass theme makes `window_fill` translucent for *every* window, not only the ones a call site
/// remembered to frost — so anything left unfrosted shows sharp content bleeding through it. This
/// sweeps them all instead of relying on per-site opt-in.
///
/// The set is every visible layer above `Order::Background`, minus `Order::Debug`: [`Window`]s at
/// `Middle`, menus and popups at `Foreground`, tooltips above that. All of them draw on
/// `Visuals::window_fill`, which a glass theme makes translucent, so all of them need a pane.
/// Rects come from egui's own area state, i.e. as of last frame — an immediate-mode surface has no
/// rect until it lays out, so it is frosted from its second frame and a resize costs one frame of
/// staleness.
///
/// [`Window`]: eframe::egui::Window
pub fn frost_open_windows(ctx: &Context) -> usize {
    let params = params(ctx);
    if !params.is_visible() || !is_available() {
        return 0;
    }
    // Collected before frosting: each frost opens an Area, which would re-enter `ctx.memory`.
    let surfaces: Vec<(Id, Rect)> = ctx.memory(|m| {
        m.areas()
            .visible_layer_ids()
            .into_iter()
            .filter(|layer| {
                matches!(
                    layer.order,
                    Order::Middle | Order::Foreground | Order::Tooltip
                )
            })
            .filter_map(|layer| m.area_rect(layer.id).map(|rect| (layer.id, rect)))
            .collect()
    });
    surfaces
        .into_iter()
        .filter(|(_, rect)| rect.is_positive())
        .filter(|(id, rect)| frost_behind_floating(ctx, *id, *rect, params))
        .count()
}

/// Frost `rect` behind one floating surface.
///
/// A floating surface owns its layer and paints its frame *and title bar* before any body content,
/// so a frost enqueued from inside the body would blur and then cover them. This puts the frost on
/// its own `Order::Background` layer, which egui drains after every panel and before every
/// `Order::Middle` layer — so it grabs the app content behind the surface regardless of when in the
/// frame it runs.
///
/// Known limitation: every frost therefore runs before *any* floating surface paints, so two
/// overlapping surfaces each blur the panels beneath them and neither blurs the other — the upper
/// one's translucent fill just dims the lower one's sharp content. A menu opened over a window is
/// the same case: its pane is hidden under that window, so it reads as translucent, not frosted.
/// Fixing it needs a frost interleaved between two sibling layers, and egui keeps area-order
/// insertion `pub(crate)`.
fn frost_behind_floating(ctx: &Context, surface_id: Id, rect: Rect, params: GlassParams) -> bool {
    let mut frosted = false;
    Area::new(surface_id.with("mtech.glass.backdrop"))
        .order(Order::Background)
        .interactable(false)
        .movable(false)
        .fixed_pos(rect.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(rect);
            frosted = frost_with(ui, rect, params);
        });
    frosted
}

/// Frost the backdrop behind `frame`, then show it on top of the blurred result.
///
/// Resolves the immediate-mode ordering problem the grab-pass path has: the rect is only known
/// after the content lays out, but the frost has to be enqueued before it paints. This frosts
/// *last* frame's rect, which is stable while the surface is open — a resize costs one frame of
/// staleness.
pub fn glass_frame<R>(
    ui: &mut Ui,
    id_salt: impl AsIdSalt,
    frame: Frame,
    contents: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    let id = ui.id().with(id_salt);
    if let Some(rect) = ui.ctx().data(|d| d.get_temp::<Rect>(id)) {
        frost(ui, rect);
    }
    let inner = frame.show(ui, contents);
    let rect = inner.response.rect;
    ui.ctx().data_mut(|d| d.insert_temp(id, rect));
    inner
}

/// A [`Frame`] shaped for glass: the theme's window stroke and corner radius over a fill weak
/// enough to let the blur read through. Pair it with [`glass_frame`].
///
/// When no backend is live the fill is composited over the window fill instead, so the surface
/// stays legible on a host that cannot blur.
pub fn glass_frame_style(ui: &Ui, params: GlassParams) -> Frame {
    let visuals = ui.visuals();
    let corner_radius = CornerRadius::same(params.corner_radius.round().clamp(0.0, 255.0) as u8);
    let fill = if is_available() && params.is_visible() {
        Color32::TRANSPARENT
    } else {
        visuals.window_fill
    };
    Frame::new()
        .fill(fill)
        .stroke(visuals.window_stroke)
        .corner_radius(corner_radius)
        .inner_margin(6)
}

/// A self-contained sample of the active material: theme-colored blobs with a glass card over
/// them. The app's own panels are flat, and a flat backdrop blurs to itself — this gives the blur
/// something to smear, so the card reads as glass or as a plain tinted pane and says which.
pub fn preview(ui: &mut Ui, params: GlassParams) -> Response {
    let height = 108.0;
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().min(560.0), height),
        Sense::hover(),
    );
    let painter = ui.painter().with_clip_rect(rect);
    let visuals = ui.visuals().clone();
    painter.rect_filled(rect, CornerRadius::same(6), visuals.extreme_bg_color);

    let blobs = [
        theme::info(ui),
        visuals.warn_fg_color,
        theme::success(ui),
        visuals.error_fg_color,
    ];
    for (i, color) in blobs.iter().enumerate() {
        let t = (i as f32 + 0.5) / blobs.len() as f32;
        painter.circle_filled(
            pos2(rect.left() + rect.width() * t, rect.center().y),
            height * 0.40,
            *color,
        );
    }

    // Enqueued between the blobs and the card, so it grabs the blobs and the card lands on top.
    let card = Rect::from_center_size(
        rect.center(),
        Vec2::new(rect.width() * 0.62, height * 0.58),
    );
    let frosted = frost_with(ui, card, params);
    let corner = CornerRadius::same(params.corner_radius.round().clamp(0.0, 255.0) as u8);
    // The theme's own surface fill, so the card shows what a real glass window looks like: blur,
    // then film, then the fill egui paints for every window and popup.
    painter.rect(
        card,
        corner,
        visuals.window_fill,
        visuals.window_stroke,
        StrokeKind::Inside,
    );
    let label = if frosted {
        "GPU backdrop blur"
    } else if is_available() {
        "blur off — tint only"
    } else {
        "no GPU blur on this host"
    };
    painter.text(
        card.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(12.0),
        visuals.text_color(),
    );
    response
}

/// Read and clear the strongest result any frost reached since the last call, logging a wiring
/// warning once if frosts were enqueued but no callback ever ran. Call once per frame.
pub fn poll_outcome() -> Option<FrostReport> {
    backend::poll_outcome()
}

/// What the frosts of one frame achieved, mirroring `backdrop_blur_egui::FrostOutcome` without
/// leaking the backend type into targets that do not compile it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrostReport {
    /// No callback ran — nothing enqueued, or every frosted rect was fully clipped.
    DidNotFire,
    /// A callback ran but its region clipped to nothing. A valid no-op.
    ClippedEmpty,
    /// A frost errored or panicked; details are in the backend's throttled warning.
    Failed,
    /// At least one surface was blurred and composited.
    Composited,
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
mod backend {
    use super::{FrostReport, GlassParams};
    use backdrop_blur_egui::{
        BlurRadius, CornerRadius, FrostOutcome, GrabPassRenderer, Presence, RepaintPolicy, Surface,
        Tint,
    };
    use eframe::egui::{Rect, Ui};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, RwLock};

    /// One GL context per process, and `eframe::App::on_exit` hands back the context without an
    /// egui `Context`, so the renderer lives here rather than in `Context::data`.
    static RENDERER: RwLock<Option<Arc<GrabPassRenderer>>> = RwLock::new(None);
    /// Frosts enqueued since the last poll, so a never-fired callback can be told apart from a
    /// frame that asked for no glass.
    static ENQUEUED: AtomicUsize = AtomicUsize::new(0);
    /// Latches the "enqueued but never fired" warning to once per process.
    static WIRING_WARNED: AtomicBool = AtomicBool::new(false);

    fn renderer() -> Option<Arc<GrabPassRenderer>> {
        RENDERER.read().ok()?.clone()
    }

    pub(super) fn is_available() -> bool {
        RENDERER.read().is_ok_and(|r| r.is_some())
    }

    pub(super) fn install(cc: &eframe::CreationContext<'_>) -> bool {
        // Building a second backend against the same context would strand the first one's GL
        // programs: `shutdown` frees only what the slot holds.
        if is_available() {
            return true;
        }
        let Some(gl) = cc.gl.as_ref() else {
            log::info!("backdrop blur unavailable: eframe is not running the glow backend");
            return false;
        };
        match GrabPassRenderer::new(gl) {
            Ok(renderer) => {
                match RENDERER.write() {
                    Ok(mut slot) => *slot = Some(Arc::new(renderer)),
                    Err(e) => {
                        log::error!("backdrop blur renderer slot poisoned: {e}");
                        return false;
                    }
                }
                log::info!("backdrop blur ready (grab-pass over glow)");
                true
            }
            Err(e) => {
                log::warn!("backdrop blur unavailable: {e}");
                false
            }
        }
    }

    pub(super) fn shutdown(gl: &backdrop_blur_egui::glow::Context) {
        let taken = RENDERER.write().ok().and_then(|mut slot| slot.take());
        if let Some(renderer) = taken {
            renderer.destroy(gl);
        }
    }

    pub(super) fn frost(ui: &Ui, rect: Rect, params: GlassParams) -> bool {
        let Some(renderer) = renderer() else {
            return false;
        };
        renderer.frost(
            ui,
            Surface {
                rect,
                blur_radius: BlurRadius::new(params.blur_radius),
                tint: Tint::from_srgb_unmultiplied(params.tint.to_srgba_unmultiplied()),
                corner_radius: CornerRadius::new(params.corner_radius),
                presence: Presence::new(params.presence),
                // The backdrop is this app's own content, so egui already repaints whenever it
                // changes; asking for more would spin the event loop for nothing.
                repaint: RepaintPolicy::Static,
            },
        );
        ENQUEUED.fetch_add(1, Ordering::Relaxed);
        true
    }

    pub(super) fn poll_outcome() -> Option<FrostReport> {
        let renderer = renderer()?;
        let enqueued = ENQUEUED.swap(0, Ordering::Relaxed);
        let outcome = renderer.take_frost_outcome();
        if enqueued > 0
            && outcome == FrostOutcome::DidNotFire
            && !WIRING_WARNED.swap(true, Ordering::Relaxed)
        {
            log::warn!(
                "backdrop blur enqueued {enqueued} surface(s) but no paint callback ran; \
                 the renderer is wired against a different egui_glow than the host paints with"
            );
        }
        Some(match outcome {
            FrostOutcome::DidNotFire => FrostReport::DidNotFire,
            FrostOutcome::ClippedEmpty => FrostReport::ClippedEmpty,
            FrostOutcome::Failed => FrostReport::Failed,
            FrostOutcome::Composited => FrostReport::Composited,
        })
    }
}

/// iOS/Android build eframe without a renderer, so there is no GL context to grab from and the
/// backend is not a dependency. Glass degrades to the fills surfaces already paint.
#[cfg(any(target_os = "ios", target_os = "android"))]
mod backend {
    use super::{FrostReport, GlassParams};
    use eframe::egui::{Rect, Ui};

    pub(super) fn is_available() -> bool {
        false
    }

    pub(super) fn install(_cc: &eframe::CreationContext<'_>) -> bool {
        false
    }

    pub(super) fn frost(_ui: &Ui, _rect: Rect, _params: GlassParams) -> bool {
        false
    }

    pub(super) fn poll_outcome() -> Option<FrostReport> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_is_never_visible_and_enabled_needs_a_dial_turned_up() {
        assert!(!GlassParams::OFF.is_visible());

        let mut params = GlassParams {
            enabled: true,
            blur_radius: 0.0,
            tint: Color32::TRANSPARENT,
            corner_radius: 8.0,
            presence: 1.0,
        };
        // Enabled but nothing to draw: no blur, no film.
        assert!(!params.is_visible());
        params.blur_radius = 18.0;
        assert!(params.is_visible());
        // A fully faded surface draws nothing however wide the blur.
        params.presence = 0.0;
        assert!(!params.is_visible());
    }

    #[test]
    fn a_pure_film_with_no_blur_is_still_visible() {
        let params = GlassParams {
            enabled: true,
            blur_radius: 0.0,
            tint: Color32::from_rgba_unmultiplied(20, 18, 40, 90),
            ..GlassParams::OFF
        };
        assert!(params.is_visible());
    }

    // The theme stores its material on the Context; a context with no theme applied reads OFF.
    #[test]
    fn params_round_trip_through_the_context() {
        let ctx = Context::default();
        assert_eq!(params(&ctx), GlassParams::OFF);

        let applied = GlassParams {
            enabled: true,
            blur_radius: 24.0,
            tint: Color32::from_rgba_unmultiplied(12, 10, 24, 110),
            corner_radius: 10.0,
            presence: 0.9,
        };
        set_params(&ctx, applied);
        assert_eq!(params(&ctx), applied);
    }
}
