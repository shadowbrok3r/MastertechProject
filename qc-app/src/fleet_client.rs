//! Inbound fleet client.
//!
//! Companion to [`crate::reporting::ReportSink`], which handles the outbound
//! direction (heartbeats and reports posted *to* the orchestrator). This
//! module handles the inbound direction:
//!
//!   1. POSTs `/api/v1/qc/register` once on startup so the orchestrator
//!      knows this agent exists even before its first heartbeat.
//!   2. GETs `/api/v1/qc/agents/{machine_id}/commands` on a slow tick
//!      (default 5 s) and queues every pending command into the
//!      [`InboundCommandRx`] channel for the UI thread to dispatch.
//!   3. POSTs `/api/v1/qc/agents/{machine_id}/ack` once the UI thread
//!      tells us a command is done.
//!
//! The actual command handling is *not* in here. The UI thread owns the
//! report sink + stress runner; this module just delivers the bytes.

use std::sync::Arc;
use std::time::Duration;

use crossbeam::channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};

const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// Bounded queue so a stuck UI loop doesn't grow memory without bound.
const CHANNEL_CAP: usize = 64;

/// One command dequeued from the orchestrator. The wire shape is
/// `{"id": "...", "issued_at": "...", "kind": "send_report" | {"custom": {...}}, "status": "pending"}`
/// — externally tagged so a bare unit variant is just a string. The
/// `status` field is server-side bookkeeping we don't care about here.
#[derive(Debug, Clone, Deserialize)]
pub struct InboundCommand {
    /// Server-side id we must quote in the eventual ack.
    pub id: String,
    pub issued_at: String,
    pub kind: InboundCommandKind,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InboundCommandKind {
    /// Ask the agent to push a `QcReport` right now.
    SendReport,
    /// Free-form payload for forward-compat commands.
    Custom { payload: serde_json::Value },
}

/// What the UI thread sends back after handling (or refusing) a command.
pub enum CommandOutcome {
    /// Command handled — fire the ack.
    Acked { command_id: String },
}

/// Public handle. Cloneable; both ends share one channel pair.
#[derive(Clone)]
pub struct FleetClient {
    inbound_rx: Arc<Mutex<Receiver<InboundCommand>>>,
    outbound_tx: Sender<CommandOutcome>,
    pub machine_id: Arc<String>,
}

use std::sync::Mutex;

impl FleetClient {
    /// Spawn the background register/poll/ack loop. Returns immediately.
    ///
    /// `orchestrator_base_url` empty -> client is a no-op (matches the
    /// `ReportSink` "dry-run" convention so a fresh install with no URL
    /// configured still runs cleanly).
    pub fn start(orchestrator_base_url: Option<String>, machine_id: String) -> Self {
        let (in_tx, in_rx) = crossbeam::channel::bounded::<InboundCommand>(CHANNEL_CAP);
        let (out_tx, out_rx) = crossbeam::channel::bounded::<CommandOutcome>(CHANNEL_CAP);
        let base_url = orchestrator_base_url.unwrap_or_default();
        let machine_id = Arc::new(machine_id);

        if !base_url.is_empty() {
            tokio::spawn(register_and_poll(
                base_url.clone(),
                machine_id.clone(),
                in_tx,
                out_rx,
            ));
            log::info!(
                "[fleet_client] started for machine_id={} base={}",
                machine_id,
                base_url
            );
        } else {
            log::info!("[fleet_client] no orchestrator URL configured — running idle");
            // We still need to keep the receiver end alive so cloned handles
            // don't panic on send. Drain the outbound channel into the void.
            std::thread::spawn(move || {
                while out_rx.recv().is_ok() {
                    // No-op: nothing to ack against.
                }
            });
        }

        Self {
            inbound_rx: Arc::new(Mutex::new(in_rx)),
            outbound_tx: out_tx,
            machine_id,
        }
    }

    /// Drain any commands the poll loop has buffered since the last call.
    /// Returns at most `max` commands per call so a backlog can't stall the
    /// UI frame.
    pub fn drain_commands(&self, max: usize) -> Vec<InboundCommand> {
        let mut out = Vec::new();
        if let Ok(rx) = self.inbound_rx.lock() {
            for _ in 0..max {
                match rx.try_recv() {
                    Ok(cmd) => out.push(cmd),
                    Err(_) => break,
                }
            }
        }
        out
    }

    /// Acknowledge a command back to the orchestrator. Fire-and-forget; the
    /// background task does the HTTP POST.
    pub fn ack(&self, command_id: String) {
        if let Err(e) = self.outbound_tx.try_send(CommandOutcome::Acked { command_id }) {
            log::warn!("[fleet_client] ack channel full: {e}");
        }
    }
}

async fn register_and_poll(
    base_url: String,
    machine_id: Arc<String>,
    inbound_tx: Sender<InboundCommand>,
    outbound_rx: Receiver<CommandOutcome>,
) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build();
    let client = match client {
        Ok(c) => c,
        Err(e) => {
            log::error!("[fleet_client] reqwest build failed: {e}");
            return;
        }
    };

    // 1. Register. Best-effort — if the orchestrator is down at startup, we
    //    rely on heartbeat auto-registration to fill the gap.
    let register_url = format!("{base_url}/api/v1/qc/register");
    let body = serde_json::json!({
        "machine_id": machine_id.as_str(),
        "agent_version": env!("CARGO_PKG_VERSION"),
    });
    match client.post(&register_url).json(&body).send().await {
        Ok(resp) if resp.status().is_success() => {
            log::info!("[fleet_client] register OK ({})", resp.status());
        }
        Ok(resp) => {
            log::warn!(
                "[fleet_client] register -> HTTP {} (will rely on heartbeat auto-register)",
                resp.status()
            );
        }
        Err(e) => {
            log::warn!(
                "[fleet_client] register failed: {e} (will rely on heartbeat auto-register)"
            );
        }
    }

    // 2. Spawn ack worker (drains outbound channel into POST /ack).
    let ack_client = client.clone();
    let ack_base = base_url.clone();
    let ack_machine_id = machine_id.clone();
    tokio::spawn(async move {
        loop {
            // Bridge the sync channel to async with spawn_blocking.
            let recvd = tokio::task::spawn_blocking({
                let rx = outbound_rx.clone();
                move || rx.recv()
            })
            .await;
            let outcome = match recvd {
                Ok(Ok(o)) => o,
                _ => break, // Channel closed.
            };
            match outcome {
                CommandOutcome::Acked { command_id } => {
                    let url = format!(
                        "{ack_base}/api/v1/qc/agents/{}/ack",
                        ack_machine_id.as_str()
                    );
                    let body = serde_json::json!({ "command_id": command_id });
                    match ack_client.post(&url).json(&body).send().await {
                        Ok(resp) if resp.status().is_success() => {
                            log::debug!(
                                "[fleet_client] ack {command_id} -> {}",
                                resp.status()
                            );
                        }
                        Ok(resp) => log::warn!(
                            "[fleet_client] ack {command_id} -> HTTP {}",
                            resp.status()
                        ),
                        Err(e) => log::warn!("[fleet_client] ack {command_id} -> {e}"),
                    }
                }
            }
        }
        log::info!("[fleet_client] ack worker exiting");
    });

    // 3. Long-lived poll loop.
    let poll_url = format!(
        "{base_url}/api/v1/qc/agents/{}/commands",
        machine_id.as_str()
    );
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;
        match client.get(&poll_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let cmds: Vec<InboundCommand> = match resp.json().await {
                    Ok(v) => v,
                    Err(e) => {
                        log::warn!("[fleet_client] poll decode failed: {e}");
                        continue;
                    }
                };
                if cmds.is_empty() {
                    continue;
                }
                log::info!(
                    "[fleet_client] received {} pending command(s) from orchestrator",
                    cmds.len()
                );
                for c in cmds {
                    if inbound_tx.try_send(c).is_err() {
                        log::warn!(
                            "[fleet_client] inbound channel full; dropping a command \
                             (UI thread is not draining fast enough)"
                        );
                    }
                }
            }
            Ok(resp) if resp.status().as_u16() == 404 => {
                // Agent not registered yet. Re-register opportunistically.
                let _ = client
                    .post(&format!("{base_url}/api/v1/qc/register"))
                    .json(&serde_json::json!({
                        "machine_id": machine_id.as_str(),
                        "agent_version": env!("CARGO_PKG_VERSION"),
                    }))
                    .send()
                    .await;
            }
            Ok(resp) => {
                log::warn!("[fleet_client] poll -> HTTP {}", resp.status());
            }
            Err(e) => {
                log::debug!("[fleet_client] poll error (will retry): {e}");
            }
        }
    }
}
