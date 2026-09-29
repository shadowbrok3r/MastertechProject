//! Relay-room audio transport. The bench board joins the room as `role=client`
//! and the bridge as `role=master`; `websocket_server2` forwards frames verbatim.
//!
//! Wire protocol on the room:
//! - board -> bridge: `{"cmd":"utt_start"}` (Text), raw PCM16LE 16 kHz mono
//!   (Binary), `{"cmd":"utt_end"}` (Text); `{"cmd":"ping"}` is answered with
//!   `{"cmd":"pong"}`.
//! - bridge -> board: `{"cmd":"state","state":"thinking"}` (repeated while the turn
//!   runs), `{"cmd":"transcript","text":"..."}`, then `{"cmd":"tts_start","text":"..."}`
//!   (Text), PCM16LE 16 kHz mono (Binary), `{"cmd":"tts_end"}` (Text).
//!   `{"cmd":"state","state":"idle"}` when nothing usable was heard;
//!   `{"cmd":"error","error":"..."}` on failure.
//!
//! A new `utt_start` supersedes any turn still running; that turn's reply is dropped.

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::audio::{i16_from_le_bytes, i16_to_le_bytes, read_wav_mono, resample, write_wav_mono, BENCH_RATE};
use crate::pipeline::{stream_reply, synthesize, transcribe, voice_prompt, Identity};

/// Samples per outbound audio frame (~64 ms at 16 kHz).
const FRAME_SAMPLES: usize = 1024;
/// Utterances shorter than 0.4 s are accidental taps.
const MIN_UTT_SAMPLES: usize = 6_400;
/// Longest text sent for the board's display.
const DISPLAY_MAX: usize = 1_200;
const KEEPALIVE: Duration = Duration::from_secs(15);
/// Longest gap between inbound frames (the relay pings every 10 s) before the socket counts as dead.
const RELAY_SILENCE: Duration = Duration::from_secs(35);

fn master_base() -> &'static str {
    if cfg!(debug_assertions) { database::WS_MASTER_URL_LOCAL } else { database::WS_MASTER_URL }
}

fn client_base() -> &'static str {
    if cfg!(debug_assertions) { database::WS_CLIENT_URL_LOCAL } else { database::WS_CLIENT_URL }
}

/// A relay `Text` frame that is a server notice rather than an app JSON message.
fn is_relay_notice(text: &str) -> bool {
    !text.trim_start().starts_with('{')
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn state_frame(state: &str) -> Message {
    Message::Text(json!({ "cmd": "state", "state": state }).to_string().into())
}

fn text_frame(cmd: &str, text: &str) -> Message {
    Message::Text(json!({ "cmd": cmd, "text": text }).to_string().into())
}

fn error_frame(msg: &str) -> Message {
    Message::Text(json!({ "cmd": "error", "error": msg }).to_string().into())
}

/// Peak and RMS amplitude of `pcm`.
fn level(pcm: &[i16]) -> (i32, f64) {
    let peak = pcm.iter().map(|s| i32::from(*s).abs()).max().unwrap_or(0);
    let energy: f64 = pcm.iter().map(|s| f64::from(*s).powi(2)).sum();
    (peak, (energy / pcm.len().max(1) as f64).sqrt())
}

/// Whisper transcribes non-speech as a bracketed tag such as `[BLANK_AUDIO]` or `(silence)`.
fn heard_nothing(transcript: &str) -> bool {
    let t = transcript.trim();
    t.is_empty() || (t.starts_with(['[', '(']) && t.ends_with([']', ')']))
}

/// ASCII text (plus the `°` and `•` the board's font carries), capped for its display.
fn display_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len().min(DISPLAY_MAX + 3));
    for c in s.chars() {
        if out.len() >= DISPLAY_MAX {
            out.push_str("...");
            break;
        }
        match c {
            '\u{2018}' | '\u{2019}' => out.push('\''),
            '\u{201C}' | '\u{201D}' => out.push('"'),
            '\u{2013}' | '\u{2014}' => out.push('-'),
            '\u{2026}' => out.push_str("..."),
            '*' | '`' => {}
            c if c.is_ascii() || c == '°' || c == '•' => out.push(c),
            _ => {}
        }
    }
    out
}

/// One utterance's turn and the channel back to the board.
struct Turn {
    id: u64,
    latest: Arc<AtomicU64>,
    tx: mpsc::Sender<Message>,
}

impl Turn {
    fn is_current(&self) -> bool {
        self.latest.load(Ordering::Acquire) == self.id
    }

    /// Sends `msg` while this is still the newest turn; `false` once superseded or disconnected.
    async fn send(&self, msg: Message) -> bool {
        self.is_current() && self.tx.send(msg).await.is_ok()
    }
}

/// Joins `room` as master and serves utterances until interrupted, reconnecting on drop.
pub async fn run_master(room: &str, id: Identity, timeout_secs: u64) -> Result<()> {
    let url = database::websocket_url_with_room(master_base(), room, "master");
    let id = Arc::new(id);
    log::info!("relay master joining room {room} at {}", master_base());
    loop {
        if let Err(e) = clear_master_slot(room).await {
            log::warn!("could not clear the master slot of {room}: {e}");
        }
        if let Err(e) = serve_once(&url, Arc::clone(&id), timeout_secs).await {
            log::warn!("relay session ended: {e}");
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
        log::info!("reconnecting to room {room}");
    }
}

/// Evicts whatever socket holds `room`'s master slot, issuing `/remove` from a separate control room.
async fn clear_master_slot(room: &str) -> Result<()> {
    let ctl = database::websocket_url_with_room(master_base(), &format!("{room}-ctl"), "master");
    let (mut ws, _resp) = tokio_tungstenite::connect_async(&ctl).await?;
    ws.send(Message::Text(format!("/remove {room} master").into())).await?;
    let reply = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(msg) = ws.next().await {
            if let Message::Text(t) = msg? {
                if t.contains(room) {
                    return Ok(t.to_string());
                }
            }
        }
        anyhow::bail!("control socket closed before the relay replied")
    })
    .await??;
    log::info!("relay: {reply}");
    let _ = ws.close(None).await;
    Ok(())
}

/// Serves one relay connection until it drops or goes silent.
async fn serve_once(url: &str, id: Arc<Identity>, timeout_secs: u64) -> Result<()> {
    let (ws, _resp) = tokio_tungstenite::connect_async(url).await?;
    let (sink, mut stream) = ws.split();
    let (tx, mut rx) = mpsc::channel::<Message>(128);

    let writer = tokio::spawn(async move {
        let mut sink = sink;
        while let Some(m) = rx.recv().await {
            if sink.send(m).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });

    let latest = Arc::new(AtomicU64::new(0));
    let mut capture: Option<Vec<i16>> = None;
    let mut watchdog = tokio::time::interval(Duration::from_secs(5));
    let mut last_inbound = Instant::now();
    loop {
        let msg = tokio::select! {
            msg = stream.next() => match msg {
                Some(msg) => msg?,
                None => break,
            },
            _ = watchdog.tick() => {
                if last_inbound.elapsed() >= RELAY_SILENCE {
                    anyhow::bail!("relay silent for {}s", last_inbound.elapsed().as_secs());
                }
                continue;
            }
        };
        last_inbound = Instant::now();
        match msg {
            Message::Text(t) => {
                let t = t.as_str();
                if t.trim() == "NO_AGENT_IN_ROOM" {
                    continue;
                }
                if is_relay_notice(t) {
                    log::info!("relay: {t}");
                    if t.contains("CLIENT_DISCONNECTED") {
                        capture = None;
                    }
                    continue;
                }
                let v: Value = match serde_json::from_str(t) {
                    Ok(v) => v,
                    Err(e) => {
                        log::warn!("bad control json: {e}");
                        continue;
                    }
                };
                match v["cmd"].as_str() {
                    Some("utt_start") => {
                        latest.fetch_add(1, Ordering::AcqRel);
                        capture = Some(Vec::new());
                        log::info!("utt_start");
                    }
                    Some("utt_end") => {
                        let pcm = capture.take().unwrap_or_default();
                        let (peak, rms) = level(&pcm);
                        log::info!(
                            "utt_end: {} samples ({:.1}s), peak {peak}, rms {rms:.0}",
                            pcm.len(),
                            pcm.len() as f64 / f64::from(BENCH_RATE)
                        );
                        if pcm.len() < MIN_UTT_SAMPLES {
                            let _ = tx.send(state_frame("idle")).await;
                            continue;
                        }
                        let _ = tx.send(state_frame("thinking")).await;
                        let turn = Turn {
                            id: latest.load(Ordering::Acquire),
                            latest: Arc::clone(&latest),
                            tx: tx.clone(),
                        };
                        spawn_turn(Arc::clone(&id), turn, pcm, timeout_secs);
                    }
                    Some("ping") => {
                        let _ = tx.send(Message::Text(r#"{"cmd":"pong"}"#.into())).await;
                    }
                    Some("pong") => {}
                    other => log::warn!("unknown cmd {other:?}"),
                }
            }
            Message::Binary(b) => {
                if let Some(buf) = capture.as_mut() {
                    buf.extend(i16_from_le_bytes(&b));
                }
            }
            Message::Ping(p) => {
                let _ = tx.send(Message::Pong(p)).await;
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    drop(tx);
    let _ = writer.await;
    Ok(())
}

/// Runs one turn off the read loop, then removes its temp files.
fn spawn_turn(id: Arc<Identity>, turn: Turn, pcm: Vec<i16>, timeout_secs: u64) {
    tokio::spawn(async move {
        let stamp = now_millis();
        let in_wav = format!("/tmp/vb_utt_{stamp}.wav");
        let out_wav = format!("/tmp/vb_reply_{stamp}.wav");
        if let Err(e) = relay_turn(&id, &turn, &pcm, &in_wav, &out_wav, timeout_secs).await {
            log::error!("turn {} failed: {e}", turn.id);
            turn.send(error_frame(&e.to_string())).await;
        }
        for path in [format!("{in_wav}.stt.txt"), in_wav, out_wav] {
            let _ = std::fs::remove_file(path);
        }
    });
}

async fn relay_turn(
    id: &Identity,
    turn: &Turn,
    pcm: &[i16],
    in_wav: &str,
    out_wav: &str,
    timeout_secs: u64,
) -> Result<()> {
    write_wav_mono(in_wav, pcm, BENCH_RATE)?;
    let wav = in_wav.to_string();
    let transcript = tokio::task::spawn_blocking(move || transcribe(&wav)).await??;
    log::info!("turn {} STT: {transcript}", turn.id);
    if heard_nothing(&transcript) {
        turn.send(text_frame("transcript", "(didn't catch that)")).await;
        turn.send(state_frame("idle")).await;
        return Ok(());
    }
    if !turn.send(text_frame("transcript", &display_text(&transcript))).await {
        log::info!("turn {} superseded", turn.id);
        return Ok(());
    }

    let (cs, prompt) = (id.connection_string(), voice_prompt(&transcript));
    let reply = with_keepalive(turn, stream_reply(&cs, id.requested_by(), &prompt, timeout_secs)).await?;
    log::info!("turn {} ASSISTANT: {reply}", turn.id);
    if !turn.is_current() {
        log::info!("turn {} superseded", turn.id);
        return Ok(());
    }

    let (text, out) = (reply.clone(), out_wav.to_string());
    tokio::task::spawn_blocking(move || synthesize(&text, &out)).await??;
    let (rate, samples) = read_wav_mono(out_wav)?;
    let pcm16 = resample(&samples, rate, BENCH_RATE);
    if !turn.send(text_frame("tts_start", &display_text(&reply))).await {
        return Ok(());
    }
    for chunk in pcm16.chunks(FRAME_SAMPLES) {
        if turn.tx.send(Message::Binary(i16_to_le_bytes(chunk).into())).await.is_err() {
            return Ok(());
        }
    }
    let _ = turn.tx.send(Message::Text(r#"{"cmd":"tts_end"}"#.into())).await;
    log::info!("turn {} sent reply: {} samples", turn.id, pcm16.len());
    Ok(())
}

/// Awaits `fut`, sending a `thinking` state every [`KEEPALIVE`] while the turn is current.
async fn with_keepalive<T>(turn: &Turn, fut: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::pin!(fut);
    loop {
        tokio::select! {
            r = &mut fut => return r,
            _ = tokio::time::sleep(KEEPALIVE) => {
                turn.send(state_frame("thinking")).await;
            }
        }
    }
}

/// Test client: joins `room` as the board would, streams `wav` as one utterance,
/// then writes the spoken reply to `VB_SIM_OUT` (default /tmp/vb_sim_reply.wav).
pub async fn run_sim_client(room: &str, wav: &str) -> Result<()> {
    let url = database::websocket_url_with_room(client_base(), room, "client");
    log::info!("sim-client joining room {room} at {}", client_base());
    let (ws, _resp) = tokio_tungstenite::connect_async(&url).await?;
    let (mut sink, mut stream) = ws.split();

    let (rate, samples) = read_wav_mono(wav)?;
    let pcm = resample(&samples, rate, BENCH_RATE);
    sink.send(Message::Text(r#"{"cmd":"utt_start"}"#.into())).await?;
    for chunk in pcm.chunks(FRAME_SAMPLES) {
        sink.send(Message::Binary(i16_to_le_bytes(chunk).into())).await?;
    }
    sink.send(Message::Text(r#"{"cmd":"utt_end"}"#.into())).await?;
    log::info!("sim-client sent {} samples, awaiting reply", pcm.len());

    let out = crate::pipeline::env_or("VB_SIM_OUT", "/tmp/vb_sim_reply.wav");
    let mut reply: Vec<i16> = Vec::new();
    let mut receiving = false;
    while let Some(msg) = stream.next().await {
        match msg? {
            Message::Text(t) => {
                let t = t.as_str();
                if is_relay_notice(t) {
                    log::info!("relay: {t}");
                    continue;
                }
                let v: Value = serde_json::from_str(t)?;
                let text = v["text"].as_str().unwrap_or("");
                match v["cmd"].as_str() {
                    Some("state") if v["state"].as_str() == Some("idle") => {
                        log::info!("bridge heard nothing usable");
                        return Ok(());
                    }
                    Some("state") => log::info!("state: {}", v["state"].as_str().unwrap_or("")),
                    Some("transcript") => println!("TRANSCRIPT: {text}"),
                    Some("tts_start") => {
                        receiving = true;
                        println!("REPLY: {text}");
                    }
                    Some("tts_end") => break,
                    Some("error") => anyhow::bail!("bridge error: {}", v["error"].as_str().unwrap_or("")),
                    _ => {}
                }
            }
            Message::Binary(b) if receiving => reply.extend(i16_from_le_bytes(&b)),
            Message::Ping(p) => sink.send(Message::Pong(p)).await?,
            Message::Close(_) => break,
            _ => {}
        }
    }
    write_wav_mono(&out, &reply, BENCH_RATE)?;
    log::info!("sim-client wrote {} samples -> {out}", reply.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_text_folds_typography_to_ascii() {
        assert_eq!(
            display_text("DDR5 \u{2014} it\u{2019}s \u{201C}faster\u{201D}\u{2026}"),
            "DDR5 - it's \"faster\"..."
        );
    }

    #[test]
    fn display_text_keeps_degree_and_drops_other_non_ascii() {
        assert_eq!(display_text("95°C • caf\u{e9} **bold**"), "95°C • caf bold");
    }

    #[test]
    fn display_text_caps_length() {
        let long = "a".repeat(DISPLAY_MAX * 2);
        let out = display_text(&long);
        assert_eq!(out.len(), DISPLAY_MAX + 3);
        assert!(out.ends_with("..."));
    }

    #[test]
    fn bracketed_whisper_tags_count_as_nothing_heard() {
        assert!(heard_nothing("[BLANK_AUDIO]"));
        assert!(heard_nothing(" (silence) "));
        assert!(heard_nothing(""));
        assert!(!heard_nothing("(laughs) what is thermal throttling?"));
        assert!(!heard_nothing("what is DDR5"));
    }

    #[test]
    fn level_reports_peak_and_rms() {
        let (peak, rms) = level(&[3, -4, 0, 0]);
        assert_eq!(peak, 4);
        assert!((rms - 2.5).abs() < 1e-9);
    }

    #[test]
    fn level_of_empty_is_zero() {
        assert_eq!(level(&[]), (0, 0.0));
    }
}
