//! One RemoteExec job: its output ring, lifecycle state and exit record.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use displays::remote_exec::{
    JobChunk, JobExit, JobSnapshot, JobState, JobStream, RiskTier,
};

/// Per-job output budget. Older chunks are evicted once this is exceeded; the
/// evicted byte count is reported so a reader can tell output was lost.
pub const RING_BYTES: u64 = 2 * 1024 * 1024;

/// Chunks retained per job, independent of the byte budget.
pub const RING_CHUNKS: usize = 4096;

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Bounded output buffer. Eviction is counted, never silent.
#[derive(Default)]
pub struct LogRing {
    chunks: VecDeque<JobChunk>,
    bytes: u64,
    next_seq: u64,
    /// Bytes dropped by eviction over the job's life.
    evicted_bytes: u64,
    /// Bytes dropped since the last chunk was appended, folded into the next.
    pending_elided: u64,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
}

impl LogRing {
    pub fn push(&mut self, job_id: &str, stream: JobStream, data: Vec<u8>) -> u64 {
        let len = data.len() as u64;
        match stream {
            JobStream::Stdout => self.stdout_bytes += len,
            JobStream::Stderr => self.stderr_bytes += len,
            JobStream::Meta => {}
        }

        let seq = self.next_seq;
        self.next_seq += 1;
        self.chunks.push_back(JobChunk {
            job_id: job_id.to_string(),
            seq,
            stream,
            data,
            elided_before: std::mem::take(&mut self.pending_elided),
        });
        self.bytes += len;

        while self.bytes > RING_BYTES || self.chunks.len() > RING_CHUNKS {
            let Some(old) = self.chunks.pop_front() else { break };
            let dropped = old.data.len() as u64;
            self.bytes = self.bytes.saturating_sub(dropped);
            self.evicted_bytes += dropped;
            self.pending_elided += dropped + old.elided_before;
        }
        seq
    }

    /// Chunks from `from_seq` onward, capped at `max_bytes`. The bool is true
    /// when the ring had already evicted part of the requested range.
    pub fn read_from(&self, from_seq: u64, max_bytes: u32) -> (Vec<JobChunk>, bool) {
        let oldest = self.chunks.front().map(|c| c.seq).unwrap_or(self.next_seq);
        let truncated = from_seq < oldest;

        let mut out = Vec::new();
        let mut budget = max_bytes as u64;
        for c in self.chunks.iter().filter(|c| c.seq >= from_seq) {
            let len = c.data.len() as u64;
            if !out.is_empty() && len > budget {
                break;
            }
            budget = budget.saturating_sub(len);
            out.push(c.clone());
            if budget == 0 {
                break;
            }
        }
        (out, truncated)
    }

    pub fn last_seq(&self) -> u64 {
        self.next_seq.saturating_sub(1)
    }

    pub fn evicted_bytes(&self) -> u64 {
        self.evicted_bytes
    }
}

/// Mutable state shared between the job's driver task and any reader.
pub struct JobInner {
    pub state: JobState,
    pub pid: Option<u32>,
    pub exit: Option<JobExit>,
    pub ring: LogRing,
    /// Set when the job reached a terminal state, for retention sweeps.
    pub ended_at_ms: Option<u64>,
}

/// A submitted job. Cloneable handle; the registry owns the canonical copy.
#[derive(Clone)]
pub struct JobHandle {
    pub job_id: String,
    pub spec_summary: String,
    pub risk: RiskTier,
    pub reason: String,
    pub tech: String,
    pub started_at_ms: u64,
    /// Cooperative stop flag polled by the driver task.
    pub cancel: Arc<AtomicBool>,
    /// Win32 job object owning the process tree, so a kill takes children too.
    pub tree: Arc<super::winjob::JobObject>,
    pub inner: Arc<Mutex<JobInner>>,
}

impl JobHandle {
    pub fn new(
        job_id: String,
        spec_summary: String,
        risk: RiskTier,
        reason: String,
        tech: String,
    ) -> Self {
        Self {
            job_id,
            spec_summary,
            risk,
            reason,
            tech,
            started_at_ms: now_ms(),
            cancel: Arc::new(AtomicBool::new(false)),
            tree: Arc::new(super::winjob::JobObject::create()),
            inner: Arc::new(Mutex::new(JobInner {
                state: JobState::Queued,
                pid: None,
                exit: None,
                ring: LogRing::default(),
                ended_at_ms: None,
            })),
        }
    }

    pub fn push_output(&self, stream: JobStream, data: Vec<u8>) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.ring.push(&self.job_id, stream, data);
        }
    }

    pub fn set_state(&self, state: JobState) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.state = state;
            if state.is_terminal() && inner.ended_at_ms.is_none() {
                inner.ended_at_ms = Some(now_ms());
            }
        }
    }

    pub fn set_pid(&self, pid: Option<u32>) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.pid = pid;
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// Marks the job finished and records its terminal state.
    pub fn finish(&self, exit: JobExit, state: JobState) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.exit = Some(exit);
            inner.state = state;
            inner.ended_at_ms = Some(now_ms());
        }
    }

    pub fn state(&self) -> JobState {
        self.inner
            .lock()
            .map(|i| i.state)
            .unwrap_or(JobState::Orphaned)
    }

    pub fn ended_at_ms(&self) -> Option<u64> {
        self.inner.lock().ok().and_then(|i| i.ended_at_ms)
    }

    /// Snapshot for the admin. `from_seq` `None` omits output entirely.
    pub fn snapshot(&self, from_seq: Option<u64>, max_bytes: u32) -> JobSnapshot {
        let (state, pid, exit, last_seq, chunks, truncated) = match self.inner.lock() {
            Ok(inner) => {
                let (chunks, truncated) = match from_seq {
                    Some(seq) => inner.ring.read_from(seq, max_bytes),
                    None => (Vec::new(), false),
                };
                (
                    inner.state,
                    inner.pid,
                    inner.exit.clone(),
                    inner.ring.last_seq(),
                    chunks,
                    truncated,
                )
            }
            Err(_) => (JobState::Orphaned, None, None, 0, Vec::new(), false),
        };

        JobSnapshot {
            job_id: self.job_id.clone(),
            state,
            spec_summary: self.spec_summary.clone(),
            risk: self.risk,
            reason: self.reason.clone(),
            tech: self.tech.clone(),
            started_at_ms: self.started_at_ms,
            last_seq,
            pid,
            exit,
            chunks,
            chunks_truncated: truncated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_reports_eviction_instead_of_dropping_silently() {
        let mut ring = LogRing::default();
        // Overfill by chunk count so eviction definitely runs.
        for i in 0..(RING_CHUNKS + 10) {
            ring.push("j", JobStream::Stdout, vec![b'x'; 8]);
            assert_eq!(ring.last_seq(), i as u64);
        }
        assert!(ring.evicted_bytes() > 0, "eviction must be counted");

        // Reading from the very start must admit the range was truncated.
        let (chunks, truncated) = ring.read_from(0, u32::MAX);
        assert!(truncated, "a read below the oldest seq must report truncation");
        assert!(chunks.iter().any(|c| c.elided_before > 0));
    }

    #[test]
    fn read_from_resumes_at_requested_seq() {
        let mut ring = LogRing::default();
        for _ in 0..10 {
            ring.push("j", JobStream::Stdout, b"line".to_vec());
        }
        let (chunks, truncated) = ring.read_from(7, u32::MAX);
        assert!(!truncated);
        assert_eq!(chunks.first().map(|c| c.seq), Some(7));
        assert_eq!(chunks.len(), 3);
    }

    #[test]
    fn max_bytes_still_yields_at_least_one_chunk() {
        let mut ring = LogRing::default();
        ring.push("j", JobStream::Stdout, vec![b'x'; 4096]);
        let (chunks, _) = ring.read_from(0, 1);
        assert_eq!(chunks.len(), 1, "a starved budget must not stall the reader");
    }

    #[test]
    fn stream_byte_counters_track_separately() {
        let mut ring = LogRing::default();
        ring.push("j", JobStream::Stdout, vec![b'a'; 10]);
        ring.push("j", JobStream::Stderr, vec![b'b'; 3]);
        assert_eq!(ring.stdout_bytes, 10);
        assert_eq!(ring.stderr_bytes, 3);
    }
}
