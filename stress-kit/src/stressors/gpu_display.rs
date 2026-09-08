//! Display-path stressor: a real swapchain per attached output, presented
//! continuously, with periodic surface reconfiguration and desktop mode
//! changes. Reports aggregate presented FPS.
//!
//! Each output gets its own logical device on the shared adapter, and its
//! swapchain rebuilds run on a thread of their own. `Surface::configure` waits
//! for its device to go idle — present queue included — with no timeout, so a
//! shared device makes every rebuild wait on flips the parked siblings have
//! stopped draining, and a rebuild that blocks anyway takes the stage with it.
//! A configure that still does not return is bounded, and the output it
//! belongs to is rebuilt — fresh window, device, swapchain and worker — with
//! the stuck call left on its own thread, so the siblings and the output carry
//! on and the stage is graded on what it presented.
//!
//! Every other GPU stressor in this crate is a compute shader — it never
//! creates a surface, never presents, and never touches the flip queue, so it
//! cannot reproduce a present/mode-set timeout (dxgkrnl `0x1b8`, `0x141`, AMD
//! Crash Defender watchdog live dumps). This one drives that path instead.
//!
//! `STRESSKIT_DISPLAY_DEBUG_WEDGE=<output>[:<frames>]` wedges one output thread
//! on purpose, for verifying the watchdog on real multi-output hardware.
//!
//! Desktop mode changes are controlled by `STRESSKIT_DISPLAY_MODESET`:
//! `refresh` (default) cycles refresh rates at the native resolution, `full`
//! also cycles resolutions, `off` leaves the desktop mode alone. Changes are
//! applied with `CDS_FULLSCREEN` so Windows restores them if the process dies.

#![cfg(feature = "gpu")]

use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};
use std::time::Instant;

#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(target_os = "windows")]
use std::time::Duration;

use crate::Metrics;

use super::gpu_common::run_unsupported;
#[cfg(target_os = "windows")]
use super::gpu_common::{emit_fatal_tick, emit_tick, TICK};

/// Wall-clock without a presented frame before an output is called stalled.
#[cfg(target_os = "windows")]
const STALL_WARN: Duration = Duration::from_secs(5);
/// Wall-clock without a presented frame that ends the stage.
#[cfg(target_os = "windows")]
const STALL_FATAL: Duration = Duration::from_secs(30);
/// Gap between self-inflicted swapchain reconfigurations on one output.
#[cfg(target_os = "windows")]
const RECONFIGURE_EVERY: Duration = Duration::from_secs(4);
/// Gap between desktop mode changes on one output.
#[cfg(target_os = "windows")]
const MODE_SET_EVERY: Duration = Duration::from_secs(12);
/// How long a configuring thread waits for its siblings to stop submitting.
#[cfg(target_os = "windows")]
const QUIESCE_TIMEOUT: Duration = Duration::from_secs(2);
/// Upper bound on a parked thread's wait. Deliberately longer than
/// [`WATCHDOG_STALL`]: a configure that outlasts it is wedged, and the
/// watchdog must end the stage rather than a parked sibling resuming into a
/// half-built swapchain.
#[cfg(target_os = "windows")]
const QUIESCE_PARK_MAX: Duration = Duration::from_secs(45);
/// Spin gap while waiting on the quiesce handshake.
#[cfg(target_os = "windows")]
const QUIESCE_POLL: Duration = Duration::from_micros(200);
/// Bound on waiting for the configure turn. A sibling that holds it longer is
/// itself wedged, so the caller gives up and retries from its frame loop
/// instead of blocking where no stall check can reach it.
#[cfg(target_os = "windows")]
const TURN_WAIT: Duration = Duration::from_secs(3);
/// Unbroken starvation on the configure turn before this output declares the
/// stage wedged. Under [`WATCHDOG_STALL`] so the thread that can name the
/// phase its siblings are stuck in reports first.
#[cfg(target_os = "windows")]
const HANG_STARVED: Duration = Duration::from_secs(20);
/// Bound on one `Surface::configure`. Past it the call is treated as wedged:
/// the pause is lifted, the turn goes back, and only that output stops
/// presenting. Sized so the whole handshake — turn, quiesce, configure — fits
/// inside [`WATCHDOG_STALL`], since parked siblings present nothing while it
/// runs.
#[cfg(target_os = "windows")]
const CONFIGURE_WAIT: Duration = Duration::from_secs(8);
/// Pump gap for an output waiting out a configure that outran its bound.
#[cfg(target_os = "windows")]
const RECOVER_POLL: Duration = Duration::from_millis(2);
/// Time an output waits out a configure that outran its bound before its
/// window, device, swapchain and worker are replaced and the stuck call is
/// left behind. A call that answers inside this window was slow, not dead.
#[cfg(target_os = "windows")]
const REBUILD_AFTER: Duration = Duration::from_secs(2);
/// Rebuilds one output may go through before it is left down for the rest of
/// the stage. Each rebuild leaks a device and a window, so a present path that
/// wedges every fresh swapchain must not be rebuilt for the whole run.
#[cfg(target_os = "windows")]
const MAX_REBUILDS: u32 = 5;
/// No presented frame and no frame-loop progress from any output for this long
/// ends the stage as a tool failure. Normal runs dip to a couple of FPS during
/// a mode change; none of them stop advancing their loops.
#[cfg(target_os = "windows")]
const WATCHDOG_STALL: Duration = Duration::from_secs(30);
/// Grace before the watchdog arms, covering adapter bring-up and the first
/// swapchain build on every output.
#[cfg(target_os = "windows")]
const WATCHDOG_WARMUP: Duration = Duration::from_secs(20);
/// Bound on joining the output threads at teardown. A thread still running
/// past it is the wedge the stage just reported, and is never waited on.
#[cfg(target_os = "windows")]
const JOIN_TIMEOUT: Duration = Duration::from_secs(5);
/// Bound on the teardown mode restore, which runs off-thread because a wedged
/// `ChangeDisplaySettingsExW` elsewhere in the process blocks a restore too.
#[cfg(target_os = "windows")]
const RESTORE_TIMEOUT: Duration = Duration::from_secs(5);
/// Window after a self-inflicted change during which `Outdated` is expected
/// rather than evidence.
#[cfg(target_os = "windows")]
const SELF_INFLICTED_GRACE: Duration = Duration::from_secs(2);
/// Consecutive surface-recreation failures tolerated after a lost surface.
#[cfg(target_os = "windows")]
const MAX_SURFACE_RECREATES: u32 = 5;
/// Quiesce attempts for a fresh surface's first configure before the output
/// is abandoned.
#[cfg(target_os = "windows")]
const INITIAL_CONFIGURE_ATTEMPTS: u32 = 3;
/// Gap between `LiveKernelReports` scans.
#[cfg(target_os = "windows")]
const DUMP_SCAN_EVERY: Duration = Duration::from_secs(2);
/// Pause after a surface fault before the next acquire attempt.
#[cfg(target_os = "windows")]
const FAULT_BACKOFF: Duration = Duration::from_millis(100);
/// Time before the driven-output count is treated as settled. Sized for a
/// spin-up next to saturated CPU lanes in a concurrent run, where an output's
/// first configure can take several quiesce rounds before its first frame, and
/// for a first configure that outran its bound and was rebuilt.
#[cfg(target_os = "windows")]
const COVERAGE_WARMUP: Duration = Duration::from_secs(15);
/// Per-pixel iterations in the frame shader — enough that a frame is real work
/// without turning the stage back into a compute test.
#[cfg(target_os = "windows")]
const SHADER_ITERS: u32 = 12;

#[cfg(target_os = "windows")]
const SHADER: &str = r#"
struct Frame {
    time:      f32,
    tint:      f32,
    band:      f32,
    inv_width: f32,
    iters:     u32,
};

@group(0) @binding(0) var<uniform> frame: Frame;

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    var p = pos.xy * 0.002;
    var acc = 0.0;
    for (var i: u32 = 0u; i < frame.iters; i = i + 1u) {
        p = vec2<f32>(
            p.x + sin(p.y * 3.0 + frame.time),
            p.y + cos(p.x * 3.0 - frame.time),
        ) * 0.5;
        acc = acc + abs(p.x) + abs(p.y);
    }

    let u = pos.x * frame.inv_width;
    let bar = 1.0 - smoothstep(0.0, 0.05, abs(u - frame.band));
    let shade = fract(acc * 0.25) * 0.2;
    return vec4<f32>(
        shade + bar,
        shade * frame.tint + bar * frame.tint,
        0.10 + shade + bar,
        1.0,
    );
}
"#;

pub(crate) fn run(
    options: crate::DisplayOptions,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    #[cfg(target_os = "windows")]
    windows_impl::run(options, cancel, tx, started_at);

    #[cfg(not(target_os = "windows"))]
    let _ = options;
    #[cfg(not(target_os = "windows"))]
    run_unsupported(
        "gpu_display",
        "display present load",
        "the display-path stressor is implemented for Windows only",
        cancel,
        tx,
        started_at,
    );
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::*;

    use std::sync::atomic::{AtomicU32, AtomicU8};
    use std::sync::Mutex;

    use super::super::display_win::{
        apply_mode, enumerate_outputs, refresh_modes_at, resolutions, restore_mode, Output,
        OutputWindow,
    };
    use super::super::gpu_common::GpuContext;
    use crate::telemetry::live_dumps_windows::LiveDumpWatcher;

    /// How aggressively the stage changes the desktop mode.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ModeSetPolicy {
        Off,
        Refresh,
        Full,
    }

    impl ModeSetPolicy {
        /// Reads `STRESSKIT_DISPLAY_MODESET`; unset or unrecognized means
        /// refresh-only.
        fn from_env() -> Self {
            match std::env::var("STRESSKIT_DISPLAY_MODESET")
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str()
            {
                "off" | "0" | "none" => Self::Off,
                "full" | "resolution" => Self::Full,
                _ => Self::Refresh,
            }
        }

        /// An explicit per-run policy wins; otherwise the environment decides.
        fn resolve(requested: Option<crate::DisplayModeSet>) -> Self {
            match requested {
                Some(crate::DisplayModeSet::Off) => Self::Off,
                Some(crate::DisplayModeSet::Refresh) => Self::Refresh,
                Some(crate::DisplayModeSet::Full) => Self::Full,
                None => Self::from_env(),
            }
        }
    }

    /// Where an output thread is. Recorded on every transition so the watchdog
    /// can say what each thread was doing when the stage stopped moving.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(u8)]
    enum Phase {
        Starting = 0,
        Pumping = 1,
        AwaitingTurn = 2,
        Configuring = 3,
        Parked = 4,
        Acquiring = 5,
        Presenting = 6,
        ModeSetting = 7,
        Done = 8,
        Recovering = 9,
        Rebuilding = 10,
    }

    impl Phase {
        fn label(self) -> &'static str {
            match self {
                Self::Starting => "starting",
                Self::Pumping => "pumping messages",
                Self::AwaitingTurn => "waiting for the configure turn",
                Self::Configuring => "inside Surface::configure",
                Self::Parked => "parked for a sibling configure",
                Self::Acquiring => "inside get_current_texture",
                Self::Presenting => "drawing and presenting",
                Self::ModeSetting => "inside ChangeDisplaySettingsEx",
                Self::Done => "finished",
                Self::Recovering => "not presenting, waiting out a configure that has not returned",
                Self::Rebuilding => {
                    "rebuilding its window and swapchain after a configure that did not return"
                }
            }
        }

        fn from_u8(raw: u8) -> Self {
            match raw {
                1 => Self::Pumping,
                2 => Self::AwaitingTurn,
                3 => Self::Configuring,
                4 => Self::Parked,
                5 => Self::Acquiring,
                6 => Self::Presenting,
                7 => Self::ModeSetting,
                8 => Self::Done,
                9 => Self::Recovering,
                10 => Self::Rebuilding,
                _ => Self::Starting,
            }
        }
    }

    #[derive(Default)]
    struct OutputStats {
        presented: AtomicU64,
        timeouts: AtomicU64,
        lost: AtomicU64,
        /// `Outdated` outside the grace window after a change this stage made.
        unexpected_outdated: AtomicU64,
        validation: AtomicU64,
        occluded: AtomicU64,
        reconfigures: AtomicU64,
        mode_sets: AtomicU64,
        /// Window, device and swapchain rebuilds after a configure that did not
        /// return. Each one leaked the attempt it replaced.
        rebuilds: AtomicU64,
        stalled: AtomicBool,
        /// Frame-loop iterations this output has completed. Separates a stalled
        /// present (loop running, no frames leaving) from a wedged thread (loop
        /// not running), which is the only way to tell a display-path fault
        /// from the stressor blocking itself.
        progress: AtomicU64,
        /// Newest [`Phase`] discriminant.
        phase: AtomicU8,
    }

    impl OutputStats {
        fn faults(&self) -> u64 {
            self.timeouts.load(Ordering::Relaxed)
                + self.lost.load(Ordering::Relaxed)
                + self.unexpected_outdated.load(Ordering::Relaxed)
                + self.validation.load(Ordering::Relaxed)
        }

        fn set_phase(&self, phase: Phase) {
            self.phase.store(phase as u8, Ordering::Relaxed);
        }

        fn phase(&self) -> Phase {
            Phase::from_u8(self.phase.load(Ordering::Relaxed))
        }
    }

    /// The per-output handles the shared handshake needs: where to record this
    /// thread's phase, and how to keep its window pumping while it waits.
    /// Waiting without pumping is what let one thread's mode change block on a
    /// sibling that was itself blocked waiting for that mode change.
    struct OutputCtx<'a> {
        stats: &'a OutputStats,
        pump: &'a dyn Fn(),
    }

    #[derive(Default)]
    struct Shared {
        outputs: Vec<OutputStats>,
        /// Latched by the first output thread that cannot continue.
        fatal: Mutex<Option<String>>,
        /// Newest recoverable complaint from any output thread.
        warn: Mutex<Option<String>>,
        threads_live: AtomicU32,
        /// Threads currently inside their frame loop. Only these submit, so
        /// only these must park for a configure; a thread still in setup
        /// neither submits nor parks.
        submitters: AtomicU32,
        /// Serializes every `Surface::configure` — first-time and re-configure
        /// alike. A configure creates or resizes a swapchain on the shared
        /// device, which must not race a sibling's configure or submissions.
        configure_turn: Mutex<()>,
        /// Set while one thread configures; siblings park instead of submitting.
        configure_pause: AtomicBool,
        /// Threads currently parked for a sibling's configure.
        parked: AtomicU32,
        /// Reconfigures skipped because the siblings never went quiet.
        quiesce_timeouts: AtomicU64,
        /// Configures skipped because a sibling held the turn past
        /// [`TURN_WAIT`]. Distinct from `quiesce_timeouts`: a busy sibling is
        /// normal, a sibling that will not let go of the turn is not.
        turn_timeouts: AtomicU64,
        /// `Surface::configure` calls that outran [`CONFIGURE_WAIT`].
        configure_wedges: AtomicU64,
        /// Output rebuilds across the stage: an output whose configure stayed
        /// out past [`REBUILD_AFTER`] got a fresh window, device and swapchain.
        outputs_rebuilt: AtomicU64,
        /// A coverage complaint has been emitted and not yet resolved.
        coverage_complained: AtomicBool,
        /// An output whose configure outran its bound has resumed presenting,
        /// and the tick loop has not said so yet.
        configure_recovered: AtomicBool,
        /// Latched by whichever detector finds the stage wedged inside its own
        /// handshake. Kept apart from `fatal`: that reports the display path,
        /// this reports the tool.
        hang: Mutex<Option<String>>,
        /// Displays whose desktop mode this stage has changed. Owned by the
        /// stage rather than by the output thread so a wedged thread cannot
        /// strand a changed mode.
        mode_touched: Mutex<Vec<String>>,
    }

    /// Frame-loop membership marker for the quiesce handshake; the counter
    /// drops with the guard on every exit path.
    struct SubmitGuard<'a>(&'a AtomicU32);

    impl<'a> SubmitGuard<'a> {
        fn enter(counter: &'a AtomicU32) -> Self {
            counter.fetch_add(1, Ordering::SeqCst);
            Self(counter)
        }
    }

    impl Drop for SubmitGuard<'_> {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// How one attempt to build or rebuild this output's swapchain ended.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ConfigureOutcome {
        Ran,
        /// The handshake turned the attempt away; the caller retries later.
        Skipped,
        /// The call outran [`CONFIGURE_WAIT`] and is still running on the
        /// worker thread. This output must present nothing until it answers.
        Wedged,
    }

    /// One `Surface::configure` request and the surface it applies to. The
    /// surface travels with the request because a lost surface is replaced.
    type ConfigureRequest = (Arc<wgpu::Surface<'static>>, wgpu::SurfaceConfiguration);

    /// The worker one output's swapchain rebuilds run on.
    type SurfaceWorker = ConfigureWorker<ConfigureRequest>;

    /// Runs one output's `Surface::configure` calls on a thread of their own.
    ///
    /// `configure` waits for its device to go idle with no timeout, so a call
    /// that does not come back cannot be abandoned where it is made. Off the
    /// frame-loop thread it can be: the caller gives up on [`CONFIGURE_WAIT`],
    /// releases the handshake, and keeps pumping its window until the worker
    /// answers. The window keeps its thread, which Win32 requires.
    struct ConfigureWorker<J> {
        jobs: mpsc::Sender<J>,
        replies: mpsc::Receiver<()>,
        /// A job the worker has not answered yet.
        pending: bool,
    }

    impl<J: Send + 'static> ConfigureWorker<J> {
        fn spawn(index: usize, run: impl Fn(J) + Send + 'static) -> Result<Self, String> {
            let (jobs, inbox) = mpsc::channel::<J>();
            let (outbox, replies) = mpsc::channel::<()>();
            std::thread::Builder::new()
                .name(format!("stress-kit-display-cfg-{index}"))
                .spawn(move || {
                    while let Ok(job) = inbox.recv() {
                        run(job);
                        if outbox.send(()).is_err() {
                            return;
                        }
                    }
                })
                .map_err(|e| format!("failed to spawn the configure worker: {e}"))?;
            Ok(Self {
                jobs,
                replies,
                pending: false,
            })
        }

        /// Runs `job`, waiting [`CONFIGURE_WAIT`] for it while pumping.
        fn submit(&mut self, job: J, ctx: &OutputCtx<'_>, stop: &AtomicBool) -> ConfigureOutcome {
            if self.pending || self.jobs.send(job).is_err() {
                return ConfigureOutcome::Wedged;
            }
            self.pending = true;
            ctx.stats.set_phase(Phase::Configuring);
            let deadline = Instant::now() + CONFIGURE_WAIT;
            while Instant::now() < deadline {
                if self.settled() {
                    return ConfigureOutcome::Ran;
                }
                if stop.load(Ordering::Relaxed) {
                    return ConfigureOutcome::Wedged;
                }
                (ctx.pump)();
                std::thread::sleep(QUIESCE_POLL);
            }
            ConfigureOutcome::Wedged
        }

        /// Whether the worker is free. False while a configure that outran its
        /// bound is still running; presenting again before it returns is the
        /// half-built-swapchain race the handshake exists to stop.
        fn settled(&mut self) -> bool {
            if !self.pending {
                return true;
            }
            match self.replies.try_recv() {
                Ok(()) => {
                    self.pending = false;
                    true
                }
                Err(_) => false,
            }
        }
    }

    impl Shared {
        fn latch_fatal(&self, msg: String) {
            log::error!("[stress-kit/gpu_display] {msg}");
            if let Ok(mut g) = self.fatal.lock() {
                g.get_or_insert(msg);
            }
        }

        fn latch_hang(&self, msg: String) {
            log::error!("[stress-kit/gpu_display] {msg}");
            if let Ok(mut g) = self.hang.lock() {
                g.get_or_insert(msg);
            }
        }

        fn hang(&self) -> Option<String> {
            self.hang.lock().ok().and_then(|g| g.clone())
        }

        /// Records a display whose mode this stage changed, before the change
        /// is applied: the call can wedge, and teardown still has to undo it.
        fn touch_mode(&self, device: &str) {
            if let Ok(mut g) = self.mode_touched.lock()
                && !g.iter().any(|d| d == device)
            {
                g.push(device.to_string());
            }
        }

        fn set_warn(&self, msg: String) {
            log::warn!("[stress-kit/gpu_display] {msg}");
            if let Ok(mut g) = self.warn.lock() {
                *g = Some(msg);
            }
        }

        fn fatal(&self) -> Option<String> {
            self.fatal.lock().ok().and_then(|g| g.clone())
        }

        fn warn(&self) -> Option<String> {
            self.warn.lock().ok().and_then(|g| g.clone())
        }

        fn total(&self, pick: fn(&OutputStats) -> u64) -> u64 {
            self.outputs.iter().map(pick).sum()
        }

        /// Runs `configure` with every presenting sibling parked so the shared
        /// device can reach idle. `self_submits` says whether the caller is
        /// itself inside its frame loop. Returns `None` when the turn or the
        /// quiet could not be had in time and the caller should retry later.
        fn with_quiesce<R>(
            &self,
            ctx: &OutputCtx<'_>,
            stop: &AtomicBool,
            self_submits: bool,
            configure: impl FnOnce() -> R,
        ) -> Option<R> {
            // Held across the whole configure: two swapchain builds on one
            // device must not overlap even with every presenter parked.
            let Some(_turn) = self.take_turn(ctx, stop) else {
                self.turn_timeouts.fetch_add(1, Ordering::Relaxed);
                return None;
            };
            // Raised before counting siblings: a thread that enters its frame
            // loop mid-configure parks at its first frame instead of
            // submitting into the build.
            self.configure_pause.store(true, Ordering::SeqCst);
            let quiet = self.other_submitters(self_submits) == 0
                || self.await_parked(ctx, stop, self_submits);
            let out = if quiet {
                ctx.stats.set_phase(Phase::Configuring);
                Some(configure())
            } else {
                None
            };
            self.configure_pause.store(false, Ordering::SeqCst);
            if out.is_none() {
                self.quiesce_timeouts.fetch_add(1, Ordering::Relaxed);
            }
            out
        }

        /// Bounded acquisition of the configure turn. A blocking `lock()` here
        /// is what turned one stalled `configure` into a wedged stage: every
        /// sibling piled up on the mutex below the frame loop's stall check, so
        /// nothing could ever report. `None` means a sibling has held it past
        /// [`TURN_WAIT`] and the caller must go round its loop instead.
        fn take_turn<'s>(
            &'s self,
            ctx: &OutputCtx<'_>,
            stop: &AtomicBool,
        ) -> Option<std::sync::MutexGuard<'s, ()>> {
            ctx.stats.set_phase(Phase::AwaitingTurn);
            let deadline = Instant::now() + TURN_WAIT;
            loop {
                match self.configure_turn.try_lock() {
                    Ok(guard) => return Some(guard),
                    // A configurer that panicked poisoned the turn; taking it
                    // anyway beats never configuring again.
                    Err(std::sync::TryLockError::Poisoned(e)) => return Some(e.into_inner()),
                    Err(std::sync::TryLockError::WouldBlock) => {}
                }
                if stop.load(Ordering::Relaxed) || Instant::now() >= deadline {
                    return None;
                }
                (ctx.pump)();
                std::thread::sleep(QUIESCE_POLL);
            }
        }

        /// Frame-loop threads other than the caller.
        fn other_submitters(&self, self_submits: bool) -> u32 {
            self.submitters
                .load(Ordering::SeqCst)
                .saturating_sub(self_submits as u32)
        }

        /// Waits until every other presenting thread is parked. Bounded, and
        /// reads `submitters` each pass so a thread that exits its frame loop
        /// cannot strand us.
        fn await_parked(
            &self,
            ctx: &OutputCtx<'_>,
            stop: &AtomicBool,
            self_submits: bool,
        ) -> bool {
            let deadline = Instant::now() + QUIESCE_TIMEOUT;
            while Instant::now() < deadline {
                if stop.load(Ordering::Relaxed) {
                    return false;
                }
                if self.parked.load(Ordering::SeqCst) >= self.other_submitters(self_submits) {
                    return true;
                }
                (ctx.pump)();
                std::thread::sleep(QUIESCE_POLL);
            }
            false
        }

        /// Parks this thread while a sibling configures. Called once per frame,
        /// before anything is submitted. Keeps pumping: a sibling's desktop
        /// mode change broadcasts to this window and does not return until the
        /// message is dispatched, so a park that stops pumping deadlocks the
        /// mode change it is parked for.
        fn park_if_paused(&self, ctx: &OutputCtx<'_>, stop: &AtomicBool) {
            if !self.configure_pause.load(Ordering::SeqCst) {
                return;
            }
            ctx.stats.set_phase(Phase::Parked);
            self.parked.fetch_add(1, Ordering::SeqCst);
            let deadline = Instant::now() + QUIESCE_PARK_MAX;
            while self.configure_pause.load(Ordering::SeqCst)
                && !stop.load(Ordering::Relaxed)
                && Instant::now() < deadline
            {
                (ctx.pump)();
                std::thread::sleep(QUIESCE_POLL);
            }
            self.parked.fetch_sub(1, Ordering::SeqCst);
        }

        /// Outputs that presented at least one frame.
        fn driven(&self) -> usize {
            self.outputs
                .iter()
                .filter(|o| o.presented.load(Ordering::Relaxed) > 0)
                .count()
        }
    }

    pub(super) fn run(
        options: crate::DisplayOptions,
        cancel: &Arc<AtomicBool>,
        tx: &mpsc::Sender<Metrics>,
        started_at: Instant,
    ) {
        let mut outputs = enumerate_outputs();
        // True attached count, kept before any cap so the coverage note stays
        // honest about what was left untested.
        let attached = outputs.len();
        if let Some(cap) = options.max_outputs.filter(|c| *c > 0 && *c < attached) {
            outputs.truncate(cap);
            log::info!(
                "[stress-kit/gpu_display] capped to {cap} of {attached} attached output(s) by request"
            );
        }
        if outputs.is_empty() {
            return run_unsupported(
                "gpu_display",
                "display present load",
                "no attached outputs; this session has no display to present to",
                cancel,
                tx,
                started_at,
            );
        }

        let ctx = match GpuContext::acquire(true) {
            Ok(c) => Arc::new(c),
            Err(e) => {
                return run_unsupported(
                    "gpu_display",
                    "display present load",
                    &e,
                    cancel,
                    tx,
                    started_at,
                )
            }
        };
        let policy = ModeSetPolicy::resolve(options.modeset);
        log::info!(
            "[stress-kit/gpu_display] {} output(s) on {} ({} backend), mode-set policy {:?}, \
             one logical device per output",
            outputs.len(),
            ctx.vendor_label,
            ctx.backend_label,
            policy
        );
        for output in &outputs {
            log::info!("[stress-kit/gpu_display] output: {}", output.describe());
        }

        let mut dumps = LiveDumpWatcher::new();
        let dumps_available = dumps.available();

        let mut shared = Shared::default();
        shared
            .outputs
            .resize_with(outputs.len(), OutputStats::default);
        // Counted before the spawn so the tick loop cannot read zero first.
        shared
            .threads_live
            .store(outputs.len() as u32, Ordering::SeqCst);
        let shared = Arc::new(shared);

        let stop = Arc::new(AtomicBool::new(false));
        let handles: Vec<_> = outputs
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, output)| {
                let ctx = ctx.clone();
                let shared = shared.clone();
                let stop = stop.clone();
                let total = outputs.len();
                std::thread::Builder::new()
                    .name(format!("stress-kit-display-{index}"))
                    .spawn(move || {
                        drive_output(&ctx, output, index, total, policy, &stop, &shared);
                        shared.threads_live.fetch_sub(1, Ordering::SeqCst);
                    })
                    .expect("stress-kit: failed to spawn gpu_display output thread")
            })
            .collect();

        let devices: Vec<String> = outputs.iter().map(|o| o.device.clone()).collect();
        tick_loop(
            cancel,
            tx,
            started_at,
            &shared,
            &ctx,
            &mut dumps,
            dumps_available,
            attached,
            &devices,
        );

        stop.store(true, Ordering::SeqCst);
        let stuck = await_threads(&shared, JOIN_TIMEOUT);
        if stuck == 0 {
            for handle in handles {
                let _ = handle.join();
            }
        } else {
            // Never joined. Joining a wedged output thread is what turned a
            // stalled stage into a stalled process: the stage could not report,
            // the child could not exit, and an operator had to kill it. The
            // handles are dropped instead, which detaches the threads.
            log::error!(
                "[stress-kit/gpu_display] {stuck} output thread(s) did not stop within {}s; \
                 detaching them so the stage can report. Their windows and swapchains are \
                 released when this process exits, and the desktop modes are app-owned \
                 (CDS_FULLSCREEN) so Windows restores them at that point.",
                JOIN_TIMEOUT.as_secs()
            );
            drop(handles);
        }
        restore_touched_modes(&shared);
        log::info!(
            "[stress-kit/gpu_display] drove {} of {} attached output(s), {} frames presented, \
             {} reconfigure(s) skipped for a busy sibling, {} for a held configure turn, \
             {} configure(s) that did not return within {}s, {} output rebuild(s)",
            shared.driven(),
            attached,
            shared.total(|o| o.presented.load(Ordering::Relaxed)),
            shared.quiesce_timeouts.load(Ordering::Relaxed),
            shared.turn_timeouts.load(Ordering::Relaxed),
            shared.configure_wedges.load(Ordering::Relaxed),
            CONFIGURE_WAIT.as_secs(),
            shared.outputs_rebuilt.load(Ordering::Relaxed)
        );
        for (output, stats) in outputs.iter().zip(&shared.outputs) {
            log::info!(
                "[stress-kit/gpu_display] {}: {} presented, {} timeout, {} lost, {} outdated, \
                 {} occluded, {} reconfigure, {} mode set, {} rebuild",
                output.device,
                stats.presented.load(Ordering::Relaxed),
                stats.timeouts.load(Ordering::Relaxed),
                stats.lost.load(Ordering::Relaxed),
                stats.unexpected_outdated.load(Ordering::Relaxed),
                stats.occluded.load(Ordering::Relaxed),
                stats.reconfigures.load(Ordering::Relaxed),
                stats.mode_sets.load(Ordering::Relaxed),
                stats.rebuilds.load(Ordering::Relaxed),
            );
        }
    }

    /// Waits for the output threads to leave `drive_output`, and returns how
    /// many were still in it when the bound expired.
    fn await_threads(shared: &Arc<Shared>, wait: Duration) -> u32 {
        let deadline = Instant::now() + wait;
        loop {
            let live = shared.threads_live.load(Ordering::SeqCst);
            if live == 0 || Instant::now() >= deadline {
                return live;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Restores every desktop mode this stage changed. Runs off-thread with a
    /// bound: a wedged `ChangeDisplaySettingsEx` anywhere in the process blocks
    /// a restore too, and teardown must not inherit that wait.
    fn restore_touched_modes(shared: &Arc<Shared>) {
        let devices = shared
            .mode_touched
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        if devices.is_empty() {
            return;
        }
        let done = Arc::new(AtomicBool::new(false));
        let flag = done.clone();
        let fallback = devices.clone();
        let spawned = std::thread::Builder::new()
            .name("stress-kit-display-restore".into())
            .spawn(move || {
                for device in &devices {
                    restore_mode(device);
                }
                flag.store(true, Ordering::SeqCst);
            });
        if spawned.is_err() {
            // Better a restore that might block than a display left on a
            // stressor-chosen mode.
            for device in &fallback {
                restore_mode(device);
            }
            return;
        }
        let deadline = Instant::now() + RESTORE_TIMEOUT;
        while !done.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        if !done.load(Ordering::SeqCst) {
            log::error!(
                "[stress-kit/gpu_display] mode restore did not finish within {}s; the modes are \
                 app-owned (CDS_FULLSCREEN) so Windows restores them when this process exits",
                RESTORE_TIMEOUT.as_secs()
            );
        }
    }

    /// Tracks whether the stage is still moving at all. Read from the tick
    /// thread, which is never inside a present, a configure or a mode change,
    /// so it answers even when every output thread is blocked.
    struct Watchdog {
        presented: u64,
        progress: u64,
        moved_at: Instant,
        stall: Duration,
        warmup: Duration,
    }

    impl Watchdog {
        fn new(now: Instant) -> Self {
            Self::with_limits(now, WATCHDOG_STALL, WATCHDOG_WARMUP)
        }

        fn with_limits(now: Instant, stall: Duration, warmup: Duration) -> Self {
            Self {
                presented: 0,
                progress: 0,
                moved_at: now,
                stall,
                warmup,
            }
        }

        /// Folds one sample. `true` once no frame has reached a screen and no
        /// output thread has got round its loop for `stall`. A stage whose
        /// loops are running but whose frames have stopped is a present stall,
        /// not a wedge, and is left to the per-output stall check.
        fn wedged(
            &mut self,
            presented: u64,
            progress: u64,
            now: Instant,
            elapsed: Duration,
        ) -> bool {
            if presented > self.presented || progress > self.progress {
                self.presented = presented;
                self.progress = progress;
                self.moved_at = now;
                return false;
            }
            elapsed >= self.warmup && now.duration_since(self.moved_at) >= self.stall
        }

        fn stuck_for(&self, now: Instant) -> Duration {
            now.duration_since(self.moved_at)
        }
    }

    /// The report the watchdog files. Names the phase every output was stuck
    /// in, and says plainly that this is the tool and not the machine, because
    /// a reader six weeks later has only this string to go on.
    fn hang_report(shared: &Shared, devices: &[String], stuck_for: Duration) -> String {
        let phases: Vec<String> = shared
            .outputs
            .iter()
            .enumerate()
            .map(|(i, stats)| {
                format!(
                    "{} {} ({} frame(s) presented)",
                    devices.get(i).map(String::as_str).unwrap_or("output"),
                    stats.phase().label(),
                    stats.presented.load(Ordering::Relaxed)
                )
            })
            .collect();
        format!(
            "gpu_display: {marker} no output presented a frame and no output thread \
             advanced its frame loop for {stuck}s, so the stage is wedged inside its own \
             handshake rather than in the display path. Threads: {phases}. {quiesce} \
             configure(s) skipped for a busy sibling, {turn} for a sibling that would not \
             release the configure turn, {wedged} that never returned, {rebuilt} output \
             rebuild(s). Zero FPS with no watchdog live dump, no TDR and no WHEA on a \
             responsive machine is a TOOL failure: the run grades INCONCLUSIVE and proves \
             nothing about this hardware in either direction. Re-run the stage; do not read \
             this as a display fault.",
            marker = crate::STRESSOR_HANG_MARKER,
            stuck = stuck_for.as_secs(),
            phases = phases.join(", "),
            quiesce = shared.quiesce_timeouts.load(Ordering::Relaxed),
            turn = shared.turn_timeouts.load(Ordering::Relaxed),
            wedged = shared.configure_wedges.load(Ordering::Relaxed),
            rebuilt = shared.outputs_rebuilt.load(Ordering::Relaxed),
        )
    }

    /// Aggregates the output threads into ticks and decides when the stage ends.
    #[allow(clippy::too_many_arguments)]
    fn tick_loop(
        cancel: &Arc<AtomicBool>,
        tx: &mpsc::Sender<Metrics>,
        started_at: Instant,
        shared: &Arc<Shared>,
        ctx: &Arc<GpuContext>,
        dumps: &mut LiveDumpWatcher,
        dumps_available: bool,
        attached: usize,
        devices: &[String],
    ) {
        let mut last_tick = Instant::now();
        let mut last_scan = Instant::now();
        let mut last_presented: u64 = 0;
        let mut watchdog_dumps: u64 = 0;
        let mut watchdog = Watchdog::new(Instant::now());

        while !cancel.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(50));

            if last_scan.elapsed() >= DUMP_SCAN_EVERY {
                last_scan = Instant::now();
                for dump in dumps.poll() {
                    watchdog_dumps += 1;
                    shared.latch_fatal(format!(
                        "gpu_display: display-path watchdog live dump appeared during the run \
                         ({}); the display miniport was reset while presenting",
                        dump.label()
                    ));
                }
            }

            if last_tick.elapsed() < TICK {
                continue;
            }
            let dt = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);
            last_tick = Instant::now();

            let presented = shared.total(|o| o.presented.load(Ordering::Relaxed));
            let fps = presented.saturating_sub(last_presented) as f64 / dt;
            last_presented = presented;
            let errors = shared.total(|o| o.faults()) + watchdog_dumps;

            if let Some(reason) = ctx.health.failure() {
                emit_fatal_tick(tx, started_at, format!("gpu_display: {reason}"), errors);
                return;
            }
            if let Some(reason) = shared.fatal() {
                emit_fatal_tick(tx, started_at, reason, errors);
                return;
            }
            if shared.threads_live.load(Ordering::SeqCst) == 0 {
                emit_fatal_tick(
                    tx,
                    started_at,
                    "gpu_display: inconclusive - every output thread exited; no swapchain is \
                     being presented, so the display path was not exercised"
                        .to_string(),
                    errors,
                );
                return;
            }
            // Ranked below every signal above: those name the machine, this
            // names the tool, and the machine's word comes first.
            if let Some(reason) = shared.hang() {
                emit_fatal_tick(tx, started_at, reason, errors);
                return;
            }
            let now = Instant::now();
            let progress = shared.total(|o| o.progress.load(Ordering::Relaxed));
            if watchdog.wedged(presented, progress, now, started_at.elapsed()) {
                let reason = hang_report(shared, devices, watchdog.stuck_for(now));
                log::error!("[stress-kit/gpu_display] {reason}");
                emit_fatal_tick(tx, started_at, reason, errors);
                return;
            }

            emit_tick(
                tx,
                started_at,
                fps,
                standing_note(shared, attached, dumps_available, started_at.elapsed()),
                errors,
            );
        }
    }

    /// The message that rides along with a non-fatal tick. A live complaint
    /// outranks the standing coverage caveats.
    fn standing_note(
        shared: &Arc<Shared>,
        attached: usize,
        dumps_available: bool,
        elapsed: Duration,
    ) -> Option<String> {
        let driven = shared.driven();
        // One-shot, ahead of any standing warn so nothing masks it: the
        // runner clears its latched inconclusive on the `resolved -` marker.
        if driven >= attached
            && shared
                .coverage_complained
                .swap(false, Ordering::SeqCst)
        {
            return Some(format!(
                "resolved - all {attached} attached output(s) are now driven; the earlier \
                 coverage shortfall no longer applies"
            ));
        }
        // Also one-shot and also ahead of any standing warn: a configure that
        // came back late leaves the output presenting again, so the shortfall
        // it reported no longer stands.
        if shared.configure_recovered.swap(false, Ordering::SeqCst) {
            return Some(
                "resolved - the configure that outran its bound returned and that output is \
                 presenting again; the earlier shortfall no longer applies"
                    .to_string(),
            );
        }
        if let Some(warn) = shared.warn() {
            return Some(warn);
        }
        // Held until the count settles; a starting run drives no output yet.
        // Any shortfall counts, not just a single output: driving 2 of 3 leaves
        // the third untested, so it cannot clear a multi-monitor fault either.
        if elapsed >= COVERAGE_WARMUP && driven < attached {
            shared.coverage_complained.store(true, Ordering::SeqCst);
            return Some(format!(
                "gpu_display: inconclusive - only {driven} of {attached} attached output(s) were \
                 driven; the full multi-display present path was not exercised, so a pass here \
                 does not clear a multi-monitor flip-queue fault. Drive every attached output and \
                 re-run. Coverage limit, not a hardware fault."
            ));
        }
        if !dumps_available {
            return Some(
                "gpu_display: inconclusive - C:\\Windows\\LiveKernelReports is unreadable, so \
                 watchdog live dumps written during this run would go undetected; re-run \
                 elevated. Coverage limit, not a hardware fault."
                    .to_string(),
            );
        }
        None
    }

    #[repr(C)]
    #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
    struct Frame {
        time: f32,
        tint: f32,
        band: f32,
        inv_width: f32,
        iters: u32,
        _pad: [u32; 3],
    }

    /// Everything one attempt at presenting to an output owns: its window, its
    /// own logical device and queue, the swapchain and the worker that
    /// configures it, and the pipeline that draws into it. Thread-affine
    /// through the window. Replaced wholesale by a rebuild, never torn down
    /// piecemeal.
    struct Presenter {
        window: OutputWindow,
        raw_handle: wgpu::rwh::RawWindowHandle,
        device: wgpu::Device,
        queue: wgpu::Queue,
        worker: SurfaceWorker,
        surface: Arc<wgpu::Surface<'static>>,
        config: wgpu::SurfaceConfiguration,
        present_modes: Vec<wgpu::PresentMode>,
        module: wgpu::ShaderModule,
        pipeline: wgpu::RenderPipeline,
        frame_buf: wgpu::Buffer,
        bind_group: wgpu::BindGroup,
    }

    impl Presenter {
        /// Builds the window, device, surface and pipeline for `output`. The
        /// swapchain is left unconfigured: the caller runs its first configure
        /// through the quiesce handshake. `attempt` is zero for the first build
        /// and counts rebuilds after it. `Err` names what could not be built.
        fn build(
            ctx: &Arc<GpuContext>,
            output: &Output,
            index: usize,
            attempt: u32,
        ) -> Result<Self, String> {
            let window = OutputWindow::new(output)
                .map_err(|e| format!("could not open a window on {} ({e})", output.device))?;
            let raw_handle = window
                .raw_handle()
                .map_err(|e| format!("no window handle for {} ({e})", output.device))?;

            // Its own logical device on the shared adapter. Sharing one device
            // is what made a configure on this output wait for flips its parked
            // siblings had stopped draining, with no timeout to escape.
            let (device, queue) = ctx
                .spawn_device(&format!("gpu_display {} #{attempt}", output.device))
                .map_err(|e| format!("no device for {} ({e})", output.device))?;
            let configure_on = device.clone();
            let configure = move |(surface, config): ConfigureRequest| {
                surface.configure(&configure_on, &config)
            };
            let worker = SurfaceWorker::spawn(index, configure)
                .map_err(|e| format!("{} ({e})", output.device))?;

            let surface = create_surface(ctx, raw_handle).map_err(|e| {
                format!(
                    "no swapchain on {} ({e}); this adapter cannot present to that output",
                    output.device
                )
            })?;
            let caps = surface.get_capabilities(&ctx.adapter);
            if caps.formats.is_empty() {
                return Err(format!(
                    "{} reports no surface formats on this adapter",
                    output.device
                ));
            }
            // Fifo is guaranteed; the rest widen the flip-queue behaviour we cover.
            let present_modes: Vec<wgpu::PresentMode> = caps.present_modes.clone();
            log::info!(
                "[stress-kit/gpu_display] {}: format {:?}, present modes {:?}",
                output.device,
                caps.formats[0],
                present_modes
            );
            let config = surface
                .get_default_config(&ctx.adapter, output.width, output.height)
                .ok_or_else(|| {
                    format!("{} is not supported by the bound adapter", output.device)
                })?;

            // Built before the swapchain, so an output whose first configure
            // does not return can resume into the frame loop when it does.
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("gpu_display module"),
                source: wgpu::ShaderSource::Wgsl(SHADER.into()),
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("gpu_display pipeline"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(config.format.into())],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            });
            let frame_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("gpu_display frame"),
                size: std::mem::size_of::<Frame>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("gpu_display bind group"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: frame_buf.as_entire_binding(),
                }],
            });

            Ok(Self {
                window,
                raw_handle,
                device,
                queue,
                worker,
                surface: Arc::new(surface),
                config,
                present_modes,
                module,
                pipeline,
                frame_buf,
                bind_group,
            })
        }

        /// Leaks this attempt. Its worker is inside `Surface::configure` on its
        /// own thread and cannot be cancelled, and the window, surface and
        /// device that call is using must not be destroyed under it from this
        /// thread, so every handle is forgotten; the process exit reclaims
        /// them, and the stage is short-lived. Only the worker's channel ends
        /// drop, so its thread exits rather than parks if the call ever returns.
        fn leak(self) {
            let Self {
                window,
                raw_handle: _,
                device,
                queue,
                worker,
                surface,
                config: _,
                present_modes: _,
                module,
                pipeline,
                frame_buf,
                bind_group,
            } = self;
            drop(worker);
            std::mem::forget(bind_group);
            std::mem::forget(frame_buf);
            std::mem::forget(pipeline);
            std::mem::forget(module);
            std::mem::forget(surface);
            std::mem::forget(queue);
            std::mem::forget(device);
            std::mem::forget(window);
        }
    }

    /// What an output waiting out a configure that outran its bound does next.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Recovery {
        /// Keep pumping; the call may still answer.
        Wait,
        /// Leak this attempt and build a fresh one.
        Rebuild,
        /// The fresh swapchain was never configured; run its first configure again.
        Reconfigure,
        /// Out of rebuilds; the output stays down.
        Exhausted,
    }

    /// `waited` is the time since this output stopped presenting;
    /// `worker_stuck` says a configure is still out on the worker.
    fn recovery_step(waited: Duration, worker_stuck: bool, rebuilds: u32) -> Recovery {
        if waited < REBUILD_AFTER {
            Recovery::Wait
        } else if !worker_stuck {
            Recovery::Reconfigure
        } else if rebuilds >= MAX_REBUILDS {
            Recovery::Exhausted
        } else {
            Recovery::Rebuild
        }
    }

    /// Owns one output end to end: its window, its swapchain, its mode changes.
    /// A configure that does not return costs the output a rebuild, not the
    /// stage: the stuck attempt is leaked and a fresh one takes over.
    fn drive_output(
        ctx: &Arc<GpuContext>,
        output: Output,
        index: usize,
        total: usize,
        policy: ModeSetPolicy,
        stop: &Arc<AtomicBool>,
        shared: &Arc<Shared>,
    ) {
        // Present/park handshakes must answer within the quiesce window even
        // next to saturated CPU lanes; this thread does milliseconds of CPU
        // work per frame, so the boost costs the other lanes nothing.
        unsafe {
            use winapi::um::processthreadsapi::{GetCurrentThread, SetThreadPriority};
            use winapi::um::winbase::THREAD_PRIORITY_ABOVE_NORMAL;
            SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL as i32);
        }

        let stats = &shared.outputs[index];
        stats.set_phase(Phase::Starting);
        let _phase_done = PhaseDone(stats);
        let mut presenter = match Presenter::build(ctx, &output, index, 0) {
            Ok(p) => p,
            Err(e) => {
                shared.latch_fatal(format!(
                    "gpu_display: inconclusive - {e}; that output's present path never ran"
                ));
                return;
            }
        };

        // The first configure creates the swapchain; presenting siblings park
        // so the build cannot race their submissions.
        let mut recovering = {
            let pump = || presenter.window.pump();
            let octx = OutputCtx { stats, pump: &pump };
            match configure_initial(
                &presenter.surface,
                &presenter.config,
                shared,
                &octx,
                stop,
                &mut presenter.worker,
            ) {
                ConfigureOutcome::Ran => false,
                ConfigureOutcome::Wedged => true,
                ConfigureOutcome::Skipped => {
                    shared.latch_fatal(format!(
                        "gpu_display: inconclusive - the swapchain on {} could not be configured \
                         while sibling outputs were presenting; that output's present path never \
                         ran",
                        output.device
                    ));
                    return;
                }
            }
        };

        let mut modes = ModeCycle::new(&output, policy);
        let started = Instant::now();
        let mut last_present = Instant::now();
        let mut last_reconfigure = Instant::now();
        // Staggered so outputs do not all change mode on the same beat.
        let stagger = MODE_SET_EVERY / (total as u32 + 1) * index as u32;
        let mut last_mode_set = Instant::now().checked_sub(stagger).unwrap_or_else(Instant::now);
        let mut expect_outdated_until = Instant::now() + SELF_INFLICTED_GRACE;
        let mut recreate_failures: u32 = 0;
        let mut stall_reported = false;
        let mut present_mode_index = 0usize;
        // Set when the handshake turns a configure away, cleared by any
        // configure that runs or any frame that presents. Unbroken starvation
        // means a sibling is wedged holding the turn, which is the stressor
        // blocking itself and must never be filed as a present stall.
        let mut starved_since: Option<Instant> = None;
        // Whether the handshake got in the way at all since the last presented
        // frame. Separate from `starved_since` so a starvation that alternates
        // with the occasional successful configure — which keeps resetting that
        // timer — still classifies the stall as the tool and not the display.
        let mut starved_seen = false;
        // When this output last stopped presenting to wait out a configure.
        let mut recovering_since = Instant::now();
        // Rebuilds so far, against `MAX_REBUILDS`; the exhausted note files once.
        let mut rebuilds: u32 = 0;
        let mut exhausted_reported = false;
        // True while a rebuilt swapchain still awaits its first completed configure.
        let mut unconfigured = false;

        // Frame-loop membership for the quiesce handshake; setup never submits,
        // and neither does an output waiting out its own wedged configure.
        let mut submit = (!recovering).then(|| SubmitGuard::enter(&shared.submitters));
        if recovering {
            warn_configure_wedged(shared, &output.device);
        }
        let wedge_after = debug_wedge_frame(index);

        while !stop.load(Ordering::Relaxed) {
            // Submits nothing and counts no progress while the worker is still
            // inside a configure; only the window keeps being pumped.
            if recovering {
                stats.set_phase(Phase::Recovering);
                presenter.window.pump();
                // A configure that answers late leaves its swapchain built.
                if !unconfigured && presenter.worker.settled() {
                    recovering = false;
                    submit = Some(SubmitGuard::enter(&shared.submitters));
                    last_present = Instant::now();
                    last_reconfigure = Instant::now();
                    last_mode_set = Instant::now();
                    expect_outdated_until = Instant::now() + SELF_INFLICTED_GRACE;
                    starved_since = None;
                    starved_seen = false;
                    log::warn!(
                        "[stress-kit/gpu_display] {}: the configure came back; presenting again",
                        output.device
                    );
                    shared.configure_recovered.store(true, Ordering::SeqCst);
                    continue;
                }
                let step = recovery_step(recovering_since.elapsed(), !unconfigured, rebuilds);
                if step == Recovery::Rebuild {
                    rebuilds += 1;
                    stats.rebuilds.fetch_add(1, Ordering::Relaxed);
                    shared.outputs_rebuilt.fetch_add(1, Ordering::Relaxed);
                    // Counted as progress for the watchdog.
                    stats.progress.fetch_add(1, Ordering::Relaxed);
                    stats.set_phase(Phase::Rebuilding);
                    log::warn!(
                        "[stress-kit/gpu_display] {}: Surface::configure has not returned for \
                         {:.0}s; leaking that attempt and rebuilding ({rebuilds} of \
                         {MAX_REBUILDS})",
                        output.device,
                        CONFIGURE_WAIT.as_secs_f32() + recovering_since.elapsed().as_secs_f32()
                    );
                    match Presenter::build(ctx, &output, index, rebuilds) {
                        Ok(fresh) => {
                            std::mem::replace(&mut presenter, fresh).leak();
                            unconfigured = true;
                        }
                        Err(e) => {
                            // The output stays down; no further rebuild is attempted.
                            rebuilds = MAX_REBUILDS;
                            exhausted_reported = true;
                            shared.set_warn(format!(
                                "gpu_display: {} rebuilding {} failed ({e}); that output stays \
                                 down for the rest of the stage while the others carry on. \
                                 Coverage limit imposed by the tool, not a hardware fault.",
                                crate::STRESSOR_LIMIT_MARKER,
                                output.device
                            ));
                            recovering_since = Instant::now();
                            std::thread::sleep(RECOVER_POLL);
                            continue;
                        }
                    }
                }
                if matches!(step, Recovery::Rebuild | Recovery::Reconfigure) {
                    // First configure of the fresh swapchain, through the quiesce handshake.
                    let pump = || presenter.window.pump();
                    let octx = OutputCtx { stats, pump: &pump };
                    match configure_initial(
                        &presenter.surface,
                        &presenter.config,
                        shared,
                        &octx,
                        stop,
                        &mut presenter.worker,
                    ) {
                        ConfigureOutcome::Ran => {
                            unconfigured = false;
                            recovering = false;
                            submit = Some(SubmitGuard::enter(&shared.submitters));
                            last_present = Instant::now();
                            last_reconfigure = Instant::now();
                            last_mode_set = Instant::now();
                            expect_outdated_until = Instant::now() + SELF_INFLICTED_GRACE;
                            starved_since = None;
                            starved_seen = false;
                            recreate_failures = 0;
                            warn_rebuilt(shared, &output.device, rebuilds);
                        }
                        ConfigureOutcome::Wedged => {
                            // A late answer leaves the swapchain built for the settled check above.
                            unconfigured = false;
                            recovering_since = Instant::now();
                            warn_configure_wedged(shared, &output.device);
                        }
                        ConfigureOutcome::Skipped => {
                            recovering_since = Instant::now();
                            log::warn!(
                                "[stress-kit/gpu_display] {}: the rebuilt swapchain could not be \
                                 configured while the siblings were presenting; retrying in {}s",
                                output.device,
                                REBUILD_AFTER.as_secs()
                            );
                        }
                    }
                    continue;
                }
                if step == Recovery::Exhausted && !exhausted_reported {
                    exhausted_reported = true;
                    warn_rebuilds_exhausted(shared, &output.device);
                }
                std::thread::sleep(RECOVER_POLL);
                continue;
            }

            if let Some(frame) = wedge_after
                && stats.presented.load(Ordering::Relaxed) >= frame
            {
                log::error!(
                    "[stress-kit/gpu_display] STRESSKIT_DISPLAY_DEBUG_WEDGE: wedging {} on \
                     purpose after {frame} frame(s); this thread stops advancing and stops \
                     pumping, which is what the watchdog has to catch",
                    output.device
                );
                loop {
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
            // Bumped every pass, before anything that can block: the watchdog
            // reads it to tell a present that stops returning frames from a
            // thread that stops running at all.
            stats.progress.fetch_add(1, Ordering::Relaxed);
            stats.set_phase(Phase::Pumping);
            presenter.window.pump();
            // Every bounded wait in the handshake pumps through this, so a
            // thread that is waiting still answers the message broadcast a
            // sibling's mode change is blocked on.
            let pump = || presenter.window.pump();
            let octx = OutputCtx { stats, pump: &pump };
            shared.park_if_paused(&octx, stop);

            let elapsed = started.elapsed().as_secs_f32();
            presenter.queue.write_buffer(
                &presenter.frame_buf,
                0,
                bytemuck::bytes_of(&Frame {
                    time: elapsed,
                    tint: 0.25 + 0.75 * (index as f32 / total.max(1) as f32),
                    band: (elapsed * 0.35).fract(),
                    inv_width: 1.0 / presenter.config.width.max(1) as f32,
                    iters: SHADER_ITERS,
                    _pad: [0; 3],
                }),
            );

            stats.set_phase(Phase::Acquiring);
            match presenter.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(frame)
                | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                    stats.set_phase(Phase::Presenting);
                    draw_and_present(
                        &presenter.device,
                        &presenter.queue,
                        &presenter.pipeline,
                        &presenter.bind_group,
                        frame,
                    );
                    stats.presented.fetch_add(1, Ordering::Relaxed);
                    last_present = Instant::now();
                    starved_since = None;
                    starved_seen = false;
                    if stall_reported {
                        stall_reported = false;
                        stats.stalled.store(false, Ordering::Relaxed);
                    }
                }
                wgpu::CurrentSurfaceTexture::Timeout => {
                    stats.timeouts.fetch_add(1, Ordering::Relaxed);
                    shared.set_warn(format!(
                        "gpu_display: present timed out acquiring a frame on {}",
                        output.device
                    ));
                    // Backed off so a wedged surface does not spin a core.
                    std::thread::sleep(FAULT_BACKOFF);
                }
                wgpu::CurrentSurfaceTexture::Outdated => {
                    if Instant::now() > expect_outdated_until {
                        stats.unexpected_outdated.fetch_add(1, Ordering::Relaxed);
                        shared.set_warn(format!(
                            "gpu_display: swapchain went outdated on {} with no mode change from \
                             this stage",
                            output.device
                        ));
                    }
                    // Left outdated when the siblings stay busy; retried next frame.
                    match request_configure(
                        shared,
                        &octx,
                        stop,
                        &mut presenter.worker,
                        &presenter.surface,
                        &presenter.config,
                        true,
                    ) {
                        ConfigureOutcome::Ran => {
                            expect_outdated_until = Instant::now() + SELF_INFLICTED_GRACE;
                            starved_since = None;
                        }
                        ConfigureOutcome::Skipped => {
                            starved_since.get_or_insert_with(Instant::now);
                            starved_seen = true;
                        }
                        ConfigureOutcome::Wedged => {
                            recovering = true;
                            recovering_since = Instant::now();
                            drop(submit.take());
                            warn_configure_wedged(shared, &output.device);
                        }
                    }
                }
                wgpu::CurrentSurfaceTexture::Lost => {
                    stats.lost.fetch_add(1, Ordering::Relaxed);
                    shared.set_warn(format!(
                        "gpu_display: surface lost on {}, recreating",
                        output.device
                    ));
                    match create_surface(ctx, presenter.raw_handle) {
                        Ok(fresh) => {
                            presenter.surface = Arc::new(fresh);
                            // A fresh surface stays unconfigured until the siblings
                            // go quiet; the next frame reports Outdated and retries.
                            if request_configure(
                                shared,
                                &octx,
                                stop,
                                &mut presenter.worker,
                                &presenter.surface,
                                &presenter.config,
                                true,
                            ) == ConfigureOutcome::Wedged
                            {
                                recovering = true;
                                recovering_since = Instant::now();
                                drop(submit.take());
                                warn_configure_wedged(shared, &output.device);
                            }
                            expect_outdated_until = Instant::now() + SELF_INFLICTED_GRACE;
                            recreate_failures = 0;
                        }
                        Err(e) => {
                            recreate_failures += 1;
                            if recreate_failures >= MAX_SURFACE_RECREATES {
                                shared.latch_fatal(format!(
                                    "gpu_display: device lost on {} — the surface could not be \
                                     recreated after {MAX_SURFACE_RECREATES} attempts ({e})",
                                    output.device
                                ));
                                return;
                            }
                        }
                    }
                    std::thread::sleep(FAULT_BACKOFF);
                }
                wgpu::CurrentSurfaceTexture::Occluded => {
                    stats.occluded.fetch_add(1, Ordering::Relaxed);
                    std::thread::sleep(FAULT_BACKOFF);
                }
                wgpu::CurrentSurfaceTexture::Validation => {
                    stats.validation.fetch_add(1, Ordering::Relaxed);
                    shared.set_warn(format!(
                        "gpu_display: inconclusive - the swapchain on {} raised a validation \
                         error, so this stage's own commands were rejected",
                        output.device
                    ));
                    std::thread::sleep(FAULT_BACKOFF);
                }
            }

            // The rest of the pass reads timers this output has stopped
            // driving, and would ask the worker for a configure it is already
            // inside.
            if recovering {
                continue;
            }

            if let Some(reason) = ctx.health.failure() {
                shared.latch_fatal(format!("gpu_display: {reason}"));
                return;
            }

            // Ahead of the stall check and on a shorter fuse: an output that
            // cannot get the stage's own configure turn is being starved by a
            // sibling, and reporting that as a stalled present queue is what
            // made a tool bug read as a display fault.
            if let Some(since) = starved_since {
                let starved = since.elapsed();
                if starved >= HANG_STARVED {
                    let holder = shared
                        .outputs
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| *i != index)
                        .map(|(i, s)| format!("output {i} {}", s.phase().label()))
                        .collect::<Vec<_>>()
                        .join(", ");
                    shared.latch_hang(format!(
                        "gpu_display: {} {} could not take the stage's configure turn for {}s; a \
                         sibling output thread is holding it ({holder}). This is the stressor \
                         blocking itself, so the run grades INCONCLUSIVE: it is not evidence \
                         about the display path in either direction.",
                        crate::STRESSOR_HANG_MARKER,
                        output.device,
                        starved.as_secs()
                    ));
                    return;
                }
            }

            let idle = last_present.elapsed();
            if idle >= STALL_FATAL {
                if starved_seen {
                    // The stage's own handshake was in the way during this
                    // stall, so the present queue is not what this proves.
                    shared.latch_hang(format!(
                        "gpu_display: {} no frame has been presented on {} for {}s, and the \
                         stage's own configure handshake turned this output away during that \
                         window. The stressor obstructed itself, so the run grades INCONCLUSIVE \
                         rather than reporting a stalled present queue.",
                        crate::STRESSOR_HANG_MARKER,
                        output.device,
                        idle.as_secs()
                    ));
                } else {
                    shared.latch_fatal(format!(
                        "gpu_display: no frame has been presented on {} for {}s; the present \
                         queue is stalled",
                        output.device,
                        idle.as_secs()
                    ));
                }
                return;
            }
            if idle >= STALL_WARN && !stall_reported {
                stall_reported = true;
                stats.stalled.store(true, Ordering::Relaxed);
                shared.set_warn(format!(
                    "gpu_display: no frame presented on {} for {}s",
                    output.device,
                    idle.as_secs()
                ));
            }

            if last_reconfigure.elapsed() >= RECONFIGURE_EVERY {
                last_reconfigure = Instant::now();
                present_mode_index = present_mode_index.wrapping_add(1);
                match reconfigure(
                    &presenter.surface,
                    &mut presenter.config,
                    &presenter.present_modes,
                    present_mode_index,
                    &output,
                    &presenter.window,
                    shared,
                    &octx,
                    stop,
                    &mut presenter.worker,
                ) {
                    ConfigureOutcome::Ran => {
                        stats.reconfigures.fetch_add(1, Ordering::Relaxed);
                        expect_outdated_until = Instant::now() + SELF_INFLICTED_GRACE;
                        starved_since = None;
                    }
                    ConfigureOutcome::Skipped => {
                        starved_since.get_or_insert_with(Instant::now);
                        starved_seen = true;
                    }
                    ConfigureOutcome::Wedged => {
                        recovering = true;
                        recovering_since = Instant::now();
                        drop(submit.take());
                        warn_configure_wedged(shared, &output.device);
                    }
                }
            }

            if !recovering && policy != ModeSetPolicy::Off && last_mode_set.elapsed() >= MODE_SET_EVERY
            {
                last_mode_set = Instant::now();
                if let Some((width, height, hz)) = modes.next_mode() {
                    // Registered with the stage before the call, not after:
                    // `ChangeDisplaySettingsEx` can wedge, and teardown still
                    // has to put this display back.
                    shared.touch_mode(&output.device);
                    stats.set_phase(Phase::ModeSetting);
                    match apply_mode(&output.device, width, height, hz) {
                        Ok(()) => {
                            stats.mode_sets.fetch_add(1, Ordering::Relaxed);
                            log::debug!(
                                "[stress-kit/gpu_display] {}: mode set to {width}x{height}@{hz}",
                                output.device
                            );
                            presenter.window.move_to(output.x, output.y, width, height);
                            presenter.config.width = width.max(1);
                            presenter.config.height = height.max(1);
                            if request_configure(
                                shared,
                                &octx,
                                stop,
                                &mut presenter.worker,
                                &presenter.surface,
                                &presenter.config,
                                true,
                            ) == ConfigureOutcome::Wedged
                            {
                                recovering = true;
                                recovering_since = Instant::now();
                                drop(submit.take());
                                warn_configure_wedged(shared, &output.device);
                            }
                            expect_outdated_until = Instant::now() + SELF_INFLICTED_GRACE;
                        }
                        Err(e) => shared.set_warn(format!("gpu_display: {e}")),
                    }
                }
            }
        }
    }

    /// Reads `STRESSKIT_DISPLAY_DEBUG_WEDGE=<output>[:<frames>]`, which wedges
    /// one output thread on purpose so the watchdog, the terminal outcome and
    /// the teardown can be verified on real multi-output hardware instead of
    /// only when the bug recurs. `None` unless the variable names this output.
    fn debug_wedge_frame(index: usize) -> Option<u64> {
        let raw = std::env::var("STRESSKIT_DISPLAY_DEBUG_WEDGE").ok()?;
        let (target, frames) = raw.split_once(':').unwrap_or((raw.as_str(), "30"));
        (target.trim().parse::<usize>().ok()? == index)
            .then(|| frames.trim().parse::<u64>().unwrap_or(30))
    }

    /// Marks an output finished on every exit path, so the watchdog never
    /// reports a returned thread as stuck in whatever it was last doing.
    struct PhaseDone<'a>(&'a OutputStats);

    impl Drop for PhaseDone<'_> {
        fn drop(&mut self) {
            self.0.set_phase(Phase::Done);
        }
    }

    /// The instance is built without a display handle, so the surface target
    /// has to carry one or wgpu-core rejects it as `MissingDisplayHandle`.
    fn surface_target(raw_handle: wgpu::rwh::RawWindowHandle) -> wgpu::SurfaceTargetUnsafe {
        wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(wgpu::rwh::RawDisplayHandle::Windows(
                wgpu::rwh::WindowsDisplayHandle::new(),
            )),
            raw_window_handle: raw_handle,
        }
    }

    fn create_surface(
        ctx: &Arc<GpuContext>,
        raw_handle: wgpu::rwh::RawWindowHandle,
    ) -> Result<wgpu::Surface<'static>, String> {
        unsafe { ctx.instance.create_surface_unsafe(surface_target(raw_handle)) }
            .map_err(|e| e.to_string())
    }

    /// One (re)configure of `surface`, serialized against sibling configures
    /// and presents by the quiesce handshake and bounded by the worker.
    #[allow(clippy::too_many_arguments)]
    fn request_configure(
        shared: &Shared,
        octx: &OutputCtx<'_>,
        stop: &AtomicBool,
        worker: &mut SurfaceWorker,
        surface: &Arc<wgpu::Surface<'static>>,
        config: &wgpu::SurfaceConfiguration,
        self_submits: bool,
    ) -> ConfigureOutcome {
        match shared.with_quiesce(octx, stop, self_submits, || {
            worker.submit((surface.clone(), config.clone()), octx, stop)
        }) {
            // Counted only when the stage is still running: a teardown that
            // interrupts a configure is not the stage obstructing itself.
            Some(ConfigureOutcome::Wedged) if !stop.load(Ordering::Relaxed) => {
                shared.configure_wedges.fetch_add(1, Ordering::Relaxed);
                ConfigureOutcome::Wedged
            }
            Some(ConfigureOutcome::Wedged) | None => ConfigureOutcome::Skipped,
            Some(outcome) => outcome,
        }
    }

    /// Says which output stopped presenting and why, in the stage's own words:
    /// a configure that does not return is the tool, not the display path.
    /// Carries the limit marker, never the inconclusive one, so the runner
    /// keeps it as a warning on the stage instead of failing the run.
    fn warn_configure_wedged(shared: &Shared, device: &str) {
        shared.set_warn(format!(
            "gpu_display: {} Surface::configure on {device} has not returned after {}s, so that \
             output stopped presenting while the others carry on; it is rebuilt if the call \
             stays out. A configure waits for its device to go idle with no timeout; this is the \
             stressor blocking itself and is not evidence about the display path. Coverage \
             limit imposed by the tool, not a hardware fault.",
            crate::STRESSOR_LIMIT_MARKER,
            CONFIGURE_WAIT.as_secs()
        ));
    }

    /// Says an output is presenting again on a fresh window, device and
    /// swapchain, and that the stuck attempt was left behind to get there.
    fn warn_rebuilt(shared: &Shared, device: &str, rebuilds: u32) {
        shared.set_warn(format!(
            "gpu_display: {} rebuilt the window, device and swapchain on {device} (rebuild \
             {rebuilds} of {MAX_REBUILDS}) after Surface::configure did not return within {}s; \
             the stuck call is left on its own thread and that output is presenting again. \
             Coverage was interrupted by the tool, not by a hardware fault.",
            crate::STRESSOR_LIMIT_MARKER,
            CONFIGURE_WAIT.as_secs()
        ));
    }

    /// Says an output stays down: every rebuild ended in another configure
    /// that did not return, so the stage stops leaking attempts on it.
    fn warn_rebuilds_exhausted(shared: &Shared, device: &str) {
        shared.set_warn(format!(
            "gpu_display: {} {device} stays down for the rest of the stage: {MAX_REBUILDS} \
             rebuild(s) each ended in a Surface::configure that did not return within {}s, and \
             the other outputs carry on without it. Coverage limit imposed by the tool, not a \
             hardware fault.",
            crate::STRESSOR_LIMIT_MARKER,
            CONFIGURE_WAIT.as_secs()
        ));
    }

    /// First configure of a fresh surface, serialized against sibling
    /// configures and presents. Bounded retries.
    #[allow(clippy::too_many_arguments)]
    fn configure_initial(
        surface: &Arc<wgpu::Surface<'static>>,
        config: &wgpu::SurfaceConfiguration,
        shared: &Shared,
        octx: &OutputCtx<'_>,
        stop: &AtomicBool,
        worker: &mut SurfaceWorker,
    ) -> ConfigureOutcome {
        for _ in 0..INITIAL_CONFIGURE_ATTEMPTS {
            if stop.load(Ordering::Relaxed) {
                return ConfigureOutcome::Skipped;
            }
            // Each attempt is progress: bring-up next to saturated CPU lanes
            // can take several quiesce rounds, and a thread still working
            // through them is not wedged.
            octx.stats.progress.fetch_add(1, Ordering::Relaxed);
            match request_configure(shared, octx, stop, worker, surface, config, false) {
                ConfigureOutcome::Skipped => {}
                outcome => return outcome,
            }
        }
        ConfigureOutcome::Skipped
    }

    fn draw_and_present(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &wgpu::RenderPipeline,
        bind_group: &wgpu::BindGroup,
        frame: wgpu::SurfaceTexture,
    ) {
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gpu_display encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gpu_display pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit(std::iter::once(encoder.finish()));
        queue.present(frame);
    }

    /// Cycles present mode, frame latency, and the presented size — the
    /// swapchain rebuild path a mode change also takes. `config` is left
    /// updated for a later attempt when the configure does not run.
    #[allow(clippy::too_many_arguments)]
    fn reconfigure(
        surface: &Arc<wgpu::Surface<'static>>,
        config: &mut wgpu::SurfaceConfiguration,
        present_modes: &[wgpu::PresentMode],
        step: usize,
        output: &Output,
        window: &OutputWindow,
        shared: &Shared,
        octx: &OutputCtx<'_>,
        stop: &AtomicBool,
        worker: &mut SurfaceWorker,
    ) -> ConfigureOutcome {
        if !present_modes.is_empty() {
            config.present_mode = present_modes[step % present_modes.len()];
        }
        config.desired_maximum_frame_latency = 1 + (step % 3) as u32;

        // Every third pass halves the presented size to force a resize.
        let (width, height) = if step % 3 == 2 {
            ((output.width / 2).max(1), (output.height / 2).max(1))
        } else {
            (output.width, output.height)
        };
        window.move_to(output.x, output.y, width, height);
        config.width = width;
        config.height = height;
        request_configure(shared, octx, stop, worker, surface, config, true)
    }

    /// The modes one output rotates through, native mode first.
    struct ModeCycle {
        modes: Vec<(u32, u32, u32)>,
        next: usize,
    }

    impl ModeCycle {
        fn new(output: &Output, policy: ModeSetPolicy) -> Self {
            let mut modes: Vec<(u32, u32, u32)> = Vec::new();
            let rates = refresh_modes_at(&output.device, output.width, output.height);
            for hz in rates {
                modes.push((output.width, output.height, hz));
            }
            if policy == ModeSetPolicy::Full {
                for (width, height) in resolutions(&output.device) {
                    if width == output.width && height == output.height {
                        continue;
                    }
                    if let Some(&hz) = refresh_modes_at(&output.device, width, height).last() {
                        modes.push((width, height, hz));
                    }
                }
            }
            // Native mode first, so the cycle always returns to it.
            modes.sort_by_key(|&(w, h, hz)| {
                (
                    (w, h) != (output.width, output.height),
                    u32::MAX - hz,
                )
            });
            log::info!(
                "[stress-kit/gpu_display] {}: {} mode(s) in the cycle",
                output.device,
                modes.len()
            );
            Self { modes, next: 0 }
        }

        /// `None` when the display advertises nothing to switch between.
        fn next_mode(&mut self) -> Option<(u32, u32, u32)> {
            if self.modes.len() < 2 {
                return None;
            }
            let mode = self.modes[self.next % self.modes.len()];
            self.next = self.next.wrapping_add(1);
            Some(mode)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        use super::super::super::gpu_common::GpuHealth;

        /// Nothing to pump in a test: no window is created, so the handshake's
        /// keep-the-window-alive hook is a no-op.
        const NO_PUMP: fn() = || {};

        /// Handshake context for a test thread that owns no window.
        fn test_ctx(stats: &OutputStats) -> OutputCtx<'_> {
            OutputCtx {
                stats,
                pump: &NO_PUMP,
            }
        }

        /// Fake presenter for the quiesce handshake: enters the frame loop
        /// and parks whenever a configure asks, without touching any GPU.
        fn spawn_presenter(
            shared: Arc<Shared>,
            stop: Arc<AtomicBool>,
        ) -> std::thread::JoinHandle<()> {
            std::thread::Builder::new()
                .name("test-presenter".into())
                .spawn(move || {
                    let stats = OutputStats::default();
                    let ctx = test_ctx(&stats);
                    let _submit = SubmitGuard::enter(&shared.submitters);
                    while !stop.load(Ordering::Relaxed) {
                        shared.park_if_paused(&ctx, &stop);
                        std::thread::sleep(Duration::from_micros(50));
                    }
                })
                .expect("spawn test presenter")
        }

        fn await_submitters(shared: &Shared, count: u32) {
            let deadline = Instant::now() + Duration::from_secs(5);
            while shared.submitters.load(Ordering::SeqCst) < count {
                assert!(Instant::now() < deadline, "presenters never entered the loop");
                std::thread::sleep(Duration::from_micros(50));
            }
        }

        /// The dual-output crash shape: a first-time configure must not run
        /// until every presenting sibling is parked.
        #[test]
        fn startup_configure_parks_every_presenting_sibling() {
            let shared = Arc::new(Shared::default());
            let stop = Arc::new(AtomicBool::new(false));
            let a = spawn_presenter(shared.clone(), stop.clone());
            let b = spawn_presenter(shared.clone(), stop.clone());
            await_submitters(&shared, 2);

            let stats = OutputStats::default();
            let ctx = test_ctx(&stats);
            let parked_during = shared.with_quiesce(&ctx, &stop, false, || {
                shared.parked.load(Ordering::SeqCst)
            });
            assert_eq!(
                parked_during,
                Some(2),
                "configure ran without both presenting siblings parked"
            );

            stop.store(true, Ordering::SeqCst);
            a.join().unwrap();
            b.join().unwrap();
            assert_eq!(shared.parked.load(Ordering::SeqCst), 0, "a park was leaked");
        }

        /// Threads still in setup neither submit nor park; a startup configure
        /// must run immediately instead of waiting on them.
        #[test]
        fn startup_configure_ignores_threads_still_in_setup() {
            let shared = Arc::new(Shared::default());
            // Live threads that have not reached their frame loop.
            shared.threads_live.store(3, Ordering::SeqCst);
            let stop = AtomicBool::new(false);
            let stats = OutputStats::default();
            let ctx = test_ctx(&stats);
            let started = Instant::now();
            assert_eq!(shared.with_quiesce(&ctx, &stop, false, || true), Some(true));
            assert!(
                started.elapsed() < QUIESCE_TIMEOUT,
                "configure waited on siblings that cannot park"
            );
        }

        /// A caller inside its own frame loop counts itself out of the
        /// handshake when it is the only presenter.
        #[test]
        fn lone_presenter_configures_directly() {
            let shared = Arc::new(Shared::default());
            let _submit = SubmitGuard::enter(&shared.submitters);
            let stop = AtomicBool::new(false);
            let stats = OutputStats::default();
            let ctx = test_ctx(&stats);
            assert_eq!(shared.with_quiesce(&ctx, &stop, true, || 7), Some(7));
        }

        /// A sibling that never parks bounds the configure instead of
        /// wedging it, and the skip is counted.
        #[test]
        fn configure_skips_when_a_sibling_never_parks() {
            let shared = Arc::new(Shared::default());
            let stop = Arc::new(AtomicBool::new(false));
            let hot = {
                let shared = shared.clone();
                let stop = stop.clone();
                std::thread::spawn(move || {
                    let _submit = SubmitGuard::enter(&shared.submitters);
                    while !stop.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_micros(50));
                    }
                })
            };
            await_submitters(&shared, 1);

            let stats = OutputStats::default();
            let ctx = test_ctx(&stats);
            assert_eq!(shared.with_quiesce(&ctx, &stop, false, || ()), None);
            assert_eq!(shared.quiesce_timeouts.load(Ordering::Relaxed), 1);
            assert_eq!(
                shared.turn_timeouts.load(Ordering::Relaxed),
                0,
                "a busy sibling was misreported as a held configure turn"
            );

            stop.store(true, Ordering::SeqCst);
            hot.join().unwrap();
        }

        /// The concurrent verify-mix crash shape (heap corruption on
        /// DESKTOP-NFOQK4J, run `0fa3d84d`): a configure taken on the
        /// zero-submitter fast path must still raise the pause, so a sibling
        /// that reaches its frame loop mid-configure parks instead of
        /// submitting into the swapchain build.
        #[test]
        fn thread_entering_frame_loop_mid_configure_parks_first() {
            let shared = Arc::new(Shared::default());
            let stop = Arc::new(AtomicBool::new(false));
            let release = Arc::new(AtomicBool::new(false));
            let submitted = Arc::new(AtomicBool::new(false));

            // Sibling in setup: enters its frame loop only once released,
            // then walks drive_output's per-frame order — park, then submit.
            let entrant = {
                let shared = shared.clone();
                let stop = stop.clone();
                let release = release.clone();
                let submitted = submitted.clone();
                std::thread::Builder::new()
                    .name("test-entrant".into())
                    .spawn(move || {
                        while !release.load(Ordering::SeqCst) {
                            std::thread::sleep(Duration::from_micros(20));
                        }
                        let stats = OutputStats::default();
                        let ctx = test_ctx(&stats);
                        let _submit = SubmitGuard::enter(&shared.submitters);
                        while !stop.load(Ordering::Relaxed) {
                            shared.park_if_paused(&ctx, &stop);
                            submitted.store(true, Ordering::SeqCst);
                            std::thread::sleep(Duration::from_micros(50));
                        }
                    })
                    .expect("spawn test entrant")
            };

            // No submitters yet, so this configure takes the fast path. The
            // sibling is released mid-configure and must park, not submit.
            let stats = OutputStats::default();
            let ctx = test_ctx(&stats);
            let submitted_mid_configure = shared.with_quiesce(&ctx, &stop, false, || {
                release.store(true, Ordering::SeqCst);
                let deadline = Instant::now() + Duration::from_secs(5);
                loop {
                    if submitted.load(Ordering::SeqCst) {
                        break true;
                    }
                    if shared.parked.load(Ordering::SeqCst) >= 1 {
                        break false;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "entrant never reached its frame loop"
                    );
                    std::thread::sleep(Duration::from_micros(50));
                }
            });
            assert_eq!(
                submitted_mid_configure,
                Some(false),
                "a thread entering its frame loop submitted while a fast-path configure was in flight"
            );

            stop.store(true, Ordering::SeqCst);
            entrant.join().unwrap();
            assert!(
                submitted.load(Ordering::SeqCst),
                "entrant never submitted after the configure finished"
            );
            assert_eq!(shared.parked.load(Ordering::SeqCst), 0, "a park was leaked");
        }

        /// A configure worker that blocks until released, standing in for a
        /// `Surface::configure` that does not return.
        fn spawn_blocking_worker(release: Arc<AtomicBool>) -> ConfigureWorker<()> {
            ConfigureWorker::spawn(0, move |()| {
                while !release.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(1));
                }
            })
            .expect("spawn configure worker")
        }

        /// The wedge from service order 2151936, at the call that causes it:
        /// `Surface::configure` waits for its device — present queue included —
        /// with no timeout, so it must be bounded off-thread or it takes the
        /// stage with it.
        #[test]
        fn a_configure_that_never_returns_is_bounded() {
            let release = Arc::new(AtomicBool::new(false));
            let mut worker = spawn_blocking_worker(release.clone());
            let stop = AtomicBool::new(false);
            let stats = OutputStats::default();
            let ctx = test_ctx(&stats);

            let started = Instant::now();
            let outcome = worker.submit((), &ctx, &stop);
            let waited = started.elapsed();

            assert_eq!(outcome, ConfigureOutcome::Wedged);
            assert!(
                waited >= CONFIGURE_WAIT && waited < CONFIGURE_WAIT + Duration::from_secs(2),
                "the caller waited {waited:?} on a configure bounded at {CONFIGURE_WAIT:?}"
            );
            // Still running: presenting again before it answers is the
            // half-built-swapchain race.
            assert!(
                !worker.settled(),
                "a running configure reported as finished"
            );

            release.store(true, Ordering::SeqCst);
            let deadline = Instant::now() + Duration::from_secs(5);
            while !worker.settled() {
                assert!(Instant::now() < deadline, "the worker never came back");
                std::thread::sleep(Duration::from_millis(2));
            }
        }

        /// A wedged configure must cost one output, not the stage: the pause
        /// is lifted and the turn goes back, so the siblings resume.
        #[test]
        fn a_wedged_configure_releases_the_handshake() {
            let release = Arc::new(AtomicBool::new(false));
            let mut worker = spawn_blocking_worker(release.clone());
            let shared = Arc::new(Shared::default());
            let stop = Arc::new(AtomicBool::new(false));
            let presenter = spawn_presenter(shared.clone(), stop.clone());
            await_submitters(&shared, 1);

            let stats = OutputStats::default();
            let ctx = test_ctx(&stats);
            let outcome = shared
                .with_quiesce(&ctx, &stop, false, || worker.submit((), &ctx, &stop))
                .expect("the handshake never reached the configure");

            assert_eq!(outcome, ConfigureOutcome::Wedged);
            assert!(
                !shared.configure_pause.load(Ordering::SeqCst),
                "a wedged configure left every sibling parked"
            );
            assert!(
                shared.configure_turn.try_lock().is_ok(),
                "a wedged configure kept the turn, so no sibling can ever configure"
            );

            release.store(true, Ordering::SeqCst);
            stop.store(true, Ordering::SeqCst);
            presenter.join().unwrap();
            assert_eq!(shared.parked.load(Ordering::SeqCst), 0, "a park was leaked");
        }

        /// The handshake plus a bounded configure must fit inside the
        /// watchdog: siblings present nothing while they are parked, so a
        /// configure allowed to run longer than the stall bound would trip the
        /// watchdog on a stage that is about to recover on its own.
        #[test]
        fn a_bounded_configure_fits_inside_the_watchdog() {
            assert!(
                TURN_WAIT + QUIESCE_TIMEOUT + CONFIGURE_WAIT < WATCHDOG_STALL,
                "a configure can hold the siblings parked past the watchdog's fuse"
            );
            assert!(
                CONFIGURE_WAIT < QUIESCE_PARK_MAX,
                "a parked sibling gives up before the configure it is parked for does"
            );
            assert!(
                CONFIGURE_WAIT + REBUILD_AFTER < WATCHDOG_STALL,
                "a stage whose every output wedges at once must reach its rebuilds — which \
                 count as progress — before the watchdog gives up on it"
            );
            assert!(
                CONFIGURE_WAIT + REBUILD_AFTER < COVERAGE_WARMUP,
                "an output whose first configure wedged must be rebuilt before the coverage \
                 count settles, or every startup wedge files a shortfall it then has to resolve"
            );
        }

        /// The park bound must outlast the watchdog, or a sibling parked for a
        /// wedged configure resumes and submits into a half-built swapchain in
        /// the window between the two — the crash the handshake exists to stop.
        #[test]
        fn a_parked_sibling_never_outlives_the_watchdog() {
            assert!(
                QUIESCE_PARK_MAX > WATCHDOG_STALL,
                "the watchdog must end a wedged stage before a parked sibling gives up"
            );
            assert!(
                HANG_STARVED < WATCHDOG_STALL,
                "the starved output should report before the aggregate watchdog, so the \
                 message can name which sibling is holding the turn"
            );
            assert!(
                TURN_WAIT < HANG_STARVED,
                "a turn wait longer than the starvation fuse can never be observed as starvation"
            );
        }

        /// The hang shape from service order 2151936: one output thread holds
        /// the configure turn and never gives it back (a `Surface::configure`
        /// that does not return), and every sibling needs it.
        ///
        /// Before the fix, `with_quiesce` took the turn with a blocking
        /// `lock()`, so the siblings piled up on the mutex *below* their frame
        /// loop's stall check: nothing could report, `run` blocked joining
        /// them, and the child process outlived its own belt. The wait is now
        /// bounded, so a starved sibling always comes back and can be graded.
        #[test]
        fn a_held_configure_turn_never_blocks_a_sibling_indefinitely() {
            let shared = Arc::new(Shared::default());
            let stop = Arc::new(AtomicBool::new(false));
            let holding = Arc::new(AtomicBool::new(false));

            // Stands in for the wedged configure: takes the turn, keeps it.
            let wedged = {
                let shared = shared.clone();
                let stop = stop.clone();
                let holding = holding.clone();
                std::thread::Builder::new()
                    .name("test-wedged-configure".into())
                    .spawn(move || {
                        let stats = OutputStats::default();
                        let ctx = test_ctx(&stats);
                        let _turn = shared.take_turn(&ctx, &stop).expect("turn was free");
                        holding.store(true, Ordering::SeqCst);
                        while !stop.load(Ordering::Relaxed) {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                    })
                    .expect("spawn wedged configurer")
            };

            let deadline = Instant::now() + Duration::from_secs(5);
            while !holding.load(Ordering::SeqCst) {
                assert!(Instant::now() < deadline, "the turn was never taken");
                std::thread::sleep(Duration::from_millis(5));
            }

            let stats = OutputStats::default();
            let ctx = test_ctx(&stats);
            let started = Instant::now();
            let out = shared.with_quiesce(&ctx, &stop, true, || unreachable!());
            let waited = started.elapsed();

            assert_eq!(out, None, "a configure ran while the turn was held elsewhere");
            assert!(
                waited < TURN_WAIT + Duration::from_secs(2),
                "the sibling waited {waited:?} on a held turn, so it never reaches its own \
                 stall check"
            );
            assert_eq!(
                shared.turn_timeouts.load(Ordering::Relaxed),
                1,
                "a held turn was not counted as one"
            );

            stop.store(true, Ordering::SeqCst);
            wedged.join().unwrap();
        }

        /// Frozen frames with frozen frame loops on two outputs: the stage is
        /// wedged in itself, so the watchdog fires and the report says so.
        #[test]
        fn watchdog_fires_when_two_outputs_stop_advancing() {
            let mut shared = Shared::default();
            shared.outputs.resize_with(2, OutputStats::default);
            let shared = Arc::new(shared);
            shared.outputs[0].set_phase(Phase::Configuring);
            shared.outputs[1].set_phase(Phase::AwaitingTurn);
            shared.turn_timeouts.store(4, Ordering::Relaxed);
            shared.outputs_rebuilt.store(2, Ordering::Relaxed);

            let start = Instant::now();
            let stall = Duration::from_millis(120);
            let mut watchdog = Watchdog::with_limits(start, stall, Duration::from_millis(0));

            // Both outputs presented, then stopped: frames and loops frozen.
            assert!(!watchdog.wedged(20, 40, start, Duration::from_secs(60)));
            let fired = watchdog.wedged(20, 40, start + stall, Duration::from_secs(60));
            assert!(fired, "the watchdog never fired on a fully frozen stage");

            let devices = [r"\\.\DISPLAY1".to_string(), r"\\.\DISPLAY2".to_string()];
            let report = hang_report(&shared, &devices, stall);

            assert!(
                report.contains(crate::STRESSOR_HANG_MARKER),
                "the report carries no stressor_hang marker: {report}"
            );
            assert!(
                !report.to_ascii_lowercase().contains("inconclusive -"),
                "the hang marker must not be shadowed by the generic inconclusive one: {report}"
            );
            assert!(report.contains("DISPLAY1"), "{report}");
            assert!(report.contains("inside Surface::configure"), "{report}");
            assert!(report.contains("waiting for the configure turn"), "{report}");
            assert!(
                report.contains("TOOL failure"),
                "the report does not say whose fault this is: {report}"
            );
            assert!(
                report.contains("2 output rebuild(s)"),
                "the report does not say how many outputs were rebuilt: {report}"
            );
        }

        /// The distinction the whole verdict rests on: loops still running with
        /// no frames coming out is a present stall, which IS evidence about the
        /// display path. The watchdog must stay out of it and leave that to the
        /// per-output stall check.
        #[test]
        fn a_present_stall_with_live_threads_is_not_a_hang() {
            let start = Instant::now();
            let stall = Duration::from_millis(120);
            let mut watchdog = Watchdog::with_limits(start, stall, Duration::from_millis(0));

            assert!(!watchdog.wedged(20, 40, start, Duration::from_secs(60)));
            // Frames frozen at 20, loops still turning.
            for step in 1..8u32 {
                let at = start + stall * step;
                assert!(
                    !watchdog.wedged(20, 40 + step as u64 * 100, at, Duration::from_secs(60)),
                    "the watchdog claimed a hang while the frame loops were still advancing"
                );
            }
        }

        /// Warmup covers adapter bring-up and the first swapchain on every
        /// output; a stage that has not started yet is not wedged.
        #[test]
        fn the_watchdog_stays_quiet_during_warmup() {
            let start = Instant::now();
            let stall = Duration::from_millis(50);
            let warmup = Duration::from_secs(20);
            let mut watchdog = Watchdog::with_limits(start, stall, warmup);

            assert!(!watchdog.wedged(0, 0, start + stall * 4, Duration::from_secs(3)));
            assert!(
                watchdog.wedged(0, 0, start + stall * 8, warmup),
                "the watchdog never armed after warmup"
            );
        }

        /// An output starved of the configure turn must file a hang, not the
        /// present-stall message: `no frame has been presented ... the present
        /// queue is stalled` on a healthy machine is exactly what was read as a
        /// hardware fault.
        #[test]
        fn starvation_latches_a_hang_and_names_the_holder() {
            let mut shared = Shared::default();
            shared.outputs.resize_with(2, OutputStats::default);
            let shared = Arc::new(shared);
            shared.outputs[1].set_phase(Phase::Configuring);

            assert!(shared.hang().is_none());
            shared.latch_hang(format!(
                "gpu_display: {} {} could not take the stage's configure turn for {}s; a sibling \
                 output thread is holding it (output 1 {}).",
                crate::STRESSOR_HANG_MARKER,
                r"\\.\DISPLAY1",
                HANG_STARVED.as_secs(),
                shared.outputs[1].phase().label()
            ));

            let latched = shared.hang().expect("a hang was not latched");
            assert!(latched.contains(crate::STRESSOR_HANG_MARKER), "{latched}");
            assert!(latched.contains("inside Surface::configure"), "{latched}");
            assert!(
                !latched.contains("present queue is stalled"),
                "a starved output filed itself as a display-path stall: {latched}"
            );
            // A second detector must not overwrite the first report.
            shared.latch_hang(format!(
                "gpu_display: {} a later report",
                crate::STRESSOR_HANG_MARKER
            ));
            assert_eq!(shared.hang().as_deref(), Some(latched.as_str()));
        }

        /// Teardown must not wait on the thread that is why teardown is
        /// happening: the bounded join is what lets the stage report at all.
        #[test]
        fn teardown_gives_up_on_a_thread_that_will_not_stop() {
            let shared = Arc::new(Shared::default());
            shared.threads_live.store(2, Ordering::SeqCst);

            let started = Instant::now();
            let stuck = await_threads(&shared, Duration::from_millis(200));
            assert_eq!(stuck, 2, "a wedged thread was reported as stopped");
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "teardown waited on threads that were never coming back"
            );

            shared.threads_live.store(0, Ordering::SeqCst);
            assert_eq!(await_threads(&shared, Duration::from_secs(5)), 0);
        }

        /// The coverage note's state machine: complaint while a shortfall
        /// stands, a one-shot `resolved -` when every output drives (not
        /// maskable by a standing warn), silence after.
        #[test]
        fn coverage_complaint_resolves_once_every_output_drives() {
            let mut shared = Shared::default();
            shared.outputs.resize_with(2, OutputStats::default);
            let shared = Arc::new(shared);
            let settled = COVERAGE_WARMUP + Duration::from_secs(1);

            // Only output 0 presents: complaint.
            shared.outputs[0].presented.store(1, Ordering::Relaxed);
            let note = standing_note(&shared, 2, true, settled).expect("complaint expected");
            assert!(note.contains("inconclusive -"), "{note}");
            assert!(note.contains("1 of 2"), "{note}");

            // Output 1 catches up: one resolution, then silence.
            shared.outputs[1].presented.store(1, Ordering::Relaxed);
            let resolved = standing_note(&shared, 2, true, settled).expect("resolution expected");
            assert!(resolved.starts_with("resolved -"), "{resolved}");
            assert_eq!(standing_note(&shared, 2, true, settled), None);

            // Re-complain (state forced back), set a standing warn, resolve
            // again: the resolution must outrank the warn, which then shows.
            shared.coverage_complained.store(true, Ordering::SeqCst);
            shared.set_warn("gpu_display: present timed out acquiring a frame on T".into());
            let resolved = standing_note(&shared, 2, true, settled).expect("resolution expected");
            assert!(resolved.starts_with("resolved -"), "{resolved}");
            let warn = standing_note(&shared, 2, true, settled).expect("warn expected");
            assert!(warn.contains("timed out"), "{warn}");
        }

        /// A configure that comes back late resolves the note it filed, and
        /// the note itself is the tool limiting its own coverage — never an
        /// `inconclusive -`, which would grade an otherwise clean 1800s run as
        /// proving nothing.
        #[test]
        fn a_recovered_configure_resolves_its_own_complaint() {
            let mut shared = Shared::default();
            shared.outputs.resize_with(2, OutputStats::default);
            let shared = Arc::new(shared);
            let settled = COVERAGE_WARMUP + Duration::from_secs(1);
            for stats in &shared.outputs {
                stats.presented.store(1, Ordering::Relaxed);
            }

            warn_configure_wedged(&shared, r"\\.\DISPLAY3");
            let note = standing_note(&shared, 2, true, settled).expect("complaint expected");
            assert!(note.contains(crate::STRESSOR_LIMIT_MARKER), "{note}");
            assert!(
                !note.contains("inconclusive -"),
                "a tool limit carries the inconclusive marker, which fails the run: {note}"
            );
            assert!(note.contains("DISPLAY3"), "{note}");

            // The resolution outranks the standing warn, and fires once.
            shared.configure_recovered.store(true, Ordering::SeqCst);
            let resolved = standing_note(&shared, 2, true, settled).expect("resolution expected");
            assert!(resolved.starts_with("resolved -"), "{resolved}");
            let warn = standing_note(&shared, 2, true, settled).expect("warn expected");
            assert!(warn.contains("has not returned"), "{warn}");
        }

        /// Every note the rebuild path files is the tool limiting itself: it
        /// carries the limit marker and neither the inconclusive nor the hang
        /// one, so the runner keeps it as a warning instead of failing the run
        /// or grading it a wedge.
        #[test]
        fn rebuild_notes_carry_the_limit_marker_only() {
            let shared = Shared::default();
            let device = r"\\.\DISPLAY2";
            warn_configure_wedged(&shared, device);
            let wedged = shared.warn().expect("wedge note");
            warn_rebuilt(&shared, device, 2);
            let rebuilt = shared.warn().expect("rebuilt note");
            warn_rebuilds_exhausted(&shared, device);
            let exhausted = shared.warn().expect("exhausted note");
            for note in [&wedged, &rebuilt, &exhausted] {
                assert!(
                    note.contains(crate::STRESSOR_LIMIT_MARKER),
                    "no limit marker: {note}"
                );
                assert!(
                    !note.to_ascii_lowercase().contains("inconclusive -"),
                    "a tool limit carries the inconclusive marker, which fails the run: {note}"
                );
                assert!(
                    !note.contains(crate::STRESSOR_HANG_MARKER),
                    "a tool limit carries the hang marker, which grades the run a wedge: {note}"
                );
                assert!(note.contains("DISPLAY2"), "{note}");
            }
            assert!(
                rebuilt.contains(&format!("rebuild 2 of {MAX_REBUILDS}")),
                "{rebuilt}"
            );
            assert!(
                exhausted.contains(&format!("{MAX_REBUILDS} rebuild(s)")),
                "{exhausted}"
            );
        }

        /// The recovery policy: wait out the grace, rebuild a stuck worker up
        /// to the cap, re-run a first configure the handshake turned away
        /// without leaking anything, and stop at the cap.
        #[test]
        fn recovery_waits_then_rebuilds_then_gives_up() {
            let early = REBUILD_AFTER - Duration::from_millis(1);
            let due = REBUILD_AFTER;
            assert_eq!(recovery_step(early, true, 0), Recovery::Wait);
            assert_eq!(recovery_step(early, false, 0), Recovery::Wait);
            assert_eq!(recovery_step(due, true, 0), Recovery::Rebuild);
            assert_eq!(
                recovery_step(due, true, MAX_REBUILDS - 1),
                Recovery::Rebuild
            );
            assert_eq!(recovery_step(due, true, MAX_REBUILDS), Recovery::Exhausted);
            assert_eq!(
                recovery_step(due, false, MAX_REBUILDS),
                Recovery::Reconfigure,
                "an unconfigured swapchain with no call out must be retried, never leaked or \
                 given up on"
            );
            assert_eq!(Phase::from_u8(Phase::Rebuilding as u8), Phase::Rebuilding);
        }

        /// A small window standing in for one output; the race under test is
        /// per-device, not per-monitor.
        fn test_output(base: &Output, offset_x: i32, name: &str) -> Output {
            Output {
                device: name.to_string(),
                x: base.x + offset_x,
                y: base.y,
                width: 320,
                height: 200,
                refresh_hz: base.refresh_hz,
                primary: false,
            }
        }

        fn present_clear_frame(
            device: &wgpu::Device,
            queue: &wgpu::Queue,
            frame: wgpu::SurfaceTexture,
        ) {
            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
            }
            queue.submit(std::iter::once(encoder.finish()));
            queue.present(frame);
        }

        /// Regression pin for the dual-output startup crash (ntdll heap AV,
        /// run `be8996be` on DESKTOP-NFOQK4J) in the topology the stage now
        /// uses: one logical device per output on one adapter, the second
        /// swapchain's first configure running while the first presents flat
        /// out. Both outputs present and neither device reports an error.
        ///
        /// The two surfaces deliberately do not share a device. `configure`
        /// waits for its device's present queue to drain, so a shared one
        /// makes each rebuild wait on flips the parked sibling has stopped
        /// draining — the wedge on service order 2151936.
        ///
        /// Run it deliberately with
        /// `cargo test -p stress-kit --lib -- --ignored two_swapchains`.
        #[test]
        #[ignore = "creates windows and drives real swapchains on whatever adapter answers"]
        fn two_swapchains_on_one_adapter_from_startup() {
            let Some(base) = enumerate_outputs().into_iter().next() else {
                eprintln!("no attached outputs in this session");
                return;
            };

            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::PRIMARY,
                ..wgpu::InstanceDescriptor::new_without_display_handle()
            });

            // Window/surface B live on this thread; its surface anchors
            // adapter selection before the presenter thread starts.
            let out_b = test_output(&base, 340, r"\\.\TEST-B");
            let window_b = OutputWindow::new(&out_b).expect("window B");
            let surface_b = unsafe {
                instance.create_surface_unsafe(surface_target(window_b.raw_handle().expect("raw B")))
            }
            .expect("surface B");

            let adapter = pollster::block_on(instance.request_adapter(
                &wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: Some(&surface_b),
                    force_fallback_adapter: false,
                    apply_limit_buckets: false,
                },
            ))
            .expect("adapter");

            let uncaptured = Arc::new(AtomicU64::new(0));
            let request = |label: &'static str| {
                let (device, queue) = pollster::block_on(adapter.request_device(
                    &wgpu::DeviceDescriptor {
                        label: Some(label),
                        required_features: wgpu::Features::empty(),
                        required_limits: wgpu::Limits::default(),
                        experimental_features: wgpu::ExperimentalFeatures::disabled(),
                        memory_hints: wgpu::MemoryHints::Performance,
                        trace: wgpu::Trace::Off,
                    },
                ))
                .unwrap_or_else(|e| panic!("{label}: {e}"));
                let counter = uncaptured.clone();
                device.on_uncaptured_error(Arc::new(move |e| {
                    eprintln!("uncaptured device error: {e}");
                    counter.fetch_add(1, Ordering::Relaxed);
                }));
                (device, queue)
            };
            let (device, queue) = request("two swapchain test A");
            let (device_b, queue_b) = request("two swapchain test B");

            let info = adapter.get_info();
            eprintln!("adapter: {} ({:?})", info.name, info.backend);
            let ctx = Arc::new(GpuContext {
                instance,
                adapter,
                device,
                queue,
                vendor_label: info.name,
                backend_label: format!("{:?}", info.backend),
                health: GpuHealth::default(),
            });

            let shared = Arc::new(Shared::default());
            let stop = Arc::new(AtomicBool::new(false));
            let presented_a = Arc::new(AtomicU64::new(0));

            // Presenter A: own window on its own thread, hot present loop
            // through the real handshake.
            let a = {
                let ctx = ctx.clone();
                let shared = shared.clone();
                let stop = stop.clone();
                let presented = presented_a.clone();
                let out_a = test_output(&base, 0, r"\\.\TEST-A");
                std::thread::Builder::new()
                    .name("test-presenter-a".into())
                    .spawn(move || {
                        let window = OutputWindow::new(&out_a).expect("window A");
                        let surface = Arc::new(
                            create_surface(&ctx, window.raw_handle().expect("raw A"))
                                .expect("surface A"),
                        );
                        let config = surface
                            .get_default_config(&ctx.adapter, out_a.width, out_a.height)
                            .expect("config A");
                        let stats = OutputStats::default();
                        let pump = || window.pump();
                        let octx = OutputCtx { stats: &stats, pump: &pump };
                        let configure_a = ctx.device.clone();
                        let configure = move |(s, c): ConfigureRequest| s.configure(&configure_a, &c);
                        let mut worker = SurfaceWorker::spawn(0, configure).expect("worker A");
                        let first = configure_initial(&surface, &config, &shared, &octx, &stop, &mut worker);
                        assert_eq!(
                            first,
                            ConfigureOutcome::Ran,
                            "first configure of surface A never ran"
                        );
                        let _submit = SubmitGuard::enter(&shared.submitters);
                        while !stop.load(Ordering::Relaxed) {
                            window.pump();
                            shared.park_if_paused(&octx, &stop);
                            match surface.get_current_texture() {
                                wgpu::CurrentSurfaceTexture::Success(frame)
                                | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                                    present_clear_frame(&ctx.device, &ctx.queue, frame);
                                    presented.fetch_add(1, Ordering::Relaxed);
                                }
                                other => {
                                    eprintln!("surface A skipped a frame: {other:?}");
                                    std::thread::sleep(Duration::from_millis(5));
                                }
                            }
                        }
                    })
                    .expect("spawn presenter A")
            };

            // B's first configure lands only after A is presenting flat out.
            let deadline = Instant::now() + Duration::from_secs(10);
            while presented_a.load(Ordering::Relaxed) < 30 {
                assert!(
                    Instant::now() < deadline,
                    "presenter A never got going on this adapter"
                );
                std::thread::sleep(Duration::from_millis(10));
            }

            let surface_b = Arc::new(surface_b);
            let config_b = surface_b
                .get_default_config(&ctx.adapter, out_b.width, out_b.height)
                .expect("config B");
            let stats_b = OutputStats::default();
            let pump_b = || window_b.pump();
            let octx_b = OutputCtx { stats: &stats_b, pump: &pump_b };
            let configure_b = device_b.clone();
            let configure = move |(s, c): ConfigureRequest| s.configure(&configure_b, &c);
            let mut worker_b = SurfaceWorker::spawn(1, configure).expect("worker B");
            let first_b =
                configure_initial(&surface_b, &config_b, &shared, &octx_b, &stop, &mut worker_b);
            assert_eq!(
                first_b,
                ConfigureOutcome::Ran,
                "surface B's first configure never ran while A was presenting"
            );

            let mut presented_b = 0u64;
            {
                let _submit = SubmitGuard::enter(&shared.submitters);
                let until = Instant::now() + Duration::from_secs(1);
                while Instant::now() < until {
                    window_b.pump();
                    shared.park_if_paused(&octx_b, &stop);
                    match surface_b.get_current_texture() {
                        wgpu::CurrentSurfaceTexture::Success(frame)
                        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                            present_clear_frame(&device_b, &queue_b, frame);
                            presented_b += 1;
                        }
                        other => {
                            eprintln!("surface B skipped a frame: {other:?}");
                            std::thread::sleep(Duration::from_millis(5));
                        }
                    }
                }
            }

            stop.store(true, Ordering::SeqCst);
            a.join().expect("presenter A panicked");

            let presented_a = presented_a.load(Ordering::Relaxed);
            eprintln!(
                "A presented {presented_a}, B presented {presented_b}, \
                 {} quiesce timeout(s)",
                shared.quiesce_timeouts.load(Ordering::Relaxed)
            );
            assert!(presented_a > 0, "surface A never presented");
            assert!(presented_b > 0, "surface B never presented");
            assert_eq!(
                uncaptured.load(Ordering::Relaxed),
                0,
                "the device reported errors during concurrent swapchain bring-up"
            );
        }

        /// Presents to the primary output on whatever adapter answers,
        /// including a software rasterizer — this proves the window, the
        /// swapchain, the shader, and the present call work, which
        /// [`GpuContext::acquire`] deliberately refuses to do on a rasterizer.
        /// Run it deliberately with
        /// `cargo test -p stress-kit -- --ignored presents_frames`.
        #[test]
        #[ignore = "covers the primary output with a fullscreen window"]
        fn presents_frames_to_the_primary_output() {
            let Some(output) = enumerate_outputs().into_iter().next() else {
                eprintln!("no attached outputs in this session");
                return;
            };
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::PRIMARY,
                ..wgpu::InstanceDescriptor::new_without_display_handle()
            });
            let window = OutputWindow::new(&output).expect("window");
            let raw_handle = window.raw_handle().expect("raw handle");
            let surface = unsafe { instance.create_surface_unsafe(surface_target(raw_handle)) }
                .expect("surface");

            let adapter = pollster::block_on(instance.request_adapter(
                &wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                    apply_limit_buckets: false,
                },
            ))
            .expect("adapter");
            let (device, queue) = pollster::block_on(
                adapter.request_device(&wgpu::DeviceDescriptor {
                    label: Some("gpu_display test"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    experimental_features: wgpu::ExperimentalFeatures::disabled(),
                    memory_hints: wgpu::MemoryHints::Performance,
                    trace: wgpu::Trace::Off,
                }),
            )
            .expect("device");

            let config = surface
                .get_default_config(&adapter, output.width, output.height)
                .expect("default config");
            surface.configure(&device, &config);

            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("gpu_display test module"),
                source: wgpu::ShaderSource::Wgsl(SHADER.into()),
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("gpu_display test pipeline"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(config.format.into())],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            });
            let frame_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("gpu_display test frame"),
                size: std::mem::size_of::<Frame>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("gpu_display test bind group"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: frame_buf.as_entire_binding(),
                }],
            });

            let ctx = Arc::new(TestCtx { device, queue });
            let mut presented = 0u32;
            let mut skipped = 0u32;
            let until = Instant::now() + smoke_duration();
            let mut i = 0u32;
            while Instant::now() < until {
                i += 1;
                window.pump();
                ctx.queue.write_buffer(
                    &frame_buf,
                    0,
                    bytemuck::bytes_of(&Frame {
                        time: i as f32 * 0.05,
                        tint: 0.5,
                        band: (i as f32 * 0.01).fract(),
                        inv_width: 1.0 / config.width as f32,
                        iters: SHADER_ITERS,
                        _pad: [0; 3],
                    }),
                );
                match surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(frame)
                    | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                        present_test_frame(&ctx, &pipeline, &bind_group, frame);
                        presented += 1;
                    }
                    other => {
                        skipped += 1;
                        eprintln!("frame {i}: {other:?}");
                    }
                }
            }
            let secs = smoke_duration().as_secs_f64();
            eprintln!(
                "{}: presented {presented} frame(s), skipped {skipped}, {:.1} FPS over {secs:.0}s",
                output.describe(),
                presented as f64 / secs
            );
            assert!(presented > 0, "no frame reached the screen");
        }

        /// How long the visual smoke test presents; `STRESSKIT_DISPLAY_SMOKE_SECS`
        /// overrides the default.
        fn smoke_duration() -> Duration {
            let secs = std::env::var("STRESSKIT_DISPLAY_SMOKE_SECS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(2)
                .clamp(1, 600);
            Duration::from_secs(secs)
        }

        /// Stand-in for [`GpuContext`] so the draw path under test does not
        /// need an adapter that passes certification.
        #[cfg(test)]
        struct TestCtx {
            device: wgpu::Device,
            queue: wgpu::Queue,
        }

        fn present_test_frame(
            ctx: &Arc<TestCtx>,
            pipeline: &wgpu::RenderPipeline,
            bind_group: &wgpu::BindGroup,
            frame: wgpu::SurfaceTexture,
        ) {
            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder = ctx
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            ctx.queue.submit(std::iter::once(encoder.finish()));
            ctx.queue.present(frame);
        }
    }
}
