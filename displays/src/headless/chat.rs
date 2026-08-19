//! Bridges `assist_message` to the zeroclaw MasterTech channel.
//!
//! Outbound: a live query picks up a technician's queued message, signs it and
//! posts it to the channel's inbound listener. Inbound: a loopback listener
//! accepts the agent's reply and writes it back as a row the client renders.
//!
//! The channel rail is used rather than `/webhook` because only a channel gets
//! conversation history and a per-technician session row on the agent host.

use std::time::Duration;

use database::live_data::Action;
use database::schema::{AssistMessage, RecordIdExt};

const LIVE_QUERY: &str =
    "LIVE SELECT * FROM assist_message WHERE direction = 'in' AND status = 'pending'";
const DEFAULT_REPLY_ADDR: &str = "127.0.0.1:9015";
/// Sender used when a queued message carries no technician; keeps an
/// unattributed conversation out of a real technician's session.
const UNKNOWN_SENDER: &str = "unattributed@pclaptops.com";
/// The channel substitutes this when a turn ends with no visible text.
const EMPTY_TURN_MARKER: &str = "I couldn't produce a visible reply";

fn env_value(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// Inbound URL plus the HMAC secret the channel verifies bodies against.
fn channel() -> Option<(String, String)> {
    Some((env_value("MTECH_ZC_CHANNEL_URL")?, env_value("MTECH_ZC_CHANNEL_SECRET")?))
}

fn hmac_hex(secret: &str, body: &[u8]) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = <Hmac<Sha256>>::new_from_slice(secret.as_bytes()).expect("hmac accepts any key");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

/// Hands one queued message to the channel; the reply arrives separately.
async fn forward(msg: AssistMessage) {
    let Some((url, secret)) = channel() else {
        log::warn!("chat: no channel configured; leaving {} pending", msg.id.key_string());
        return;
    };
    match AssistMessage::claim(&msg.id).await {
        Ok(true) => {},
        Ok(false) => return,
        Err(e) => {
            log::warn!("chat: claim failed for {}: {e}", msg.id.key_string());
            return;
        },
    }
    if msg.tech.is_none() {
        log::warn!("chat: {} has no tech; attributing to {UNKNOWN_SENDER}", msg.id.key_string());
    }
    let body = serde_json::json!({
        "sender": msg.tech.as_deref().unwrap_or(UNKNOWN_SENDER),
        "content": msg.text,
        "thread_id": msg.room,
    });
    let raw = serde_json::to_vec(&body).unwrap_or_default();
    let signature = format!("sha256={}", hmac_hex(&secret, &raw));

    let sent = reqwest::Client::new()
        .post(&url)
        .header("Content-Type", "application/json")
        .header("x-webhook-signature", signature)
        .body(raw)
        .send()
        .await;
    match sent {
        Ok(resp) if resp.status().is_success() => {
            log::info!("chat: forwarded {} to the agent channel", msg.id.key_string());
        },
        Ok(resp) => {
            let code = resp.status();
            let text = resp.text().await.unwrap_or_default();
            let err = format!("channel {code}: {}", text.chars().take(200).collect::<String>());
            log::warn!("chat: {} rejected - {err}", msg.id.key_string());
            let _ = AssistMessage::mark_failed(&msg.id, &err).await;
        },
        Err(e) => {
            log::warn!("chat: {} post failed - {e}", msg.id.key_string());
            let _ = AssistMessage::mark_failed(&msg.id, &e.to_string()).await;
        },
    }
}

/// Gateway base plus bearer token, for the session admin API.
fn gateway() -> Option<(String, String)> {
    let url = env_value("MTECH_ZC_GATEWAY")?;
    let token = env_value("MTECH_ZC_TOKEN").or_else(|| env_value("ZEROCLAW_GATEWAY_TOKEN"))?;
    Some((url.trim_end_matches('/').to_string(), token))
}

/// Finds the agent session for a room. The key embeds a sanitized sender, so it
/// is matched by prefix rather than reconstructed.
async fn session_key_for_room(room: &str) -> Option<String> {
    let (url, token) = gateway()?;
    let body: serde_json::Value = reqwest::Client::new()
        .get(format!("{url}/api/sessions"))
        .bearer_auth(&token)
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let prefix = format!("_{room}_");
    body["sessions"]
        .as_array()?
        .iter()
        .filter_map(|s| s["session_key"].as_str())
        .find(|k| k.starts_with("webhook_") && k.contains(&prefix))
        .map(str::to_string)
}

/// Gives the session a readable name and records its key on the transcript.
async fn label_session(room: &str) {
    if AssistMessage::session_key_for(room).await.unwrap_or(None).is_some() {
        return;
    }
    let Some(key) = session_key_for_room(room).await else {
        log::debug!("chat: no agent session found for room {room} yet");
        return;
    };
    if let Err(e) = AssistMessage::set_session_key(room, &key).await {
        log::warn!("chat: could not record session key for {room}: {e}");
    }
    match AssistMessage::link_diagnostic_sessions(room, &key).await {
        Ok(0) => {},
        Ok(n) => log::info!("chat: linked {n} diagnostic session(s) to {key}"),
        Err(e) => log::warn!("chat: could not link diagnostic sessions for {room}: {e}"),
    }
    let Some(label) = AssistMessage::room_label(room).await.unwrap_or(None) else {
        return;
    };
    let Some((url, token)) = gateway() else { return };
    let sent = reqwest::Client::new()
        .put(format!("{url}/api/sessions/{key}"))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "name": label }))
        .send()
        .await;
    match sent {
        Ok(resp) if resp.status().is_success() => {
            log::info!("chat: named session {key} \"{label}\"");
        },
        Ok(resp) => log::warn!("chat: rename of {key} returned {}", resp.status()),
        Err(e) => log::warn!("chat: rename of {key} failed: {e}"),
    }
}

/// The channel's outbound payload.
#[derive(serde::Deserialize)]
struct ChannelReply {
    content: String,
    #[serde(default)]
    thread_id: Option<String>,
    #[serde(default)]
    recipient: Option<String>,
}

/// Accepts agent replies on loopback and files them against their conversation.
async fn serve_replies() {
    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::post;
    use axum::{Json, Router};

    let addr = env_value("MTECH_ZC_REPLY_ADDR").unwrap_or_else(|| DEFAULT_REPLY_ADDR.to_string());
    let token = env_value("MTECH_ZC_REPLY_TOKEN");
    if token.is_none() {
        log::warn!("chat: MTECH_ZC_REPLY_TOKEN unset; replies are accepted unauthenticated");
    }

    async fn handle(
        State(expected): State<Option<String>>,
        headers: HeaderMap,
        Json(reply): Json<ChannelReply>,
    ) -> StatusCode {
        if let Some(expected) = expected.as_deref() {
            let got = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("");
            if got.trim() != expected {
                return StatusCode::UNAUTHORIZED;
            }
        }
        let Some(room) = reply.thread_id.or(reply.recipient).filter(|r| !r.trim().is_empty()) else {
            return StatusCode::BAD_REQUEST;
        };
        if reply.content.trim().is_empty() {
            return StatusCode::BAD_REQUEST;
        }
        let room = room.trim();
        let filed = if reply.content.contains(EMPTY_TURN_MARKER) {
            log::warn!("chat: empty agent turn for room {room}");
            AssistMessage::reply_empty(room).await
        } else {
            AssistMessage::reply(room, &reply.content, None).await
        };
        match filed {
            Ok(()) => {
                log::info!("chat: reply filed for room {room}");
                let room = room.to_string();
                tokio::spawn(async move { label_session(&room).await });
                StatusCode::OK
            },
            Err(e) => {
                log::warn!("chat: could not file reply for {room}: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            },
        }
    }

    let app = Router::new().route("/zc/reply", post(handle)).with_state(token);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            log::error!("chat: cannot bind reply listener on {addr}: {e}");
            return;
        },
    };
    log::info!("chat: reply listener on http://{addr}/zc/reply");
    if let Err(e) = axum::serve(listener, app).await {
        log::error!("chat: reply listener stopped: {e}");
    }
}

/// Runs the bridge for the life of the process, restarting on stream loss.
pub fn spawn_chat_bridge() {
    tokio::spawn(serve_replies());
    tokio::spawn(async move {
        loop {
            for msg in AssistMessage::pending_inbound(20).await.unwrap_or_default() {
                forward(msg).await;
            }
            let (tx, rx) = crossbeam::channel::unbounded::<(Action, AssistMessage)>();
            let listener = tokio::spawn(database::live_data::listen_data_filtered::<AssistMessage>(
                tx,
                LIVE_QUERY.to_string(),
                Vec::new(),
                None,
            ));
            log::info!("chat: watching {LIVE_QUERY}");
            loop {
                match rx.try_recv() {
                    Ok((Action::Create | Action::Update, msg)) => {
                        if msg.status == "pending" && msg.is_from_tech() {
                            tokio::spawn(forward(msg));
                        }
                    },
                    Ok(_) => {},
                    Err(crossbeam::channel::TryRecvError::Empty) => {
                        if listener.is_finished() {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    },
                    Err(crossbeam::channel::TryRecvError::Disconnected) => break,
                }
            }
            log::warn!("chat: live stream ended; retrying in 10s");
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });
}
