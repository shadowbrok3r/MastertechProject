//! Display-path stressor: a real swapchain per attached output, presented
//! continuously, with periodic surface reconfiguration and desktop mode
//! changes. Reports aggregate presented FPS.
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
/// first configure can take several quiesce rounds before its first frame.
#[cfg(target_os = "windows")]
const COVERAGE_WARMUP: Duration = Duration::from_secs(10);
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
        /// A coverage complaint has been emitted and not yet resolved.
        coverage_complained: AtomicBool,
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
            "[stress-kit/gpu_display] {} output(s) on {} ({} backend), mode-set policy {:?}",
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
             {} reconfigure(s) skipped for a busy sibling, {} for a held configure turn",
            shared.driven(),
            attached,
            shared.total(|o| o.presented.load(Ordering::Relaxed)),
            shared.quiesce_timeouts.load(Ordering::Relaxed),
            shared.turn_timeouts.load(Ordering::Relaxed)
        );
        for (output, stats) in outputs.iter().zip(&shared.outputs) {
            log::info!(
                "[stress-kit/gpu_display] {}: {} presented, {} timeout, {} lost, {} outdated, \
                 {} occluded, {} reconfigure, {} mode set",
                output.device,
                stats.presented.load(Ordering::Relaxed),
                stats.timeouts.load(Ordering::Relaxed),
                stats.lost.load(Ordering::Relaxed),
                stats.unexpected_outdated.load(Ordering::Relaxed),
                stats.occluded.load(Ordering::Relaxed),
                stats.reconfigures.load(Ordering::Relaxed),
                stats.mode_sets.load(Ordering::Relaxed),
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
             release the configure turn. Zero FPS with no watchdog live dump, no TDR and no \
             WHEA on a responsive machine is a TOOL failure: the run grades INCONCLUSIVE and \
             proves nothing about this hardware in either direction. Re-run the stage; do \
             not read this as a display fault.",
            marker = crate::STRESSOR_HANG_MARKER,
            stuck = stuck_for.as_secs(),
            phases = phases.join(", "),
            quiesce = shared.quiesce_timeouts.load(Ordering::Relaxed),
            turn = shared.turn_timeouts.load(Ordering::Relaxed),
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

    /// Owns one output end to end: its window, its swapchain, its mode changes.
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
        let window = match OutputWindow::new(&output) {
            Ok(w) => w,
            Err(e) => {
                shared.latch_fatal(format!(
                    "gpu_display: inconclusive - could not open a window on {} ({e}); that \
                     output's present path never ran",
                    output.device
                ));
                return;
            }
        };
        let raw_handle = match window.raw_handle() {
            Ok(h) => h,
            Err(e) => {
                shared.latch_fatal(format!(
                    "gpu_display: inconclusive - no window handle for {} ({e})",
                    output.device
                ));
                return;
            }
        };
        // Every bounded wait in the handshake pumps through this, so a thread
        // that is waiting still answers the message broadcast a sibling's mode
        // change is blocked on.
        let pump = || window.pump();
        let octx = OutputCtx { stats, pump: &pump };

        let mut surface = match create_surface(ctx, raw_handle) {
            Ok(s) => s,
            Err(e) => {
                shared.latch_fatal(format!(
                    "gpu_display: inconclusive - no swapchain on {} ({e}); this adapter cannot \
                     present to that output",
                    output.device
                ));
                return;
            }
        };

        let caps = surface.get_capabilities(&ctx.adapter);
        if caps.formats.is_empty() {
            shared.latch_fatal(format!(
                "gpu_display: inconclusive - {} reports no surface formats on this adapter",
                output.device
            ));
            return;
        }
        // Fifo is guaranteed; the rest widen the flip-queue behaviour we cover.
        let present_modes: Vec<wgpu::PresentMode> = caps.present_modes.clone();
        log::info!(
            "[stress-kit/gpu_display] {}: format {:?}, present modes {:?}",
            output.device,
            caps.formats[0],
            present_modes
        );

        let mut config = match surface.get_default_config(&ctx.adapter, output.width, output.height)
        {
            Some(c) => c,
            None => {
                shared.latch_fatal(format!(
                    "gpu_display: inconclusive - {} is not supported by the bound adapter",
                    output.device
                ));
                return;
            }
        };
        // The first configure creates the swapchain; presenting siblings park
        // so the build cannot race their submissions.
        if !configure_initial(&surface, ctx, &config, shared, &octx, stop) {
            shared.latch_fatal(format!(
                "gpu_display: inconclusive - the swapchain on {} could not be configured while \
                 sibling outputs were presenting; that output's present path never ran",
                output.device
            ));
            return;
        }

        let module = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("gpu_display module"),
                source: wgpu::ShaderSource::Wgsl(SHADER.into()),
            });
        let pipeline = ctx
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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

        let frame_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu_display frame"),
            size: std::mem::size_of::<Frame>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gpu_display bind group"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_buf.as_entire_binding(),
            }],
        });

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

        // Frame-loop membership for the quiesce handshake; setup never submits.
        let _submit = SubmitGuard::enter(&shared.submitters);
        let wedge_after = debug_wedge_frame(index);

        while !stop.load(Ordering::Relaxed) {
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
            window.pump();
            shared.park_if_paused(&octx, stop);

            let elapsed = started.elapsed().as_secs_f32();
            ctx.queue.write_buffer(
                &frame_buf,
                0,
                bytemuck::bytes_of(&Frame {
                    time: elapsed,
                    tint: 0.25 + 0.75 * (index as f32 / total.max(1) as f32),
                    band: (elapsed * 0.35).fract(),
                    inv_width: 1.0 / config.width.max(1) as f32,
                    iters: SHADER_ITERS,
                    _pad: [0; 3],
                }),
            );

            stats.set_phase(Phase::Acquiring);
            match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(frame)
                | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                    stats.set_phase(Phase::Presenting);
                    draw_and_present(ctx, &pipeline, &bind_group, frame);
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
                    if shared
                        .with_quiesce(&octx, stop, true, || {
                            surface.configure(&ctx.device, &config)
                        })
                        .is_some()
                    {
                        expect_outdated_until = Instant::now() + SELF_INFLICTED_GRACE;
                        starved_since = None;
                    } else {
                        starved_since.get_or_insert_with(Instant::now);
                        starved_seen = true;
                    }
                }
                wgpu::CurrentSurfaceTexture::Lost => {
                    stats.lost.fetch_add(1, Ordering::Relaxed);
                    shared.set_warn(format!(
                        "gpu_display: surface lost on {}, recreating",
                        output.device
                    ));
                    match create_surface(ctx, raw_handle) {
                        Ok(fresh) => {
                            surface = fresh;
                            // A fresh surface stays unconfigured until the siblings
                            // go quiet; the next frame reports Outdated and retries.
                            shared.with_quiesce(&octx, stop, true, || {
                                surface.configure(&ctx.device, &config)
                            });
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
                let applied = reconfigure(
                    &surface,
                    ctx,
                    &mut config,
                    &present_modes,
                    present_mode_index,
                    &output,
                    &window,
                    shared,
                    &octx,
                    stop,
                );
                if applied {
                    stats.reconfigures.fetch_add(1, Ordering::Relaxed);
                    expect_outdated_until = Instant::now() + SELF_INFLICTED_GRACE;
                    starved_since = None;
                } else {
                    starved_since.get_or_insert_with(Instant::now);
                    starved_seen = true;
                }
            }

            if policy != ModeSetPolicy::Off && last_mode_set.elapsed() >= MODE_SET_EVERY {
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
                            window.move_to(output.x, output.y, width, height);
                            config.width = width.max(1);
                            config.height = height.max(1);
                            shared.with_quiesce(&octx, stop, true, || {
                                surface.configure(&ctx.device, &config)
                            });
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

    /// First configure of a fresh surface, serialized against sibling
    /// configures and presents. Bounded retries; `false` when it never ran.
    fn configure_initial(
        surface: &wgpu::Surface<'static>,
        ctx: &Arc<GpuContext>,
        config: &wgpu::SurfaceConfiguration,
        shared: &Shared,
        octx: &OutputCtx<'_>,
        stop: &AtomicBool,
    ) -> bool {
        for _ in 0..INITIAL_CONFIGURE_ATTEMPTS {
            if stop.load(Ordering::Relaxed) {
                return false;
            }
            // Each attempt is progress: bring-up next to saturated CPU lanes
            // can take several quiesce rounds, and a thread still working
            // through them is not wedged.
            octx.stats.progress.fetch_add(1, Ordering::Relaxed);
            if shared
                .with_quiesce(octx, stop, false, || surface.configure(&ctx.device, config))
                .is_some()
            {
                return true;
            }
        }
        false
    }

    fn draw_and_present(
        ctx: &Arc<GpuContext>,
        pipeline: &wgpu::RenderPipeline,
        bind_group: &wgpu::BindGroup,
        frame: wgpu::SurfaceTexture,
    ) {
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
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
        ctx.queue.submit(std::iter::once(encoder.finish()));
        ctx.queue.present(frame);
    }

    /// Cycles present mode, frame latency, and the presented size — the
    /// swapchain rebuild path a mode change also takes. Returns false when the
    /// siblings never went quiet, leaving `config` updated for a later attempt.
    #[allow(clippy::too_many_arguments)]
    fn reconfigure(
        surface: &wgpu::Surface<'static>,
        ctx: &Arc<GpuContext>,
        config: &mut wgpu::SurfaceConfiguration,
        present_modes: &[wgpu::PresentMode],
        step: usize,
        output: &Output,
        window: &OutputWindow,
        shared: &Shared,
        octx: &OutputCtx<'_>,
        stop: &AtomicBool,
    ) -> bool {
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
        shared
            .with_quiesce(octx, stop, true, || surface.configure(&ctx.device, config))
            .is_some()
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

        fn present_clear_frame(ctx: &GpuContext, frame: wgpu::SurfaceTexture) {
            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder = ctx
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
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
            ctx.queue.submit(std::iter::once(encoder.finish()));
            ctx.queue.present(frame);
        }

        /// Regression pin for the dual-output startup crash (ntdll heap AV,
        /// run `be8996be` on DESKTOP-NFOQK4J): the second swapchain's first
        /// configure runs while the first presents flat out on the same
        /// device. Serialized correctly, both outputs present and the device
        /// reports no errors. Run it deliberately with
        /// `cargo test -p stress-kit --lib -- --ignored two_swapchains`.
        #[test]
        #[ignore = "creates windows and drives real swapchains on whatever adapter answers"]
        fn two_swapchains_share_one_device_from_startup() {
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
            let (device, queue) = pollster::block_on(
                adapter.request_device(&wgpu::DeviceDescriptor {
                    label: Some("two swapchain test"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    experimental_features: wgpu::ExperimentalFeatures::disabled(),
                    memory_hints: wgpu::MemoryHints::Performance,
                    trace: wgpu::Trace::Off,
                }),
            )
            .expect("device");

            let uncaptured = Arc::new(AtomicU64::new(0));
            let uncaptured_in_handler = uncaptured.clone();
            device.on_uncaptured_error(Arc::new(move |e| {
                eprintln!("uncaptured device error: {e}");
                uncaptured_in_handler.fetch_add(1, Ordering::Relaxed);
            }));

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
                        let surface =
                            create_surface(&ctx, window.raw_handle().expect("raw A"))
                                .expect("surface A");
                        let config = surface
                            .get_default_config(&ctx.adapter, out_a.width, out_a.height)
                            .expect("config A");
                        let stats = OutputStats::default();
                        let pump = || window.pump();
                        let octx = OutputCtx { stats: &stats, pump: &pump };
                        assert!(
                            configure_initial(&surface, &ctx, &config, &shared, &octx, &stop),
                            "first configure of surface A never ran"
                        );
                        let _submit = SubmitGuard::enter(&shared.submitters);
                        while !stop.load(Ordering::Relaxed) {
                            window.pump();
                            shared.park_if_paused(&octx, &stop);
                            match surface.get_current_texture() {
                                wgpu::CurrentSurfaceTexture::Success(frame)
                                | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                                    present_clear_frame(&ctx, frame);
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

            let config_b = surface_b
                .get_default_config(&ctx.adapter, out_b.width, out_b.height)
                .expect("config B");
            let stats_b = OutputStats::default();
            let pump_b = || window_b.pump();
            let octx_b = OutputCtx { stats: &stats_b, pump: &pump_b };
            assert!(
                configure_initial(&surface_b, &ctx, &config_b, &shared, &octx_b, &stop),
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
                            present_clear_frame(&ctx, frame);
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
