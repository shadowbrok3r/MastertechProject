//! Script execution for this machine.
//!
//! The registry lives in `displays` because the contract is shared; the bodies
//! live here because every one of them is Windows-specific and reaches into
//! `crate::utilities`.
//!
//! Families move over one at a time. A script no executor claims still runs down
//! the original path in `tabs::scripts`, so the tab keeps working throughout.

use std::sync::OnceLock;

use displays::scripts::executor::ScriptExecutorRegistry;

pub mod informational;
pub mod junkware;
pub(crate) mod powershell;
pub mod stress;
pub mod tuneup;

static REGISTRY: OnceLock<ScriptExecutorRegistry> = OnceLock::new();

/// The registered executors, built once.
pub fn registry() -> &'static ScriptExecutorRegistry {
    REGISTRY.get_or_init(|| {
        let mut registry = ScriptExecutorRegistry::new();
        registry.register(Box::new(informational::InformationalExecutor));
        registry.register(Box::new(junkware::JunkwareExecutor));
        registry.register(Box::new(stress::StressExecutor));
        registry.register(Box::new(tuneup::TuneupExecutor));
        registry
    })
}
