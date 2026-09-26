//! Client side of a conversation with a Codex agent session, over the agent tables.

use std::time::Duration;

use crate::schema::{AgentEvent, AgentThread, AgentTurn, AssistRequest, RecordId};
use crate::{db, sleep_compat};

/// Longest opening note an `assist_request` carries.
const REQUEST_NOTE_MAX: usize = 500;
const POLL: Duration = Duration::from_secs(2);
const OPEN_TIMEOUT: Duration = Duration::from_secs(90);
const OPENER_TIMEOUT: Duration = Duration::from_secs(300);
const OPENER: &str = "Open this session. My request follows as the next message.";

/// A message delivered to a session: its thread, and the event seq the reply follows.
#[derive(Debug, Clone)]
pub struct Sent {
    pub thread: RecordId,
    pub after_seq: i64,
}

/// Sends `text` to the live session for `connection_string`, or opens one with it.
pub async fn send(
    connection_string: &str,
    requested_by: Option<&str>,
    store: Option<&str>,
    service_number: Option<&str>,
    text: &str,
) -> anyhow::Result<Sent> {
    if let Some(thread) = AgentThread::active_for_connection(connection_string).await? {
        let after_seq = last_seq(&thread.id).await?;
        AgentTurn::ask(&thread.id, "start", text).await?;
        return Ok(Sent { thread: thread.id, after_seq });
    }
    let fits = text.chars().count() <= REQUEST_NOTE_MAX;
    let note = if fits { text } else { OPENER };
    let request =
        AssistRequest::create_from_chat(connection_string, requested_by, store, service_number, note, false).await?;
    let thread = await_thread(&request, OPEN_TIMEOUT).await?;
    if fits {
        return Ok(Sent { thread, after_seq: 0 });
    }
    await_reply(&thread, 0, OPENER_TIMEOUT).await?;
    let after_seq = last_seq(&thread).await?;
    AgentTurn::ask(&thread, "start", text).await?;
    Ok(Sent { thread, after_seq })
}

/// Sends `text` and returns the agent's final message for that turn.
pub async fn ask(
    connection_string: &str,
    requested_by: Option<&str>,
    text: &str,
    timeout: Duration,
) -> anyhow::Result<String> {
    let sent = send(connection_string, requested_by, None, None, text).await?;
    await_reply(&sent.thread, sent.after_seq, timeout).await
}

/// Waits for the broker to link an `assist_request` to the session it opened.
pub async fn await_thread(request: &RecordId, timeout: Duration) -> anyhow::Result<RecordId> {
    let mut waited = Duration::ZERO;
    loop {
        if let Some(req) = AssistRequest::get(request).await? {
            if let Some(thread) = req.agent_thread {
                return Ok(thread);
            }
            if req.status == "failed" {
                anyhow::bail!(
                    "the agent host could not open a session: {}",
                    req.dispatch_error.unwrap_or_else(|| "unknown error".into())
                );
            }
        }
        if waited >= timeout {
            anyhow::bail!("no agent session opened within {}s; is admin-agent running?", timeout.as_secs());
        }
        sleep_compat(POLL).await;
        waited += POLL;
    }
}

/// Waits for the turn after `after_seq` to finish and returns the agent's last message.
pub async fn await_reply(thread: &RecordId, after_seq: i64, timeout: Duration) -> anyhow::Result<String> {
    let mut waited = Duration::ZERO;
    loop {
        let status = AgentThread::get(thread).await?.map(|t| (t.status, t.error));
        let reply = AgentEvent::history(thread, after_seq, 500)
            .await?
            .into_iter()
            .rev()
            .find(|e| e.kind == "agent" && e.done)
            .map(|e| e.text);
        match (status, reply) {
            (Some((s, err)), _) if s == "failed" => {
                anyhow::bail!("the agent session failed: {}", err.unwrap_or_else(|| "unknown error".into()))
            }
            (Some((s, _)), Some(text)) if s == "idle" || s == "closed" => return Ok(text),
            (Some((s, _)), None) if s == "closed" => anyhow::bail!("the agent session closed without replying"),
            (None, _) => anyhow::bail!("the agent session no longer exists"),
            _ => {}
        }
        if waited >= timeout {
            anyhow::bail!("the agent did not finish within {}s", timeout.as_secs());
        }
        sleep_compat(POLL).await;
        waited += POLL;
    }
}

/// Highest event seq recorded on a thread, 0 when it has none.
pub async fn last_seq(thread: &RecordId) -> anyhow::Result<i64> {
    let mut res = db()
        .query("RETURN math::max((SELECT VALUE seq FROM agent_event WHERE thread = $thread)) ?? 0")
        .bind(("thread", thread.clone()))
        .await?;
    let seq: Option<i64> = res.take(0)?;
    Ok(seq.unwrap_or(0))
}

/// The first fenced code block in `reply`, or the whole reply when it has none.
pub fn code_block(reply: &str) -> String {
    let Some(open) = reply.find("```") else {
        return reply.trim().to_string();
    };
    let after = &reply[open + 3..];
    let body = after.split_once('\n').map_or("", |(_, rest)| rest);
    match body.find("```") {
        Some(close) => body[..close].trim_end().to_string(),
        None => body.trim_end().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::code_block;

    #[test]
    fn a_fenced_block_is_extracted_without_its_language_tag() {
        let reply = "Here you go:\n```powershell\nGet-Service | Out-Host\n```\nDone.";
        assert_eq!(code_block(reply), "Get-Service | Out-Host");
    }

    #[test]
    fn a_reply_without_a_fence_is_kept_whole() {
        assert_eq!(code_block("  Get-Date  "), "Get-Date");
    }

    #[test]
    fn an_unclosed_fence_keeps_everything_after_it() {
        assert_eq!(code_block("```ps1\nGet-Date\n"), "Get-Date");
    }
}
