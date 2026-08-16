//! Display-path stressor: a real swapchain per attached output, presented
//! continuously, with periodic surface reconfiguration and desktop mode
//! changes. Reports aggregate presented FPS.
//!
//! Every other GPU stressor in this crate is a compute shader — it never
//! creates a surface, never presents, and never touches the flip queue, so it
//! cannot reproduce a present/mode-set timeout (dxgkrnl `0x1b8`, `0x141`, AMD
//! Crash Defender watchdog live dumps). This one drives that path instead.
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
/// Upper bound on a parked thread's wait. Exceeds `QUIESCE_TIMEOUT` plus a
/// configure so a sibling cannot resume submitting mid-reconfigure.
#[cfg(target_os = "windows")]
const QUIESCE_PARK_MAX: Duration = Duration::from_secs(8);
/// Spin gap while waiting on the quiesce handshake.
#[cfg(target_os = "windows")]
const QUIESCE_POLL: Duration = Duration::from_micros(200);
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

    use std::sync::atomic::AtomicU32;
    use std::sync::Mutex;

    use super::super::display_win::{
        apply_mode, enumerate_outputs, refresh_modes_at, resolutions, ModeGuard, Output,
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
    }

    impl OutputStats {
        fn faults(&self) -> u64 {
            self.timeouts.load(Ordering::Relaxed)
                + self.lost.load(Ordering::Relaxed)
                + self.unexpected_outdated.load(Ordering::Relaxed)
                + self.validation.load(Ordering::Relaxed)
        }
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
        /// A coverage complaint has been emitted and not yet resolved.
        coverage_complained: AtomicBool,
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
        /// itself inside its frame loop. Returns `None` when the siblings did
        /// not go quiet in time and the caller should retry later.
        fn with_quiesce<R>(
            &self,
            stop: &AtomicBool,
            self_submits: bool,
            configure: impl FnOnce() -> R,
        ) -> Option<R> {
            // Held across the whole configure: two swapchain builds on one
            // device must not overlap even with every presenter parked.
            let _turn = self.configure_turn.lock().ok()?;
            // Raised before counting siblings: a thread that enters its frame
            // loop mid-configure parks at its first frame instead of
            // submitting into the build.
            self.configure_pause.store(true, Ordering::SeqCst);
            let quiet = self.other_submitters(self_submits) == 0
                || self.await_parked(stop, self_submits);
            let out = quiet.then(configure);
            self.configure_pause.store(false, Ordering::SeqCst);
            if out.is_none() {
                self.quiesce_timeouts.fetch_add(1, Ordering::Relaxed);
            }
            out
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
        fn await_parked(&self, stop: &AtomicBool, self_submits: bool) -> bool {
            let deadline = Instant::now() + QUIESCE_TIMEOUT;
            while Instant::now() < deadline {
                if stop.load(Ordering::Relaxed) {
                    return false;
                }
                if self.parked.load(Ordering::SeqCst) >= self.other_submitters(self_submits) {
                    return true;
                }
                std::thread::sleep(QUIESCE_POLL);
            }
            false
        }

        /// Parks this thread while a sibling configures. Called once per frame,
        /// before anything is submitted.
        fn park_if_paused(&self, stop: &AtomicBool) {
            if !self.configure_pause.load(Ordering::SeqCst) {
                return;
            }
            self.parked.fetch_add(1, Ordering::SeqCst);
            let deadline = Instant::now() + QUIESCE_PARK_MAX;
            while self.configure_pause.load(Ordering::SeqCst)
                && !stop.load(Ordering::Relaxed)
                && Instant::now() < deadline
            {
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

        tick_loop(
            cancel,
            tx,
            started_at,
            &shared,
            &ctx,
            &mut dumps,
            dumps_available,
            attached,
        );

        stop.store(true, Ordering::SeqCst);
        for handle in handles {
            let _ = handle.join();
        }
        log::info!(
            "[stress-kit/gpu_display] drove {} of {} attached output(s), {} frames presented, \
             {} reconfigure(s) skipped for a busy sibling",
            shared.driven(),
            attached,
            shared.total(|o| o.presented.load(Ordering::Relaxed)),
            shared.quiesce_timeouts.load(Ordering::Relaxed)
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
    ) {
        let mut last_tick = Instant::now();
        let mut last_scan = Instant::now();
        let mut last_presented: u64 = 0;
        let mut watchdog_dumps: u64 = 0;

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
        if !configure_initial(&surface, ctx, &config, shared, stop) {
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
        let mut guard = ModeGuard::new();
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

        // Frame-loop membership for the quiesce handshake; setup never submits.
        let _submit = SubmitGuard::enter(&shared.submitters);

        while !stop.load(Ordering::Relaxed) {
            window.pump();
            shared.park_if_paused(stop);

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

            match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(frame)
                | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                    draw_and_present(ctx, &pipeline, &bind_group, frame);
                    stats.presented.fetch_add(1, Ordering::Relaxed);
                    last_present = Instant::now();
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
                        .with_quiesce(stop, true, || surface.configure(&ctx.device, &config))
                        .is_some()
                    {
                        expect_outdated_until = Instant::now() + SELF_INFLICTED_GRACE;
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
                            shared.with_quiesce(stop, true, || surface.configure(&ctx.device, &config));
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

            let idle = last_present.elapsed();
            if idle >= STALL_FATAL {
                shared.latch_fatal(format!(
                    "gpu_display: no frame has been presented on {} for {}s; the present queue is \
                     stalled",
                    output.device,
                    idle.as_secs()
                ));
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
                    stop,
                );
                if applied {
                    stats.reconfigures.fetch_add(1, Ordering::Relaxed);
                    expect_outdated_until = Instant::now() + SELF_INFLICTED_GRACE;
                }
            }

            if policy != ModeSetPolicy::Off && last_mode_set.elapsed() >= MODE_SET_EVERY {
                last_mode_set = Instant::now();
                if let Some((width, height, hz)) = modes.next_mode() {
                    guard.track(&output.device);
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
                            shared.with_quiesce(stop, true, || surface.configure(&ctx.device, &config));
                            expect_outdated_until = Instant::now() + SELF_INFLICTED_GRACE;
                        }
                        Err(e) => shared.set_warn(format!("gpu_display: {e}")),
                    }
                }
            }
        }

        // `guard` drops before `window` and `surface`, restoring the mode
        // first — on every return above as well.
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
        stop: &AtomicBool,
    ) -> bool {
        for _ in 0..INITIAL_CONFIGURE_ATTEMPTS {
            if stop.load(Ordering::Relaxed) {
                return false;
            }
            if shared
                .with_quiesce(stop, false, || surface.configure(&ctx.device, config))
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
            .with_quiesce(stop, true, || surface.configure(&ctx.device, config))
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

        /// Fake presenter for the quiesce handshake: enters the frame loop
        /// and parks whenever a configure asks, without touching any GPU.
        fn spawn_presenter(
            shared: Arc<Shared>,
            stop: Arc<AtomicBool>,
        ) -> std::thread::JoinHandle<()> {
            std::thread::Builder::new()
                .name("test-presenter".into())
                .spawn(move || {
                    let _submit = SubmitGuard::enter(&shared.submitters);
                    while !stop.load(Ordering::Relaxed) {
                        shared.park_if_paused(&stop);
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

            let parked_during =
                shared.with_quiesce(&stop, false, || shared.parked.load(Ordering::SeqCst));
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
            let started = Instant::now();
            assert_eq!(shared.with_quiesce(&stop, false, || true), Some(true));
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
            assert_eq!(shared.with_quiesce(&stop, true, || 7), Some(7));
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

            assert_eq!(shared.with_quiesce(&stop, false, || ()), None);
            assert_eq!(shared.quiesce_timeouts.load(Ordering::Relaxed), 1);

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
                        let _submit = SubmitGuard::enter(&shared.submitters);
                        while !stop.load(Ordering::Relaxed) {
                            shared.park_if_paused(&stop);
                            submitted.store(true, Ordering::SeqCst);
                            std::thread::sleep(Duration::from_micros(50));
                        }
                    })
                    .expect("spawn test entrant")
            };

            // No submitters yet, so this configure takes the fast path. The
            // sibling is released mid-configure and must park, not submit.
            let submitted_mid_configure = shared.with_quiesce(&stop, false, || {
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
                        assert!(
                            configure_initial(&surface, &ctx, &config, &shared, &stop),
                            "first configure of surface A never ran"
                        );
                        let _submit = SubmitGuard::enter(&shared.submitters);
                        while !stop.load(Ordering::Relaxed) {
                            window.pump();
                            shared.park_if_paused(&stop);
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
            assert!(
                configure_initial(&surface_b, &ctx, &config_b, &shared, &stop),
                "surface B's first configure never ran while A was presenting"
            );

            let mut presented_b = 0u64;
            {
                let _submit = SubmitGuard::enter(&shared.submitters);
                let until = Instant::now() + Duration::from_secs(1);
                while Instant::now() < until {
                    window_b.pump();
                    shared.park_if_paused(&stop);
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
