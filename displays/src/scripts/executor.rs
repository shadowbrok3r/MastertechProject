//! What it means to run a script, independent of who is running it.
//!
//! Every surface — the egui tab, terminal mode, the remote client, MCP — starts a
//! script and then needs to learn when it finished and what happened. That is the
//! whole shared contract, and it is expressed here as a spawn plus a one-shot
//! completion channel.
//!
//! Deliberately not `async`. An `async fn -> ScriptResult` would force a runtime
//! on every caller, and the egui tab has none on its UI thread — its executors are
//! `std::thread::spawn` precisely because it cannot block a frame. A
//! [`ScriptHandle`] serves all four: the two with a per-frame pump `try_recv` it,
//! and the two that already have a runtime wrap it in `spawn_blocking`. Wrapping
//! sync in async costs one task; the reverse means inventing a runtime.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossbeam::channel::Receiver;

use super::id::ScriptId;
use super::{ScriptCategory, ScriptChannels, ScriptLogEntry};
use crate::scripts::catalog::ScriptDef;

/// How a script finished.
#[derive(Debug, Clone)]
pub enum ScriptResult {
    Success(String),
    Warning(String),
    Error(String),
    Skipped(String),
}

impl ScriptResult {
    pub fn is_success(&self) -> bool {
        matches!(self, ScriptResult::Success(_))
    }

    pub fn is_failure(&self) -> bool {
        matches!(self, ScriptResult::Error(_))
    }

    pub fn message(&self) -> &str {
        match self {
            ScriptResult::Success(msg)
            | ScriptResult::Warning(msg)
            | ScriptResult::Error(msg)
            | ScriptResult::Skipped(msg) => msg,
        }
    }
}

/// Everything a host learns when a script ends.
///
/// This exists because completion was previously inferred by scanning the log for
/// a line that looked terminal: a mid-run warning ended the script a second later
/// and started the next one concurrently, and an error followed by an info line
/// read as success.
#[derive(Debug, Clone)]
pub struct ScriptOutcome {
    pub id: ScriptId,
    /// Identifies the queue entry, so two copies of the same script stay distinct.
    pub run_token: u64,
    pub result: ScriptResult,
    /// Captured nowhere before this; `None` when the work was not a child process.
    pub exit_code: Option<i32>,
    /// Replaces sniffing the log for the reboot marker on the local path.
    pub reboot_recommended: bool,
    /// `stress_test_run` key, straight from the controller instead of the log.
    pub run_id: Option<String>,
    pub duration: Duration,
}

impl ScriptOutcome {
    /// A result with nothing else to report.
    pub fn plain(id: ScriptId, run_token: u64, result: ScriptResult, duration: Duration) -> Self {
        Self {
            id,
            run_token,
            result,
            exit_code: None,
            reboot_recommended: false,
            run_id: None,
            duration,
        }
    }
}

/// Cooperative stop signal handed to a running script.
///
/// Stopping has three honest tiers. A script that polls this between stages stops
/// promptly; a stress run hands the flag to `stress_runner::drive_blocking_cancellable`
/// so the load actually drops; and work already inside an uninterruptible call — a
/// running installer, an in-flight download — is allowed to finish while the queue
/// refuses to start anything further.
#[derive(Clone)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancelToken {
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
    }

    /// The raw flag, for handing to a driver that owns its own stop loop.
    pub fn as_flag(&self) -> Arc<AtomicBool> {
        self.flag.clone()
    }
}

/// A script in flight.
///
/// `done` carries exactly one [`ScriptOutcome`] and is then closed. A disconnect
/// without a message means the worker died, which a host must treat as a failure
/// rather than waiting out the timeout.
pub struct ScriptHandle {
    pub run_token: u64,
    pub done: Receiver<ScriptOutcome>,
    pub cancel: CancelToken,
}

/// Context a script runs against.
#[derive(Clone, Default)]
pub struct ScriptContext {
    pub service_number: Option<String>,
    pub customer_email: Option<String>,
    /// Session a run links itself to, when one is open.
    pub diagnostic_session_id: Option<String>,
    pub channels: ScriptChannels,
}

impl ScriptContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_service_number(mut self, sn: impl Into<String>) -> Self {
        self.service_number = Some(sn.into());
        self
    }

    pub fn with_customer_email(mut self, email: impl Into<String>) -> Self {
        self.customer_email = Some(email.into());
        self
    }

    pub fn with_diagnostic_session(mut self, id: impl Into<String>) -> Self {
        self.diagnostic_session_id = Some(id.into());
        self
    }

    pub fn log(&self, entry: ScriptLogEntry) {
        let _ = self.channels.log_tx.try_send(entry);
    }

    pub fn log_info(
        &self,
        category: ScriptCategory,
        script_name: &str,
        message: impl Into<String>,
    ) {
        self.log(ScriptLogEntry::info(category, script_name, message));
    }

    pub fn log_success(
        &self,
        category: ScriptCategory,
        script_name: &str,
        message: impl Into<String>,
    ) {
        self.log(ScriptLogEntry::success(category, script_name, message));
    }

    pub fn log_warning(
        &self,
        category: ScriptCategory,
        script_name: &str,
        message: impl Into<String>,
    ) {
        self.log(ScriptLogEntry::warning(category, script_name, message));
    }

    pub fn log_error(
        &self,
        category: ScriptCategory,
        script_name: &str,
        message: impl Into<String>,
    ) {
        self.log(ScriptLogEntry::error(category, script_name, message));
    }

    pub fn report_progress(&self, script_id: &str, current: u64, total: u64) {
        let _ = self
            .channels
            .progress_tx
            .try_send((script_id.to_string(), current, total));
    }
}

/// Starts scripts. One implementation covers a family, not a single entry.
pub trait ScriptExecutor: Send + Sync {
    /// Whether this executor runs `id`. Answering for a family is what lets one
    /// stress executor claim every stress id by asking the catalog, instead of
    /// registering an object per script.
    fn handles(&self, id: &ScriptId) -> bool;

    /// Starts the work and returns immediately. The outcome arrives on the handle.
    fn spawn(
        &self,
        def: &ScriptDef,
        ctx: &ScriptContext,
        run_token: u64,
        cancel: CancelToken,
    ) -> ScriptHandle;
}

/// The executors a host has registered.
#[derive(Default)]
pub struct ScriptExecutorRegistry {
    executors: Vec<Box<dyn ScriptExecutor>>,
}

impl ScriptExecutorRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, executor: Box<dyn ScriptExecutor>) {
        self.executors.push(executor);
    }

    pub fn find(&self, id: &ScriptId) -> Option<&dyn ScriptExecutor> {
        self.executors
            .iter()
            .find(|e| e.handles(id))
            .map(|e| e.as_ref())
    }

    pub fn is_empty(&self) -> bool {
        self.executors.is_empty()
    }

    pub fn len(&self) -> usize {
        self.executors.len()
    }

    /// Starts `def`, or reports the miss as a failed outcome so a host never has
    /// to special-case an unclaimed script.
    pub fn spawn(
        &self,
        def: &ScriptDef,
        ctx: &ScriptContext,
        run_token: u64,
        cancel: CancelToken,
    ) -> ScriptHandle {
        match self.find(&def.id) {
            Some(executor) => executor.spawn(def, ctx, run_token, cancel),
            None => {
                let (tx, done) = crossbeam::channel::bounded(1);
                let _ = tx.send(ScriptOutcome::plain(
                    def.id.clone(),
                    run_token,
                    ScriptResult::Error(format!("no executor claims '{}'", def.id)),
                    Duration::ZERO,
                ));
                ScriptHandle {
                    run_token,
                    done,
                    cancel,
                }
            }
        }
    }
}

#[cfg(test)]
mod executor_tests {
    use super::*;

    struct Claims(&'static str);

    impl ScriptExecutor for Claims {
        fn handles(&self, id: &ScriptId) -> bool {
            id.as_str().starts_with(self.0)
        }

        fn spawn(
            &self,
            def: &ScriptDef,
            _ctx: &ScriptContext,
            run_token: u64,
            cancel: CancelToken,
        ) -> ScriptHandle {
            let (tx, done) = crossbeam::channel::bounded(1);
            let _ = tx.send(ScriptOutcome::plain(
                def.id.clone(),
                run_token,
                ScriptResult::Success("ok".into()),
                Duration::ZERO,
            ));
            ScriptHandle {
                run_token,
                done,
                cancel,
            }
        }
    }

    fn def(id: &str) -> ScriptDef {
        crate::scripts::catalog::CATALOG
            .get(&ScriptId::new(id))
            .expect("catalog entry")
            .clone()
    }

    #[test]
    fn an_executor_claims_a_family_not_one_script() {
        let mut registry = ScriptExecutorRegistry::new();
        registry.register(Box::new(Claims("stress-")));

        assert!(registry.find(&ScriptId::new("stress-cpu")).is_some());
        assert!(registry.find(&ScriptId::new("stress-gpu-matmul")).is_some());
        assert!(registry.find(&ScriptId::new("activate-cps")).is_none());
    }

    /// An unclaimed script must report a failure, not leave a host waiting on a
    /// handle that never completes.
    #[test]
    fn an_unclaimed_script_fails_immediately() {
        let registry = ScriptExecutorRegistry::new();
        let handle = registry.spawn(
            &def("activate-cps"),
            &ScriptContext::new(),
            7,
            CancelToken::new(),
        );

        let outcome = handle.done.recv().expect("an outcome is always sent");
        assert_eq!(outcome.run_token, 7);
        assert!(outcome.result.is_failure());
    }

    #[test]
    fn the_outcome_carries_the_run_token_back() {
        let mut registry = ScriptExecutorRegistry::new();
        registry.register(Box::new(Claims("activate-")));
        let handle = registry.spawn(
            &def("activate-cps"),
            &ScriptContext::new(),
            42,
            CancelToken::new(),
        );

        let outcome = handle.done.recv().expect("outcome");
        assert_eq!(
            outcome.run_token, 42,
            "the queue entry must stay identifiable"
        );
        assert!(outcome.result.is_success());
    }

    #[test]
    fn a_cancel_token_is_shared_with_its_clones() {
        let token = CancelToken::new();
        let worker = token.clone();
        assert!(!worker.is_cancelled());
        token.cancel();
        assert!(
            worker.is_cancelled(),
            "a running script would never see the stop"
        );
        assert!(worker.as_flag().load(Ordering::Relaxed));
    }
}
