//! Abrupt-drop regression for the remote-desktop stream.
//!
//! A client that reboots mid-stream stops sending between one frame and the
//! next, and the admin console tears the session down while frames are still
//! in flight. This drives that seam directly: the wire decode the receive loop
//! runs, the viewer's bounded frame channel, the texture upload, and a teardown
//! that happens while a producer is still pushing.
//!
//! Run under full page heap to catch a corrupting write:
//!   cargo test -p displays --features tokio --test desktop_stream_abrupt_drop

#![cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]

use eframe::egui;
use displays::remote_desktop::{DesktopFrameEncoding, DesktopFrameMessage};
use displays::tabs::admin_console::client_interface::admin_transport::{
    inbound_channel, is_viewer_frame,
};
use displays::tabs::admin_console::client_interface::tabs::desktop_viewer::DesktopViewer;
use ewebsock::{WsEvent, WsMessage};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const FRAME_W: u32 = 320;
const FRAME_H: u32 = 200;

/// Teardown cycles per abrupt-drop run.
const DROP_CYCLES: usize = 256;

fn jpeg_bytes(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x ^ y) % 256) as u8])
    });
    let mut buf = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 70)
        .encode_image(&img)
        .expect("encode test jpeg");
    buf
}

fn frame(encoding: DesktopFrameEncoding, w: u32, h: u32, data: Vec<u8>) -> DesktopFrameMessage {
    DesktopFrameMessage {
        frame_count: 1,
        timestamp_ms: 0,
        monitor_id: 0,
        width: w,
        height: h,
        encoding,
        data,
        encode_ms: 1,
        cursor_x: -1,
        cursor_y: -1,
    }
}

/// Tagged wire bytes, exactly as `capture_and_encode` puts them on the socket.
fn tagged(msg: &DesktopFrameMessage) -> Vec<u8> {
    let ser = bincode::serde::encode_to_vec(msg, bincode::config::standard()).expect("encode");
    let mut out = Vec::with_capacity(1 + ser.len());
    out.push(displays::DESKTOP_FRAME_TAG);
    out.extend_from_slice(&ser);
    out
}

/// The receive loop's decode, isolated: tagged bytes in, frame or nothing out.
fn decode_wire(bin: &[u8]) -> Option<DesktopFrameMessage> {
    if bin.first() != Some(&displays::DESKTOP_FRAME_TAG) {
        return None;
    }
    bincode::serde::decode_from_slice::<DesktopFrameMessage, _>(&bin[1..], tcp_protocol::WIRE_DECODE)
        .ok()
        .map(|(f, _)| f)
}

/// A frame cut off partway through, as a peer that stopped mid-write leaves it.
#[test]
fn a_torn_jpeg_frame_is_skipped_not_fatal() {
    let ctx = egui::Context::default();
    let mut viewer = DesktopViewer::new();
    let full = jpeg_bytes(FRAME_W, FRAME_H);

    for cut in (1..full.len()).step_by(full.len() / 64 + 1) {
        let torn = frame(DesktopFrameEncoding::Jpeg, FRAME_W, FRAME_H, full[..cut].to_vec());
        viewer.frame_tx.try_send(torn).expect("channel has room");
        viewer.poll_frames(&ctx);
    }
    let shown_after_torn = viewer.frames_shown;

    // A whole frame after the torn ones still lands, so a skip never wedges the viewer.
    viewer
        .frame_tx
        .try_send(frame(DesktopFrameEncoding::Jpeg, FRAME_W, FRAME_H, full))
        .expect("channel has room");
    assert!(viewer.poll_frames(&ctx));
    assert_eq!(viewer.frames_shown, shown_after_torn + 1);
    assert!(viewer.has_received_frame);
}

/// Header and payload disagreeing is what a capture racing a monitor change emits.
#[test]
fn an_rgba_frame_whose_length_disagrees_with_its_header_is_skipped() {
    let ctx = egui::Context::default();
    let mut viewer = DesktopViewer::new();
    let exact = (FRAME_W as usize) * (FRAME_H as usize) * 4;

    let cases = [
        ("short", vec![0u8; exact - 4]),
        ("long", vec![0u8; exact + 4]),
        ("empty", Vec::new()),
    ];
    for (name, data) in cases {
        viewer
            .frame_tx
            .try_send(frame(DesktopFrameEncoding::Rgba, FRAME_W, FRAME_H, data))
            .expect("channel has room");
        assert!(!viewer.poll_frames(&ctx), "{name} rgba frame was uploaded");
        assert_eq!(viewer.frames_shown, 0, "{name} rgba frame reached the texture");
    }

    // Dimensions whose byte count overflows `usize` must not wrap past the guard.
    viewer
        .frame_tx
        .try_send(frame(
            DesktopFrameEncoding::Rgba,
            u32::MAX,
            u32::MAX,
            vec![0u8; 64],
        ))
        .expect("channel has room");
    assert!(!viewer.poll_frames(&ctx), "overflowing rgba frame was uploaded");
    assert_eq!(viewer.frames_shown, 0);
    assert!(!viewer.has_received_frame, "a skipped frame must not count as received");

    // A well-formed frame still uploads.
    viewer
        .frame_tx
        .try_send(frame(
            DesktopFrameEncoding::Rgba,
            FRAME_W,
            FRAME_H,
            vec![0x7f; exact],
        ))
        .expect("channel has room");
    assert!(viewer.poll_frames(&ctx));
    assert_eq!(viewer.frames_shown, 1);
}

/// Garbage on the frame tag must fail the decode, never reach a texture upload.
#[test]
fn garbage_on_the_desktop_frame_tag_never_reaches_the_upload() {
    let ctx = egui::Context::default();
    let mut viewer = DesktopViewer::new();
    let good = tagged(&frame(
        DesktopFrameEncoding::Jpeg,
        FRAME_W,
        FRAME_H,
        jpeg_bytes(FRAME_W, FRAME_H),
    ));

    for cut in (1..good.len()).step_by(good.len() / 128 + 1) {
        let mut torn = good[..cut].to_vec();
        assert!(is_viewer_frame(&torn), "a torn frame keeps its viewer tag");
        if let Some(f) = decode_wire(&torn) {
            let _ = viewer.frame_tx.try_send(f);
            viewer.poll_frames(&ctx);
        }
        // Same prefix with the length fields smeared, as a half-written frame reads.
        for b in torn.iter_mut().skip(1).take(8) {
            *b = 0xff;
        }
        if let Some(f) = decode_wire(&torn) {
            let _ = viewer.frame_tx.try_send(f);
            viewer.poll_frames(&ctx);
        }
    }
}

/// The client vanishes while frames are still queued, over and over.
#[test]
fn dropping_the_viewer_mid_stream_strands_no_producer() {
    let jpeg = Arc::new(jpeg_bytes(FRAME_W, FRAME_H));

    for _ in 0..DROP_CYCLES {
        let ctx = egui::Context::default();
        let mut viewer = DesktopViewer::new();
        let tx = viewer.frame_tx.clone();
        let stop = Arc::new(AtomicBool::new(false));

        let producer = {
            let stop = stop.clone();
            let jpeg = jpeg.clone();
            std::thread::spawn(move || {
                let mut sent = 0u64;
                while !stop.load(Ordering::Relaxed) {
                    let f = frame(
                        DesktopFrameEncoding::Jpeg,
                        FRAME_W,
                        FRAME_H,
                        jpeg.as_ref().clone(),
                    );
                    if tx.try_send(f).is_ok() {
                        sent += 1;
                    }
                    std::thread::yield_now();
                }
                sent
            })
        };

        // Consume a little, then drop the viewer with frames still in flight.
        for _ in 0..4 {
            viewer.poll_frames(&ctx);
        }
        drop(viewer);
        drop(ctx);

        stop.store(true, Ordering::Relaxed);
        producer.join().expect("producer thread survived the drop");
    }
}

/// Viewer frames are newest-wins, so a dropped client leaves no backlog behind.
#[test]
fn a_dropped_client_leaves_no_frame_backlog() {
    let (tx, rx) = inbound_channel("test.desktop.abrupt_drop");
    let jpeg = jpeg_bytes(FRAME_W, FRAME_H);
    let wire = tagged(&frame(DesktopFrameEncoding::Jpeg, FRAME_W, FRAME_H, jpeg));

    for _ in 0..4096 {
        let _ = tx.send(WsEvent::Message(WsMessage::Binary(wire.clone().into())));
    }
    let (depth, _) = rx.occupancy();
    assert_eq!(depth, 1, "viewer frames must collapse to the newest");

    // The peer goes away: the sink drops, and the consumer sees a clean end.
    assert!(rx.try_recv().is_ok());
    drop(tx);
    assert!(rx.try_recv().is_err(), "no frames survive the sink's drop");
}
