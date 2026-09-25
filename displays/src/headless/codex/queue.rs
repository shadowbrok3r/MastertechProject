//! Technician messages queued behind a running turn: sent one per turn, oldest first.

use std::collections::VecDeque;

use database::schema::AgentTurn;

/// Why a stop holds the queue.
pub const STOPPED: &str = "You stopped the turn.";
/// Why a failed turn holds the queue.
pub const FAILED: &str = "The last turn failed.";

/// A thread's queue turns, and why it waits for the technician when it does.
#[derive(Debug, Default)]
pub struct TurnQueue {
    items: VecDeque<AgentTurn>,
    held: Option<String>,
}

impl TurnQueue {
    /// Adds a turn at the back; one already queued under the same id is left alone.
    pub fn push(&mut self, turn: AgentTurn) -> bool {
        if self.items.iter().any(|t| t.id == turn.id) {
            return false;
        }
        self.items.push_back(turn);
        true
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn held(&self) -> Option<&str> {
        self.held.as_deref()
    }

    /// Holds the queue while it has anything in it; true when that started a hold.
    pub fn hold(&mut self, why: &str) -> bool {
        if self.items.is_empty() || self.held.is_some() {
            return false;
        }
        self.held = Some(why.to_string());
        true
    }

    /// Lifts a hold; true when one was lifted.
    pub fn resume(&mut self) -> bool {
        self.held.take().is_some()
    }

    /// The oldest turn, unless the queue is held.
    pub fn next(&mut self) -> Option<AgentTurn> {
        if self.held.is_some() {
            return None;
        }
        self.items.pop_front()
    }

    /// A turn from [`next`](Self::next) did not go out: back to the front, and the queue held.
    pub fn undelivered(&mut self, turn: AgentTurn, why: &str) {
        self.items.push_front(turn);
        self.held = Some(format!("Not sent: {why}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::schema::RecordId;

    fn turn(key: &str) -> AgentTurn {
        AgentTurn {
            id: RecordId::new("agent_turn", key),
            thread: RecordId::new("agent_thread", "t"),
            kind: "queue".into(),
            text: key.into(),
            tech: None,
            status: "queued".into(),
            error: None,
            images: Vec::new(),
            created_at: None,
            sent_at: None,
        }
    }

    fn queue(keys: &[&str]) -> TurnQueue {
        let mut q = TurnQueue::default();
        for k in keys {
            q.push(turn(k));
        }
        q
    }

    fn text(t: Option<AgentTurn>) -> Option<String> {
        t.map(|t| t.text)
    }

    #[test]
    fn turns_go_out_oldest_first() {
        let mut q = queue(&["one", "two", "three"]);
        assert_eq!(text(q.next()).as_deref(), Some("one"));
        assert_eq!(text(q.next()).as_deref(), Some("two"));
        assert_eq!(text(q.next()).as_deref(), Some("three"));
        assert_eq!(q.next(), None);
    }

    #[test]
    fn the_same_turn_is_queued_once() {
        let mut q = queue(&["one"]);
        assert!(!q.push(turn("one")), "a turn delivered twice was queued twice");
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn a_hold_keeps_the_rest_until_resumed() {
        let mut q = queue(&["one", "two"]);
        q.next();
        assert!(q.hold(FAILED));
        assert!(!q.hold(STOPPED), "a second hold replaced the first reason");
        assert_eq!(q.held(), Some(FAILED));
        assert_eq!(q.next(), None, "sent into a turn that just failed");
        assert!(q.resume());
        assert!(!q.resume());
        assert_eq!(text(q.next()).as_deref(), Some("two"));
    }

    #[test]
    fn a_message_queued_while_held_waits_behind_the_held_ones() {
        let mut q = queue(&["one"]);
        q.hold(STOPPED);
        q.push(turn("two"));
        assert_eq!(q.next(), None);
        q.resume();
        assert_eq!(text(q.next()).as_deref(), Some("one"));
        assert_eq!(text(q.next()).as_deref(), Some("two"));
    }

    #[test]
    fn a_stop_holds_the_queue_and_an_empty_queue_holds_nothing() {
        let mut q = queue(&["one"]);
        assert!(q.hold(STOPPED));
        assert_eq!(q.next(), None, "sent after a stop");
        let mut empty = TurnQueue::default();
        assert!(!empty.hold(STOPPED));
        assert!(!empty.hold(FAILED));
        empty.push(turn("later"));
        assert_eq!(text(empty.next()).as_deref(), Some("later"), "a stop with nothing queued held a later message");
    }

    #[test]
    fn a_turn_that_did_not_go_out_returns_to_the_front() {
        let mut q = queue(&["one", "two"]);
        let first = q.next().expect("one");
        q.undelivered(first, "the socket closed");
        assert_eq!(q.held(), Some("Not sent: the socket closed"));
        assert!(q.resume());
        assert_eq!(text(q.next()).as_deref(), Some("one"));
    }
}
