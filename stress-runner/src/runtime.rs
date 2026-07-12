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

/// Block-on a future, returning its result.  Used sparingly — only for the
/// initial `StressTestRun::create` which must complete before we surface a
/// `run_id` to the UI.
pub(crate) fn block_on<F, T>(fut: F) -> T
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    if let Some(handle) = HOST_HANDLE.get() {
        // `Handle::block_on` panics if called from inside the runtime's own
        // thread, so we spawn a task and wait on it via a oneshot channel
        // instead.  Safe to call from any thread that isn't a tokio worker.
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        handle.spawn(async move {
            let result = fut.await;
            let _ = tx.send(result);
        });
        rx.recv()
            .expect("stress-runner: host runtime dropped the block_on task")
    } else {
        FALLBACK_RT.block_on(fut)
    }
}
