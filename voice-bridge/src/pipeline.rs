//! STT (whisper.cpp), the Mastertech assistant turn (`agent_chat`) and the voice identity.

use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};

pub fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Who the assistant turns run as, and how the connection string is derived.
pub struct Identity {
    /// The email when signed in as a tech (a warm, reusable thread); `None` for guest.
    authed_tech: Option<String>,
    /// The email label for `requested_by` and the guest thread name, if any.
    label: Option<String>,
}

impl Identity {
    /// Signs in as the tech when an email and `VB_TECH_PASSWORD` are present,
    /// otherwise as guest. `email_override` is the file-mode positional arg.
    pub async fn init(email_override: Option<String>) -> Result<Self> {
        let label = email_override
            .or_else(|| std::env::var("VB_TECH_EMAIL").ok())
            .filter(|e| !e.is_empty());
        let password = std::env::var("VB_TECH_PASSWORD").ok().filter(|p| !p.is_empty());
        match (label.as_deref(), password.as_deref()) {
            (Some(email), Some(password)) => {
                database::Database::new(email.to_string(), password.to_string(), None).await?;
                log::info!("signed in as {email}");
                Ok(Self { authed_tech: Some(email.to_string()), label })
            }
            _ => {
                database::init_database().await?;
                log::info!("running as guest");
                Ok(Self { authed_tech: None, label })
            }
        }
    }

    pub fn requested_by(&self) -> Option<&str> {
        self.label.as_deref()
    }

    /// A signed-in tech reuses one warm thread; guest opens a fresh one per utterance.
    pub fn connection_string(&self) -> String {
        match &self.authed_tech {
            Some(email) => format!("general:voice:{email}"),
            None => {
                let who = self.label.as_deref().unwrap_or("guest");
                format!("general:voice:{who}:{}", now_millis())
            }
        }
    }
}

pub fn voice_prompt(transcript: &str) -> String {
    format!(
        "Voice mode: reply in one or two short spoken sentences for text-to-speech; use tools only if necessary.\n\n{transcript}"
    )
}

pub fn transcribe(wav: &str) -> Result<String> {
    let bin = env_or("WHISPER_BIN", "/home/shadowbroker/voice/whisper.cpp/build/bin/whisper-cli");
    let model = env_or("WHISPER_MODEL", "/home/shadowbroker/voice/whisper.cpp/models/ggml-base.en.bin");
    let prefix = format!("{wav}.stt");
    let status = Command::new(&bin)
        .args(["-m", &model, "-f", wav, "-nt", "-np", "-otxt", "-of", &prefix])
        .status()
        .with_context(|| format!("running {bin}"))?;
    if !status.success() {
        bail!("whisper-cli failed: {status}");
    }
    let text = std::fs::read_to_string(format!("{prefix}.txt"))?.trim().to_string();
    Ok(text)
}

/// Opens a session and returns finished agent messages as they land, finalizing on idle.
pub async fn stream_reply(cs: &str, tech: Option<&str>, text: &str, timeout_secs: u64) -> Result<String> {
    use database::agent_chat::{poll_reply, send, ReplyState};
    let sent = send(cs, tech, None, None, text).await?;
    log::info!("thread {:?} (after_seq {})", sent.thread, sent.after_seq);
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

/// Full turn: a spoken-utterance WAV in, the assistant's reply text and a spoken WAV out.
pub async fn run_turn(id: &Identity, in_wav: &str, out_wav: &str, timeout_secs: u64) -> Result<String> {
    let transcript = transcribe(in_wav)?;
    log::info!("STT: {transcript}");
    if transcript.is_empty() {
        bail!("empty transcript");
    }
    let reply = stream_reply(
        &id.connection_string(),
        id.requested_by(),
        &voice_prompt(&transcript),
        timeout_secs,
    )
    .await?;
    log::info!("ASSISTANT: {reply}");
    let wav = crate::voices::synthesize(&reply, &crate::voices::ActiveVoice::load().get())?;
    std::fs::write(out_wav, wav)?;
    log::info!("TTS -> {out_wav}");
    Ok(reply)
}
