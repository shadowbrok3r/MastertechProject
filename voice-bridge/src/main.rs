//! Server voice pipeline for the bench console: STT (whisper.cpp) -> the tech's
//! Mastertech assistant (`agent_chat`, the Ctrl+K path) -> TTS (Piper).
//!
//! Modes:
//! - `voice-bridge <input.wav> [tech_email]` — one WAV in, a spoken reply WAV out.
//! - `voice-bridge --relay [room]` — join the relay room as master and serve the
//!   board's spoken utterances live (default room `VOICE-DEV`, or `VB_ROOM`), plus
//!   the voice lab page on `VB_LAB_ADDR` (default `0.0.0.0:8765`).
//! - `voice-bridge --sim-client <room> <input.wav>` — stand in for the board to
//!   test the relay path end to end without hardware.
//!
//! Identity: with `VB_TECH_EMAIL` (or the file-mode arg) and `VB_TECH_PASSWORD`,
//! signs in as that tech and reuses one warm `general:voice:<email>` thread;
//! otherwise runs as guest with a fresh thread per utterance.

mod audio;
mod lab;
mod pipeline;
mod relay;
mod voices;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};

use pipeline::{env_or, run_turn, Identity};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let mut args = std::env::args().skip(1);
    let first = args
        .next()
        .context("usage: voice-bridge <input.wav> [tech_email] | --relay [room] | --sim-client <room> <input.wav>")?;
    let timeout_secs: u64 = env_or("VB_TIMEOUT", "600").parse().unwrap_or(600);

    match first.as_str() {
        "--relay" => {
            let room = args
                .next()
                .or_else(|| std::env::var("VB_ROOM").ok())
                .unwrap_or_else(|| "VOICE-DEV".to_string());
            let ctx = relay::TurnContext {
                id: Identity::init(None).await?,
                voice: Arc::new(voices::ActiveVoice::load()),
                timeout_secs,
            };
            let board = Arc::new(relay::BoardLink::default());
            let addr: SocketAddr = env_or("VB_LAB_ADDR", "0.0.0.0:8765").parse().context("VB_LAB_ADDR")?;
            let (voice, link) = (Arc::clone(&ctx.voice), Arc::clone(&board));
            tokio::spawn(async move {
                if let Err(e) = lab::serve(addr, voice, link).await {
                    log::error!("voice lab stopped: {e}");
                }
            });
            relay::run_master(&room, ctx, board).await?;
        }
        "--sim-client" => {
            let room = args.next().context("usage: --sim-client <room> <input.wav>")?;
            let wav = args.next().context("usage: --sim-client <room> <input.wav>")?;
            relay::run_sim_client(&room, &wav).await?;
        }
        _ => {
            let in_wav = first;
            let tech_email = args.next();
            let out_wav = env_or("VB_OUT", "/tmp/vb_reply.wav");
            let id = Identity::init(tech_email).await?;
            let reply = run_turn(&id, &in_wav, &out_wav, timeout_secs).await?;
            println!("{reply}");
        }
    }
    Ok(())
}
