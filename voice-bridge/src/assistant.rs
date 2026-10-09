//! One voice turn on the tech's assistant thread: agent text as it streams, and the approvals it waits on.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use database::agent_chat;
use database::schema::{AgentApproval, AgentEvent, AgentThread, AgentTurn, RecordIdExt};
use tokio::sync::mpsc;

const POLL: Duration = Duration::from_millis(500);
const INTERRUPT_WAIT: Duration = Duration::from_secs(10);
/// Assistant tools a spoken "yes" may approve; every other gated tool needs a tap on the board.
const VOICE_APPROVABLE: &[&str] = &["create_task", "assign_task", "notify_user", "schedule_task", "route_part"];

/// What a running turn reports.
#[derive(Debug)]
pub enum TurnEvent {
    /// Newly finished sentences of an agent message, the message so far, and whether it is complete.
    Speech { text: String, message: String, done: bool },
    Approval(AgentApproval),
    /// An announced approval, by key, that is no longer pending.
    Resolved(String),
}

/// A spoken reply to an approval prompt.
#[derive(Debug, PartialEq, Eq)]
pub enum Answer {
    Yes,
    No,
    Other,
}

/// Whether a spoken "yes" may approve this request.
pub fn voice_may_approve(a: &AgentApproval) -> bool {
    a.kind == "tool_call" && a.tool.as_deref().is_some_and(|t| VOICE_APPROVABLE.contains(&t))
}

/// Classifies a transcript as a yes, a no, or anything else.
pub fn classify(transcript: &str) -> Answer {
    let cleaned: String = transcript
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '\'' || c.is_whitespace() { c } else { ' ' })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let Some(first) = words.first() else { return Answer::Other };
    const NO: &[&str] = &["no", "nope", "nah", "deny", "decline", "cancel", "stop", "don't", "dont", "negative"];
    const YES: &[&str] = &["yes", "yeah", "yep", "yup", "sure", "ok", "okay", "approve", "approved", "confirm", "go", "do"];
    let negated = words.windows(2).any(|w| matches!(w, ["do" | "please", "not"])) || words.contains(&"don't");
    if NO.contains(first) || negated {
        Answer::No
    } else if YES.contains(first) && words.len() <= 4 {
        Answer::Yes
    } else {
        Answer::Other
    }
}

/// Byte index just past the last finished sentence in `text[from..]`, or `from` when none has finished.
pub fn sentence_end(text: &str, from: usize) -> usize {
    let bytes = text.as_bytes();
    let mut end = from;
    for i in from..bytes.len().saturating_sub(1) {
        let ends_sentence = matches!(bytes[i], b'.' | b'!' | b'?') && bytes[i + 1].is_ascii_whitespace();
        if ends_sentence || bytes[i] == b'\n' {
            end = i + 1;
        }
    }
    end
}

/// Interrupts a busy thread for `cs` and waits briefly for it to go idle.
async fn settle(cs: &str) -> Result<()> {
    let Some(thread) = AgentThread::active_for_connection(cs).await? else { return Ok(()) };
    if !thread.is_busy() {
        return Ok(());
    }
    AgentTurn::ask(&thread.id, "interrupt", "").await?;
    let until = Instant::now() + INTERRUPT_WAIT;
    while Instant::now() < until {
        tokio::time::sleep(POLL).await;
        if !AgentThread::get(&thread.id).await?.is_some_and(|t| t.is_busy()) {
            return Ok(());
        }
    }
    log::warn!("thread still busy after the interrupt; the new turn queues behind it");
    Ok(())
}

/// Sends `prompt` and reports the turn as it runs; returns the final agent message.
pub async fn drive(
    cs: &str,
    tech: Option<&str>,
    prompt: &str,
    timeout: Duration,
    events: &mpsc::Sender<TurnEvent>,
) -> Result<String> {
    settle(cs).await?;
    let sent = agent_chat::send(cs, tech, None, None, prompt).await?;
    log::info!("thread {:?} (after_seq {})", sent.thread, sent.after_seq);
    let deadline = Instant::now() + timeout;
    let mut spoken: HashMap<String, usize> = HashMap::new();
    let mut finished: HashSet<String> = HashSet::new();
    let mut announced: HashSet<String> = HashSet::new();
    loop {
        let status = AgentThread::get(&sent.thread).await?.map(|t| (t.status, t.error));
        let agent: Vec<AgentEvent> = AgentEvent::history(&sent.thread, sent.after_seq, 500)
            .await?
            .into_iter()
            .filter(|e| e.kind == "agent")
            .collect();
        for e in &agent {
            let key = e.id.key_string();
            if finished.contains(&key) {
                continue;
            }
            let from = spoken.get(&key).copied().unwrap_or(0);
            let upto = if e.done { e.text.len() } else { sentence_end(&e.text, from) };
            if let (Some(text), Some(message)) = (e.text.get(from..upto), e.text.get(..upto)) {
                if upto > from || e.done {
                    let ev = TurnEvent::Speech { text: text.to_string(), message: message.to_string(), done: e.done };
                    events.send(ev).await?;
                }
            }
            spoken.insert(key.clone(), upto);
            if e.done {
                finished.insert(key);
            }
        }

        let pending = AgentApproval::list_pending_for_thread(&sent.thread).await?;
        let keys: HashSet<String> = pending.iter().map(|a| a.id.key_string()).collect();
        for a in pending {
            if announced.insert(a.id.key_string()) {
                events.send(TurnEvent::Approval(a)).await?;
            }
        }
        for key in announced.iter().filter(|k| !keys.contains(*k)).cloned().collect::<Vec<_>>() {
            announced.remove(&key);
            events.send(TurnEvent::Resolved(key)).await?;
        }

        let reply = agent.iter().rev().find(|e| e.done).map(|e| e.text.clone());
        match (status, reply) {
            (Some((s, err)), _) if s == "failed" => {
                bail!("the agent session failed: {}", err.unwrap_or_else(|| "unknown error".into()))
            }
            (Some((s, _)), Some(text)) if s == "idle" || s == "closed" => return Ok(text),
            (Some((s, _)), None) if s == "closed" => bail!("the agent session closed without replying"),
            (None, _) => bail!("the agent session no longer exists"),
            _ => {}
        }
        if Instant::now() >= deadline {
            bail!("the agent did not finish within {}s", timeout.as_secs());
        }
        tokio::time::sleep(POLL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentences_end_at_punctuation_followed_by_space() {
        let t = "Checking now. The fan reads 3.5 volts! Still";
        let first = sentence_end(t, 0);
        assert_eq!(&t[..first], "Checking now. The fan reads 3.5 volts!");
        assert_eq!(sentence_end(t, first), first);
    }

    #[test]
    fn no_finished_sentence_keeps_the_cursor() {
        assert_eq!(sentence_end("Working on it", 0), 0);
        assert_eq!(sentence_end("One. Two", 5), 5);
    }

    #[test]
    fn newlines_end_a_chunk() {
        let t = "First line\nsecond";
        assert_eq!(&t[..sentence_end(t, 0)], "First line\n");
    }

    #[test]
    fn short_affirmatives_are_yes() {
        for t in ["Yes.", "yeah go ahead", "Okay, do it.", "Approve", "sure"] {
            assert_eq!(classify(t), Answer::Yes, "{t}");
        }
    }

    #[test]
    fn negatives_win_over_affirmative_words() {
        for t in ["No.", "nope", "Don't do that.", "do not send it", "cancel", "Stop"] {
            assert_eq!(classify(t), Answer::No, "{t}");
        }
    }

    #[test]
    fn anything_else_is_other() {
        for t in ["", "send it to Jacob instead of me please", "what time is it", "yes but only if the customer agreed to it"] {
            assert_eq!(classify(t), Answer::Other, "{t}");
        }
    }
}
