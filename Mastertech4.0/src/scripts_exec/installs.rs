//! Installs that download from the network.
//!
//! These run on the host runtime rather than a thread of their own, and report
//! completion when the install returns. The original path fired them off with
//! `tokio::spawn` and never learned the result, so the queue moved on while the
//! installer was still running.
//!
//! None of them are interruptible: Stop leaves the running install alone and
//! starts nothing after it.

use std::time::Instant;

use displays::scripts::catalog::ScriptDef;
use displays::scripts::executor::{
    CancelToken, ScriptContext, ScriptExecutor, ScriptHandle, ScriptOutcome, ScriptResult,
};
use displays::scripts::id::ScriptId;

use super::env;

const LIBREOFFICE_URL: &str = "https://ninite.com/libreoffice/ninite.exe";

/// Ids this executor claims.
const HANDLED: &[&str] = &["activate-seb", "install-libreoffice"];

pub struct InstallExecutor;

impl ScriptExecutor for InstallExecutor {
    fn handles(&self, id: &ScriptId) -> bool {
        HANDLED.contains(&id.as_str())
    }

    fn spawn(
        &self,
        def: &ScriptDef,
        ctx: &ScriptContext,
        run_token: u64,
        cancel: CancelToken,
    ) -> ScriptHandle {
        let (tx, done) = crossbeam::channel::bounded(1);
        let id = def.id.clone();
        let def = def.clone();
        let ctx = ctx.clone();
        let started = Instant::now();

        let spawned = env::spawn_async({
            let tx = tx.clone();
            let id = id.clone();
            async move {
                let result = run(&def, &ctx).await;
                let _ = tx.send(ScriptOutcome::plain(
                    id,
                    run_token,
                    result,
                    started.elapsed(),
                ));
            }
        });

        if let Err(e) = spawned {
            let _ = tx.send(ScriptOutcome::plain(
                id,
                run_token,
                ScriptResult::Error(e.into()),
                started.elapsed(),
            ));
        }

        ScriptHandle {
            run_token,
            done,
            cancel,
        }
    }
}

#[cfg(target_os = "windows")]
async fn run(def: &ScriptDef, ctx: &ScriptContext) -> ScriptResult {
    match def.id.as_str() {
        "activate-seb" => activate_seb(ctx, def).await,
        "install-libreoffice" => install_libreoffice(ctx, def).await,
        other => ScriptResult::Error(format!("install executor does not run '{other}'")),
    }
}

#[cfg(target_os = "windows")]
async fn activate_seb(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    use crate::utilities::scripts::install_supereasybackup;

    let (category, name) = (def.category(), def.name.as_str());

    let Some(email) = ctx.customer_email.clone().filter(|e| !e.is_empty()) else {
        let msg = "Customer email required for SEB activation";
        ctx.log_warning(category, name, msg);
        return ScriptResult::Skipped(msg.into());
    };

    ctx.log_info(
        category.clone(),
        name,
        format!("Installing SuperEasyBackup for {email}..."),
    );

    match install_supereasybackup(email, env::http(), env::progress_sink(ctx, &def.id)).await {
        Ok(_) => {
            ctx.log_success(category, name, "SuperEasyBackup installed successfully");
            ScriptResult::Success("SuperEasyBackup installed successfully".into())
        }
        Err(e) => {
            let msg = format!("SEB install failed: {e}");
            ctx.log_error(category, name, msg.clone());
            ScriptResult::Error(msg)
        }
    }
}

#[cfg(target_os = "windows")]
async fn install_libreoffice(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    use crate::utilities::scripts::install_program;

    let (category, name) = (def.category(), def.name.as_str());
    ctx.log_info(
        category.clone(),
        name,
        "Downloading LibreOffice via Ninite...",
    );

    match install_program(
        LIBREOFFICE_URL.to_string(),
        env::http(),
        env::progress_sink(ctx, &def.id),
    )
    .await
    {
        Ok(_) => {
            ctx.log_success(category, name, "LibreOffice installed successfully");
            ScriptResult::Success("LibreOffice installed successfully".into())
        }
        Err(e) => {
            let msg = format!("LibreOffice install failed: {e}");
            ctx.log_error(category, name, msg.clone());
            ScriptResult::Error(msg)
        }
    }
}

#[cfg(not(target_os = "windows"))]
async fn run(def: &ScriptDef, ctx: &ScriptContext) -> ScriptResult {
    let msg = "Only available on Windows";
    ctx.log_warning(def.category(), def.name.as_str(), msg);
    ScriptResult::Skipped(msg.into())
}

#[cfg(test)]
mod install_executor_tests {
    use super::*;
    use displays::scripts::catalog::{CATALOG, Surface};

    #[test]
    fn every_claimed_id_is_real_and_offered() {
        for id in HANDLED {
            let def = CATALOG
                .get(&ScriptId::new(*id))
                .unwrap_or_else(|| panic!("{id} is not in the catalog"));
            assert!(
                def.offered_on(Surface::Egui),
                "{id} is claimed but never offered in the tab"
            );
        }
    }

    /// Activate CPS fetches one key pair and installs two products from it, so
    /// it is not two of these back to back.
    #[test]
    fn activate_cps_is_still_unclaimed() {
        let executor = InstallExecutor;
        assert!(!executor.handles(&ScriptId::new("activate-cps")));
        assert!(!executor.handles(&ScriptId::new("data-transfer")));
    }
}
