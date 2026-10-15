//! Windows GPU TDR counter: Display 4101/4109 System events and dxgkrnl `0x117`/`0x141` live dumps.

use std::collections::{HashSet, VecDeque};
use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows::Win32::System::EventLog::{EVT_HANDLE, EvtRender, EvtRenderEventXml};

use super::live_dumps_windows::collect_dumps;
use super::whea_windows::visit_matching;

const TDR_EVENT_FILTER: &str = "Provider[@Name='Display' or @Name='nvlddmkm' or \
    @Name='amdkmdag' or @Name='amdkmdap' or @Name='igdkmd64' or @Name='igdkmdn64' or \
    @Name='igdkmdnd64'] and (EventID=4101 or EventID=4109)";
const RECORD_ID_TAG: &str = "<EventRecordID>";
const LIVE_KERNEL_REPORTS: &str = r"C:\Windows\LiveKernelReports";
const WER_REPORT_QUEUE: &str = r"C:\ProgramData\Microsoft\Windows\WER\ReportQueue";
/// Lowercased prefix of WER queue folders holding kernel reports (`Kernel_141_…`).
const WER_KERNEL_PREFIX: &str = "kernel_";
const MIN_POLL_INTERVAL: Duration = Duration::from_secs(5);
/// Longest gap between a TDR's event and its dump for the two to count once.
const PAIR_WINDOW: Duration = Duration::from_secs(30);

const VIDEO_TDR_TIMEOUT_DETECTED: u32 = 0x117;
const VIDEO_ENGINE_TIMEOUT_DETECTED: u32 = 0x141;
const KERNEL_DUMP_SIGNATURE: &[u8; 8] = b"PAGEDU64";
const OFF_BUGCHECK_CODE: usize = 0x38;
const HEADER_LEN: usize = OFF_BUGCHECK_CODE + 4;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TdrCounters {
    /// TDRs since the monitor opened; never decreases.
    pub delta_since_program_start: u64,
    /// TDR events retained in the System log at open plus `delta_since_program_start`.
    pub absolute_since_boot: u64,
}

/// Record IDs of matching TDR events newer than the given record; `None` when the query fails.
type EventSource = fn(u64) -> Option<Vec<u64>>;

pub struct TdrMonitor {
    new_events: EventSource,
    /// Newest TDR event record seen; `None` while the System channel is unreadable.
    event_cursor: Option<u64>,
    retained_at_open: u64,
    dumps: TdrDumpLedger,
    tally: TdrTally,
    cached: TdrCounters,
    last_polled: Instant,
}

impl TdrMonitor {
    /// `None` when neither the System channel nor any live-dump folder can be read.
    pub fn open() -> Option<Self> {
        let monitor = Self::with_sources(
            system_tdr_events,
            TdrDumpLedger::open(vec![
                DumpRoot::tree(LIVE_KERNEL_REPORTS),
                DumpRoot::wer_queue(WER_REPORT_QUEUE),
            ]),
        );
        match (monitor.event_cursor, monitor.dumps.available()) {
            (None, false) => {
                log::warn!(
                    "stress-kit/tdr: System channel and live-dump folders unreadable — TDR counter disabled"
                );
                return None;
            }
            (None, true) => log::warn!(
                "stress-kit/tdr: System channel unreadable — counting TDR live dumps only"
            ),
            (Some(cursor), dumps) => log::debug!(
                "stress-kit/tdr: {} TDR events retained (newest record {cursor}), live dumps readable = {dumps}",
                monitor.retained_at_open
            ),
        }
        Some(monitor)
    }

    fn with_sources(new_events: EventSource, dumps: TdrDumpLedger) -> Self {
        let mut monitor = Self {
            new_events,
            event_cursor: None,
            retained_at_open: 0,
            dumps,
            tally: TdrTally::default(),
            cached: TdrCounters::default(),
            last_polled: Instant::now() - MIN_POLL_INTERVAL,
        };
        monitor.baseline_events();
        monitor.cached.absolute_since_boot = monitor.retained_at_open;
        monitor
    }

    /// Latest counters, re-read at most every `MIN_POLL_INTERVAL`.
    pub fn poll(&mut self) -> TdrCounters {
        if self.last_polled.elapsed() < MIN_POLL_INTERVAL {
            return self.cached.clone();
        }
        self.poll_now()
    }

    /// Re-reads both sources regardless of the poll interval.
    pub fn poll_now(&mut self) -> TdrCounters {
        self.last_polled = Instant::now();
        let events = self.read_new_events();
        let dumps = self.dumps.poll();
        let total = self.tally.record(events, dumps, Instant::now());
        self.cached = TdrCounters {
            delta_since_program_start: total,
            absolute_since_boot: self.retained_at_open + total,
        };
        self.cached.clone()
    }

    /// Sets the event cursor to the newest retained TDR event; leaves it unset when the query fails.
    fn baseline_events(&mut self) {
        if let Some(ids) = (self.new_events)(0) {
            self.event_cursor = Some(ids.iter().copied().max().unwrap_or(0));
            self.retained_at_open = ids.len() as u64;
        }
    }

    /// TDR events logged since the last read.
    fn read_new_events(&mut self) -> u64 {
        let Some(cursor) = self.event_cursor else {
            self.baseline_events();
            return 0;
        };
        let Some(ids) = (self.new_events)(cursor) else {
            return 0;
        };
        if let Some(&newest) = ids.iter().max() {
            self.event_cursor = Some(newest.max(cursor));
        }
        ids.len() as u64
    }
}

fn tdr_event_query(after: u64) -> String {
    format!("*[System[{TDR_EVENT_FILTER} and EventRecordID>{after}]]")
}

fn system_tdr_events(after: u64) -> Option<Vec<u64>> {
    let mut ids = Vec::new();
    visit_matching(&tdr_event_query(after), |event| {
        if let Some(id) = record_id(event) {
            ids.push(id);
        }
    })?;
    Some(ids)
}

/// `EventRecordID` of one event, read from its rendered XML.
fn record_id(event: EVT_HANDLE) -> Option<u64> {
    let mut buf = vec![0u16; 2048];
    let mut used = 0u32;
    let mut properties = 0u32;
    for _ in 0..2 {
        let rendered = unsafe {
            EvtRender(
                None,
                event,
                EvtRenderEventXml.0,
                (buf.len() * 2) as u32,
                Some(buf.as_mut_ptr().cast()),
                &mut used,
                &mut properties,
            )
        };
        if rendered.is_ok() {
            let len = (used as usize / 2).min(buf.len());
            return parse_record_id(&String::from_utf16_lossy(&buf[..len]));
        }
        // `used` holds the required size in bytes after ERROR_INSUFFICIENT_BUFFER.
        buf = vec![0u16; (used as usize).div_ceil(2).max(buf.len() + 1)];
    }
    None
}

fn parse_record_id(xml: &str) -> Option<u64> {
    let start = xml.find(RECORD_ID_TAG)? + RECORD_ID_TAG.len();
    let end = start + xml[start..].find('<')?;
    xml[start..end].trim().parse().ok()
}

/// Running TDR count from per-poll event and dump increments; an event and a dump within `PAIR_WINDOW` count once.
#[derive(Default)]
struct TdrTally {
    total: u64,
    unpaired_events: VecDeque<Instant>,
    unpaired_dumps: VecDeque<Instant>,
}

impl TdrTally {
    fn record(&mut self, events: u64, dumps: u64, now: Instant) -> u64 {
        let expired = |at: &Instant| now.saturating_duration_since(*at) > PAIR_WINDOW;
        while self.unpaired_events.front().is_some_and(expired) {
            self.unpaired_events.pop_front();
        }
        while self.unpaired_dumps.front().is_some_and(expired) {
            self.unpaired_dumps.pop_front();
        }

        let events = events - pair_off(&mut self.unpaired_dumps, events);
        let dumps = dumps - pair_off(&mut self.unpaired_events, dumps);
        let paired = events.min(dumps);
        self.total += events.max(dumps);
        self.unpaired_events
            .extend(std::iter::repeat_n(now, (events - paired) as usize));
        self.unpaired_dumps
            .extend(std::iter::repeat_n(now, (dumps - paired) as usize));
        self.total
    }
}

/// Removes up to `n` already-counted increments from `queue`; returns how many it removed.
fn pair_off(queue: &mut VecDeque<Instant>, n: u64) -> u64 {
    let take = queue.len().min(n as usize);
    queue.drain(..take);
    take as u64
}

/// A folder searched for live dumps; a WER queue is searched only in its `Kernel_*` reports.
struct DumpRoot {
    path: PathBuf,
    wer_queue: bool,
    /// Read successfully or absent at open, or read successfully since.
    watching: bool,
}

impl DumpRoot {
    fn tree(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            wer_queue: false,
            watching: false,
        }
    }

    fn wer_queue(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            wer_queue: true,
            watching: false,
        }
    }

    fn dumps(&self) -> std::io::Result<Vec<PathBuf>> {
        if !self.wer_queue {
            return collect_dumps(&self.path).map(|found| found.into_iter().collect());
        }
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&self.path)?.flatten() {
            let kernel = entry
                .file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .starts_with(WER_KERNEL_PREFIX);
            if kernel && let Ok(found) = collect_dumps(&entry.path()) {
                out.extend(found);
            }
        }
        Ok(out)
    }

    /// Dumps on disk now; an absent folder reads as empty.
    fn dumps_or_empty(&self) -> std::io::Result<Vec<PathBuf>> {
        match self.dumps() {
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
            other => other,
        }
    }
}

/// TDR live dumps that appeared after the ledger opened, keyed by lowercased file name.
struct TdrDumpLedger {
    roots: Vec<DumpRoot>,
    /// Present when its root was first read, or classified as a non-TDR dump since.
    settled: HashSet<String>,
    /// Classified as a TDR dump since open.
    tdrs: HashSet<String>,
}

impl TdrDumpLedger {
    /// Records every dump already on disk; an unreadable root is retried on each poll.
    fn open(roots: Vec<DumpRoot>) -> Self {
        let mut ledger = Self {
            roots: Vec::with_capacity(roots.len()),
            settled: HashSet::new(),
            tdrs: HashSet::new(),
        };
        for mut root in roots {
            if let Ok(found) = root.dumps_or_empty() {
                root.watching = true;
                ledger.settle(&found);
            }
            ledger.roots.push(root);
        }
        ledger
    }

    fn available(&self) -> bool {
        self.roots.iter().any(|r| r.watching)
    }

    fn settle(&mut self, found: &[PathBuf]) {
        self.settled
            .extend(found.iter().filter_map(|p| dump_key(p)));
    }

    /// TDR dumps first seen in this poll; a root readable for the first time is baselined instead.
    fn poll(&mut self) -> u64 {
        let mut fresh = Vec::new();
        let mut recovered = Vec::new();
        for root in &mut self.roots {
            let Ok(found) = root.dumps_or_empty() else {
                continue;
            };
            if root.watching {
                fresh.extend(found);
            } else {
                root.watching = true;
                recovered.extend(found);
            }
        }
        self.settle(&recovered);

        let mut new_tdrs = 0;
        for path in fresh {
            let Some(key) = dump_key(&path) else { continue };
            if self.settled.contains(&key) || self.tdrs.contains(&key) {
                continue;
            }
            match read_header(&path) {
                DumpHeader::Tdr => {
                    self.tdrs.insert(key);
                    new_tdrs += 1;
                }
                DumpHeader::Other => {
                    self.settled.insert(key);
                }
                DumpHeader::Incomplete => {}
            }
        }
        new_tdrs
    }
}

fn dump_key(path: &Path) -> Option<String> {
    Some(path.file_name()?.to_string_lossy().to_ascii_lowercase())
}

#[derive(Debug, PartialEq, Eq)]
enum DumpHeader {
    /// Kernel dump whose bugcheck is a TDR timeout.
    Tdr,
    /// Any other dump; never re-read.
    Other,
    /// Unreadable or not yet written; re-read on the next poll.
    Incomplete,
}

fn read_header(path: &Path) -> DumpHeader {
    let mut buf = [0u8; HEADER_LEN];
    match File::open(path).and_then(|mut f| f.read_exact(&mut buf)) {
        Ok(()) => classify_header(&buf),
        Err(_) => DumpHeader::Incomplete,
    }
}

fn classify_header(header: &[u8]) -> DumpHeader {
    let (Some(signature), Some(&[a, b, c, d])) = (
        header.get(..KERNEL_DUMP_SIGNATURE.len()),
        header.get(OFF_BUGCHECK_CODE..HEADER_LEN),
    ) else {
        return DumpHeader::Incomplete;
    };
    if signature.iter().all(|&byte| byte == 0) {
        return DumpHeader::Incomplete;
    }
    if signature != KERNEL_DUMP_SIGNATURE {
        return DumpHeader::Other;
    }
    match u32::from_le_bytes([a, b, c, d]) {
        0 => DumpHeader::Incomplete,
        VIDEO_TDR_TIMEOUT_DETECTED | VIDEO_ENGINE_TIMEOUT_DETECTED => DumpHeader::Tdr,
        _ => DumpHeader::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn kernel_dump(bugcheck: u32) -> Vec<u8> {
        let mut buf = vec![0u8; 0x100];
        buf[..8].copy_from_slice(KERNEL_DUMP_SIGNATURE);
        buf[OFF_BUGCHECK_CODE..HEADER_LEN].copy_from_slice(&bugcheck.to_le_bytes());
        buf
    }

    fn write(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    /// Per-process scratch folder, removed on drop.
    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("stress-kit-tdr-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn reports(&self) -> PathBuf {
            self.0.join("LiveKernelReports")
        }

        fn queue(&self) -> PathBuf {
            self.0.join("ReportQueue")
        }

        fn ledger(&self) -> TdrDumpLedger {
            TdrDumpLedger::open(vec![
                DumpRoot::tree(self.reports()),
                DumpRoot::wer_queue(self.queue()),
            ])
        }

        fn watchdog_dump(&self, name: &str, bugcheck: u32) {
            write(
                &self.reports().join("WATCHDOG").join(name),
                &kernel_dump(bugcheck),
            );
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn ids_after(log: &Mutex<Vec<u64>>, after: u64) -> Option<Vec<u64>> {
        Some(
            log.lock()
                .unwrap()
                .iter()
                .copied()
                .filter(|&id| id > after)
                .collect(),
        )
    }

    fn poll_now(monitor: &mut TdrMonitor) -> u64 {
        monitor.poll_now().delta_since_program_start
    }

    #[test]
    fn header_classifies_tdr_bugchecks() {
        assert_eq!(classify_header(&kernel_dump(0x141)), DumpHeader::Tdr);
        assert_eq!(classify_header(&kernel_dump(0x117)), DumpHeader::Tdr);
    }

    #[test]
    fn header_rejects_other_dumps() {
        assert_eq!(classify_header(&kernel_dump(0x1a8)), DumpHeader::Other);
        assert_eq!(
            classify_header(&kernel_dump(0xa100_0001)),
            DumpHeader::Other
        );
        let mut user_mode = kernel_dump(0x141);
        user_mode[..8].copy_from_slice(b"MDMP\x93\xa7\x00\x00");
        assert_eq!(classify_header(&user_mode), DumpHeader::Other);
    }

    #[test]
    fn header_not_yet_written_is_retried() {
        assert_eq!(classify_header(&[0u8; HEADER_LEN]), DumpHeader::Incomplete);
        assert_eq!(classify_header(&kernel_dump(0)), DumpHeader::Incomplete);
        assert_eq!(classify_header(b"PAGEDU64"), DumpHeader::Incomplete);
    }

    #[test]
    fn record_id_is_read_from_event_xml() {
        let xml = "<Event><System><Provider Name='Display'/><EventID>4101</EventID>\
                   <EventRecordID>35324</EventRecordID><Channel>System</Channel></System></Event>";
        assert_eq!(parse_record_id(xml), Some(35324));
        assert_eq!(parse_record_id("<Event><System/></Event>"), None);
    }

    #[test]
    fn tally_counts_each_source_alone() {
        let t0 = Instant::now();
        assert_eq!(TdrTally::default().record(1, 0, t0), 1);
        assert_eq!(TdrTally::default().record(0, 1, t0), 1);
    }

    #[test]
    fn tally_counts_an_event_and_its_dump_once() {
        let t0 = Instant::now();
        assert_eq!(TdrTally::default().record(1, 1, t0), 1);

        let mut event_first = TdrTally::default();
        event_first.record(1, 0, t0);
        assert_eq!(event_first.record(0, 1, t0 + Duration::from_secs(5)), 1);

        let mut dump_first = TdrTally::default();
        dump_first.record(0, 1, t0);
        assert_eq!(dump_first.record(1, 0, t0 + Duration::from_secs(5)), 1);
    }

    #[test]
    fn tally_counts_an_event_only_and_a_later_dump_only_tdr_separately() {
        let t0 = Instant::now();
        let mut tally = TdrTally::default();
        assert_eq!(tally.record(1, 0, t0), 1);
        assert_eq!(tally.record(0, 0, t0 + Duration::from_secs(10)), 1);
        assert_eq!(
            tally.record(0, 1, t0 + PAIR_WINDOW + Duration::from_secs(1)),
            2
        );
    }

    #[test]
    fn tally_counts_repeated_tdrs_with_fewer_dumps() {
        let t0 = Instant::now();
        let mut tally = TdrTally::default();
        assert_eq!(tally.record(3, 1, t0), 3);
        assert_eq!(tally.record(0, 1, t0 + Duration::from_secs(5)), 3);
        assert_eq!(tally.record(0, 1, t0 + Duration::from_secs(10)), 3);
        assert_eq!(tally.record(0, 1, t0 + Duration::from_secs(15)), 4);
    }

    #[test]
    fn ledger_ignores_dumps_present_at_open() {
        let tmp = TempRoot::new("ledger-baseline");
        tmp.watchdog_dump("WATCHDOG-20260101-0000.dmp", 0x141);
        let mut dumps = tmp.ledger();
        assert!(dumps.available());
        assert_eq!(dumps.poll(), 0, "pre-existing TDR dump counted as new");
    }

    #[test]
    fn ledger_counts_new_tdr_dumps_once_across_both_roots() {
        let tmp = TempRoot::new("ledger-fresh");
        std::fs::create_dir_all(tmp.reports()).unwrap();
        std::fs::create_dir_all(tmp.queue()).unwrap();
        let mut dumps = tmp.ledger();

        let name = "WATCHDOG-20261002-1430.dmp";
        tmp.watchdog_dump(name, 0x141);
        assert_eq!(dumps.poll(), 1);

        write(
            &tmp.queue().join("Kernel_141_abc").join(name),
            &kernel_dump(0x141),
        );
        assert_eq!(dumps.poll(), 0, "WER copy of the same dump counted twice");

        write(
            &tmp.reports().join("AMD_WATCHDOG").join("AMD-1.dmp"),
            &kernel_dump(0xa100_0001),
        );
        write(
            &tmp.queue().join("AppCrash_x").join("app.dmp"),
            &kernel_dump(0x117),
        );
        assert_eq!(dumps.poll(), 0, "non-TDR or non-kernel-report dump counted");

        write(
            &tmp.queue()
                .join("Kernel_117_def")
                .join("WATCHDOG-20261002-1440.dmp"),
            &kernel_dump(0x117),
        );
        assert_eq!(dumps.poll(), 1);
        assert_eq!(dumps.poll(), 0);
    }

    #[test]
    fn ledger_retries_a_dump_whose_header_is_not_written_yet() {
        let tmp = TempRoot::new("ledger-retry");
        std::fs::create_dir_all(tmp.reports()).unwrap();
        let mut dumps = tmp.ledger();

        let path = tmp.reports().join("WATCHDOG").join("WATCHDOG-1.dmp");
        write(&path, &[0u8; 16]);
        assert_eq!(dumps.poll(), 0);
        write(&path, &kernel_dump(0x141));
        assert_eq!(dumps.poll(), 1);
    }

    #[test]
    fn ledger_counts_a_dump_in_a_folder_created_after_open() {
        let tmp = TempRoot::new("ledger-absent");
        let mut dumps = tmp.ledger();
        assert!(dumps.available());
        assert_eq!(dumps.poll(), 0);

        write(
            &tmp.queue().join("Kernel_141_abc").join("WATCHDOG-1.dmp"),
            &kernel_dump(0x141),
        );
        assert_eq!(dumps.poll(), 1);
    }

    #[test]
    fn ledger_retries_an_unreadable_root_and_baselines_it_when_it_opens() {
        let tmp = TempRoot::new("ledger-unreadable");
        write(&tmp.reports(), b"not a folder");
        let mut dumps = TdrDumpLedger::open(vec![DumpRoot::tree(tmp.reports())]);
        assert!(!dumps.available());
        assert_eq!(dumps.poll(), 0);

        std::fs::remove_file(tmp.reports()).unwrap();
        tmp.watchdog_dump("WATCHDOG-1.dmp", 0x141);
        assert_eq!(dumps.poll(), 0, "dump present when the root opened counted");
        assert!(dumps.available());

        tmp.watchdog_dump("WATCHDOG-2.dmp", 0x141);
        assert_eq!(dumps.poll(), 1);
    }

    #[test]
    fn monitor_counts_a_synthetic_tdr_event_and_its_dump_once() {
        static LOG: Mutex<Vec<u64>> = Mutex::new(Vec::new());
        fn events(after: u64) -> Option<Vec<u64>> {
            ids_after(&LOG, after)
        }
        LOG.lock().unwrap().extend([3, 9]);
        let tmp = TempRoot::new("monitor");
        std::fs::create_dir_all(tmp.reports()).unwrap();
        let mut monitor = TdrMonitor::with_sources(events, tmp.ledger());
        assert_eq!(poll_now(&mut monitor), 0);

        LOG.lock().unwrap().push(12);
        assert_eq!(poll_now(&mut monitor), 1);

        tmp.watchdog_dump("WATCHDOG-1.dmp", 0x141);
        assert_eq!(poll_now(&mut monitor), 1);

        tmp.watchdog_dump("WATCHDOG-2.dmp", 0x117);
        let c = monitor.poll_now();
        assert_eq!(c.delta_since_program_start, 2);
        assert_eq!(c.absolute_since_boot, 4);
    }

    #[test]
    fn monitor_keeps_counting_after_old_events_roll_off_the_log() {
        static LOG: Mutex<Vec<u64>> = Mutex::new(Vec::new());
        fn events(after: u64) -> Option<Vec<u64>> {
            ids_after(&LOG, after)
        }
        LOG.lock().unwrap().extend(1..=5);
        let tmp = TempRoot::new("monitor-wrap");
        let mut monitor = TdrMonitor::with_sources(events, tmp.ledger());

        LOG.lock().unwrap().remove(0);
        assert_eq!(poll_now(&mut monitor), 0);
        LOG.lock().unwrap().push(6);
        assert_eq!(poll_now(&mut monitor), 1);
        LOG.lock().unwrap().drain(..3);
        assert_eq!(
            poll_now(&mut monitor),
            1,
            "rolled-off events lowered the count"
        );
    }

    #[test]
    fn monitor_counts_a_dump_when_the_event_log_is_unreadable() {
        fn no_events(_: u64) -> Option<Vec<u64>> {
            None
        }
        let tmp = TempRoot::new("monitor-no-events");
        std::fs::create_dir_all(tmp.reports()).unwrap();
        let mut monitor = TdrMonitor::with_sources(no_events, tmp.ledger());

        tmp.watchdog_dump("WATCHDOG-1.dmp", 0x141);
        assert_eq!(poll_now(&mut monitor), 1);
    }

    #[test]
    fn poll_is_throttled_and_poll_now_is_not() {
        static LOG: Mutex<Vec<u64>> = Mutex::new(Vec::new());
        fn events(after: u64) -> Option<Vec<u64>> {
            ids_after(&LOG, after)
        }
        let tmp = TempRoot::new("monitor-throttle");
        let mut monitor = TdrMonitor::with_sources(events, tmp.ledger());
        assert_eq!(monitor.poll().delta_since_program_start, 0);

        LOG.lock().unwrap().push(1);
        assert_eq!(monitor.poll().delta_since_program_start, 0);
        assert_eq!(poll_now(&mut monitor), 1);
    }

    #[test]
    fn event_query_filters_tdr_ids_sources_and_newer_records() {
        let query = tdr_event_query(35231);
        for part in [
            "EventID=4101",
            "EventID=4109",
            "@Name='Display'",
            "@Name='nvlddmkm'",
            "EventRecordID>35231",
        ] {
            assert!(query.contains(part), "{part} missing from {query}");
        }
    }

    #[test]
    fn event_query_opens_against_the_system_channel() {
        assert!(
            system_tdr_events(0).is_some(),
            "System channel rejected {}",
            tdr_event_query(0)
        );
    }

    // Reads live sources: cargo test -p stress-kit --lib tdr -- --ignored --nocapture
    #[test]
    #[ignore]
    fn opens_against_live_sources() {
        let mut m = TdrMonitor::open().expect("no TDR source readable");
        println!("TDR counters: {:?}", m.poll_now());
    }
}
