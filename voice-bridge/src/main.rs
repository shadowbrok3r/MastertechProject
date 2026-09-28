//! Server voice pipeline: STT (whisper.cpp) -> the tech's Mastertech assistant
//! (`database::agent_chat::ask`, the Ctrl+K path) -> TTS (Piper). Stand-in CLI:
//! `voice-bridge <input.wav> [tech_email]`; a WAV in, a spoken reply WAV out.
//!
//! Identity: with `VB_TECH_EMAIL` (or the CLI arg) and `VB_TECH_PASSWORD` set,
//! signs in as that tech and reuses one warm `general:voice:<email>` thread
//! (prompt-cache hits + follow-ups). Without a password it runs as guest,
//! opening a fresh thread per utterance.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{bail, Context, Result};

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn transcribe(wav: &str) -> Result<String> {
    let bin = env_or("WHISPER_BIN", "/home/shadowbroker/voice/whisper.cpp/build/bin/whisper-cli");
    let model = env_or("WHISPER_MODEL", "/home/shadowbroker/voice/whisper.cpp/models/ggml-base.en.bin");
    let prefix = "/tmp/vb_stt";
    let status = Command::new(&bin)
        .args(["-m", &model, "-f", wav, "-nt", "-np", "-otxt", "-of", prefix])
        .status()
        .with_context(|| format!("running {bin}"))?;
    if !status.success() {
        bail!("whisper-cli failed: {status}");
    }
    let text = std::fs::read_to_string(format!("{prefix}.txt"))?.trim().to_string();
    Ok(text)
}

fn synthesize(text: &str, out_wav: &str) -> Result<()> {
    let bin = env_or("PIPER_BIN", "/home/shadowbroker/voice/piper/piper/piper");
    let voice = env_or("PIPER_VOICE", "/home/shadowbroker/voice/piper/voices/en_US-lessac-medium.onnx");
    let mut child = Command::new(&bin)
        .args(["--model", &voice, "--output_file", out_wav])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .with_context(|| format!("running {bin}"))?;
    child.stdin.take().context("piper stdin")?.write_all(text.as_bytes())?;
    if !child.wait()?.success() {
        bail!("piper failed");
    }
    Ok(())
}

/// Opens a session and returns finished agent messages as they land, finalizing on idle.
async fn stream_reply(cs: &str, tech: Option<&str>, text: &str, timeout_secs: u64) -> Result<String> {
    use database::agent_chat::{poll_reply, send, ReplyState};
    let sent = send(cs, tech, None, None, text).await?;
    let mut last = String::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match poll_reply(&sent.thread, sent.after_seq).await? {
            ReplyState::Done(final_text) => return Ok(final_text),
            ReplyState::Waiting(Some(msg)) if msg != last => {
                log::info!("chunk: {msg}");
                last = msg;
            }
            _ => {}
        }
        if std::time::Instant::now() >= deadline {
            bail!("agent did not finish within {timeout_secs}s");
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let mut args = std::env::args().skip(1);
    let in_wav = args.next().context("usage: voice-bridge <input.wav> [tech_email]")?;
    let out_wav = env_or("VB_OUT", "/tmp/vb_reply.wav");

    let tech_email = args
        .next()
        .or_else(|| std::env::var("VB_TECH_EMAIL").ok())
        .filter(|e| !e.is_empty());
    let tech_password = std::env::var("VB_TECH_PASSWORD").ok().filter(|p| !p.is_empty());

    // Signs in as the tech (a warm, reusable thread) when both an email and a
    // password are present; otherwise as guest (a fresh thread per utterance).
    let authed_tech = match (tech_email.as_deref(), tech_password.as_deref()) {
        (Some(email), Some(password)) => {
            database::Database::new(email.to_string(), password.to_string(), None).await?;
            log::info!("signed in as {email}");
            Some(email.to_string())
        }
        _ => {
            database::init_database().await?;
            log::info!("running as guest");
            None
        }
    };

    let transcript = transcribe(&in_wav)?;
    log::info!("STT: {transcript}");
    if transcript.is_empty() {
        bail!("empty transcript");
    }

    let cs = match authed_tech.as_deref() {
        Some(email) => format!("general:voice:{email}"),
        None => {
            let who = tech_email.as_deref().unwrap_or("guest");
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            format!("general:voice:{who}:{nonce}")
        }
    };
    let timeout_secs: u64 = env_or("VB_TIMEOUT", "600").parse().unwrap_or(600);

    let prompt = format!(
        "Voice mode: reply in one or two short spoken sentences for text-to-speech; use tools only if necessary.\n\n{transcript}"
    );

    let reply = stream_reply(&cs, tech_email.as_deref(), &prompt, timeout_secs).await?;
    log::info!("ASSISTANT: {reply}");

    synthesize(&reply, &out_wav)?;
    log::info!("TTS -> {out_wav}");
    println!("{reply}");
    Ok(())
}
