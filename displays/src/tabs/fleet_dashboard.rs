//! Warehouse fleet dashboard tab.
//!
//! Renders a read-only summary of the QC fleet: which machines are online,
//! their last heartbeat, CPU load, and last report time. Data is sourced
//! from the orchestrator REST API via a background poller spawned by
//! [`start_fleet_poller`] and drained per frame by [`SharedContext::drain_fleet_updates`].

use std::time::Duration;

use crossbeam::channel::Sender;
use eframe::egui::{Color32, Grid, Label, RichText, Ui};
use crate::app_state::{FleetAgentSummary, SharedContext};
use crate::{PlatformSpawner, Spawner};

/// Poll cadence for `/api/v1/qc/agents`. 15 s matches the cadence the
/// warehouse dashboard needs (fast enough to spot a machine going dark
/// within one HVAC-cycle, slow enough that 50 dashboards don't DoS axum).
const POLL_INTERVAL: Duration = Duration::from_secs(15);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

async fn fleet_sleep(duration: Duration) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        tokio::time::sleep(duration).await;
    }
    #[cfg(target_arch = "wasm32")]
    {
        use js_sys::Promise;
        use wasm_bindgen_futures::JsFuture;

        let ms = duration.as_millis().min(i32::MAX as u128) as f64;
        let promise = Promise::new(&mut |resolve, _reject| {
            if let Some(window) = web_sys::window() {
                let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                    &resolve,
                    ms as i32,
                );
            }
        });
        let _ = JsFuture::from(promise).await;
    }
}

fn build_fleet_client() -> Result<reqwest::Client, reqwest::Error> {
    let builder = reqwest::Client::builder();
    // `ClientBuilder::timeout` is not available in the wasm reqwest build.
    #[cfg(not(target_arch = "wasm32"))]
    let builder = builder.timeout(REQUEST_TIMEOUT);
    builder.build()
}

/// Spawn a background task that polls the orchestrator's agent list every
/// [`POLL_INTERVAL`] and writes the latest snapshot through `tx`.
///
/// The task lives for the process lifetime. Call this **once** per
/// orchestrator URL; SharedContext's `fleet_poller_running` flag is the
/// guard so the host can detect URL changes and re-spawn against the new
/// base. Old pollers keep running silently against dead URLs and just log
/// warnings — that's preferable to a complex shutdown channel for a feature
/// that's read-only and side-effect-free.
pub fn start_fleet_poller(base_url: String, tx: Sender<Vec<FleetAgentSummary>>) {
    if base_url.is_empty() {
        log::info!("[fleet_poller] no orchestrator URL configured — poller not started");
        return;
    }

    PlatformSpawner::spawn(async move {
        let client = match build_fleet_client() {
            Ok(c) => c,
            Err(e) => {
                log::error!("[fleet_poller] reqwest build failed: {e}");
                return;
            }
        };

        let url = format!("{base_url}/api/v1/qc/agents");
        log::info!("[fleet_poller] started polling {url} every {POLL_INTERVAL:?}");

        loop {
            #[cfg(target_arch = "wasm32")]
            {
                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        match resp.json::<Vec<FleetAgentSummary>>().await {
                            Ok(rows) => {
                                log::debug!(
                                    "[fleet_poller] {} agent row(s) from orchestrator",
                                    rows.len()
                                );
                                let _ = tx.try_send(rows);
                            }
                            Err(e) => log::warn!("[fleet_poller] decode failed: {e}"),
                        }
                    }
                    Ok(resp) => log::warn!("[fleet_poller] HTTP {} from {url}", resp.status()),
                    Err(e) => log::debug!("[fleet_poller] request error (will retry): {e}"),
                }
            }

            #[cfg(not(target_arch = "wasm32"))]
            {
                match tokio::time::timeout(REQUEST_TIMEOUT, client.get(&url).send()).await {
                    Ok(Ok(resp)) if resp.status().is_success() => {
                        match resp.json::<Vec<FleetAgentSummary>>().await {
                            Ok(rows) => {
                                log::debug!(
                                    "[fleet_poller] {} agent row(s) from orchestrator",
                                    rows.len()
                                );
                                let _ = tx.try_send(rows);
                            }
                            Err(e) => log::warn!("[fleet_poller] decode failed: {e}"),
                        }
                    }
                    Ok(Ok(resp)) => log::warn!("[fleet_poller] HTTP {} from {url}", resp.status()),
                    Ok(Err(e)) => log::debug!("[fleet_poller] request error (will retry): {e}"),
                    Err(_) => log::debug!("[fleet_poller] request timed out (will retry)"),
                }
            }

            fleet_sleep(POLL_INTERVAL).await;
        }
    });
}

impl SharedContext {
    /// Pull at most the most-recent fleet snapshot out of the bounded
    /// channel and into [`SharedContext::fleet_agents`]. Cheap — the
    /// channel is bounded(1), so this is one `try_recv` per frame.
    pub fn drain_fleet_updates(&mut self) {
        let mut latest = None;
        while let Ok(rows) = self.fleet_agents_rx.try_recv() {
            latest = Some(rows);
        }
        if let Some(rows) = latest {
            self.fleet_agents = Some(rows);
        }
    }

    /// Idempotently start the poller against `database::orchestrator_url()`.
    /// Safe to call every frame; `fleet_poller_running` is the no-op guard.
    pub fn ensure_fleet_poller(&mut self) {
        if self.fleet_poller_running {
            return;
        }
        let url = database::orchestrator_url().to_string();
        if url.is_empty() {
            self.fleet_poller_running = true;
            return;
        }
        start_fleet_poller(url, self.fleet_agents_tx.clone());
        self.fleet_poller_running = true;
    }
}

impl SharedContext {
    /// Render the "Fleet Dashboard" tab for warehouse employees.
    pub fn fleet_dashboard(&mut self, ui: &mut Ui) {
        ui.heading("Fleet Dashboard");
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Live QC machine status.  Use the Admin Console tab to connect to a specific machine.",
            )
            .weak(),
        );
        ui.separator();
        ui.add_space(8.0);

        if let Some(ref agents) = self.fleet_agents {
            if agents.is_empty() {
                ui.colored_label(Color32::YELLOW, "No agents registered with the orchestrator yet.");
                return;
            }
            Grid::new("fleet_dashboard_grid")
                .num_columns(5)
                .striped(true)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("Machine").strong());
                    ui.label(RichText::new("Version").strong());
                    ui.label(RichText::new("CPU %").strong());
                    ui.label(RichText::new("Last Heartbeat").strong());
                    ui.label(RichText::new("Last Report").strong());
                    ui.end_row();

                    for agent in agents {
                        let cpu_color = if agent.cpu_avg_pct > 90.0 {
                            Color32::LIGHT_RED
                        } else if agent.cpu_avg_pct > 60.0 {
                            Color32::YELLOW
                        } else {
                            Color32::LIGHT_GREEN
                        };

                        ui.add(Label::new(&agent.machine_id));
                        ui.add(Label::new(&agent.agent_version));
                        ui.colored_label(cpu_color, format!("{:.1}%", agent.cpu_avg_pct));
                        ui.add(Label::new(
                            RichText::new(&agent.last_heartbeat).monospace().small(),
                        ));
                        ui.add(Label::new(
                            RichText::new(
                                agent.last_report_at.as_deref().unwrap_or("—"),
                            )
                            .monospace()
                            .small(),
                        ));
                        ui.end_row();
                    }
                });
        } else {
            let url = database::orchestrator_url();
            if url.is_empty() {
                ui.colored_label(
                    Color32::GRAY,
                    "Fleet reporting disabled: `ORCHESTRATOR_URL` / `ORCHESTRATOR_URL_DEV` \
                     is empty in .env. Set it and rebuild to see live fleet data.",
                );
            } else {
                ui.colored_label(
                    Color32::GRAY,
                    format!("Waiting for first /api/v1/qc/agents response from {url} …"),
                );
            }
        }
    }
}
