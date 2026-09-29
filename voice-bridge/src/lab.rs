//! Voice lab: a LAN page for auditioning Piper voices, playing them on the board and choosing the board's voice.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use axum::extract::State;
use axum::http::{header, HeaderName, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::audio::{parse_wav_mono, resample, BENCH_RATE};
use crate::relay::BoardLink;
use crate::voices::{self, ActiveVoice, VoiceSettings};

const PAGE: &str = include_str!("lab.html");
const MAX_TEXT_CHARS: usize = 600;

#[derive(Clone)]
struct Lab {
    voice: Arc<ActiveVoice>,
    board: Arc<BoardLink>,
}

#[derive(Deserialize)]
struct SpeakRequest {
    text: String,
    #[serde(flatten)]
    settings: VoiceSettings,
}

type Reply<T> = Result<T, (StatusCode, String)>;

fn rejected(e: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, e.to_string())
}

fn failed(e: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

pub async fn serve(addr: SocketAddr, voice: Arc<ActiveVoice>, board: Arc<BoardLink>) -> Result<()> {
    let app = Router::new()
        .route("/", get(|| async { Html(PAGE) }))
        .route("/api/voices", get(list))
        .route("/api/synth", post(synth))
        .route("/api/play", post(play))
        .route("/api/active", post(set_active))
        .route("/api/volume", post(set_volume))
        .with_state(Lab { voice, board });
    let listener = tokio::net::TcpListener::bind(addr).await?;
    log::info!("voice lab listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn list(State(lab): State<Lab>) -> Json<Value> {
    Json(json!({
        "voices": voices::catalog(),
        "active": lab.voice.get(),
        "board_online": lab.board.online(),
        "volume": lab.board.volume(),
    }))
}

#[derive(Deserialize)]
struct VolumeRequest {
    level: u8,
}

async fn set_volume(State(lab): State<Lab>, Json(req): Json<VolumeRequest>) -> Reply<Json<Value>> {
    if !lab.board.online() {
        return Err((StatusCode::CONFLICT, "the board is offline".into()));
    }
    let level = req.level.min(100);
    lab.board.set_volume(level).await.map_err(failed)?;
    Ok(Json(json!({ "volume": level })))
}

/// The trimmed text, its WAV, and the synthesis time in ms.
async fn render(req: SpeakRequest) -> Reply<(String, Vec<u8>, u128)> {
    let text = req.text.trim().to_string();
    if text.is_empty() || text.chars().count() > MAX_TEXT_CHARS {
        return Err(rejected(format!("text must be 1 to {MAX_TEXT_CHARS} characters")));
    }
    let settings = req.settings.validated(&voices::catalog()).map_err(rejected)?;
    let started = Instant::now();
    let spoken = text.clone();
    let wav = tokio::task::spawn_blocking(move || voices::synthesize(&spoken, &settings))
        .await
        .map_err(failed)?
        .map_err(failed)?;
    Ok((text, wav, started.elapsed().as_millis()))
}

async fn synth(Json(req): Json<SpeakRequest>) -> Reply<Response> {
    let (_, wav, ms) = render(req).await?;
    let headers = [
        (header::CONTENT_TYPE, "audio/wav".to_string()),
        (HeaderName::from_static("x-synth-ms"), ms.to_string()),
    ];
    Ok((headers, wav).into_response())
}

async fn play(State(lab): State<Lab>, Json(req): Json<SpeakRequest>) -> Reply<Json<Value>> {
    if !lab.board.online() {
        return Err((StatusCode::CONFLICT, "the board is offline".into()));
    }
    let (text, wav, ms) = render(req).await?;
    let (rate, samples) = parse_wav_mono(&wav).map_err(failed)?;
    let pcm = resample(&samples, rate, BENCH_RATE);
    lab.board.speak(&text, &pcm).await.map_err(failed)?;
    Ok(Json(json!({ "ms": ms, "seconds": pcm.len() as f32 / BENCH_RATE as f32 })))
}

async fn set_active(State(lab): State<Lab>, Json(settings): Json<VoiceSettings>) -> Reply<Json<VoiceSettings>> {
    let settings = settings.validated(&voices::catalog()).map_err(rejected)?;
    lab.voice.set(settings.clone()).map_err(failed)?;
    log::info!("board voice set to {} (speaker {})", settings.voice, settings.speaker);
    Ok(Json(settings))
}
