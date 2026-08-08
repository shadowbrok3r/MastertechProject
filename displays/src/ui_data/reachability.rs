//! Slice 5 of the connected-client refactor: per-admin TCP
//! reachability probing.
//!
//! ## Why this exists
//!
//! Slice 1 of the visibility filter (in [`crate::tabs::tasks::client_cards`])
//! gated the connected-clients list on a *proxy* signal: "the
//! client published `local_ip` + `tcp_port`." That's necessary
//! but not sufficient — a misconfigured firewall, a dead
//! listener thread, or the admin being on a different network
//! can all leave coords published but the client unreachable.
//! Operators kept hitting "TCP connect timed out (retrying…)"
//! storms on clients the list claimed were available.
//!
//! This module replaces the proxy with proof: a background task
//! that periodically tries to open a TCP connection to each
//! advertised endpoint and records the result. The filter then
//! uses the recorded result, so the list never shows a client
//! we couldn't actually reach within the last few minutes.
//!
//! ## Why **local** state and not a DB field
//!
//! Reachability is per-admin-network. An admin VPN'd into the
//! office reaches clients an admin at home doesn't. If the
//! prober wrote `tcp_reachable` to the shared SurrealDB row,
//! different admins would clobber each other's results
//! according to who probed last. So the result lives on
//! [`crate::app_state::SharedContext::reachability_cache`] —
//! every admin keeps their own snapshot.
//!
//! ## Probe shape
//!
//! We do a raw `TcpStream::connect` with a 2-second timeout and
//! immediately drop the socket. Successful connect means the
//! listener is bound and accepting, which is what the admin
//! handshake needs in [`crate::tabs::admin_console::client_interface::admin_transport`].
//! We deliberately do *not* send the handshake bytes — that
//! would make the listener think a real admin is connecting and
//! mess with its session bookkeeping. A bare connect-and-close
//! is enough to prove reachability.

use std::time::Duration;

/// How long we wait for a TCP connect before declaring the
/// endpoint unreachable. Short enough that one slow client
/// doesn't hold up the whole probe round; long enough to absorb
/// a sluggish network.
#[cfg(not(target_arch = "wasm32"))]
const PROBE_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// How often we run a full probe round. Steady-state cadence —
/// every connected client is re-probed at least this often even
/// if it was already known reachable.
#[cfg(not(target_arch = "wasm32"))]
const PROBE_INTERVAL: Duration = Duration::from_secs(30);

/// Reachability snapshot for one client. Held in
/// [`crate::app_state::SharedContext::reachability_cache`] keyed
/// by `connection_string`.
#[derive(Debug, Clone)]
pub struct ReachabilityStatus {
    /// `true` when the most recent probe succeeded.
    pub reachable: bool,
    /// When the most recent probe completed. `Instant` rather
    /// than `DateTime<Utc>` because we only care about wall-time
    /// freshness for the UI filter — comparing across machines
    /// or sessions doesn't make sense for per-admin local state.
    pub last_probed_at: web_time::Instant,
    /// Set when `reachable == false`; carries the OS error
    /// message so the operator can see *why* it's unreachable
    /// (firewall vs. dead listener vs. wrong address).
    pub error: Option<String>,
    /// The endpoint we actually probed. Stored so the UI can
    /// surface it next to the row's status badge — operators
    /// otherwise have to dig through the client details grid to
    /// know whether the published address makes sense (e.g.
    /// `192.168.x.x` when they expected `10.x.x.x`).
    pub probed_endpoint: String,
}

/// Wire shape that the prober pushes on
/// `reachability_tx`. Drained by `receive_shared_ui` into
/// `reachability_cache`.
#[derive(Debug, Clone)]
pub struct ReachabilityEvent {
    pub connection_string: String,
    pub status: ReachabilityStatus,
}

/// Maximum age of a successful probe for a client to count as
/// reachable in the visibility filter. Set deliberately wider
/// than [`PROBE_INTERVAL`] so a single missed cycle (transient
/// network blip, prober just woke up) doesn't blink the client
/// out of the list. 10 minutes ≈ 20 missed cycles, which is
/// long enough that anything within that window is "still
/// probably reachable" — but short enough that a machine that's
/// genuinely gone (e.g. unplugged) disappears within a normal
/// shift.
const REACHABILITY_FRESHNESS_SECS: u64 = 600;

/// Returns `true` when the cached probe result for this
/// connection_string says reachable *and* the probe is recent
/// enough to trust. Absent entries (never probed) and stale
/// entries both return `false` — strict mode, matches the
/// user's "100% certainty" requirement.
pub fn is_tcp_reachable(
    cache: &std::collections::HashMap<String, ReachabilityStatus>,
    connection_string: &str,
) -> bool {
    let Some(entry) = cache.get(connection_string) else {
        return false;
    };
    if !entry.reachable {
        return false;
    }
    entry.last_probed_at.elapsed() < Duration::from_secs(REACHABILITY_FRESHNESS_SECS)
}

/// Count how many connected clients with advertised TCP coords
/// have *not yet* been probed (no entry in the cache at all).
/// Used by the My Tasks / Admin Console empty-state copy so the
/// operator sees "Probing 3 clients…" instead of a confusing
/// silent empty list at startup.
pub fn pending_probe_count(
    cache: &std::collections::HashMap<String, ReachabilityStatus>,
    clients: &[database::schema::ConnectedClient],
) -> usize {
    clients
        .iter()
        .filter(|c| {
            c.local_ip
                .as_deref()
                .is_some_and(|ip| !ip.is_empty())
                && c.tcp_port.is_some()
                && !cache.contains_key(&c.connection_string)
        })
        .count()
}

/// Spawn the per-admin reachability prober. Runs forever
/// (until `wait_for_shutdown` fires) on the tokio runtime,
/// probing every connected client with advertised TCP coords
/// once every [`PROBE_INTERVAL`].
///
/// Idempotent in design — calling this twice will start two
/// probers and the second one's writes will just race with the
/// first to the same cache, which is harmless (last write
/// wins, both writes are correct). Callers should still only
/// spawn it once, hence the `prober_spawned` guard on
/// `SharedContext`.
///
/// The prober uses `database::db` directly to fetch the
/// live client list each round — that's the source of truth
/// and avoids racing with the UI thread's in-memory snapshot.
///
/// Not compiled for WASM — browsers have no raw TCP socket API.
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_prober(
    tx: crossbeam::channel::Sender<ReachabilityEvent>,
    clients: std::sync::Arc<std::sync::Mutex<Vec<database::schema::ConnectedClient>>>,
) {
    use crate::{PlatformSpawner, Spawner};

    PlatformSpawner::spawn(async move {
        // Holds the last round's error text so an unchanging failure warns once.
        let mut last_err: Option<String> = None;
        loop {
            match run_probe_round(&tx, &clients).await {
                Ok(()) => last_err = None,
                Err(e) => {
                    let msg = format!("{e:?}");
                    if last_err.as_deref() == Some(msg.as_str()) {
                        log::debug!("reachability prober: round failed: {msg}");
                    } else {
                        log::warn!("reachability prober: round failed: {msg}");
                        last_err = Some(msg);
                    }
                }
            }

            // Sleep with a shutdown-aware wait: as soon as the
            // global shutdown signal fires we bail out, so the
            // tokio runtime drop in eframe-exit isn't held up
            // waiting on a 30-second sleep.
            tokio::select! {
                _ = crate::wait_for_shutdown() => {
                    log::debug!("reachability prober -> shutdown signaled; exiting");
                    return;
                }
                _ = tokio::time::sleep(PROBE_INTERVAL) => {}
            }
        }
    });
}

/// One pass: snapshot the current client list, fan probes out to
/// every eligible row, ship results back through `tx`.
///
/// Not compiled for WASM — raw TCP connections aren't available in browsers.
#[cfg(not(target_arch = "wasm32"))]
async fn run_probe_round(
    tx: &crossbeam::channel::Sender<ReachabilityEvent>,
    clients_arc: &std::sync::Arc<std::sync::Mutex<Vec<database::schema::ConnectedClient>>>,
) -> anyhow::Result<()> {
    // Snapshot under lock then release before any await — we never
    // hold the mutex across an await point.
    let clients: Vec<database::schema::ConnectedClient> = match clients_arc.lock() {
        Ok(guard) => guard
            .iter()
            .filter(|c| c.connected)
            .cloned()
            .collect(),
        Err(e) => {
            // Poisoning is permanent; warn on the first round only.
            static POISON_WARNED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if POISON_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                log::debug!("reachability prober: clients mutex poisoned ({e}); skipping round");
            } else {
                log::warn!("reachability prober: clients mutex poisoned ({e}); skipping round");
            }
            return Ok(());
        }
    };

    let mut handles = Vec::new();
    for client in clients {
        let Some(ip) = client.local_ip.clone().filter(|s| !s.is_empty()) else {
            continue;
        };
        let Some(port) = client.tcp_port else {
            continue;
        };
        let endpoint = format!("{ip}:{port}");
        let connection_string = client.connection_string.clone();
        let tx_inner = tx.clone();

        // Spawn each probe concurrently. TCP connects are cheap;
        // a fleet of 100 clients turns into 100 small tasks
        // each waiting up to 2 seconds. Bounded total wall-time
        // ≈ PROBE_CONNECT_TIMEOUT regardless of fleet size.
        handles.push(tokio::spawn(async move {
            let status = probe_endpoint(&endpoint).await;
            let _ = tx_inner.try_send(ReachabilityEvent {
                connection_string,
                status,
            });
        }));
    }

    // Wait for the whole round to finish so PROBE_INTERVAL is
    // measured *between rounds* rather than between starts —
    // avoids stacking rounds on top of each other if probes are
    // slow.
    for h in handles {
        h.await?;
    }
    Ok(())
}

/// Attempt one TCP connect to `endpoint` with the standard
/// [`PROBE_CONNECT_TIMEOUT`]. Always returns a
/// [`ReachabilityStatus`] — failures are converted to
/// `reachable: false` with the OS error in `error`, so the cache
/// gets updated even on the negative path.
///
/// Not compiled for WASM — `tokio::net::TcpStream` is unavailable there.
#[cfg(not(target_arch = "wasm32"))]
async fn probe_endpoint(endpoint: &str) -> ReachabilityStatus {
    let now = web_time::Instant::now();
    let result = tokio::time::timeout(
        PROBE_CONNECT_TIMEOUT,
        tokio::net::TcpStream::connect(endpoint),
    )
    .await;
    match result {
        Ok(Ok(stream)) => {
            // Drop the stream immediately. We deliberately don't
            // send anything down it — see module-level doc.
            drop(stream);
            ReachabilityStatus {
                reachable: true,
                last_probed_at: now,
                error: None,
                probed_endpoint: endpoint.to_string(),
            }
        }
        Ok(Err(e)) => ReachabilityStatus {
            reachable: false,
            last_probed_at: now,
            error: Some(e.to_string()),
            probed_endpoint: endpoint.to_string(),
        },
        Err(_) => ReachabilityStatus {
            reachable: false,
            last_probed_at: now,
            error: Some(format!(
                "connect timeout after {}s",
                PROBE_CONNECT_TIMEOUT.as_secs()
            )),
            probed_endpoint: endpoint.to_string(),
        },
    }
}
