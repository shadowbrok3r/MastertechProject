//! GPU-side executed-workgroup counter that proves dispatched work ran.

#![cfg(feature = "gpu")]

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex};

/// Bytes of the executed-workgroup counter.
const COUNTER_BYTES: u64 = 4;
/// Counter readbacks in flight at most; a submit finding none free carries no copy.
const MAX_READBACKS: usize = 64;
/// Dispatches retired with no executed workgroup before the GPU counts as not running them.
pub(super) const UNEXECUTED_DISPATCH_LIMIT: u64 = 64;
/// Consecutive failed counter readbacks before the GPU's work counts as unproven.
const MAX_READBACK_FAILURES: u32 = 5;

/// Why a GPU's dispatched work cannot be credited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum WitnessFault {
    /// Dispatches retired while the counter never advanced.
    NotExecuting { dispatches: u64 },
    /// The counter could not be read back.
    Unreadable { attempts: u32, error: String },
}

impl fmt::Display for WitnessFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotExecuting { dispatches } => write!(
                f,
                "the GPU retired {dispatches} dispatches without running any of their workgroups"
            ),
            Self::Unreadable { attempts, error } => write!(
                f,
                "the GPU's executed-workgroup counter could not be read back {attempts} times \
                 ({error})"
            ),
        }
    }
}

/// Counter readbacks set against the dispatches submitted before each one.
#[derive(Debug, Default)]
struct ExecutionProof {
    last_counter: u32,
    last_submitted: u64,
    /// Dispatches submitted since the counter last advanced.
    unexecuted: u64,
}

impl ExecutionProof {
    /// Workgroups executed since the last read, given a counter read after `submitted` dispatches.
    fn observe(&mut self, counter: u32, submitted: u64) -> u64 {
        let executed = u64::from(counter.wrapping_sub(self.last_counter));
        let dispatched = submitted.saturating_sub(self.last_submitted);
        self.last_counter = counter;
        self.last_submitted = submitted;
        self.unexecuted = if executed > 0 {
            0
        } else {
            self.unexecuted.saturating_add(dispatched)
        };
        executed
    }

    fn fault(&self) -> Option<WitnessFault> {
        (self.unexecuted >= UNEXECUTED_DISPATCH_LIMIT).then_some(WitnessFault::NotExecuting {
            dispatches: self.unexecuted,
        })
    }
}

/// One counter readback buffer and the result of its map.
struct Readback {
    staging: wgpu::Buffer,
    /// Map result, written by the map callback.
    mapped: Arc<Mutex<Option<Result<(), wgpu::BufferAsyncError>>>>,
}

impl Readback {
    fn new(device: &wgpu::Device) -> Self {
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("witness readback"),
            size: COUNTER_BYTES,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            staging,
            mapped: Arc::new(Mutex::new(None)),
        }
    }

    fn request(&self) {
        let slot = self.mapped.clone();
        self.staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |res| {
                if let Ok(mut g) = slot.lock() {
                    *g = Some(res);
                }
            });
    }

    /// Counter value once the map resolved, unmapping only a buffer that mapped; `None` while pending.
    fn take(&self) -> Option<Result<u32, String>> {
        let res = self.mapped.lock().ok()?.take()?;
        Some(res.map_err(|e| e.to_string()).and_then(|()| {
            let value = self
                .staging
                .slice(..)
                .get_mapped_range()
                .map(|view| bytemuck::pod_read_unaligned::<u32>(&view[..COUNTER_BYTES as usize]))
                .map_err(|e| e.to_string());
            self.staging.unmap();
            value
        }))
    }
}

/// Executed-workgroup counter, a readback per submit, and the work proven so far.
pub(super) struct Witness {
    device: wgpu::Device,
    counter: wgpu::Buffer,
    /// Unmapped readbacks ready for the next submit.
    idle: Vec<Readback>,
    /// Requested readbacks, oldest first, each with the dispatches submitted through it.
    pending: VecDeque<(Readback, u64)>,
    submitted: u64,
    failures: u32,
    proof: ExecutionProof,
}

impl Witness {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        Self::with_counter_usage(
            device,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        )
    }

    fn with_counter_usage(device: &wgpu::Device, usage: wgpu::BufferUsages) -> Self {
        let counter = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("witness counter"),
            size: COUNTER_BYTES,
            usage,
            mapped_at_creation: false,
        });
        Self {
            device: device.clone(),
            counter,
            idle: Vec::new(),
            pending: VecDeque::new(),
            submitted: 0,
            failures: 0,
            proof: ExecutionProof::default(),
        }
    }

    /// Bind group entry for the counter at `binding`.
    pub(super) fn entry(&self, binding: u32) -> wgpu::BindGroupEntry<'_> {
        wgpu::BindGroupEntry {
            binding,
            resource: self.counter.as_entire_binding(),
        }
    }

    /// Submits `encoder` as one dispatch followed by a copy of the counter into a free readback.
    pub(super) fn submit(
        &mut self,
        queue: &wgpu::Queue,
        mut encoder: wgpu::CommandEncoder,
    ) -> wgpu::SubmissionIndex {
        let readback = self
            .idle
            .pop()
            .or_else(|| (self.pending.len() < MAX_READBACKS).then(|| Readback::new(&self.device)));
        if let Some(rb) = &readback {
            encoder.copy_buffer_to_buffer(&self.counter, 0, &rb.staging, 0, COUNTER_BYTES);
        }
        let index = queue.submit(std::iter::once(encoder.finish()));
        self.submitted += 1;
        if let Some(rb) = readback {
            rb.request();
            self.pending.push_back((rb, self.submitted));
        }
        index
    }

    /// Workgroups proven by readbacks resolved since the last call; `Err` once the work is unproven.
    pub(super) fn absorb(&mut self) -> Result<u64, WitnessFault> {
        let mut executed = 0;
        while let Some(read) = self.take() {
            match read {
                Ok((counter, submitted)) => {
                    self.failures = 0;
                    executed += self.proof.observe(counter, submitted);
                    if let Some(fault) = self.proof.fault() {
                        return Err(fault);
                    }
                }
                Err(error) => {
                    self.failures += 1;
                    if self.failures >= MAX_READBACK_FAILURES {
                        return Err(WitnessFault::Unreadable {
                            attempts: self.failures,
                            error,
                        });
                    }
                }
            }
        }
        Ok(executed)
    }

    /// Oldest resolved readback as `(counter, dispatches submitted)`; `None` while it is pending.
    fn take(&mut self) -> Option<Result<(u32, u64), String>> {
        let read = self.pending.front()?.0.take()?;
        let (readback, submitted) = self.pending.pop_front()?;
        if read.is_ok() {
            self.idle.push(readback);
        }
        Some(read.map(|counter| (counter, submitted)))
    }
}

#[cfg(test)]
pub(super) mod test_support {
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::time::{Duration, Instant};

    use crate::Metrics;

    /// Asserts `shader` validates and declares `witness` at `@group(0) @binding(binding)`.
    pub(in crate::stressors) fn assert_witness_binding(shader: &str, binding: u32) {
        let module = wgpu::naga::front::wgsl::parse_str(shader).expect("shader parses");
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("shader validates");
        let witness = module
            .global_variables
            .iter()
            .find(|(_, v)| v.name.as_deref() == Some("witness"))
            .and_then(|(_, v)| v.binding.as_ref().map(|b| (b.group, b.binding)))
            .expect("witness binding");
        assert_eq!(witness, (0, binding));
        assert!(
            shader.contains("atomicAdd(&witness[0], 1u)"),
            "shader never bumps the witness"
        );
    }

    /// Samples from `rx` until a fatal one arrives, the sender drops, or `timeout` passes.
    pub(in crate::stressors) fn samples_until_fatal(
        rx: &mpsc::Receiver<Metrics>,
        timeout: Duration,
    ) -> Vec<Metrics> {
        let deadline = Instant::now() + timeout;
        let mut samples = Vec::new();
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(m) => {
                    let fatal = m.fatal;
                    samples.push(m);
                    if fatal {
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        samples
    }

    /// Highest throughput among `samples`.
    pub(in crate::stressors) fn peak(samples: &[Metrics]) -> f64 {
        samples.iter().map(|m| m.throughput).fold(0.0, f64::max)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::stressors::gpu_common::{GpuContext, wait_latest};

    const WG: u32 = 4096;

    #[test]
    fn executed_workgroups_are_the_counter_delta() {
        let mut proof = ExecutionProof::default();
        assert_eq!(proof.observe(WG, 1), u64::from(WG));
        assert_eq!(proof.observe(WG * 11, 11), u64::from(WG) * 10);
        assert_eq!(proof.fault(), None);
    }

    #[test]
    fn dispatches_retired_after_execution_stops_prove_nothing() {
        let mut proof = ExecutionProof::default();
        assert_eq!(proof.observe(WG * 10, 10), u64::from(WG) * 10);
        assert_eq!(proof.observe(WG * 10, 1_000), 0);
        assert_eq!(
            proof.fault(),
            Some(WitnessFault::NotExecuting { dispatches: 990 })
        );
    }

    #[test]
    fn retired_but_unexecuted_dispatches_fault_at_the_limit() {
        let mut proof = ExecutionProof::default();
        assert_eq!(proof.observe(0, 1), 0);
        assert_eq!(proof.fault(), None);
        assert_eq!(proof.observe(0, UNEXECUTED_DISPATCH_LIMIT), 0);
        assert_eq!(
            proof.fault(),
            Some(WitnessFault::NotExecuting {
                dispatches: UNEXECUTED_DISPATCH_LIMIT
            })
        );
    }

    #[test]
    fn one_executed_workgroup_resets_the_unexecuted_run() {
        let mut proof = ExecutionProof::default();
        proof.observe(0, UNEXECUTED_DISPATCH_LIMIT - 1);
        assert_eq!(proof.observe(1, UNEXECUTED_DISPATCH_LIMIT + 10), 1);
        assert_eq!(proof.fault(), None);
    }

    #[test]
    fn partial_execution_proves_only_the_executed_workgroups() {
        let mut proof = ExecutionProof::default();
        assert_eq!(proof.observe(WG / 2, 1), u64::from(WG / 2));
        assert_eq!(proof.observe(WG / 2 + WG / 4, 2), u64::from(WG / 4));
    }

    #[test]
    fn counter_wraparound_still_counts_executed_workgroups() {
        let mut proof = ExecutionProof {
            last_counter: u32::MAX - 99,
            last_submitted: 7,
            ..Default::default()
        };
        assert_eq!(proof.observe(WG - 100, 8), u64::from(WG));
    }

    #[test]
    fn faults_name_the_dispatches_and_the_readback_error() {
        let not_running = WitnessFault::NotExecuting { dispatches: 70 }.to_string();
        assert!(
            not_running.contains("retired 70 dispatches without running"),
            "{not_running}"
        );
        let unreadable = WitnessFault::Unreadable {
            attempts: 5,
            error: "lost".into(),
        }
        .to_string();
        assert!(
            unreadable.contains("read back 5 times (lost)"),
            "{unreadable}"
        );
    }

    /// Noop device: submissions retire and the counter is never written.
    #[test]
    fn a_device_that_never_runs_dispatches_faults_without_credit() {
        let ctx = GpuContext::noop().expect("noop device");
        let mut witness = Witness::new(&ctx.device);
        let mut proven = 0;
        let deadline = Instant::now() + Duration::from_secs(10);
        let fault = loop {
            assert!(
                Instant::now() < deadline,
                "no fault after {} submits",
                witness.submitted
            );
            let encoder = ctx.device.create_command_encoder(&Default::default());
            witness.submit(&ctx.queue, encoder);
            let _ = ctx.device.poll(wait_latest());
            match witness.absorb() {
                Ok(n) => proven += n,
                Err(fault) => break fault,
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(proven, 0);
        assert!(
            matches!(fault, WitnessFault::NotExecuting { dispatches } if dispatches >= UNEXECUTED_DISPATCH_LIMIT),
            "{fault:?}"
        );
    }

    /// Noop context with a witness whose counter the test can write.
    fn writable_witness() -> (GpuContext, Witness) {
        let ctx = GpuContext::noop().expect("noop device");
        let usage = wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST;
        let witness = Witness::with_counter_usage(&ctx.device, usage);
        (ctx, witness)
    }

    /// Sets the counter to `executed`, as a GPU running every workgroup would, and submits.
    fn dispatch(ctx: &GpuContext, witness: &mut Witness, executed: u32) {
        ctx.queue
            .write_buffer(&witness.counter, 0, bytemuck::bytes_of(&executed));
        let encoder = ctx.device.create_command_encoder(&Default::default());
        witness.submit(&ctx.queue, encoder);
    }

    #[test]
    fn each_tick_is_credited_with_exactly_the_dispatches_completed_in_it() {
        let (ctx, mut witness) = writable_witness();
        let mut executed = 0;
        for dispatches_in_tick in [1_u64, 3, 7, 2, 6, 4] {
            let mut credited = 0;
            for _ in 0..dispatches_in_tick {
                executed += WG;
                dispatch(&ctx, &mut witness, executed);
                ctx.device.poll(wait_latest()).expect("poll");
                credited += witness.absorb().expect("proven");
            }
            assert_eq!(credited, dispatches_in_tick * u64::from(WG));
        }
        assert_eq!(ctx.health.failure(), None);
    }

    #[test]
    fn queued_dispatches_are_each_credited_once_they_complete() {
        let (ctx, mut witness) = writable_witness();
        for k in 1..=32 {
            dispatch(&ctx, &mut witness, k * WG);
        }
        assert_eq!(witness.pending.len(), 32);
        ctx.device.poll(wait_latest()).expect("poll");
        assert_eq!(witness.absorb(), Ok(32 * u64::from(WG)));
        assert!(witness.pending.is_empty());
        assert_eq!(witness.absorb(), Ok(0));
    }

    #[test]
    fn dispatches_past_the_readback_cap_are_credited_by_the_next_readback() {
        let (ctx, mut witness) = writable_witness();
        let past_cap = MAX_READBACKS as u32 + 5;
        for k in 1..=past_cap {
            dispatch(&ctx, &mut witness, k * WG);
        }
        assert_eq!(witness.pending.len(), MAX_READBACKS);
        ctx.device.poll(wait_latest()).expect("poll");
        assert_eq!(witness.absorb(), Ok(MAX_READBACKS as u64 * u64::from(WG)));

        dispatch(&ctx, &mut witness, (past_cap + 1) * WG);
        ctx.device.poll(wait_latest()).expect("poll");
        assert_eq!(witness.absorb(), Ok(6 * u64::from(WG)));
        assert_eq!(witness.proof.fault(), None);
    }

    /// Unmapping a pending readback aborts its map, so its callback reports a failure.
    fn dispatch_with_failed_readback(ctx: &GpuContext, witness: &mut Witness, executed: u32) {
        dispatch(ctx, witness, executed);
        let (readback, _) = witness.pending.back().expect("a readback");
        readback.staging.unmap();
        ctx.device.poll(wait_latest()).expect("poll");
    }

    #[test]
    fn a_failed_readback_is_not_unmapped_again() {
        let (ctx, mut witness) = writable_witness();
        dispatch_with_failed_readback(&ctx, &mut witness, WG);
        assert_eq!(witness.absorb(), Ok(0));
        assert_eq!(witness.failures, 1);
        assert_eq!(ctx.health.failure(), None);

        dispatch(&ctx, &mut witness, 2 * WG);
        ctx.device.poll(wait_latest()).expect("poll");
        assert_eq!(witness.absorb(), Ok(2 * u64::from(WG)));
        assert_eq!(witness.failures, 0);
    }

    #[test]
    fn consecutive_failed_readbacks_leave_the_work_unproven() {
        let (ctx, mut witness) = writable_witness();
        for k in 1..MAX_READBACK_FAILURES {
            dispatch_with_failed_readback(&ctx, &mut witness, k * WG);
            assert_eq!(witness.absorb(), Ok(0));
        }
        dispatch_with_failed_readback(&ctx, &mut witness, MAX_READBACK_FAILURES * WG);
        assert!(
            matches!(witness.absorb(), Err(WitnessFault::Unreadable { attempts, .. }) if attempts == MAX_READBACK_FAILURES)
        );
        assert_eq!(ctx.health.failure(), None);
    }
}
