//! What a script needs from the host process: a runtime, an HTTP client and a
//! way to report download progress.
//!
//! These are deliberately not on `ScriptContext`. That type is the contract every
//! surface shares and compiles for wasm; a `reqwest::Client` and a tokio handle
//! belong to this binary.

use std::sync::{LazyLock, OnceLock};

use crossbeam::channel::Sender;
use displays::scripts::executor::ScriptContext;
use displays::scripts::id::ScriptId;
use reqwest::Client;
use tokio::runtime::Handle;

static RUNTIME: OnceLock<Handle> = OnceLock::new();

/// Shared by every script that downloads, matching the one the tab holds.
static HTTP: LazyLock<Client> = LazyLock::new(Client::new);

/// Registers the host runtime so an executor can start async work from the
/// `std::thread` it runs on. Call once at startup from inside the runtime.
pub fn set_runtime_handle(handle: Handle) {
    let _ = RUNTIME.set(handle);
}

pub(crate) fn http() -> Client {
    HTTP.clone()
}

/// Starts `fut` on the host runtime, or reports that no runtime was registered
/// rather than dropping the future and leaving the queue waiting.
pub(crate) fn spawn_async<F>(fut: F) -> Result<(), &'static str>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    match RUNTIME.get() {
        Some(handle) => {
            handle.spawn(fut);
            Ok(())
        }
        None => Err("no tokio runtime is registered for script execution"),
    }
}

/// A `(current, total)` sink for the installers, forwarded onto the progress
/// channel every surface already reads. The thread ends when the installer drops
/// its sender.
pub(crate) fn progress_sink(ctx: &ScriptContext, id: &ScriptId) -> Sender<(u64, u64)> {
    let (tx, rx) = crossbeam::channel::unbounded::<(u64, u64)>();
    let ctx = ctx.clone();
    let id = id.as_str().to_string();

    std::thread::spawn(move || {
        while let Ok((current, total)) = rx.recv() {
            ctx.report_progress(&id, current, total);
        }
    });

    tx
}
