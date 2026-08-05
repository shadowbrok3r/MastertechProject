//! Tokio runtime resolution for background DB writes.
//!
//! `RunController`'s worker thread is `std::thread`-spawned (not a tokio
//! context), so it can't `tokio::spawn` directly.  At the same time, the
//! SurrealDB connection in `database::db` is opened on **whichever
//! runtime called `init_database()` first** — typically the host app's
//! `#[tokio::main]` runtime.  Async futures driven on a *different* runtime
//! often hang because their reactor lives elsewhere.
//!
//! Resolution strategy:
//! 1. If the host app has called [`set_runtime_handle`] (recommended), use
//!    that handle so DB futures run on the same runtime that owns the WS
//!    connection.
//! 2. Otherwise fall back to a dedicated `stress-runner` runtime.  This will
//!    still work for any DB driver whose internal state is runtime-agnostic
//!    (most are, via channels), but it's less robust than option 1.

use once_cell::sync::{Lazy, OnceCell};
use tokio::runtime::{Handle, Runtime};

/// Optional host-provided runtime handle.  When `Some`, all DB work is
/// scheduled on this handle.
static HOST_HANDLE: OnceCell<Handle> = OnceCell::new();

/// Fallback runtime used when the host hasn't supplied a handle.  Created
/// lazily on first use.
static FALLBACK_RT: Lazy<Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("stress-runner-db")
        .enable_all()
        .build()
        .expect("stress-runner: failed to build fallback tokio runtime")
});

/// Register the host app's tokio runtime so stress-runner's DB writes
/// execute on the **same** runtime that owns the SurrealDB connection.
/// Call this once at startup from inside your `#[tokio::main]` async fn,
/// passing `tokio::runtime::Handle::current()`.
///
/// Calling more than once is a no-op (only the first handle wins).  Calling
/// before `init_database` is fine — the handle is only used at run time.
pub fn set_runtime_handle(handle: Handle) {
    let _ = HOST_HANDLE.set(handle);
}

/// Spawn a future on the resolved runtime.  Returns immediately.
pub(crate) fn spawn<F>(fut: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    if let Some(handle) = HOST_HANDLE.get() {
        handle.spawn(fut);
    } else {
        FALLBACK_RT.spawn(fut);
    }
}

/// Ceiling on any single blocking DB call; a wedged socket surfaces as an
/// `Err` instead of parking the worker thread forever.
pub(crate) const DB_CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Block-on a fallible DB future, bounded by [`DB_CALL_TIMEOUT`].  Returns the
/// future's own result, or an `Err` when it doesn't resolve in time (lost
/// websocket response, zombie socket) or the host runtime drops the task.
pub(crate) fn block_on<F, T>(fut: F) -> anyhow::Result<T>
where
    F: std::future::Future<Output = anyhow::Result<T>> + Send + 'static,
    T: Send + 'static,
{
    if let Some(handle) = HOST_HANDLE.get() {
        // `Handle::block_on` panics if called from inside the runtime's own
        // thread, so we spawn a task and wait on it via a channel instead.
        // Safe to call from any thread that isn't a tokio worker.
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        handle.spawn(async move {
            let result = fut.await;
            let _ = tx.send(result);
        });
        match rx.recv_timeout(DB_CALL_TIMEOUT) {
            Ok(result) => result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(anyhow::anyhow!(
                "DB call did not complete within {}s (websocket response lost or socket wedged)",
                DB_CALL_TIMEOUT.as_secs()
            )),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(anyhow::anyhow!(
                "host runtime dropped the DB call task (runtime shutting down)"
            )),
        }
    } else {
        FALLBACK_RT.block_on(async {
            match tokio::time::timeout(DB_CALL_TIMEOUT, fut).await {
                Ok(result) => result,
                Err(_) => Err(anyhow::anyhow!(
                    "DB call did not complete within {}s",
                    DB_CALL_TIMEOUT.as_secs()
                )),
            }
        })
    }
}
