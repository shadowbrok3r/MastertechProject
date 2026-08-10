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
    DesktopMonitorInfo, DesktopMouseButton, DesktopShot,
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
    set_clipboard_sync(true);

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

/// True while the capture thread is alive.
pub fn is_streaming() -> bool {
    CAPTURE_RUNNING.load(Ordering::SeqCst)
}

/// Stop desktop streaming. The capture thread exits after its current frame.
pub fn stop_desktop_stream() {
    STOP_FLAG.store(true, Ordering::SeqCst);
    set_clipboard_sync(false);
}

/// Blocks or restores system sleep/hibernation and display timeout for the
/// calling thread; the OS drops the requirement automatically if the thread dies.
#[cfg(target_os = "windows")]
fn set_keep_awake(active: bool) {
    use windows::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
    };
    let state = if active {
        ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
    } else {
        ES_CONTINUOUS
    };
    unsafe { SetThreadExecutionState(state) };
    log::info!(
        "remote desktop: keep-awake {}",
        if active { "enabled" } else { "released" }
    );
}

#[cfg(not(target_os = "windows"))]
fn set_keep_awake(_active: bool) {}

/// Wakes a powered-off display by injecting a 1px mouse jiggle.
#[cfg(target_os = "windows")]
fn wake_display() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT,
    };
    let jiggle = |dx: i32, dy: i32| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [jiggle(1, 0), jiggle(-1, 0)];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent != inputs.len() as u32 {
        log::warn!("remote desktop: wake_display SendInput injected {sent}/2 events");
    }
}

#[cfg(not(target_os = "windows"))]
fn wake_display() {}

fn capture_loop() {
    set_keep_awake(true);
    wake_display();
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
    set_keep_awake(false);
    set_clipboard_sync(false);
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

/// Captures one monitor and JPEG-encodes it, recording the geometry the input
/// thread needs to map normalized coordinates.
fn grab(monitor_id: u32, scale: f32, quality: u8) -> anyhow::Result<Option<DesktopShot>> {
    let monitors = xcap::Monitor::all()?;
    let Some(monitor) = pick_monitor(&monitors, monitor_id) else {
        return Ok(None);
    };

    let mon_x = monitor.x().unwrap_or(0);
    let mon_y = monitor.y().unwrap_or(0);
    let rgba = monitor
        .capture_image()
        .map_err(|e| match crate::window_info::unreachable_input_desktop() {
            Some(why) => anyhow::anyhow!("{why}, so this session cannot capture the screen: {e}"),
            None => anyhow::Error::from(e),
        })?;
    let (w, h) = (rgba.width(), rgba.height());
    // Input mapping reads this; a capture must happen before injection works.
    *ACTIVE_GEOM.lock().unwrap() = Some(MonitorGeom {
        x: mon_x,
        y: mon_y,
        width: w,
        height: h,
    });

    let scale = scale.clamp(0.1, 1.0);
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
    let mut enc = JpegEncoder::new_with_quality(&mut buf, quality.clamp(1, 100));
    enc.encode_image(&rgb)?;

    Ok(Some(DesktopShot {
        monitor_id: monitor.id().unwrap_or(0),
        width: tw,
        height: th,
        monitor_width: w,
        monitor_height: h,
        encode_ms: encode_start.elapsed().as_millis() as u32,
        jpeg: buf,
    }))
}

/// One-shot capture for the MCP path, bypassing the push stream entirely.
pub fn capture_once(monitor: u32, scale: f32, quality: u8) -> anyhow::Result<DesktopShot> {
    grab(monitor, scale, quality)?
        .ok_or_else(|| anyhow::anyhow!("no monitors available to capture"))
}

fn capture_and_encode(cfg: &CaptureConfig, frame_count: u64) -> anyhow::Result<Option<Vec<u8>>> {
    let Some(shot) = grab(cfg.monitor, cfg.scale, cfg.quality)? else {
        return Ok(None);
    };

    let msg = DesktopFrameMessage {
        frame_count,
        timestamp_ms: now_ms(),
        monitor_id: shot.monitor_id,
        width: shot.width,
        height: shot.height,
        encoding: DesktopFrameEncoding::Jpeg,
        data: shot.jpeg,
        encode_ms: shot.encode_ms,
        cursor_x: -1,
        cursor_y: -1,
    };

    let ser = bincode::serde::encode_to_vec(&msg, bincode::config::standard())?;
    let mut tagged = Vec::with_capacity(1 + ser.len());
    tagged.push(DESKTOP_FRAME_TAG);
    tagged.extend_from_slice(&ser);
    Ok(Some(tagged))
}

/// Gap between injected characters.
///
/// A whole string handed to `Enigo::text` goes out as one rapid burst of
/// synthetic key events, and a target whose input queue cannot keep up drops or
/// repeats characters: `"Claude drove"` was observed arriving as
/// `"vvvvvvvvvvve"` roughly one time in three. Pacing costs ~8 ms per character
/// and makes it deterministic.
const TYPE_CHAR_DELAY: Duration = Duration::from_millis(8);

/// Types `text` one character at a time so the target can keep up.
fn type_paced(enigo: &mut Enigo, text: &str) -> anyhow::Result<()> {
    let mut buf = [0u8; 4];
    for ch in text.chars() {
        enigo
            .text(ch.encode_utf8(&mut buf))
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        std::thread::sleep(TYPE_CHAR_DELAY);
    }
    Ok(())
}

/// Injects a batch of events on the input thread, then waits `settle_ms` so the
/// UI can react before the caller screenshots.
///
/// Returns an error when nothing has been captured yet: pointer coordinates are
/// normalized against the active monitor, so injecting before a capture would
/// click somewhere arbitrary.
pub fn inject_batch(events: Vec<DesktopInputEvent>, settle_ms: u32) -> anyhow::Result<usize> {
    let needs_geom = events
        .iter()
        .any(|e| matches!(e, DesktopInputEvent::MouseMove { .. } | DesktopInputEvent::MouseButton { .. }));
    if needs_geom && ACTIVE_GEOM.lock().map(|g| g.is_none()).unwrap_or(true) {
        anyhow::bail!(
            "no monitor geometry yet; take a screenshot before sending pointer input so \
             coordinates map to the right screen"
        );
    }

    let tx = desktop_input_sender();
    let n = events.len();
    for ev in events {
        tx.send(ev)
            .map_err(|e| anyhow::anyhow!("desktop input thread is gone: {e}"))?;
    }
    if settle_ms > 0 {
        std::thread::sleep(Duration::from_millis(settle_ms.min(10_000) as u64));
    }
    Ok(n)
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

// ── Clipboard mirroring ───────────────────────────────────────────────────────
//
// A single thread owns the `arboard::Clipboard` for the process lifetime: on
// X11 the clipboard contents are served by the owning process, so a handle
// dropped after `set_text` loses the copied data. The same thread both polls
// for local changes and applies inbound text, so `last` covers both directions
// and neither side echoes what it just received.

/// How often the client re-reads its own clipboard while mirroring is on.
const CLIPBOARD_POLL: Duration = Duration::from_millis(300);

/// Clipboard payloads above this are dropped rather than mirrored.
const CLIPBOARD_MAX_BYTES: usize = 1024 * 1024;

/// How long an inbound apply waits for the clipboard thread to finish writing.
const CLIPBOARD_APPLY_TIMEOUT: Duration = Duration::from_millis(500);

static CLIPBOARD_ENABLED: AtomicBool = AtomicBool::new(false);
static CLIPBOARD_TX: OnceLock<Sender<ClipboardMsg>> = OnceLock::new();

enum ClipboardMsg {
    /// Text from the admin to place on this machine's clipboard, plus an ack
    /// the caller waits on.
    Apply(String, Sender<()>),
    /// Mirroring was turned on; re-seed `last` from the current contents.
    Reseed,
}

fn clipboard_sender() -> &'static Sender<ClipboardMsg> {
    CLIPBOARD_TX.get_or_init(|| {
        let (tx, rx) = crossbeam::channel::unbounded::<ClipboardMsg>();
        if let Err(e) = std::thread::Builder::new()
            .name("remote-desktop-clipboard".into())
            .spawn(move || clipboard_loop(rx))
        {
            log::error!("remote desktop: failed to spawn clipboard thread: {e}");
        }
        tx
    })
}

/// Turn clipboard mirroring on or off. Called on desktop stream start/stop and
/// by [`displays::Cmd::ClipboardSyncEnable`].
pub fn set_clipboard_sync(enabled: bool) {
    let was = CLIPBOARD_ENABLED.swap(enabled, Ordering::SeqCst);
    if was == enabled {
        return;
    }
    // Wake the thread so a disabled->enabled flip reseeds instead of pushing
    // whatever was copied while mirroring was off.
    let _ = clipboard_sender().send(ClipboardMsg::Reseed);
    log::info!(
        "remote desktop: clipboard mirroring {}",
        if enabled { "enabled" } else { "disabled" }
    );
}

/// Place admin-side clipboard text onto this machine's clipboard, blocking
/// until the write lands so a paste chord right behind it sees the new value.
pub fn apply_clipboard(text: String) {
    if !CLIPBOARD_ENABLED.load(Ordering::SeqCst) {
        return;
    }
    if text.len() > CLIPBOARD_MAX_BYTES {
        log::warn!(
            "remote desktop: dropping {} byte inbound clipboard (cap {CLIPBOARD_MAX_BYTES})",
            text.len()
        );
        return;
    }
    let (ack_tx, ack_rx) = crossbeam::channel::bounded(1);
    if clipboard_sender()
        .send(ClipboardMsg::Apply(text, ack_tx))
        .is_err()
    {
        return;
    }
    if ack_rx.recv_timeout(CLIPBOARD_APPLY_TIMEOUT).is_err() {
        log::warn!("remote desktop: clipboard apply did not ack within {CLIPBOARD_APPLY_TIMEOUT:?}");
    }
}

fn clipboard_loop(rx: Receiver<ClipboardMsg>) {
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            log::error!("remote desktop: clipboard init failed: {e}");
            return;
        }
    };
    let mut last = clipboard.get_text().unwrap_or_default();

    loop {
        // Parked entirely while mirroring is off, so a connected-but-idle
        // client costs nothing.
        let msg = if CLIPBOARD_ENABLED.load(Ordering::SeqCst) {
            match rx.recv_timeout(CLIPBOARD_POLL) {
                Ok(m) => Some(m),
                Err(crossbeam::channel::RecvTimeoutError::Timeout) => None,
                Err(crossbeam::channel::RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match rx.recv() {
                Ok(m) => Some(m),
                Err(_) => return,
            }
        };

        match msg {
            Some(ClipboardMsg::Apply(text, ack)) => {
                if text != last {
                    match clipboard.set_text(text.clone()) {
                        Ok(()) => last = text,
                        Err(e) => log::warn!("remote desktop: clipboard set failed: {e}"),
                    }
                }
                let _ = ack.try_send(());
            }
            Some(ClipboardMsg::Reseed) => {
                last = clipboard.get_text().unwrap_or_default();
            }
            None => {}
        }

        if !CLIPBOARD_ENABLED.load(Ordering::SeqCst) {
            continue;
        }
        // A non-text clipboard (image, file drop) reads as an error; leaving
        // `last` alone means the next text copy still registers as a change.
        let Ok(text) = clipboard.get_text() else {
            continue;
        };
        if text == last {
            continue;
        }
        last = text.clone();
        if text.len() > CLIPBOARD_MAX_BYTES {
            log::warn!(
                "remote desktop: not mirroring {} byte clipboard (cap {CLIPBOARD_MAX_BYTES})",
                text.len()
            );
            continue;
        }
        match bincode::serde::encode_to_vec(
            &displays::Cmd::ClipboardSync { text },
            bincode::config::standard(),
        ) {
            Ok(payload) => crate::tcp_listener::broadcast_desktop_cmd(payload),
            Err(e) => log::warn!("remote desktop: clipboard encode failed: {e}"),
        }
    }
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
        DesktopInputEvent::Text(text) => type_paced(enigo, &text)?,
        DesktopInputEvent::ClipboardSet(text) => apply_clipboard(text),
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
