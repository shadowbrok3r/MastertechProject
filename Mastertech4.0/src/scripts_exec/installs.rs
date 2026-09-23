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
const HANDLED: &[&str] = &[
    "activate-cps",
    "activate-webroot",
    "activate-superanti",
    "activate-seb",
    "install-libreoffice",
];

/// Offered remotely only; the tab activates both products through Activate CPS.
#[cfg(test)]
const REMOTE_ONLY: &[&str] = &["activate-webroot", "activate-superanti"];

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
                let (result, reboot_recommended) = run(&def, &ctx).await;
                let mut outcome = ScriptOutcome::plain(id, run_token, result, started.elapsed());
                outcome.reboot_recommended = reboot_recommended;
                let _ = tx.send(outcome);
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
async fn run(def: &ScriptDef, ctx: &ScriptContext) -> (ScriptResult, bool) {
    match def.id.as_str() {
        "activate-cps" => activate_cps(ctx, def).await,
        "activate-webroot" => activate_webroot(ctx, def).await,
        "activate-superanti" => (activate_superanti(ctx, def).await, false),
        "activate-seb" => (activate_seb(ctx, def).await, false),
        "install-libreoffice" => (install_libreoffice(ctx, def).await, false),
        other => (
            ScriptResult::Error(format!("install executor does not run '{other}'")),
            false,
        ),
    }
}

/// One key fetch feeds both products, which is why this is not the sequence of
/// `activate-webroot` and `activate-superanti` that each fetch their own.
#[cfg(target_os = "windows")]
async fn activate_cps(ctx: &ScriptContext, def: &ScriptDef) -> (ScriptResult, bool) {
    use crate::tabs::tur_sheet::get_ticket::SendRequest;
    use crate::utilities::scripts::{install_sas, install_webroot};

    let (category, name) = (def.category(), def.name.as_str());

    let Some(service_number) = ctx.service_number.clone().filter(|s| !s.is_empty()) else {
        let msg = "Service number required for CPS activation";
        ctx.log_warning(category, name, msg);
        return (ScriptResult::Skipped(msg.into()), false);
    };

    kill_sas_processes(ctx, def).await;

    ctx.log_info(category.clone(), name, "Fetching CPS keys...");

    let keys = match SendRequest::get_cps(service_number, env::http()).await {
        Ok(keys) if !keys.is_empty() => keys,
        Ok(_) => {
            let msg = "No CPS keys found for this service order";
            ctx.log_error(category, name, msg);
            return (ScriptResult::Error(msg.into()), false);
        }
        Err(e) => {
            let msg = format!("Failed to fetch keys: {e}");
            ctx.log_error(category, name, msg.clone());
            return (ScriptResult::Error(msg), false);
        }
    };

    let key = keys.first().cloned().unwrap_or_default();
    let mut reboot_recommended = false;
    let mut failed = Vec::new();

    ctx.log_info(category.clone(), name, "Installing Webroot...");
    match install_webroot(
        key.webroot_key.clone(),
        env::http(),
        env::progress_sink(ctx, &def.id),
    )
    .await
    {
        Ok(outcome) => {
            reboot_recommended = outcome.reboot_recommended();
            ctx.log_success(
                category.clone(),
                name,
                format!("Webroot licensed and active ({outcome})"),
            );
        }
        Err(e) => {
            failed.push("Webroot");
            ctx.log_error(
                category.clone(),
                name,
                format!("Webroot install failed: {e}"),
            );
        }
    }

    ctx.log_info(category.clone(), name, "Installing SuperAntiSpyware...");
    match install_sas(
        key.superanti_key,
        env::http(),
        env::progress_sink(ctx, &def.id),
    )
    .await
    {
        Ok(proof) => {
            ctx.log_success(
                category.clone(),
                name,
                format!("SuperAntiSpyware installed and activated: {proof}"),
            );
        }
        Err(e) => {
            failed.push("SuperAntiSpyware");
            ctx.log_error(category.clone(), name, format!("SAS install failed: {e}"));
        }
    }

    if reboot_recommended {
        // The marker is the only reboot channel the wire has, so it is still
        // logged even though the outcome now carries the same fact.
        ctx.log_success(
            category.clone(),
            name,
            format!(
                "{} Webroot was re-keyed over an existing install — reboot to finalize activation",
                displays::scripts::REBOOT_RECOMMENDED_MARKER
            ),
        );
    }

    if failed.is_empty() {
        (
            ScriptResult::Success("Webroot and SuperAntiSpyware licensed".into()),
            reboot_recommended,
        )
    } else {
        let msg = format!("{} install failed", failed.join(" and "));
        (ScriptResult::Error(msg), reboot_recommended)
    }
}

/// The first CPS key pair for the service order, or the failure already logged.
#[cfg(target_os = "windows")]
async fn fetch_cps_key(
    ctx: &ScriptContext,
    def: &ScriptDef,
    service_number: String,
) -> Result<database::schema::GetKeysResponse, ScriptResult> {
    use crate::tabs::tur_sheet::get_ticket::SendRequest;

    let (category, name) = (def.category(), def.name.as_str());
    ctx.log_info(category.clone(), name, "Fetching CPS keys...");
    match SendRequest::get_cps(service_number, env::http()).await {
        Ok(keys) => match keys.into_iter().next() {
            Some(key) => Ok(key),
            None => {
                let msg = "No CPS keys found for this service order";
                ctx.log_error(category, name, msg);
                Err(ScriptResult::Error(msg.into()))
            }
        },
        Err(e) => {
            let msg = format!("Failed to get CPS keys: {e}");
            ctx.log_error(category, name, msg.clone());
            Err(ScriptResult::Error(msg))
        }
    }
}

/// Installs and licenses Webroot alone, reporting whether a reboot finalizes it.
#[cfg(target_os = "windows")]
async fn activate_webroot(ctx: &ScriptContext, def: &ScriptDef) -> (ScriptResult, bool) {
    use crate::utilities::scripts::install_webroot;

    let (category, name) = (def.category(), def.name.as_str());
    let Some(service_number) = ctx.service_number.clone().filter(|s| !s.is_empty()) else {
        let msg = "Webroot activation requires SO number";
        ctx.log_warning(category, name, msg);
        return (ScriptResult::Skipped(msg.into()), false);
    };
    let key = match fetch_cps_key(ctx, def, service_number).await {
        Ok(key) => key,
        Err(result) => return (result, false),
    };
    ctx.log_info(
        category.clone(),
        name,
        format!("Webroot key: {}", key.webroot_key),
    );

    match install_webroot(
        key.webroot_key,
        env::http(),
        env::progress_sink(ctx, &def.id),
    )
    .await
    {
        Ok(outcome) => {
            let msg = format!("Webroot licensed and active ({outcome})");
            ctx.log_success(category, name, msg.clone());
            (ScriptResult::Success(msg), outcome.reboot_recommended())
        }
        Err(e) => {
            let msg = format!("Webroot install error: {e}");
            ctx.log_error(category, name, msg.clone());
            (ScriptResult::Error(msg), false)
        }
    }
}

/// Installs and activates SuperAntiSpyware alone.
#[cfg(target_os = "windows")]
async fn activate_superanti(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    use crate::utilities::scripts::antivirus::kill_sas_processes;
    use crate::utilities::scripts::install_sas;

    let (category, name) = (def.category(), def.name.as_str());
    let Some(service_number) = ctx.service_number.clone().filter(|s| !s.is_empty()) else {
        let msg = "SuperAnti activation requires SO number";
        ctx.log_warning(category, name, msg);
        return ScriptResult::Skipped(msg.into());
    };

    let killed = tokio::task::spawn_blocking(kill_sas_processes)
        .await
        .unwrap_or(0);
    ctx.log_info(
        category.clone(),
        name,
        format!("Killed {killed} SAS processes"),
    );

    let key = match fetch_cps_key(ctx, def, service_number).await {
        Ok(key) => key,
        Err(result) => return result,
    };
    ctx.log_info(
        category.clone(),
        name,
        format!("SuperAnti key: {}", key.superanti_key),
    );

    match install_sas(
        key.superanti_key,
        env::http(),
        env::progress_sink(ctx, &def.id),
    )
    .await
    {
        Ok(proof) => {
            let msg = format!("SAS installed and activated: {proof}");
            ctx.log_success(category, name, msg.clone());
            ScriptResult::Success(msg)
        }
        Err(e) => {
            let msg = format!("SAS install error: {e}");
            ctx.log_error(category, name, msg.clone());
            ScriptResult::Error(msg)
        }
    }
}

/// Clears SAS before the install so the running tray app does not hold its own
/// files. Enumerating processes and `taskkill` both block, so they run off the
/// runtime.
#[cfg(target_os = "windows")]
async fn kill_sas_processes(ctx: &ScriptContext, def: &ScriptDef) {
    use crate::utilities::scripts::get_running_processes;

    let ctx = ctx.clone();
    let category = def.category();
    let name = def.name.to_string();

    let _ = tokio::task::spawn_blocking(move || {
        let Ok(processes) = get_running_processes() else {
            return;
        };
        for process in processes {
            let process_name = process.process_name.to_lowercase();
            let exe_path = process.exe_path.clone().unwrap_or_default().to_lowercase();
            if process_name.contains("sascore")
                || exe_path.contains("superanti")
                || process_name.contains("superanti")
            {
                ctx.log_info(
                    category.clone(),
                    &name,
                    format!("Killing SAS process (PID: {})", process.id),
                );
                let _ = std::process::Command::new("taskkill")
                    .args(["/PID", &format!("{}", process.id), "/F"])
                    .output();
            }
        }
    })
    .await;
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
async fn run(def: &ScriptDef, ctx: &ScriptContext) -> (ScriptResult, bool) {
    let msg = "Only available on Windows";
    ctx.log_warning(def.category(), def.name.as_str(), msg);
    (ScriptResult::Skipped(msg.into()), false)
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
            if REMOTE_ONLY.contains(id) {
                assert!(def.offered_on(Surface::Remote), "{id} is offered nowhere");
            } else {
                assert!(
                    def.offered_on(Surface::Egui),
                    "{id} is claimed but never offered in the tab"
                );
            }
        }
    }

    /// Activate CPS fetches one key pair and installs both products from it. If
    /// it ever declares those two as children, a composite would run it as two
    /// scripts that each fetch their own keys.
    #[test]
    fn activate_cps_is_not_declared_as_a_composite() {
        let def = CATALOG
            .get(&ScriptId::new("activate-cps"))
            .expect("catalog entry");
        assert!(
            def.runs.is_empty(),
            "activate-cps declares children, which would double the key fetch"
        );
    }

    /// Its two products stay in the catalog for the remote path, which does run
    /// them as separate scripts.
    #[test]
    fn the_two_products_are_still_their_own_entries() {
        for id in ["activate-webroot", "activate-superanti"] {
            let def = CATALOG.get(&ScriptId::new(id)).expect("catalog entry");
            assert!(!def.offered_on(Surface::Egui), "{id} is offered in the tab");
        }
    }

    #[test]
    fn data_transfer_is_still_unclaimed() {
        let executor = InstallExecutor;
        assert!(!executor.handles(&ScriptId::new("data-transfer")));
    }
}
