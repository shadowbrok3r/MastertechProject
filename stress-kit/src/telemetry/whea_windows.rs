//! WHEA machine-check error counter via the Windows Event Log API: counts
//! `Microsoft-Windows-WHEA-Logger` records in the `System` channel, fatal
//! (Level 1/2) and corrected (Level 3) separately. Informational records are
//! excluded by the query.

use std::time::{Duration, Instant};

use windows::Win32::System::EventLog::{
    EvtClose, EvtNext, EvtQuery, EvtQueryChannelPath, EvtQueryForwardDirection, EVT_HANDLE,
};
use windows::core::PCWSTR;

use super::WheaCounters;

const CHANNEL: &str = "System";
const QUERY_FATAL: &str =
    "*[System[Provider[@Name='Microsoft-Windows-WHEA-Logger'] and (Level=1 or Level=2)]]";
const QUERY_CORRECTED: &str =
    "*[System[Provider[@Name='Microsoft-Windows-WHEA-Logger'] and Level=3]]";
const MIN_POLL_INTERVAL: Duration = Duration::from_secs(5);
const EVT_NEXT_BATCH: usize = 32;

#[derive(Clone, Copy, Default)]
struct Counts {
    corrected: u64,
    fatal: u64,
}

pub struct WheaMonitor {
    baseline: Counts,
    cached: WheaCounters,
    last_polled: Instant,
}

impl WheaMonitor {
    /// `None` when the WHEA event source can't be queried (channel missing,
    /// access denied); the caller flags the run "WHEA monitoring unavailable"
    /// instead of recording a vacuous pass.
    pub fn open() -> Option<Self> {
        let baseline = count_all()?;
        log::debug!(
            "stress-kit/whea: WHEA-Logger on {CHANNEL} (baseline = {} corrected, {} fatal)",
            baseline.corrected,
            baseline.fatal
        );
        Some(Self {
            baseline,
            cached: WheaCounters {
                delta_since_program_start: 0,
                total_retained: baseline.corrected + baseline.fatal,
                corrected_delta: 0,
                fatal_delta: 0,
            },
            last_polled: Instant::now() - MIN_POLL_INTERVAL,
        })
    }

    /// Latest counters. Throttles the underlying queries to `MIN_POLL_INTERVAL`;
    /// intermediate calls return the cached value.
    pub fn poll(&mut self) -> WheaCounters {
        if self.last_polled.elapsed() < MIN_POLL_INTERVAL {
            return self.cached.clone();
        }
        self.last_polled = Instant::now();

        if let Some(cur) = count_all() {
            let corrected_delta = cur.corrected.saturating_sub(self.baseline.corrected);
            let fatal_delta = cur.fatal.saturating_sub(self.baseline.fatal);
            self.cached = WheaCounters {
                delta_since_program_start: corrected_delta + fatal_delta,
                total_retained: cur.corrected + cur.fatal,
                corrected_delta,
                fatal_delta,
            };
        }
        self.cached.clone()
    }
}

fn count_all() -> Option<Counts> {
    let corrected = count_matching(QUERY_CORRECTED)?;
    let fatal = count_matching(QUERY_FATAL)?;
    Some(Counts { corrected, fatal })
}

/// Runs one channel XPath query and counts matching events; `None` when the
/// query can't be opened.
fn count_matching(query: &str) -> Option<u64> {
    let channel = to_wide(CHANNEL);
    let query_w = to_wide(query);
    let flags = EvtQueryChannelPath.0 | EvtQueryForwardDirection.0;

    let handle = match unsafe {
        EvtQuery(
            None,
            PCWSTR(channel.as_ptr()),
            PCWSTR(query_w.as_ptr()),
            flags,
        )
    } {
        Ok(h) => h,
        Err(e) => {
            log::debug!("stress-kit/whea: EvtQuery failed for '{query}': {e}");
            return None;
        }
    };

    let mut count: u64 = 0;
    let mut batch = [0isize; EVT_NEXT_BATCH];
    loop {
        let mut returned: u32 = 0;
        // Err terminates the enumeration (ERROR_NO_MORE_ITEMS at the end).
        if unsafe { EvtNext(handle, &mut batch, u32::MAX, 0, &mut returned) }.is_err() {
            break;
        }
        let n = returned as usize;
        if n == 0 {
            break;
        }
        count += n as u64;
        for &h in &batch[..n] {
            let _ = unsafe { EvtClose(EVT_HANDLE(h)) };
        }
    }
    let _ = unsafe { EvtClose(handle) };
    Some(count)
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Live-log smoke test; ignored in CI because it depends on machine state
    // and Event Log access. Run with:
    //   cargo test -p stress-kit --lib whea -- --ignored --nocapture
    #[test]
    #[ignore]
    fn opens_against_live_system_log() {
        let mut m = WheaMonitor::open().expect("EvtQuery against System channel failed");
        let c = m.poll();
        println!(
            "WHEA baseline: total_retained={} (corrected_delta={} fatal_delta={} delta={})",
            c.total_retained, c.corrected_delta, c.fatal_delta, c.delta_since_program_start
        );
    }
}
