//! Safe wrappers over the C shims: ES8311/ES7210 audio, the ST7703/LVGL display and GT911 touch.

use std::ffi::{c_char, CString};

use crate::endpoint::Frame;
use crate::viz;

// UI_WAVE_POINTS and UI_BANDS in display_shim.h; audio_shim's tap holds 1024 samples.
const _: () = assert!(viz::WAVE_POINTS == 64 && viz::BANDS == 16 && viz::WINDOW <= 1024);

extern "C" {
    fn audio_init() -> i32;
    fn audio_write(buf: *const u8, len: usize) -> i32;
    fn audio_read(buf: *mut u8, len: usize) -> i32;
    fn audio_set_amp(on: i32);
    fn audio_capture(on: i32);
    fn audio_tap_latest(mic1: *mut i16, mic2: *mut i16, n: usize);
    fn audio_sr_init() -> i32;
    fn audio_wake_take() -> i32;
    fn audio_level_frame(db: *mut f32, speech: *mut i32) -> i32;
    fn audio_set_volume(level: i32);
    fn audio_play_begin();
    fn audio_play_push(buf: *const u8, len: usize) -> i32;
    fn audio_play_end();
    fn audio_play_stop();
    fn audio_play_active() -> i32;
    fn display_init() -> i32;
    fn ui_start() -> i32;
    fn ui_attach_touch() -> i32;
    fn ui_ptt_pressed() -> i32;
    fn ui_set_status(text: *const c_char, color: u32);
    fn ui_set_transcript(text: *const c_char);
    fn ui_set_reply(text: *const c_char);
    fn ui_viz_update(wave1: *const i16, wave2: *const i16, bars1: *const u8, bars2: *const u8);
    fn ui_show_approval(text: *const c_char, hint: *const c_char, can_approve: i32);
    fn ui_hide_approval();
    fn ui_approval_choice() -> i32;
    fn ui_set_volume(level: i32);
    fn ui_volume_poll(released: *mut i32) -> i32;
}

/// MasterTech TUI palette (Deep Pink default), as 0xRRGGBB.
pub mod color {
    pub const MUTED: u32 = 0xBAC2DE;
    pub const ACCENT: u32 = 0xFF1493;
    pub const TERTIARY: u32 = 0xCBA6F7;
    pub const SUCCESS: u32 = 0xA6E3A1;
    pub const ERROR: u32 = 0xF38BA8;
    pub const WARN: u32 = 0xF9E2AF;
}

fn check(code: i32) -> Result<(), i32> {
    if code == 0 {
        Ok(())
    } else {
        Err(code)
    }
}

pub fn init_display() -> Result<(), i32> {
    check(unsafe { display_init() })
}

pub fn start_ui() -> Result<(), i32> {
    check(unsafe { ui_start() })
}

/// Needs the I2C bus that [`init_audio`] installs.
pub fn attach_touch() -> Result<(), i32> {
    check(unsafe { ui_attach_touch() })
}

pub fn init_audio() -> Result<(), i32> {
    check(unsafe { audio_init() })
}

pub fn speaker_write(pcm: &[u8]) -> usize {
    unsafe { audio_write(pcm.as_ptr(), pcm.len()) }.max(0) as usize
}

pub fn mic_read(buf: &mut [u8]) -> usize {
    unsafe { audio_read(buf.as_mut_ptr(), buf.len()) }.max(0) as usize
}

pub fn set_amp(on: bool) {
    unsafe { audio_set_amp(i32::from(on)) }
}

/// Starts (dropping stale audio) or stops queueing mic PCM for [`mic_read`].
pub fn mic_capture(on: bool) {
    unsafe { audio_capture(i32::from(on)) }
}

/// The latest `viz::WINDOW` samples of each mic, oldest first.
pub fn tap_latest(mic1: &mut [i16; viz::WINDOW], mic2: &mut [i16; viz::WINDOW]) {
    unsafe { audio_tap_latest(mic1.as_mut_ptr(), mic2.as_mut_ptr(), viz::WINDOW) }
}

pub fn viz_update(
    wave1: &[i16; viz::WAVE_POINTS],
    wave2: &[i16; viz::WAVE_POINTS],
    bars1: &[u8; viz::BANDS],
    bars2: &[u8; viz::BANDS],
) {
    unsafe { ui_viz_update(wave1.as_ptr(), wave2.as_ptr(), bars1.as_ptr(), bars2.as_ptr()) }
}

/// Starts esp-sr's wake word and voice activity detection.
pub fn init_wake() -> Result<(), i32> {
    check(unsafe { audio_sr_init() })
}

/// True once per detected wake word.
pub fn wake_heard() -> bool {
    unsafe { audio_wake_take() != 0 }
}

/// The oldest queued 32 ms mic level frame.
pub fn level_frame() -> Option<Frame> {
    let (mut db, mut speech) = (0f32, 0i32);
    (unsafe { audio_level_frame(&mut db, &mut speech) } != 0).then_some(Frame { db, speech: speech != 0 })
}

pub fn set_speaker_volume(level: u8) {
    unsafe { audio_set_volume(i32::from(level)) }
}

pub fn show_volume(level: u8) {
    unsafe { ui_set_volume(i32::from(level)) }
}

/// The volume slider's newest value and whether it was released since the last poll.
pub fn volume_poll() -> Option<(u8, bool)> {
    let mut released = 0;
    let level = unsafe { ui_volume_poll(&mut released) };
    u8::try_from(level).ok().map(|l| (l.min(100), released != 0))
}

pub fn show_approval(text: &str, hint: &str, can_approve: bool) {
    let (text, hint) = (c_text(text), c_text(hint));
    unsafe { ui_show_approval(text.as_ptr(), hint.as_ptr(), i32::from(can_approve)) }
}

pub fn hide_approval() {
    unsafe { ui_hide_approval() }
}

/// A tap on the approval card: `Some(true)` approve, `Some(false)` deny or skip.
pub fn approval_choice() -> Option<bool> {
    match unsafe { ui_approval_choice() } {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

pub fn play_begin() {
    unsafe { audio_play_begin() }
}

pub fn play_push(pcm: &[u8]) {
    unsafe { audio_play_push(pcm.as_ptr(), pcm.len()) };
}

pub fn play_end() {
    unsafe { audio_play_end() }
}

pub fn play_stop() {
    unsafe { audio_play_stop() }
}

pub fn play_active() -> bool {
    unsafe { audio_play_active() != 0 }
}

pub fn ptt_pressed() -> bool {
    unsafe { ui_ptt_pressed() != 0 }
}

fn c_text(text: &str) -> CString {
    CString::new(text.replace('\0', "")).unwrap_or_default()
}

pub fn set_status(text: &str, color: u32) {
    let text = c_text(text);
    unsafe { ui_set_status(text.as_ptr(), color) }
}

pub fn set_transcript(text: &str) {
    let text = c_text(text);
    unsafe { ui_set_transcript(text.as_ptr()) }
}

pub fn set_reply(text: &str) {
    let text = c_text(text);
    unsafe { ui_set_reply(text.as_ptr()) }
}
