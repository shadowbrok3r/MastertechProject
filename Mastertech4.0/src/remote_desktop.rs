//! Native full-desktop capture and OS input injection for admin remote control.
//!
//! Capture runs on a dedicated `std::thread` (never the WASM-plugin path, which
//! holds the egui-thread plugin lock) and pushes JPEG frames into the
//! [`crate::tcp_listener`] desktop broadcast. Input injection runs on a second
//! thread that owns a single [`enigo::Enigo`] and maps normalized admin-side
//! coordinates to absolute screen pixels using the active monitor geometry.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crossbeam::channel::{Receiver, Sender};
use displays::remote_desktop::{
    DesktopFrameEncoding, DesktopFrameMessage, DesktopInputEvent, DesktopModifiers,
    DesktopMonitorInfo, DesktopMouseButton,
};
use displays::DESKTOP_FRAME_TAG;
use enigo::{Axis, Button, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;

/// Geometry of the monitor currently being streamed, in virtual-desktop pixels.
/// Set by the capture thread; read by the input thread to map normalized
/// pointer coordinates to absolute screen pixels.
#[derive(Clone, Copy)]
struct MonitorGeom {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

static ACTIVE_GEOM: Mutex<Option<MonitorGeom>> = Mutex::new(None);

#[derive(Clone)]
struct CaptureConfig {
    monitor: u32,
    fps: u32,
    quality: u8,
    scale: f32,
}

static CONFIG: Mutex<Option<CaptureConfig>> = Mutex::new(None);
static CAPTURE_RUNNING: AtomicBool = AtomicBool::new(false);
static STOP_FLAG: AtomicBool = AtomicBool::new(false);

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Start (or reconfigure) desktop streaming. Idempotent: a second call while a
/// stream is live just updates the config the capture loop reads each frame.
pub fn start_desktop_stream(monitor: u32, fps: u32, quality: u8, scale: f32) {
    let cfg = CaptureConfig {
        monitor,
        fps: fps.clamp(1, 60),
        quality: quality.clamp(1, 100),
        scale: scale.clamp(0.1, 1.0),
    };
    *CONFIG.lock().unwrap() = Some(cfg);

    if CAPTURE_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        STOP_FLAG.store(false, Ordering::SeqCst);
        if let Err(e) = std::thread::Builder::new()
            .name("remote-desktop-capture".into())
            .spawn(capture_loop)
        {
            log::error!("remote desktop: failed to spawn capture thread: {e}");
            CAPTURE_RUNNING.store(false, Ordering::SeqCst);
        } else {
            log::info!("remote desktop: capture started (monitor {monitor}, {fps} fps)");
        }
    }
}

/// Stop desktop streaming. The capture thread exits after its current frame.
pub fn stop_desktop_stream() {
    STOP_FLAG.store(true, Ordering::SeqCst);
}

fn capture_loop() {
    let mut frame_count: u64 = 0;
    let mut idle_ticks: u32 = 0;
    while !STOP_FLAG.load(Ordering::Relaxed) {
        let Some(cfg) = CONFIG.lock().unwrap().clone() else {
            break;
        };
        let target_dt = Duration::from_secs_f32(1.0 / cfg.fps.max(1) as f32);
        let started = Instant::now();

        // Stop capturing after ~3s with no admin sessions subscribed, so an
        // ungraceful admin disconnect doesn't leave the client capturing.
        if crate::tcp_listener::desktop_frame_subscriber_count() == 0 {
            idle_ticks = idle_ticks.saturating_add(1);
            if idle_ticks > cfg.fps.max(1) * 3 {
                log::info!("remote desktop: no subscribers, auto-stopping capture");
                break;
            }
        } else {
            idle_ticks = 0;
        }

        match capture_and_encode(&cfg, frame_count) {
            Ok(Some(tagged)) => {
                crate::tcp_listener::broadcast_desktop_frame(tagged);
                frame_count = frame_count.wrapping_add(1);
            }
            Ok(None) => {}
            Err(e) => {
                log::warn!("remote desktop: capture failed: {e}");
                std::thread::sleep(Duration::from_millis(250));
            }
        }

        let elapsed = started.elapsed();
        if elapsed < target_dt {
            std::thread::sleep(target_dt - elapsed);
        }
    }
    CAPTURE_RUNNING.store(false, Ordering::SeqCst);
    log::info!("remote desktop: capture stopped");
}

fn pick_monitor(monitors: &[xcap::Monitor], id: u32) -> Option<xcap::Monitor> {
    if let Some(m) = monitors
        .iter()
        .find(|m| m.id().map(|i| i == id).unwrap_or(false))
    {
        return Some(m.clone());
    }
    if let Some(m) = monitors
        .iter()
        .find(|m| m.is_primary().unwrap_or(false))
    {
        return Some(m.clone());
    }
    monitors.first().cloned()
}

fn capture_and_encode(cfg: &CaptureConfig, frame_count: u64) -> anyhow::Result<Option<Vec<u8>>> {
    let monitors = xcap::Monitor::all()?;
    let Some(monitor) = pick_monitor(&monitors, cfg.monitor) else {
        return Ok(None);
    };

    let mon_x = monitor.x().unwrap_or(0);
    let mon_y = monitor.y().unwrap_or(0);
    let rgba = monitor.capture_image()?;
    let (w, h) = (rgba.width(), rgba.height());
    *ACTIVE_GEOM.lock().unwrap() = Some(MonitorGeom {
        x: mon_x,
        y: mon_y,
        width: w,
        height: h,
    });

    let scale = cfg.scale.clamp(0.1, 1.0);
    let tw = ((w as f32 * scale) as u32).max(1);
    let th = ((h as f32 * scale) as u32).max(1);
    let scaled = if tw != w || th != h {
        image::imageops::resize(&rgba, tw, th, FilterType::Triangle)
    } else {
        rgba
    };

    let encode_start = Instant::now();
    let rgb = image::DynamicImage::ImageRgba8(scaled).into_rgb8();
    let mut buf: Vec<u8> = Vec::new();
    let mut enc = JpegEncoder::new_with_quality(&mut buf, cfg.quality.clamp(1, 100));
    enc.encode_image(&rgb)?;
    let encode_ms = encode_start.elapsed().as_millis() as u32;

    let msg = DesktopFrameMessage {
        frame_count,
        timestamp_ms: now_ms(),
        monitor_id: monitor.id().unwrap_or(0),
        width: tw,
        height: th,
        encoding: DesktopFrameEncoding::Jpeg,
        data: buf,
        encode_ms,
        cursor_x: -1,
        cursor_y: -1,
    };

    let ser = bincode::serde::encode_to_vec(&msg, bincode::config::standard())?;
    let mut tagged = Vec::with_capacity(1 + ser.len());
    tagged.push(DESKTOP_FRAME_TAG);
    tagged.extend_from_slice(&ser);
    Ok(Some(tagged))
}

/// Enumerate the client's monitors for [`displays::Cmd::DesktopMonitorList`].
pub fn enumerate_monitors() -> Vec<DesktopMonitorInfo> {
    let Ok(monitors) = xcap::Monitor::all() else {
        return Vec::new();
    };
    monitors
        .iter()
        .map(|m| DesktopMonitorInfo {
            id: m.id().unwrap_or(0),
            name: m.name().unwrap_or_default(),
            x: m.x().unwrap_or(0),
            y: m.y().unwrap_or(0),
            width: m.width().unwrap_or(0),
            height: m.height().unwrap_or(0),
            is_primary: m.is_primary().unwrap_or(false),
            scale_factor: m.scale_factor().unwrap_or(1.0),
        })
        .collect()
}

// ── Input injection ───────────────────────────────────────────────────────────

static INPUT_TX: OnceLock<Sender<DesktopInputEvent>> = OnceLock::new();

/// Lazily spawns the injection thread on first use and returns its sender.
pub fn desktop_input_sender() -> &'static Sender<DesktopInputEvent> {
    INPUT_TX.get_or_init(|| {
        let (tx, rx) = crossbeam::channel::unbounded::<DesktopInputEvent>();
        if let Err(e) = std::thread::Builder::new()
            .name("remote-desktop-input".into())
            .spawn(move || input_loop(rx))
        {
            log::error!("remote desktop: failed to spawn input thread: {e}");
        }
        tx
    })
}

fn input_loop(rx: Receiver<DesktopInputEvent>) {
    let mut enigo = match Enigo::new(&Settings::default()) {
        Ok(e) => e,
        Err(e) => {
            log::error!("remote desktop: enigo init failed: {e}");
            return;
        }
    };
    let mut held = DesktopModifiers::default();
    while let Ok(ev) = rx.recv() {
        if let Err(e) = apply_input(&mut enigo, ev, &mut held) {
            log::warn!("remote desktop: input inject failed: {e}");
        }
    }
}

fn norm_to_abs(nx: f32, ny: f32) -> Option<(i32, i32)> {
    let g = (*ACTIVE_GEOM.lock().unwrap())?;
    let x = g.x + (nx.clamp(0.0, 1.0) * g.width as f32).round() as i32;
    let y = g.y + (ny.clamp(0.0, 1.0) * g.height as f32).round() as i32;
    Some((x, y))
}

/// Position the cursor at absolute virtual-desktop pixel coordinates.
/// Uses `SetCursorPos` on Windows (correct across all monitors, including
/// negative origins); falls back to enigo's primary-normalized `Abs` elsewhere.
fn position_cursor(_enigo: &mut Enigo, x: i32, y: i32) -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;
        unsafe { SetCursorPos(x, y).map_err(|e| anyhow::anyhow!("{e}"))? };
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        _enigo
            .move_mouse(x, y, enigo::Coordinate::Abs)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(())
    }
}

fn sync_modifiers(
    enigo: &mut Enigo,
    want: DesktopModifiers,
    held: &mut DesktopModifiers,
) -> anyhow::Result<()> {
    let mut set = |key: Key, want: bool, held: &mut bool| -> anyhow::Result<()> {
        if want != *held {
            enigo
                .key(key, if want { Direction::Press } else { Direction::Release })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            *held = want;
        }
        Ok(())
    };
    set(Key::Control, want.ctrl, &mut held.ctrl)?;
    set(Key::Shift, want.shift, &mut held.shift)?;
    set(Key::Alt, want.alt, &mut held.alt)?;
    set(Key::Meta, want.meta, &mut held.meta)?;
    Ok(())
}

fn apply_input(
    enigo: &mut Enigo,
    ev: DesktopInputEvent,
    held: &mut DesktopModifiers,
) -> anyhow::Result<()> {
    match ev {
        DesktopInputEvent::MouseMove { x, y } => {
            if let Some((ax, ay)) = norm_to_abs(x, y) {
                position_cursor(enigo, ax, ay)?;
            }
        }
        DesktopInputEvent::MouseButton { x, y, button, pressed } => {
            if let Some((ax, ay)) = norm_to_abs(x, y) {
                position_cursor(enigo, ax, ay)?;
            }
            let b = match button {
                DesktopMouseButton::Left => Button::Left,
                DesktopMouseButton::Right => Button::Right,
                DesktopMouseButton::Middle => Button::Middle,
            };
            enigo
                .button(b, if pressed { Direction::Press } else { Direction::Release })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        DesktopInputEvent::MouseScroll { delta_x, delta_y } => {
            if delta_y != 0.0 {
                enigo
                    .scroll(-(delta_y.round() as i32), Axis::Vertical)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
            }
            if delta_x != 0.0 {
                enigo
                    .scroll(delta_x.round() as i32, Axis::Horizontal)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
            }
        }
        DesktopInputEvent::Key { key_name, pressed, modifiers } => {
            sync_modifiers(enigo, modifiers, held)?;
            if let Some(key) = map_key(&key_name) {
                enigo
                    .key(key, if pressed { Direction::Press } else { Direction::Release })
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
            }
        }
        DesktopInputEvent::Text(text) => {
            enigo.text(&text).map_err(|e| anyhow::anyhow!("{e}"))?;
        }
    }
    Ok(())
}

/// Maps an `egui::Key::name()` string to an enigo key.
fn map_key(name: &str) -> Option<Key> {
    let key = match name {
        "ArrowDown" => Key::DownArrow,
        "ArrowLeft" => Key::LeftArrow,
        "ArrowRight" => Key::RightArrow,
        "ArrowUp" => Key::UpArrow,
        "Escape" => Key::Escape,
        "Tab" => Key::Tab,
        "Backspace" => Key::Backspace,
        "Enter" => Key::Return,
        "Space" => Key::Space,
        "Delete" => Key::Delete,
        "Home" => Key::Home,
        "End" => Key::End,
        "PageUp" => Key::PageUp,
        "PageDown" => Key::PageDown,
        "F1" => Key::F1,
        "F2" => Key::F2,
        "F3" => Key::F3,
        "F4" => Key::F4,
        "F5" => Key::F5,
        "F6" => Key::F6,
        "F7" => Key::F7,
        "F8" => Key::F8,
        "F9" => Key::F9,
        "F10" => Key::F10,
        "F11" => Key::F11,
        "F12" => Key::F12,
        other => {
            let c = if let Some(stripped) = other.strip_prefix("Num") {
                stripped.chars().next().filter(|_| stripped.len() == 1)
            } else if other.len() == 1 {
                other.chars().next().map(|c| c.to_ascii_lowercase())
            } else {
                None
            };
            return c.map(Key::Unicode);
        }
    };
    Some(key)
}
