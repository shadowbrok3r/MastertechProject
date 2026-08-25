//! Memory-growth regression for an admin session whose UI never paints.
//!
//! The socket reader is a background task; its consumer, `WebSocketClient::receive`,
//! only runs while egui paints. This drives the producer at a session-realistic
//! byte rate with no consumer at all and asserts both the queue's own gauge and
//! the process's private commit stay bounded.

#![cfg(not(target_arch = "wasm32"))]

use displays::buffer_census;
use displays::tabs::admin_console::client_interface::admin_transport::{
    inbound_channel, MAX_INBOUND_BYTES,
};
use ewebsock::{WsEvent, WsMessage};

/// Reported leak rate on the admin workstation, 2026-08-24.
const LEAK_RATE_BYTES_PER_SEC: usize = 13_600_000;

/// Session length the producer simulates.
const SESSION_SECS: usize = 60;

/// One remote-viewer frame carrying a full font atlas.
const FRAME_BYTES: usize = 256 * 1024;

/// Growth allowed over the run: the queue's own cap plus allocator slack.
const MAX_GROWTH_BYTES: usize = MAX_INBOUND_BYTES + 96 * 1024 * 1024;

fn viewer_frame() -> WsEvent {
    let mut payload = vec![0u8; FRAME_BYTES];
    payload[0] = displays::EGUI_FRAME_TAG;
    WsEvent::Message(WsMessage::Binary(payload))
}

fn control_frame() -> WsEvent {
    WsEvent::Message(WsMessage::Binary(vec![0x42; FRAME_BYTES]))
}

#[test]
fn an_unpainted_session_does_not_grow_without_bound() {
    let label = "test.session.unpainted";
    let (tx, rx) = inbound_channel(label);

    let produced = LEAK_RATE_BYTES_PER_SEC * SESSION_SECS;
    let frames = produced / FRAME_BYTES;

    // Settle the allocator before the baseline so start-up growth is not counted.
    for _ in 0..64 {
        let _ = tx.send(viewer_frame());
    }
    let before = private_bytes();

    // One control frame in four; the rest is remote-viewer traffic.
    for i in 0..frames {
        let _ = tx.send(if i % 4 == 0 { control_frame() } else { viewer_frame() });
    }

    let after = private_bytes();
    let (depth, bytes) = rx.occupancy();

    assert!(
        bytes <= MAX_INBOUND_BYTES + FRAME_BYTES,
        "{produced} bytes produced with no consumer left {bytes} queued in {depth} frames"
    );

    let gauge = buffer_census::snapshot()
        .into_iter()
        .find(|g| g.label == label)
        .expect("the queue registers itself in the buffer census");
    assert!(gauge.dropped > 0, "the cap dropped nothing, so it never engaged");
    assert!(gauge.peak_bytes <= MAX_INBOUND_BYTES + FRAME_BYTES);

    if let (Some(before), Some(after)) = (before, after) {
        let growth = after.saturating_sub(before);
        assert!(
            growth <= MAX_GROWTH_BYTES,
            "private bytes grew {growth} over a {SESSION_SECS}s session that produced {produced} bytes \
             (cap {MAX_GROWTH_BYTES})"
        );
    }
}

#[test]
fn draining_the_queue_returns_it_to_empty() {
    let (tx, rx) = inbound_channel("test.session.drains_to_empty");
    for _ in 0..512 {
        let _ = tx.send(control_frame());
    }
    while rx.try_recv().is_ok() {}
    assert_eq!(rx.occupancy(), (0, 0));
}

/// This process's private commit, or `None` where it cannot be read.
#[cfg(windows)]
fn private_bytes() -> Option<usize> {
    #[repr(C)]
    #[derive(Default)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(
            process: isize,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }

    let mut counters = ProcessMemoryCounters {
        cb: size_of::<ProcessMemoryCounters>() as u32,
        ..Default::default()
    };
    let ok = unsafe {
        K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb)
    };
    (ok != 0).then_some(counters.pagefile_usage)
}

#[cfg(not(windows))]
fn private_bytes() -> Option<usize> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kb: usize = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}
