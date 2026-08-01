//! Wire types for RemoteExec: long-running privileged jobs on a connected
//! client, driven from the admin console.
//!
//! These bypass the WASM plugin path entirely. `com.mastertech.repair`'s
//! `run_command` is capped by the PluginManager watchdog, so anything past
//! that deadline is unreachable through a plugin; a RemoteExec job is owned by
//! the client, survives the admin disconnecting, and reports a real exit code.
//!
//! The client is the authority on job state. The admin polls
//! [`crate::Cmd::RemoteJobQuery`] and receives whole [`JobSnapshot`] values;
//! any live push is a latency optimisation over the same data, never a
//! separate source of truth.

use facet::Facet;
use serde::{Deserialize, Serialize};

/// Which interpreter runs a [`RemoteJobSpec::Shell`] script.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Facet)]
#[repr(u8)]
pub enum ShellKind {
    /// `powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File`
    PowerShell,
    /// `pwsh.exe` with the same arguments; falls back to PowerShell when absent.
    Pwsh,
    /// `cmd.exe /C`
    Cmd,
}

/// What a job does. Only [`RemoteJobSpec::Shell`] is accepted today; the other
/// variants are reserved so adding them later does not shift bincode indices.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Facet)]
#[repr(u8)]
pub enum RemoteJobSpec {
    Shell {
        shell: ShellKind,
        script: String,
        cwd: Option<String>,
        env: Vec<(String, String)>,
        /// Hard wall-clock cap. `None` uses the client default.
        timeout_secs: Option<u64>,
        /// Suppress captured output from the durable journal.
        redact: bool,
    },
}

/// How much damage a job could do, declared by the caller and recorded.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Facet)]
#[repr(u8)]
pub enum RiskTier {
    /// Reads state, changes nothing.
    Read,
    /// Changes state reversibly.
    Mutate,
    /// Removes data, or changes boot/driver/security state.
    Destructive,
}

/// Which pipe a [`JobChunk`] came from.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Facet)]
#[repr(u8)]
pub enum JobStream {
    Stdout,
    Stderr,
    /// Emitted by the runtime, not the process — eviction notices, timeouts.
    Meta,
}

/// A contiguous run of captured output.
///
/// `data` is bytes, not `String`: console output is frequently CP437 or
/// UTF-16, and decoding at the client edge loses it irrecoverably.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Facet)]
pub struct JobChunk {
    pub job_id: String,
    /// Monotonic per job, never reset; the admin resumes from `last_seq`.
    pub seq: u64,
    pub stream: JobStream,
    #[serde(with = "b64_bytes")]
    pub data: Vec<u8>,
    /// Bytes the in-memory ring dropped before this chunk.
    pub elided_before: u64,
}

/// Base64 codec for [`JobChunk::data`]. Serde would otherwise emit a `Vec<u8>`
/// as one JSON number per byte, roughly quadrupling every tail response.
mod b64_bytes {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        base64::engine::general_purpose::STANDARD
            .decode(&s)
            .map_err(serde::de::Error::custom)
    }
}

/// Why a job stopped.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Facet)]
#[repr(u8)]
pub enum JobOutcome {
    /// The process ran to completion; see `exit_code`.
    Exited,
    TimedOut,
    /// Graceful cancel requested and honoured.
    Cancelled,
    /// Process tree terminated.
    Killed,
    SpawnFailed(String),
    /// The consent gate refused the job.
    GateDenied(String),
}

/// Terminal record for a job. Retained after exit so a reconnecting admin can
/// still collect it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Facet)]
pub struct JobExit {
    pub job_id: String,
    pub outcome: JobOutcome,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    /// Bytes evicted from the RAM ring over the job's life.
    pub truncated_bytes: u64,
    pub last_seq: u64,
}

/// Out-of-band instruction to a running job.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Facet)]
#[repr(u8)]
pub enum JobSignal {
    /// Cooperative stop, then terminate the tree.
    Cancel,
    /// Terminate the tree immediately.
    Kill,
    /// Leave it running and stop streaming.
    Detach,
}

/// Lifecycle position of a job.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Facet)]
#[repr(u8)]
pub enum JobState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
    /// Found in the journal with a dead pid after a restart. Never re-run: a
    /// half-finished install must not repeat.
    Orphaned,
}

impl JobState {
    pub fn is_terminal(self) -> bool {
        !matches!(self, JobState::Queued | JobState::Running)
    }
}

/// Everything the admin needs about one job, plus any output it asked for.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Facet)]
pub struct JobSnapshot {
    pub job_id: String,
    pub state: JobState,
    /// Short human description of the spec, for listings.
    pub spec_summary: String,
    pub risk: RiskTier,
    pub reason: String,
    pub tech: String,
    pub started_at_ms: u64,
    pub last_seq: u64,
    pub pid: Option<u32>,
    pub exit: Option<JobExit>,
    /// Chunks from the requested `from_seq`, when the query asked for output.
    pub chunks: Vec<JobChunk>,
    /// True when the ring could not serve the requested `from_seq` in full.
    pub chunks_truncated: bool,
}

/// Result of an arm/disarm request.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Facet)]
pub struct GateStatus {
    pub armed: bool,
    pub tech: Option<String>,
    pub diagnostic_session_id: Option<String>,
    pub expires_in_secs: Option<u64>,
    pub running_jobs: u32,
    /// Set when a request was refused, naming which precondition failed.
    pub denied_reason: Option<String>,
}

/// What this client build can run, so the admin does not have to guess.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Facet)]
pub struct RemoteExecCapabilities {
    /// Spec kinds this build accepts, e.g. `["shell"]`.
    pub job_kinds: Vec<String>,
    pub shells: Vec<ShellKind>,
    pub protocol_version: u32,
    pub client_version: String,
    pub max_concurrent_jobs: u32,
    /// Ring capacity per job, so the admin can pace its polling.
    pub ring_bytes: u64,
    pub default_timeout_secs: u64,
}

/// Bumped when the RemoteExec contract changes in a way the admin must notice.
pub const REMOTE_EXEC_PROTOCOL_VERSION: u32 = 1;

/// `RemotePluginToolResult.plugin_id` the client answers RemoteExec commands with.
pub const NATIVE_REMOTE_EXEC_PLUGIN_ID: &str = "native.remote-exec";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_data_rides_as_base64_not_a_byte_array() {
        let c = JobChunk {
            job_id: "j".into(),
            seq: 3,
            stream: JobStream::Stdout,
            data: b"hi".to_vec(),
            elided_before: 0,
        };
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(
            json["data"], "aGk=",
            "the admin decodes base64; a JSON number array would break it and quadruple every tail"
        );
        assert_eq!(serde_json::from_value::<JobChunk>(json).unwrap(), c);
    }

    #[test]
    fn non_utf8_bytes_survive_the_round_trip() {
        // Console output is frequently CP437 or UTF-16; decoding at the client
        // edge would lose it irrecoverably.
        let data = vec![0xff, 0x00, 0xfe, 0x80];
        let c = JobChunk {
            job_id: "j".into(),
            seq: 0,
            stream: JobStream::Stderr,
            data: data.clone(),
            elided_before: 7,
        };
        let back: JobChunk =
            serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back.data, data);
        assert_eq!(back.elided_before, 7);
    }

    #[test]
    fn stream_names_match_what_the_admin_matches_on() {
        // render_job_snapshot in mcp_bridge routes on these exact strings.
        assert_eq!(serde_json::to_value(JobStream::Stdout).unwrap(), "Stdout");
        assert_eq!(serde_json::to_value(JobStream::Stderr).unwrap(), "Stderr");
        assert_eq!(serde_json::to_value(JobStream::Meta).unwrap(), "Meta");
    }

    #[test]
    fn only_queued_and_running_are_non_terminal() {
        for s in [JobState::Queued, JobState::Running] {
            assert!(!s.is_terminal(), "{s:?}");
        }
        for s in [
            JobState::Succeeded,
            JobState::Failed,
            JobState::Cancelled,
            JobState::TimedOut,
            JobState::Orphaned,
        ] {
            assert!(s.is_terminal(), "{s:?}");
        }
    }

    #[test]
    fn job_state_names_match_the_admins_terminal_check() {
        // remote_exec_wait treats anything other than these two as terminal.
        assert_eq!(serde_json::to_value(JobState::Queued).unwrap(), "Queued");
        assert_eq!(serde_json::to_value(JobState::Running).unwrap(), "Running");
    }
}
