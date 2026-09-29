//! Relay-room audio transport. The bench board joins the room as `role=client`
//! and the bridge as `role=master`; `websocket_server2` forwards frames verbatim.
//!
//! Wire protocol on the room:
//! - board -> bridge: `{"cmd":"utt_start"}` (Text), raw PCM16LE 16 kHz mono
//!   (Binary), `{"cmd":"utt_end"}` (Text).
//! - bridge -> board: `{"cmd":"state","state":"thinking"}`, then
//!   `{"cmd":"tts_start","text":"..."}` (Text), PCM16LE 16 kHz mono (Binary),
//!   `{"cmd":"tts_end"}` (Text). On failure `{"cmd":"error","error":"..."}`.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::audio::{i16_from_le_bytes, i16_to_le_bytes, read_wav_mono, resample, write_wav_mono, BENCH_RATE};
use crate::pipeline::{run_turn, Identity};

/// Samples per outbound audio frame (~64 ms at 16 kHz).
const FRAME_SAMPLES: usize = 1024;

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

/// Joins `room` as master and serves utterances until interrupted, reconnecting on drop.
pub async fn run_master(room: &str, id: Identity, timeout_secs: u64) -> Result<()> {
    let url = database::websocket_url_with_room(master_base(), room, "master");
    let id = Arc::new(id);
    log::info!("relay master joining room {room} at {}", master_base());
    loop {
        if let Err(e) = serve_once(&url, Arc::clone(&id), timeout_secs).await {
            log::warn!("relay session ended: {e}");
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
        log::info!("reconnecting to room {room}");
    }
}

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

    let mut capture: Option<Vec<i16>> = None;
    while let Some(msg) = stream.next().await {
        let msg = msg?;
        match msg {
            Message::Text(t) => {
                let t = t.as_str();
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
                        log::info!("utt_start");
                        capture = Some(Vec::new());
                    }
                    Some("utt_end") => {
                        let pcm = capture.take().unwrap_or_default();
                        log::info!("utt_end: {} samples", pcm.len());
                        let _ = tx.send(state_frame("thinking")).await;
                        spawn_turn(Arc::clone(&id), tx.clone(), pcm, timeout_secs);
                    }
                    Some("ping") => {
                        let _ = tx.send(Message::Text(r#"{"cmd":"pong"}"#.into())).await;
                    }
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

fn state_frame(state: &str) -> Message {
    Message::Text(json!({ "cmd": "state", "state": state }).to_string().into())
}

fn error_frame(msg: &str) -> Message {
    Message::Text(json!({ "cmd": "error", "error": msg }).to_string().into())
}

/// Runs one full turn off the read loop, streaming the reply back through `tx`.
fn spawn_turn(id: Arc<Identity>, tx: mpsc::Sender<Message>, pcm: Vec<i16>, timeout_secs: u64) {
    tokio::spawn(async move {
        let stamp = now_millis();
        let in_wav = format!("/tmp/vb_utt_{stamp}.wav");
        let out_wav = format!("/tmp/vb_reply_{stamp}.wav");
        if let Err(e) = write_wav_mono(&in_wav, &pcm, BENCH_RATE) {
            let _ = tx.send(error_frame(&e.to_string())).await;
            return;
        }
        let turn = run_turn(&id, &in_wav, &out_wav, timeout_secs);
        tokio::pin!(turn);
        let result = loop {
            tokio::select! {
                r = &mut turn => break r,
                _ = tokio::time::sleep(Duration::from_secs(15)) => {
                    let _ = tx.send(state_frame("thinking")).await;
                }
            }
        };
        let reply = match result {
            Ok(r) => r,
            Err(e) => {
                log::error!("turn failed: {e}");
                let _ = tx.send(error_frame(&e.to_string())).await;
                return;
            }
        };
        let (rate, samples) = match read_wav_mono(&out_wav) {
            Ok(v) => v,
            Err(e) => {
                let _ = tx.send(error_frame(&e.to_string())).await;
                return;
            }
        };
        let pcm16 = resample(&samples, rate, BENCH_RATE);
        let _ = tx
            .send(Message::Text(json!({ "cmd": "tts_start", "text": reply }).to_string().into()))
            .await;
        for chunk in pcm16.chunks(FRAME_SAMPLES) {
            if tx.send(Message::Binary(i16_to_le_bytes(chunk).into())).await.is_err() {
                return;
            }
        }
        let _ = tx.send(Message::Text(r#"{"cmd":"tts_end"}"#.into())).await;
        log::info!("sent reply: {} samples", pcm16.len());
    });
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
                match v["cmd"].as_str() {
                    Some("state") => log::info!("state: {}", v["state"].as_str().unwrap_or("")),
                    Some("tts_start") => {
                        receiving = true;
                        if let Some(text) = v["text"].as_str() {
                            println!("REPLY: {text}");
                        }
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
