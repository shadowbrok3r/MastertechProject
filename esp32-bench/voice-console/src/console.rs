//! The voice session with voice-bridge over the relay room: push-to-talk or the wake word, mic PCM
//! up, streamed speech down, approvals and the speaker volume (wire contract in voice-bridge/src/relay.rs).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use esp_idf_svc::hal::reset::{self, ResetReason};
use esp_idf_svc::ws::client::{EspWebSocketClient, FrameType};
use serde_json::{json, Value};

use crate::endpoint::{End, Endpointer, Noise, NoiseBaseline};
use crate::ffi::{self, color};
use crate::settings::Settings;

const SAMPLE_RATE: u32 = 16_000;
/// 64 ms of 16 kHz mono PCM16.
const MIC_CHUNK: usize = 2048;
const PING_EVERY: Duration = Duration::from_secs(20);
const BRIDGE_STALE: Duration = Duration::from_secs(50);
const MAX_TALK: Duration = Duration::from_secs(30);
/// Ends a hands-free utterance whose level frames stop arriving.
const MAX_VOICE_TALK: Duration = Duration::from_secs(17);
const REPLY_WAIT: Duration = Duration::from_secs(60);
const HEAP_LOG_EVERY: Duration = Duration::from_secs(60);
/// Relay outage that restarts the chip.
const RELAY_DOWN_RESTART: Duration = Duration::from_secs(300);
const DEFAULT_VOLUME: u8 = 75;
const UTT_START: &str = r#"{"cmd":"utt_start"}"#;
const UTT_END: &str = r#"{"cmd":"utt_end"}"#;

/// Socket state the websocket callback forwards alongside relay text.
pub const WS_CONNECTED: &str = "__ws_connected__";
pub const WS_DISCONNECTED: &str = "__ws_disconnected__";

/// A frame for the relay, produced off the session thread.
pub enum Outgoing {
    Text(String),
    Audio(Vec<u8>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Idle,
    Listening,
    Thinking,
    Speaking,
    Approval,
}

/// Mic capture state shared by the session, the mic thread and the relay callback.
#[derive(Default)]
pub struct Mic {
    capture: AtomicBool,
    /// The `utt_end` frame that closes the current utterance.
    end: Mutex<Option<String>>,
}

impl Mic {
    pub fn capturing(&self) -> bool {
        self.capture.load(Ordering::Acquire)
    }

    fn start(&self) {
        *self.end_frame() = None;
        self.capture.store(true, Ordering::Release);
    }

    /// Stops capture; the mic thread closes the utterance with `end`.
    fn close(&self, end: String) {
        *self.end_frame() = Some(end);
        self.stop();
    }

    fn stop(&self) {
        self.capture.store(false, Ordering::Release);
    }

    fn end_frame(&self) -> MutexGuard<'_, Option<String>> {
        self.end.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Streams mic PCM while capturing, bracketed by `utt_start`/`utt_end`.
pub fn mic_loop(mic: Arc<Mic>, out: SyncSender<Outgoing>) {
    loop {
        if !mic.capturing() {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        ffi::mic_capture(true);
        let started = out.send(Outgoing::Text(UTT_START.into())).is_ok();
        while started && mic.capturing() {
            let mut buf = vec![0u8; MIC_CHUNK];
            let n = ffi::mic_read(&mut buf);
            if n == 0 {
                continue;
            }
            buf.truncate(n);
            if out.send(Outgoing::Audio(buf)).is_err() {
                break;
            }
        }
        ffi::mic_capture(false);
        let end = mic.end_frame().take().unwrap_or_else(|| UTT_END.into());
        if !started || out.send(Outgoing::Text(end)).is_err() {
            return;
        }
    }
}

pub fn log_heap() {
    use esp_idf_svc::sys::{
        heap_caps_get_free_size, heap_caps_get_largest_free_block, heap_caps_get_minimum_free_size,
        MALLOC_CAP_INTERNAL, MALLOC_CAP_SPIRAM,
    };
    let (free, low, largest, psram) = unsafe {
        (
            heap_caps_get_free_size(MALLOC_CAP_INTERNAL),
            heap_caps_get_minimum_free_size(MALLOC_CAP_INTERNAL),
            heap_caps_get_largest_free_block(MALLOC_CAP_INTERNAL),
            heap_caps_get_free_size(MALLOC_CAP_SPIRAM),
        )
    };
    log::info!("heap: internal {free} B free ({low} B low-water, {largest} B largest), psram {psram} B free");
}

/// `db` rounded to 0.1 dB for relay frames.
fn tenths(db: f32) -> f64 {
    f64::from((db * 10.0).round()) / 10.0
}

/// Plays a 440 Hz tone for `ms` to the speaker.
fn play_tone(ms: u32) {
    let n = (SAMPLE_RATE * ms / 1000) as usize;
    let mut pcm = Vec::<u8>::with_capacity(n * 2);
    for i in 0..n {
        let t = i as f32 / SAMPLE_RATE as f32;
        let s = (t * 440.0 * std::f32::consts::TAU).sin() * 9000.0;
        pcm.extend_from_slice(&(s as i16).to_le_bytes());
    }
    ffi::set_amp(true);
    ffi::speaker_write(&pcm);
}

/// Mic→speaker loopback until `run` clears.
fn echo_loop(run: Arc<AtomicBool>) {
    let mut buf = [0u8; MIC_CHUNK];
    ffi::set_amp(true);
    ffi::mic_capture(true);
    while run.load(Ordering::Relaxed) {
        let n = ffi::mic_read(&mut buf);
        if n > 0 {
            ffi::speaker_write(&buf[..n]);
        }
    }
    ffi::mic_capture(false);
}

/// The approval card on screen.
struct Pending {
    id: String,
    /// Open the mic once the spoken prompt finishes.
    listen: bool,
    prompt_started: bool,
}

pub struct Console {
    client: EspWebSocketClient<'static>,
    inbox: Receiver<String>,
    outbox: Receiver<Outgoing>,
    mic: Arc<Mic>,
    echo: Arc<AtomicBool>,
    settings: Option<Settings>,
    wake: bool,
    volume: u8,
    phase: Phase,
    phase_since: Instant,
    /// Follows a hands-free utterance; `None` while the button drives it.
    endpoint: Option<Endpointer>,
    baseline: NoiseBaseline,
    /// Room noise when the current utterance started.
    utt_noise: Option<Noise>,
    turn_active: bool,
    approval: Option<Pending>,
    online: bool,
    last_bridge: Option<Instant>,
    last_ping: Option<Instant>,
    last_heap_log: Instant,
    /// When the relay socket went down; `None` while it is up.
    relay_down_since: Option<Instant>,
    await_release: bool,
}

impl Console {
    pub fn new(
        client: EspWebSocketClient<'static>,
        inbox: Receiver<String>,
        outbox: Receiver<Outgoing>,
        mic: Arc<Mic>,
        settings: Option<Settings>,
        wake: bool,
    ) -> Self {
        let now = Instant::now();
        let volume = settings.as_ref().and_then(Settings::volume).unwrap_or(DEFAULT_VOLUME);
        ffi::set_speaker_volume(volume);
        ffi::show_volume(volume);
        Self {
            client,
            inbox,
            outbox,
            mic,
            echo: Arc::new(AtomicBool::new(false)),
            settings,
            wake,
            volume,
            phase: Phase::Idle,
            phase_since: now,
            endpoint: None,
            baseline: NoiseBaseline::default(),
            utt_noise: None,
            turn_active: false,
            approval: None,
            online: false,
            last_bridge: None,
            last_ping: None,
            last_heap_log: now,
            relay_down_since: Some(now),
            await_release: false,
        }
    }

    /// Runs the session until the relay event channel closes.
    pub fn run(mut self) -> Result<()> {
        self.show_phase();
        loop {
            self.drain_levels();
            self.poll_input();
            match self.inbox.recv_timeout(Duration::from_millis(15)) {
                Ok(msg) => self.on_message(&msg),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => bail!("relay event channel closed"),
            }
            self.flush_outbox();
            self.tick();
        }
    }

    /// Feeds mic level frames to the noise baseline and any hands-free utterance.
    fn drain_levels(&mut self) {
        while let Some(frame) = ffi::level_frame() {
            let quiet = !matches!(self.phase, Phase::Listening | Phase::Speaking) && !ffi::play_active();
            self.baseline.push(frame.db, quiet);
            if let Some(end) = self.endpoint.as_mut().and_then(|ep| ep.feed(frame)) {
                self.finish_voice(end);
            }
        }
    }

    fn poll_input(&mut self) {
        self.poll_ptt();
        let woke = self.wake && ffi::wake_heard();
        if woke && self.phase != Phase::Listening {
            log::info!("wake word");
            self.start_voice();
        }
        if let Some(allow) = ffi::approval_choice() {
            self.decide(allow);
        }
        if let Some((level, released)) = ffi::volume_poll() {
            ffi::set_speaker_volume(level);
            self.volume = level;
            if released {
                self.store_volume();
                self.report_volume();
                if matches!(self.phase, Phase::Idle | Phase::Thinking) {
                    play_tone(120);
                }
            }
        }
    }

    fn poll_ptt(&mut self) {
        let down = ffi::ptt_pressed();
        if !down {
            self.await_release = false;
        }
        if self.phase != Phase::Listening {
            if down && !self.await_release {
                self.start_talk();
            }
        } else if self.endpoint.is_none() && !down {
            self.end_talk(false, "released", None);
        } else if self.endpoint.is_none() && self.phase_since.elapsed() >= MAX_TALK {
            self.await_release = true;
            self.end_talk(false, "too_long", None);
        }
    }

    /// Opens the mic; false while the bridge is offline.
    fn start_talk(&mut self) -> bool {
        self.await_release = true;
        if !self.online {
            ffi::set_status("Bridge offline", color::ERROR);
            return false;
        }
        ffi::play_stop();
        if self.approval.is_none() {
            ffi::set_transcript("");
            ffi::set_reply("");
        }
        self.utt_noise = self.baseline.noise();
        self.mic.start();
        self.set_phase(Phase::Listening);
        true
    }

    /// Opens the mic hands-free, gated against the room noise.
    fn start_voice(&mut self) {
        if !self.start_talk() {
            return;
        }
        let gate = self.utt_noise.map(|n| n.gate());
        match (self.utt_noise, gate) {
            (Some(n), Some(g)) => {
                log::info!("listening: room {:.1} dB (p90 {:.1}), gate {g:.1} dB", n.median_db, n.p90_db)
            }
            _ => log::info!("listening: no noise baseline yet, VAD only"),
        }
        self.endpoint = Some(Endpointer::new(gate));
    }

    fn finish_voice(&mut self, end: End) {
        let Some(ep) = self.endpoint.take() else { return };
        log::info!("utterance {}: {} ms, {} ms of speech", end.as_str(), ep.elapsed_ms(), ep.speech_ms());
        self.end_talk(end == End::NoSpeech, end.as_str(), Some(&ep));
    }

    /// Ends the utterance; the bridge drops a discarded one and answers with the current state.
    fn end_talk(&mut self, discard: bool, end: &str, ep: Option<&Endpointer>) {
        let mut frame = json!({ "cmd": "utt_end", "end": end });
        if discard {
            frame["discard"] = true.into();
        }
        if let Some(n) = self.utt_noise {
            frame["noise_db"] = tenths(n.median_db).into();
        }
        if let Some(g) = ep.and_then(Endpointer::gate) {
            frame["gate_db"] = tenths(g).into();
        }
        if let Some(s) = ep.and_then(Endpointer::speech_db) {
            frame["speech_db"] = tenths(s).into();
        }
        self.mic.close(frame.to_string());
        self.turn_active = true;
        self.set_phase(Phase::Thinking);
    }

    /// Mic capture runs only while listening.
    fn set_phase(&mut self, phase: Phase) {
        if phase != Phase::Listening {
            self.mic.stop();
            self.endpoint = None;
        }
        self.phase = phase;
        self.phase_since = Instant::now();
        self.show_phase();
    }

    fn show_phase(&self) {
        match self.phase {
            Phase::Idle if !self.online => ffi::set_status("Waiting for bridge...", color::MUTED),
            Phase::Idle if self.wake => ffi::set_status("Say \"Jarvis\" or hold to talk", color::SUCCESS),
            Phase::Idle => ffi::set_status("Hold to talk", color::SUCCESS),
            Phase::Listening => ffi::set_status("Listening...", color::ACCENT),
            Phase::Thinking => ffi::set_status("Thinking...", color::TERTIARY),
            Phase::Speaking => ffi::set_status("Speaking...", color::ACCENT),
            Phase::Approval => ffi::set_status("Needs your OK", color::WARN),
        }
    }

    /// Returns to idle with `status` shown in place of the idle prompt.
    fn fail(&mut self, status: &str) {
        self.turn_active = false;
        self.set_phase(Phase::Idle);
        ffi::set_status(status, color::ERROR);
    }

    fn after_turn_phase(&self) -> Phase {
        if self.approval.is_some() {
            Phase::Approval
        } else if self.turn_active {
            Phase::Thinking
        } else {
            Phase::Idle
        }
    }

    fn on_message(&mut self, msg: &str) {
        for part in msg.split('\n').map(str::trim).filter(|s| !s.is_empty()) {
            if part.starts_with('{') {
                self.on_json(part);
            } else {
                self.on_notice(part);
            }
        }
    }

    fn on_notice(&mut self, notice: &str) {
        match notice {
            WS_CONNECTED | "MASTER_CONNECTED" => {
                if notice == WS_CONNECTED {
                    self.relay_down_since = None;
                }
                self.hello();
                self.ping();
                self.report_volume();
            }
            WS_DISCONNECTED | "MASTER_DISCONNECTED" => {
                if notice == WS_DISCONNECTED {
                    self.relay_down_since.get_or_insert_with(Instant::now);
                }
                self.set_online(false);
            }
            other => log::info!("relay: {other}"),
        }
    }

    /// Tells the bridge why this boot started and how long it has run.
    fn hello(&mut self) {
        let uptime_s = unsafe { esp_idf_svc::sys::esp_timer_get_time() } / 1_000_000;
        let frame = json!({
            "cmd": "hello",
            "firmware": env!("CARGO_PKG_VERSION"),
            "reset": format!("{:?}", ResetReason::get()),
            "uptime_s": uptime_s,
        })
        .to_string();
        self.send_text(&frame);
    }

    fn on_json(&mut self, line: &str) {
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("bad relay json ({e}): {line}");
                return;
            }
        };
        let text = v["text"].as_str().unwrap_or("");
        match v["cmd"].as_str().unwrap_or("") {
            "pong" | "tts_end" => self.mark_bridge(),
            "state" => {
                self.mark_bridge();
                if v["state"].as_str() == Some("idle") {
                    self.turn_active = false;
                    if self.phase == Phase::Thinking {
                        self.set_phase(Phase::Idle);
                    }
                }
            }
            "transcript" => {
                self.mark_bridge();
                ffi::set_transcript(&format!("You: {text}"));
            }
            "tts_start" => {
                self.mark_bridge();
                match self.phase {
                    Phase::Listening => ffi::play_stop(),
                    Phase::Approval => {
                        if let Some(p) = self.approval.as_mut() {
                            p.prompt_started = true;
                        }
                    }
                    _ => {
                        ffi::set_reply(text);
                        self.set_phase(Phase::Speaking);
                    }
                }
            }
            "reply" => {
                self.mark_bridge();
                if self.phase != Phase::Listening {
                    ffi::set_reply(text);
                }
            }
            "approval" => {
                self.mark_bridge();
                self.show_approval(&v);
            }
            "approval_done" => {
                self.mark_bridge();
                if self.approval.as_ref().is_some_and(|p| Some(p.id.as_str()) == v["id"].as_str()) {
                    self.clear_approval();
                }
            }
            "volume" => {
                self.mark_bridge();
                if let Some(level) = v["level"].as_u64() {
                    self.volume = level.min(100) as u8;
                    ffi::set_speaker_volume(self.volume);
                    ffi::show_volume(self.volume);
                    self.store_volume();
                }
            }
            "error" => {
                self.mark_bridge();
                let err = v["error"].as_str().unwrap_or("unknown error");
                log::warn!("bridge error: {err}");
                if self.phase != Phase::Listening {
                    self.clear_approval();
                    ffi::set_reply(err);
                    self.fail("Something went wrong");
                }
            }
            cmd => self.on_command(cmd),
        }
    }

    fn show_approval(&mut self, v: &Value) {
        let Some(id) = v["id"].as_str() else { return };
        let question = v["kind"].as_str() == Some("question");
        let voice = v["voice"].as_bool().unwrap_or(false);
        let hint = match (question, voice) {
            (true, _) => "Answer out loud, or tap Skip",
            (false, true) => "Say yes or no, or tap",
            (false, false) => "Tap Approve to allow",
        };
        ffi::show_approval(v["text"].as_str().unwrap_or(""), hint, !question);
        self.approval = Some(Pending { id: id.to_string(), listen: question || voice, prompt_started: false });
        self.turn_active = true;
        if self.phase != Phase::Listening {
            self.set_phase(Phase::Approval);
        }
    }

    fn clear_approval(&mut self) {
        if self.approval.take().is_some() {
            ffi::hide_approval();
        }
        if self.phase == Phase::Approval {
            self.set_phase(self.after_turn_phase());
        }
    }

    /// A tap on the approval card.
    fn decide(&mut self, allow: bool) {
        let Some(p) = self.approval.as_ref() else { return };
        let frame = json!({ "cmd": "decide", "id": p.id, "allow": allow }).to_string();
        self.send_text(&frame);
        if self.phase == Phase::Listening {
            self.end_talk(true, "tap", None);
        }
        self.clear_approval();
    }

    fn store_volume(&self) {
        if let Some(s) = self.settings.as_ref() {
            s.set_volume(self.volume);
        }
    }

    fn report_volume(&mut self) {
        let frame = json!({ "cmd": "volume", "level": self.volume }).to_string();
        self.send_text(&frame);
    }

    /// Bench test commands from a relay master.
    fn on_command(&mut self, cmd: &str) {
        match cmd {
            "ping" => self.send_text(r#"{"cmd":"pong"}"#),
            "beep" => {
                play_tone(400);
                self.send_text(r#"{"ok":true,"result":{"beeped":true}}"#);
            }
            "echo_on" => {
                if !self.echo.swap(true, Ordering::Relaxed) {
                    let run = self.echo.clone();
                    let _ = std::thread::Builder::new().stack_size(8192).spawn(move || echo_loop(run));
                }
                self.send_text(r#"{"ok":true,"result":{"echo":true}}"#);
            }
            "echo_off" => {
                self.echo.store(false, Ordering::Relaxed);
                self.send_text(r#"{"ok":true,"result":{"echo":false}}"#);
            }
            "reboot" => {
                log::warn!("restart requested over the relay");
                reset::restart();
            }
            "status" => {
                let reply = json!({
                    "ok": true,
                    "result": {
                        "role": "voice-console",
                        "firmware": env!("CARGO_PKG_VERSION"),
                        "phase": format!("{:?}", self.phase),
                        "online": self.online,
                        "wake": self.wake,
                        "volume": self.volume,
                        "noise_db": self.baseline.noise().map(|n| tenths(n.median_db)),
                    }
                })
                .to_string();
                self.send_text(&reply);
            }
            _ => {}
        }
    }

    fn tick(&mut self) {
        match self.phase {
            Phase::Speaking if !ffi::play_active() => self.set_phase(self.after_turn_phase()),
            Phase::Listening => {
                if self.endpoint.is_some() && self.phase_since.elapsed() >= MAX_VOICE_TALK {
                    self.finish_voice(End::TooLong);
                }
            }
            Phase::Approval => {
                let ready = self.approval.as_ref().is_some_and(|p| p.listen && p.prompt_started);
                if ready && !ffi::play_active() {
                    if let Some(p) = self.approval.as_mut() {
                        p.listen = false;
                    }
                    self.start_voice();
                }
            }
            Phase::Thinking => {
                let quiet_since = self.last_bridge.map_or(self.phase_since, |t| t.max(self.phase_since));
                if quiet_since.elapsed() >= REPLY_WAIT {
                    self.fail("No reply");
                }
            }
            Phase::Idle | Phase::Speaking => {}
        }
        if self.last_ping.map_or(true, |t| t.elapsed() >= PING_EVERY) {
            self.ping();
        }
        if self.online && self.last_bridge.map_or(true, |t| t.elapsed() >= BRIDGE_STALE) {
            self.set_online(false);
        }
        if self.relay_down_since.is_some_and(|t| t.elapsed() >= RELAY_DOWN_RESTART) {
            log::error!("relay unreachable for {RELAY_DOWN_RESTART:?}; restarting");
            reset::restart();
        }
        if self.last_heap_log.elapsed() >= HEAP_LOG_EVERY {
            self.last_heap_log = Instant::now();
            log_heap();
            if let Some(n) = self.baseline.noise() {
                log::info!("room noise {:.1} dB (p90 {:.1}), gate {:.1} dB", n.median_db, n.p90_db, n.gate());
            }
        }
    }

    fn mark_bridge(&mut self) {
        self.last_bridge = Some(Instant::now());
        self.set_online(true);
    }

    fn set_online(&mut self, online: bool) {
        if self.online == online {
            return;
        }
        self.online = online;
        log::info!("bridge {}", if online { "online" } else { "offline" });
        if self.phase == Phase::Idle {
            self.show_phase();
        }
    }

    fn ping(&mut self) {
        self.last_ping = Some(Instant::now());
        self.send_text(r#"{"cmd":"ping"}"#);
    }

    fn flush_outbox(&mut self) {
        while let Ok(frame) = self.outbox.try_recv() {
            match frame {
                Outgoing::Text(text) => self.send_text(&text),
                Outgoing::Audio(pcm) => self.send(FrameType::Binary(false), &pcm),
            }
        }
    }

    fn send_text(&mut self, text: &str) {
        self.send(FrameType::Text(false), text.as_bytes());
    }

    fn send(&mut self, kind: FrameType, data: &[u8]) {
        if !self.client.is_connected() {
            return;
        }
        if let Err(e) = self.client.send(kind, data) {
            log::warn!("relay send failed: {e}");
        }
    }
}
