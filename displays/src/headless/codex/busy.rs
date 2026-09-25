//! Whether a thread's turn is running, and what the agent is doing in it, from what codex reports.

use database::schema::AgentActivity;
use serde_json::Value;

/// Where a thread's turn stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Idle,
    Running,
    Approval,
}

impl Phase {
    /// The `agent_thread.status` this phase is stored as.
    pub fn status(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::Approval => "waiting_approval",
        }
    }
}

/// The app-server's own `thread/status/changed` report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerStatus {
    Idle,
    Active,
    Error,
}

/// Something the runner saw that can move the phase or the activity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    TurnStarted,
    TurnEnded,
    /// An item started: proof the agent is working.
    Working(AgentActivity),
    /// Streamed text of an item; changes the activity only while a turn runs.
    Streaming(AgentActivity),
    /// A tool call, command or compaction finished.
    StepDone,
    Approval(String),
    Decided,
    Error {
        will_retry: bool,
    },
    Server(ServerStatus),
    /// What `thread/read` showed right after an attach.
    Attached {
        in_progress: bool,
    },
}

/// The phase and activity the runner last derived.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Busy {
    pub phase: Phase,
    pub activity: AgentActivity,
}

impl Busy {
    pub fn is_idle(&self) -> bool {
        self.phase == Phase::Idle
    }

    /// The state after `signal`.
    pub fn after(&self, signal: &Signal) -> Self {
        use Phase::{Approval, Idle, Running};
        let (phase, activity) = match (self.phase, signal) {
            (_, Signal::TurnStarted) => (Running, AgentActivity::Starting),
            (_, Signal::TurnEnded) => (Idle, AgentActivity::Idle),
            (Approval, Signal::Working(_) | Signal::Streaming(_) | Signal::StepDone) => {
                return self.clone();
            }
            (_, Signal::Working(a)) => (Running, a.clone()),
            (Running, Signal::Streaming(a)) => (Running, a.clone()),
            (Idle, Signal::Streaming(_) | Signal::StepDone) => return self.clone(),
            (Running, Signal::StepDone) => (Running, AgentActivity::Thinking),
            (_, Signal::Approval(tool)) => (Approval, AgentActivity::Approval(tool.clone())),
            (_, Signal::Decided) => (Running, AgentActivity::Thinking),
            (Approval, Signal::Error { will_retry: true }) => return self.clone(),
            (_, Signal::Error { will_retry: true }) => (Running, AgentActivity::Retrying),
            (_, Signal::Error { will_retry: false }) => (Idle, AgentActivity::Idle),
            (_, Signal::Server(ServerStatus::Idle | ServerStatus::Error)) => {
                (Idle, AgentActivity::Idle)
            }
            (Idle, Signal::Server(ServerStatus::Active)) => (Running, AgentActivity::Thinking),
            (_, Signal::Server(ServerStatus::Active)) => return self.clone(),
            (_, Signal::Attached { in_progress: true }) => (Running, AgentActivity::Thinking),
            (_, Signal::Attached { in_progress: false }) => (Idle, AgentActivity::Idle),
        };
        Self { phase, activity }
    }
}

/// The signal an item's start or completion carries.
pub fn item_signal(item_type: &str, completed: bool, item: &Value) -> Option<Signal> {
    let tool = || {
        item.get("tool")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    if completed {
        return matches!(
            item_type,
            "mcpToolCall" | "dynamicToolCall" | "commandExecution" | "contextCompaction"
        )
        .then_some(Signal::StepDone);
    }
    let activity = match item_type {
        "reasoning" => AgentActivity::Thinking,
        "agentMessage" => AgentActivity::Writing,
        "mcpToolCall" | "dynamicToolCall" => AgentActivity::Tool(tool()),
        "commandExecution" => AgentActivity::Command,
        "contextCompaction" => AgentActivity::Compacting,
        _ => return None,
    };
    Some(Signal::Working(activity))
}

/// The status a `thread/status/changed` notification reports, as a string or a `{type}` object.
pub fn server_status(params: &Value) -> Option<ServerStatus> {
    let status = params.get("status")?;
    let word = status
        .as_str()
        .or_else(|| status.get("type").and_then(Value::as_str))?;
    match word {
        "idle" | "notLoaded" => Some(ServerStatus::Idle),
        "active" => Some(ServerStatus::Active),
        "systemError" => Some(ServerStatus::Error),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn busy(phase: Phase, activity: AgentActivity) -> Busy {
        Busy { phase, activity }
    }

    fn run(signals: &[Signal]) -> Busy {
        signals.iter().fold(Busy::default(), |b, s| b.after(s))
    }

    #[test]
    fn a_turn_runs_from_its_start_to_its_end() {
        let tool = AgentActivity::Tool("get_client_info".into());
        let b = run(&[Signal::TurnStarted]);
        assert_eq!(b, busy(Phase::Running, AgentActivity::Starting));
        let b = b
            .after(&Signal::Working(AgentActivity::Thinking))
            .after(&Signal::Working(tool.clone()));
        assert_eq!(b, busy(Phase::Running, tool));
        let b = b.after(&Signal::StepDone);
        assert_eq!(b, busy(Phase::Running, AgentActivity::Thinking));
        let b = b
            .after(&Signal::Streaming(AgentActivity::Writing))
            .after(&Signal::TurnEnded);
        assert_eq!(b, Busy::default());
    }

    #[test]
    fn a_started_item_proves_work_even_after_a_missed_turn_start() {
        let b = run(&[Signal::Working(AgentActivity::Thinking)]);
        assert_eq!(b.phase, Phase::Running);
    }

    #[test]
    fn late_streamed_text_never_reopens_an_ended_turn() {
        let b = run(&[
            Signal::TurnStarted,
            Signal::TurnEnded,
            Signal::Streaming(AgentActivity::Writing),
            Signal::StepDone,
        ]);
        assert_eq!(b, Busy::default());
    }

    #[test]
    fn an_approval_holds_until_decided_and_ignores_stray_activity() {
        let b = run(&[
            Signal::TurnStarted,
            Signal::Approval("remote_exec_start".into()),
        ]);
        assert_eq!(
            b,
            busy(
                Phase::Approval,
                AgentActivity::Approval("remote_exec_start".into())
            )
        );
        assert_eq!(b.after(&Signal::Working(AgentActivity::Writing)), b);
        assert_eq!(b.after(&Signal::Error { will_retry: true }), b);
        assert_eq!(
            b.after(&Signal::Decided),
            busy(Phase::Running, AgentActivity::Thinking)
        );
    }

    #[test]
    fn a_final_error_ends_the_turn_and_a_retry_keeps_it_running() {
        let b = run(&[Signal::TurnStarted, Signal::Error { will_retry: true }]);
        assert_eq!(b, busy(Phase::Running, AgentActivity::Retrying));
        assert_eq!(
            b.after(&Signal::Error { will_retry: false }),
            Busy::default()
        );
        assert_eq!(
            run(&[Signal::Error { will_retry: true }]).phase,
            Phase::Running
        );
    }

    #[test]
    fn the_server_status_corrects_a_drifted_phase() {
        let stuck = busy(Phase::Running, AgentActivity::Writing);
        assert_eq!(
            stuck.after(&Signal::Server(ServerStatus::Idle)),
            Busy::default()
        );
        assert_eq!(
            stuck.after(&Signal::Server(ServerStatus::Error)),
            Busy::default()
        );
        assert_eq!(
            Busy::default()
                .after(&Signal::Server(ServerStatus::Active))
                .phase,
            Phase::Running
        );
        assert_eq!(stuck.after(&Signal::Server(ServerStatus::Active)), stuck);
    }

    #[test]
    fn an_attach_reads_the_turn_the_server_still_runs() {
        assert_eq!(
            run(&[Signal::Attached { in_progress: true }]).phase,
            Phase::Running
        );
        let stuck = busy(Phase::Approval, AgentActivity::Approval("x".into()));
        assert_eq!(
            stuck.after(&Signal::Attached { in_progress: false }),
            Busy::default()
        );
    }

    #[test]
    fn items_signal_their_activity_when_they_start_and_a_step_when_they_finish() {
        let tool = json!({ "type": "dynamicToolCall", "tool": "scripts_list" });
        assert_eq!(
            item_signal("dynamicToolCall", false, &tool),
            Some(Signal::Working(AgentActivity::Tool("scripts_list".into())))
        );
        assert_eq!(
            item_signal("dynamicToolCall", true, &tool),
            Some(Signal::StepDone)
        );
        assert_eq!(
            item_signal("reasoning", false, &json!({})),
            Some(Signal::Working(AgentActivity::Thinking))
        );
        assert_eq!(
            item_signal("contextCompaction", false, &json!({})),
            Some(Signal::Working(AgentActivity::Compacting))
        );
        assert_eq!(item_signal("agentMessage", true, &json!({})), None);
        assert_eq!(item_signal("userMessage", false, &json!({})), None);
    }

    #[test]
    fn server_statuses_read_as_a_word_or_a_typed_object() {
        assert_eq!(
            server_status(&json!({ "status": "idle" })),
            Some(ServerStatus::Idle)
        );
        assert_eq!(
            server_status(&json!({ "status": { "type": "active", "activeFlags": [] } })),
            Some(ServerStatus::Active)
        );
        assert_eq!(
            server_status(&json!({ "status": { "type": "notLoaded" } })),
            Some(ServerStatus::Idle)
        );
        assert_eq!(
            server_status(&json!({ "status": "systemError" })),
            Some(ServerStatus::Error)
        );
        assert_eq!(server_status(&json!({})), None);
    }
}
