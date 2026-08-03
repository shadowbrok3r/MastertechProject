//! The grab-pass adapter: frosted glass for an `eframe`-on-glow app (and the `cage` kiosk). Unlike
//! the own-loop path, the host owns the GL loop; this adapter rides egui's **paint callback** —
//! egui_glow invokes a closure with the live `glow::Context` mid-frame, after the backdrop is drawn
//! and before the panel's foreground — and inside it grabs the backdrop, blurs it, and composites
//! the frosted surface back into the same framebuffer.
//!
//! This crate stays `#![forbid(unsafe_code)]`: all GL is in `backdrop-blur-glow`. The one thing a
//! safe adapter cannot do — capture `GL_DRAW_FRAMEBUFFER_BINDING` — is the glow crate's safe
//! [`current_draw_framebuffer`] helper.
//!
//! # The load-bearing construction (DESIGN §5)
//!
//! [`callback_region`] turns egui's paint-callback geometry into the **GL bottom-left** region to
//! grab and composite. egui's `ViewportInPixels` already carries `from_bottom_px` (GL-origin), so
//! the mapping is a field read + an `i32 → u32` cast — **no `height − y` flip**. It is a pure
//! function, unit-tested below, so the orientation wiring is proven without a GPU.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use backdrop_blur_core::{BlurError, BlurRequest, FrostEffect, GlRegion, RepaintPolicy, Scale};
use backdrop_blur_glow::{FramebufferSize, GlowBlur, current_draw_framebuffer};

use crate::Surface;

/// The strongest thing any frost callback achieved since the host last took the report
/// (via [`GrabPassRenderer::take_frost_outcome`]). Adapter vocabulary, deliberately not in
/// core: [`DidNotFire`](Self::DidNotFire) means "egui never invoked my paint callback",
/// a concept the seam cannot see.
///
/// The discriminants order the variants weakest→strongest; the callback records via
/// `fetch_max`, so a multi-surface frame reports the best result any surface reached.
/// **The asymmetry is deliberate:** a composited surface masks a same-frame
/// [`Failed`](Self::Failed) in this value — this answers *"did the glass appear?"* (the
/// wiring/version-skew signal), it is **not** a failure detector. The once-per-episode
/// `log::warn!` in the callback is the failure channel.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrostOutcome {
    /// No frost paint callback ran. Either nothing was enqueued, or egui skipped the
    /// callback (fully clipped/offscreen rect) — or, under `panic = "unwind"` only, the
    /// process never got to record anything (see the private `FrostGuard`'s abort caveat).
    DidNotFire = 0,
    /// A callback fired but there was nothing to draw: the region clipped to nothing at
    /// the adapter (`callback_region` → `None`) or at the backend
    /// ([`FrostEffect::ClippedEmpty`]). A valid no-op, not an error.
    ClippedEmpty = 1,
    /// A callback fired and the frost failed — [`BlurError`] from the backend, or a panic
    /// mid-frost (recorded by the drop guard during unwind). Best-effort: the warning is
    /// throttled to once per failure episode.
    Failed = 2,
    /// At least one frosted surface was blurred and composited into the framebuffer.
    Composited = 3,
}

impl FrostOutcome {
    /// Decode the raw atomic value. Total by construction — every store site casts one of
    /// the four named discriminants, so `4..=255` is unreachable in practice; the crate
    /// forbids `unsafe` (no transmute) and the contract forbids panicking on a value we
    /// ourselves never wrote, so the catch-all folds to the reset value.
    fn from_raw(raw: u8) -> Self {
        match raw {
            1 => Self::ClippedEmpty,
            2 => Self::Failed,
            3 => Self::Composited,
            // 0 is the reset value; anything else is unreachable by construction (see above).
            _ => Self::DidNotFire,
        }
    }
}

impl From<FrostEffect> for FrostOutcome {
    fn from(effect: FrostEffect) -> Self {
        match effect {
            FrostEffect::Composited => Self::Composited,
            FrostEffect::ClippedEmpty => Self::ClippedEmpty,
        }
    }
}

/// Panic honesty for the outcome report: armed at callback entry (the successor of the old
/// set-`ran`-at-entry protocol), committed by exactly one exit path. If the callback unwinds
/// before a commit — the mutex-poisoning scenario the recovery below anticipates — `Drop`
/// records [`FrostOutcome::Failed`] during unwind, so a fired-then-panicked callback is never
/// reported as [`FrostOutcome::DidNotFire`]. A pre-stored `Failed` floor would not work:
/// `fetch_max` can only raise, so it would corrupt a legitimate `ClippedEmpty`.
///
/// Caveat: under `panic = "abort"` no destructor runs — the guard is unwind-only honesty.
struct FrostGuard<'a> {
    outcome: &'a AtomicU8,
    committed: bool,
}

impl<'a> FrostGuard<'a> {
    fn arm(outcome: &'a AtomicU8) -> Self {
        Self {
            outcome,
            committed: false,
        }
    }

    /// Record this exit path's outcome and disarm the panic fallback. `fetch_max` keeps the
    /// strongest outcome across the frame's callbacks; `Relaxed` is sound because these are
    /// RMWs on a single atomic and nothing else is published through it (frame data
    /// synchronizes via the `Mutex` and the GL stream) — inferring *other* state from an
    /// observed value would require upgrading to `Release`/`Acquire`.
    fn commit(&mut self, outcome: FrostOutcome) {
        self.outcome.fetch_max(outcome as u8, Ordering::Relaxed);
        self.committed = true;
    }
}

impl Drop for FrostGuard<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.outcome
                .fetch_max(FrostOutcome::Failed as u8, Ordering::Relaxed);
        }
    }
}

/// The frost callback's one fallible exit: commits the frost result's outcome to the guard and
/// manages the once-per-episode warn latch — success clears the latch (the episode, if any,
/// ended), the first failure of an episode sets it, later failures stay silent. The warn decision
/// is the RETURN VALUE, not a side effect: `Some(error)` means "first failure of an episode —
/// warn with it" (the callback logs it), `None` means stay silent (success, or a repeat failure
/// inside an episode). Pure control flow — no GL, no logging — so the episode contract is
/// unit-tested below against the returned decision itself.
fn commit_frost_result(
    result: Result<FrostEffect, BlurError>,
    outcome_guard: &mut FrostGuard<'_>,
    warned: &AtomicBool,
) -> Option<BlurError> {
    match result {
        // Recovered (or never failed) — re-arm the once-per-episode failure warning.
        // Any `FrostEffect` re-arms (composited or clipped-empty), matching the pre-widening
        // any-Ok behavior; the outcome still distinguishes the two for the host.
        Ok(effect) => {
            warned.store(false, Ordering::Relaxed);
            outcome_guard.commit(FrostOutcome::from(effect));
            None
        }
        // A paint callback cannot propagate an error; the frost is best-effort. Warn ONCE per
        // failure episode (GLOW_DESIGN §7): under RepaintPolicy::Live this runs every frame,
        // so an unthrottled warn floods a kiosk log. Only the first bad frame of an episode
        // surfaces the error to the caller's `log::warn!`.
        Err(e) => {
            outcome_guard.commit(FrostOutcome::Failed);
            (!warned.swap(true, Ordering::Relaxed)).then_some(e)
        }
    }
}

/// Drives the grab-pass (eframe-on-glow) frosted-glass path: holds the glow backend behind a mutex
/// (the paint callback is `Fn + Send + Sync`, so it cannot own `&mut`) and enqueues a paint callback
/// per frosted [`Surface`].
///
/// # Lifecycle
///
/// Build once from the host's `glow::Context` (e.g. `eframe::CreationContext::gl`). Call
/// [`destroy`](Self::destroy) from `eframe::App::on_exit` (where `Frame::gl()` is still current) so
/// the backend's GL objects are freed while the context lives — never in `Drop` (DESIGN §11).
pub struct GrabPassRenderer {
    blur: Arc<Mutex<GlowBlur>>,
    /// The strongest [`FrostOutcome`] any frost callback recorded since the host last took it
    /// (`fetch_max`-encoded `u8`; the result cannot be returned synchronously from a paint
    /// callback — GLOW_DESIGN §7). Each callback's exit path commits its outcome; a
    /// [`FrostGuard`] records `Failed` if a callback unwinds first. The host polls
    /// [`take_frost_outcome`](Self::take_frost_outcome) once per frame, after paint.
    outcome: Arc<AtomicU8>,
    /// Throttles the best-effort frost-failure warning to once per failure episode: set the first
    /// time a [`frost`](Self::frost) callback errors, cleared on the next success. Under
    /// [`RepaintPolicy::Live`] the callback runs every frame, so without this latch a
    /// persistently-failing context would flood the log (DESIGN §7's "warn once").
    warned: Arc<AtomicBool>,
    /// The thread the renderer was built on — the documented eframe wiring constructs on the
    /// paint thread (`CreationContext` runs there), and off-thread GL is undefined behavior.
    /// Each frost callback `debug_assert`s it runs on this thread (GLOW_DESIGN §6).
    paint_thread: std::thread::ThreadId,
}

impl GrabPassRenderer {
    /// Build the backend against the host's shared GL context (probe its capabilities, compile the
    /// programs). Returns [`BlurError::UnsupportedContext`] if the context is below the backend's
    /// minimums — desktop GL 3.3, GLES 3.0, or WebGL 2.0 (raised inside [`GlowBlur::new`]).
    ///
    /// **Precondition:** construct on the paint thread (the documented eframe wiring builds from
    /// `eframe::CreationContext::gl`, which runs there) — the frost callbacks `debug_assert` they
    /// run on the construction thread, because off-thread GL is undefined behavior.
    ///
    /// [`BlurError::UnsupportedContext`]: backdrop_blur_core::BlurError::UnsupportedContext
    pub fn new(gl: &Arc<glow::Context>) -> Result<Self, BlurError> {
        let blur = GlowBlur::new(gl)?;
        Ok(Self {
            blur: Arc::new(Mutex::new(blur)),
            outcome: Arc::new(AtomicU8::new(FrostOutcome::DidNotFire as u8)),
            warned: Arc::new(AtomicBool::new(false)),
            paint_thread: std::thread::current().id(),
        })
    }

    /// Free the backend's GL objects. Call from `eframe::App::on_exit` while the context is current
    /// (DESIGN §11). Idempotent. Recovers a poisoned lock so the GL objects are still freed after a
    /// prior frost panic — otherwise they would leak (DESIGN §6/§12: the poisoned path is recovered,
    /// never silently dropped).
    pub fn destroy(&self, gl: &glow::Context) {
        let mut guard = match self.blur.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                log::warn!(
                    "backdrop-blur grab-pass mutex was poisoned by a prior panic; recovered during destroy"
                );
                self.blur.clear_poison();
                poisoned.into_inner()
            }
        };
        guard.destroy(gl);
    }

    /// The strongest [`FrostOutcome`] any frost callback recorded **since the last call** —
    /// "take" because it reads **and resets** (to [`FrostOutcome::DidNotFire`]). Call it once
    /// per frame, after paint: skipping a frame lets a stale `Composited` mask a later
    /// `Failed`, since the report only ratchets upward between takes.
    ///
    /// What each variant means for the host: `DidNotFire` — egui never invoked the callback
    /// (nothing enqueued, or the rect was fully clipped; the wiring/version-skew check);
    /// `ClippedEmpty` — a callback fired but the region clipped to nothing, or the request's
    /// `target_rect` was zero-area (`Region::is_empty`; a valid no-op either way);
    /// `Failed` — a frost errored or panicked (details in the throttled `log::warn!`);
    /// `Composited` — at least one frosted surface actually painted. A `Composited` report
    /// masks a same-frame `Failed` by design — see [`FrostOutcome`].
    pub fn take_frost_outcome(&self) -> FrostOutcome {
        FrostOutcome::from_raw(
            self.outcome
                .swap(FrostOutcome::DidNotFire as u8, Ordering::Relaxed),
        )
    }

    /// Enqueue a frosted [`Surface`] for this frame: drive its repaint policy and add a paint
    /// callback that, mid-frame, grabs the backdrop behind `surface.rect`, blurs it, and composites
    /// the frosted surface back. The crate owns only the background; the surface's foreground is the
    /// host's (painted in its own later pass), and a frosted surface adds nothing to the AccessKit
    /// tree.
    ///
    /// # Contract (see the crate-root "Grab-pass contracts")
    ///
    /// - **Call this *before* painting the surface's foreground.** The callback grabs whatever is in
    ///   the framebuffer at its position; content drawn after it lands on top of the blur, content
    ///   drawn before it gets blurred away. There is no runtime guard — order is on the caller.
    /// - **Fade via `surface.presence`, not `ui.multiply_opacity`.** egui's opacity multiplier does
    ///   not reach paint callbacks and silently no-ops on the blur; [`Presence`] is the supported
    ///   fade dial.
    /// - **For a dynamically-sized surface, pass *last frame's* rect** (stashed in egui temp memory):
    ///   the rect is unknown until content lays out, but the frost must be enqueued before it paints.
    /// - After the frame, [`take_frost_outcome`](Self::take_frost_outcome) reports the strongest
    ///   [`FrostOutcome`] any frost reached since the last take (read-and-clear; call once per
    ///   frame, after paint).
    ///
    /// [`Presence`]: backdrop_blur_core::Presence
    pub fn frost(&self, ui: &egui::Ui, surface: Surface) {
        // The adapter drives liveness — a stale backdrop cannot be silently forgotten (DESIGN §4.6).
        match surface.repaint {
            RepaintPolicy::Live => ui.ctx().request_repaint(),
            RepaintPolicy::Bounded(after) => ui.ctx().request_repaint_after(after),
            RepaintPolicy::Static => {}
        }

        let blur = Arc::clone(&self.blur);
        let outcome = Arc::clone(&self.outcome);
        let warned = Arc::clone(&self.warned);
        let paint_thread = self.paint_thread;
        let callback = egui_glow::CallbackFn::new(move |info, painter| {
            // Armed at callback ENTRY (GLOW_DESIGN §7), the successor of the old set-`ran`-at-entry
            // flag: every exit path below commits its outcome, and a panic before a commit is
            // recorded as `Failed` by the guard's Drop — so a fired callback can never read back
            // as `DidNotFire`, even when it clips to nothing or dies mid-frost.
            let mut outcome_guard = FrostGuard::arm(&outcome);
            debug_assert_eq!(
                std::thread::current().id(),
                paint_thread,
                "grab-pass paint callbacks must run on the thread the renderer was created on \
                 (off-thread GL is undefined behavior)"
            );
            let gl = painter.gl();

            let Some((region, framebuffer_size)) = callback_region(&info) else {
                // The panel clipped to nothing — a valid no-op. Deliberately does NOT touch
                // `warned`: this path never reached the frost, so it is no evidence a failure
                // episode ended; re-arming here would reintroduce the per-frame log flood the
                // latch exists to prevent.
                outcome_guard.commit(FrostOutcome::ClippedEmpty);
                return;
            };
            // A poisoned lock means a prior frost panicked mid-blur. GlowBlur is a bag of GL handles
            // with no invariant a panic can corrupt, so clear the poison and recover the guard rather
            // than leave frost permanently dead and silent (DESIGN §6/§12: log_err + flat fallback,
            // never silently skipped). clear_poison makes the warning self-limiting — once per panic,
            // not once per frame under Live.
            let mut guard = match blur.lock() {
                Ok(guard) => guard,
                Err(poisoned) => {
                    log::warn!(
                        "backdrop-blur grab-pass mutex was poisoned by a prior panic; recovered"
                    );
                    blur.clear_poison();
                    poisoned.into_inner()
                }
            };

            // Material off the surface, geometry from the GL-origin region; both source and target
            // are the same on-screen rect (v1: the backdrop directly behind the glass).
            let request = BlurRequest {
                source_region: region.into_region(),
                target_rect: region.into_region(),
                blur_radius: surface.blur_radius,
                tint: surface.tint,
                corner_radius: surface.corner_radius,
                presence: surface.presence,
            };
            let target = current_draw_framebuffer(gl);
            if let Some(e) = commit_frost_result(
                guard.frost_region(gl, target, region, framebuffer_size, &request),
                &mut outcome_guard,
                &warned,
            ) {
                log::warn!("backdrop-blur grab-pass frost failed: {e}");
            }
        });

        ui.painter().add(egui::PaintCallback {
            rect: surface.rect,
            callback: Arc::new(callback),
        });
    }
}

/// Turn egui's paint-callback geometry into the **GL bottom-left** region to grab + composite, plus
/// the full framebuffer size (the composite viewport). Pure — no GL — so the orientation wiring is
/// unit-tested. The region is `viewport ∩ clip_rect ∩ framebuffer`; `None` when that is empty (a
/// no-op). egui's `from_bottom_px` is already GL-origin, so this never computes a `height − y` flip.
fn callback_region(info: &egui::PaintCallbackInfo) -> Option<(GlRegion, FramebufferSize)> {
    let ppp = info.pixels_per_point;
    let to_gl = |v: &egui::epaint::ViewportInPixels| {
        GlRegion::from_bottom_px(
            [v.left_px.max(0) as u32, v.from_bottom_px.max(0) as u32],
            [v.width_px.max(0) as u32, v.height_px.max(0) as u32],
            Scale::new(ppp),
        )
    };
    let viewport = to_gl(&info.viewport_in_pixels());
    let clip = to_gl(&info.clip_rect_in_pixels());
    let region = viewport.intersect(&clip)?.clip_to(info.screen_size_px)?;
    Some((region, FramebufferSize(info.screen_size_px)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(
        viewport: egui::Rect,
        clip: egui::Rect,
        screen: [u32; 2],
        ppp: f32,
    ) -> egui::PaintCallbackInfo {
        egui::PaintCallbackInfo {
            viewport,
            clip_rect: clip,
            pixels_per_point: ppp,
            screen_size_px: screen,
        }
    }

    fn rect(min: (f32, f32), size: (f32, f32)) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(min.0, min.1), egui::vec2(size.0, size.1))
    }

    #[test]
    fn callback_region_maps_egui_top_to_gl_high_y_no_flip() {
        // A 800x600 screen; a panel flush with egui's TOP (y 0..50). In GL bottom-left, the top of
        // the screen is HIGH y: from_bottom = 600 - 50 - 0 = 550. This is the whole orientation
        // contract (DESIGN §5) — proven through egui's own from_bottom_px, with no manual flip.
        let panel = rect((0.0, 0.0), (100.0, 50.0));
        let (region, fb) =
            callback_region(&info(panel, panel, [800, 600], 1.0)).expect("non-empty");
        assert_eq!(region.origin_bl(), [0, 550]);
        assert_eq!(region.size(), [100, 50]);
        assert_eq!(fb, FramebufferSize([800, 600]));
    }

    #[test]
    fn callback_region_maps_egui_bottom_to_gl_low_y() {
        // A panel flush with egui's BOTTOM (y 550..600) → GL from_bottom = 600 - 50 - 550 = 0.
        let panel = rect((0.0, 550.0), (100.0, 50.0));
        let (region, _) = callback_region(&info(panel, panel, [800, 600], 1.0)).expect("non-empty");
        assert_eq!(region.origin_bl(), [0, 0]);
        assert_eq!(region.size(), [100, 50]);
    }

    #[test]
    fn callback_region_scales_by_pixels_per_point() {
        // ppp = 2.0: a 100x50-pt panel at (10,10) → 200x100 px at left=20; from_bottom = 1200-100-20.
        let panel = rect((10.0, 10.0), (100.0, 50.0));
        let (region, fb) =
            callback_region(&info(panel, panel, [1600, 1200], 2.0)).expect("non-empty");
        assert_eq!(region.size(), [200, 100]);
        assert_eq!(region.origin_bl(), [20, 1200 - 100 - 20]);
        assert_eq!(fb, FramebufferSize([1600, 1200]));
    }

    #[test]
    fn callback_region_is_none_when_clip_is_disjoint() {
        // The clip rect shares no area with the viewport → the panel paints nothing.
        let viewport = rect((0.0, 0.0), (100.0, 100.0));
        let clip = rect((400.0, 400.0), (50.0, 50.0));
        assert!(callback_region(&info(viewport, clip, [800, 600], 1.0)).is_none());
    }

    #[test]
    fn from_raw_round_trips_every_discriminant() {
        for outcome in [
            FrostOutcome::DidNotFire,
            FrostOutcome::ClippedEmpty,
            FrostOutcome::Failed,
            FrostOutcome::Composited,
        ] {
            assert_eq!(FrostOutcome::from_raw(outcome as u8), outcome);
        }
    }

    #[test]
    fn from_raw_folds_an_out_of_range_value_to_did_not_fire() {
        // Unreachable by construction (every store casts a named discriminant), but the decode
        // is total: an unknown raw value folds to the reset value rather than panicking.
        assert_eq!(FrostOutcome::from_raw(4), FrostOutcome::DidNotFire);
        assert_eq!(FrostOutcome::from_raw(u8::MAX), FrostOutcome::DidNotFire);
    }

    #[test]
    fn from_frost_effect_preserves_the_variant() {
        assert_eq!(
            FrostOutcome::from(FrostEffect::Composited),
            FrostOutcome::Composited
        );
        assert_eq!(
            FrostOutcome::from(FrostEffect::ClippedEmpty),
            FrostOutcome::ClippedEmpty
        );
    }

    #[test]
    fn frost_guard_records_failed_when_dropped_uncommitted() {
        // The panic path: the guard unwinds without a commit → Failed, but never lowers an
        // already-committed stronger outcome (fetch_max only raises).
        let outcome = AtomicU8::new(FrostOutcome::DidNotFire as u8);
        drop(FrostGuard::arm(&outcome));
        assert_eq!(
            FrostOutcome::from_raw(outcome.load(Ordering::Relaxed)),
            FrostOutcome::Failed
        );

        let outcome = AtomicU8::new(FrostOutcome::DidNotFire as u8);
        let mut armed = FrostGuard::arm(&outcome);
        armed.commit(FrostOutcome::Composited);
        drop(armed);
        assert_eq!(
            FrostOutcome::from_raw(outcome.load(Ordering::Relaxed)),
            FrostOutcome::Composited
        );
    }

    #[test]
    fn commit_ratchets_to_the_strongest_outcome() {
        // Multi-surface frame: a clipped surface cannot mask a composited one.
        let outcome = AtomicU8::new(FrostOutcome::DidNotFire as u8);
        FrostGuard::arm(&outcome).commit(FrostOutcome::Composited);
        FrostGuard::arm(&outcome).commit(FrostOutcome::ClippedEmpty);
        assert_eq!(
            FrostOutcome::from_raw(outcome.load(Ordering::Relaxed)),
            FrostOutcome::Composited
        );
    }

    fn synthetic_failure() -> Result<FrostEffect, BlurError> {
        Err(BlurError::UnsupportedContext {
            detail: "synthetic".into(),
        })
    }

    #[test]
    fn commit_frost_result_warns_once_per_failure_episode() {
        // The full episode contract, asserted on the RETURNED warn decision (a fresh armed guard
        // per call, as in the real flow): fail → fail → ok → fail must decide Some, None, None,
        // Some — the first failure of each episode warns, the repeat stays silent, and a success
        // re-arms the latch for the next episode. Asserting the decision itself (not just the
        // latch) is what makes an inverted warn condition fail this test.
        let outcome = AtomicU8::new(FrostOutcome::DidNotFire as u8);
        let warned = AtomicBool::new(false);

        let mut guard = FrostGuard::arm(&outcome);
        assert!(commit_frost_result(synthetic_failure(), &mut guard, &warned).is_some());
        drop(guard);
        assert!(warned.load(Ordering::Relaxed));

        let mut guard = FrostGuard::arm(&outcome);
        assert!(commit_frost_result(synthetic_failure(), &mut guard, &warned).is_none());
        drop(guard);
        assert!(warned.load(Ordering::Relaxed));
        assert_eq!(
            FrostOutcome::from_raw(outcome.load(Ordering::Relaxed)),
            FrostOutcome::Failed
        );

        let mut guard = FrostGuard::arm(&outcome);
        assert!(commit_frost_result(Ok(FrostEffect::Composited), &mut guard, &warned).is_none());
        drop(guard);
        assert!(!warned.load(Ordering::Relaxed));

        let mut guard = FrostGuard::arm(&outcome);
        assert!(commit_frost_result(synthetic_failure(), &mut guard, &warned).is_some());
        drop(guard);
        assert!(warned.load(Ordering::Relaxed));
    }

    #[test]
    fn commit_frost_result_clears_the_latch_on_success() {
        // An ongoing failure episode (latch set): a success ends it — the latch clears (re-arming
        // the warning for the next episode), the decision is silent (`None` — success never
        // warns), and the outcome ratchets to Composited.
        let outcome = AtomicU8::new(FrostOutcome::DidNotFire as u8);
        let warned = AtomicBool::new(true);

        let mut guard = FrostGuard::arm(&outcome);
        assert!(commit_frost_result(Ok(FrostEffect::Composited), &mut guard, &warned).is_none());
        assert!(!warned.load(Ordering::Relaxed));
        assert_eq!(
            outcome.load(Ordering::Relaxed),
            FrostOutcome::Composited as u8
        );
    }

    #[test]
    fn commit_frost_result_routes_a_clipped_empty_effect() {
        // The Ok arm must route the ACTUAL effect, not hard-code Composited: an
        // `Ok(FrostEffect::ClippedEmpty)` commit leaves the atomic at ClippedEmpty.
        let outcome = AtomicU8::new(FrostOutcome::DidNotFire as u8);
        let warned = AtomicBool::new(false);

        let mut guard = FrostGuard::arm(&outcome);
        assert!(commit_frost_result(Ok(FrostEffect::ClippedEmpty), &mut guard, &warned).is_none());
        assert_eq!(
            outcome.load(Ordering::Relaxed),
            FrostOutcome::ClippedEmpty as u8
        );
    }
}

/// Tier-1 control-flow tests for the renderer against a **real** surfaceless GL context (the glow
/// crate's shared `gl_harness`): construction, idempotent teardown, poisoned-lock recovery, and the
/// take-outcome read-and-clear semantics. The paint CALLBACK itself needs a live egui+glow paint
/// pass and stays untested here (see GLOW_IMPL's "integration test not built" note). One fresh
/// harness context per test; CI runs with `--test-threads=1`.
#[cfg(all(test, feature = "gl-snapshots", not(target_arch = "wasm32")))]
mod gl_tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::atomic::Ordering;

    use backdrop_blur_glow::gl_harness::headless_gl;

    use super::*;

    #[test]
    fn new_succeeds_on_the_headless_context() {
        let harness = headless_gl();
        let gl = harness.context_arc();
        let renderer = GrabPassRenderer::new(&gl).expect("new");
        // Teardown before the harness Drop: the renderer owns GL objects that must be freed while
        // the EGL context is still alive.
        renderer.destroy(&gl);
    }

    #[test]
    fn destroy_is_idempotent() {
        let harness = headless_gl();
        let gl = harness.context_arc();
        let renderer = GrabPassRenderer::new(&gl).expect("new");
        renderer.destroy(&gl);
        // The second destroy must be a no-op via GlowBlur's destroyed flag — not panicking IS the
        // assertion.
        renderer.destroy(&gl);
    }

    #[test]
    fn destroy_recovers_a_poisoned_lock() {
        let harness = headless_gl();
        let gl = harness.context_arc();
        let renderer = GrabPassRenderer::new(&gl).expect("new");

        // Poison the blur mutex the way a mid-frost panic would: unwind while holding the guard.
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _g = renderer.blur.lock().expect("pre-poison lock");
            panic!("poison the mutex");
        }));
        assert!(renderer.blur.is_poisoned());

        // The recovery path: destroy must clear the poison and still free the GL objects.
        renderer.destroy(&gl);
        assert!(!renderer.blur.is_poisoned());
    }

    #[test]
    fn take_frost_outcome_reads_and_clears_on_a_live_renderer() {
        let harness = headless_gl();
        let gl = harness.context_arc();
        let renderer = GrabPassRenderer::new(&gl).expect("new");

        renderer
            .outcome
            .store(FrostOutcome::Composited as u8, Ordering::Relaxed);
        assert_eq!(renderer.take_frost_outcome(), FrostOutcome::Composited);
        assert_eq!(renderer.take_frost_outcome(), FrostOutcome::DidNotFire);

        renderer.destroy(&gl);
    }
}
