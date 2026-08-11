//! Direct pre-boot link (console side).
//!
//! The firmware app dials this console's TCP listener (endpoint advertised to
//! axum so a box can discover it), sends a HELLO with its serial, then streams
//! frames. The console writes input, stream-control, and plugin-run frames back
//! over the same socket. This is the low-latency path that bypasses the HTTP
//! relay; the relay stays the always-on registrar and the fallback.
//!
//! Wire framing is `tcp_protocol`'s length-prefixed `[u32 total_len][tag][body]`
//! with the `preboot` tag set. Native-only (no listener on wasm).

#![cfg(not(target_arch = "wasm32"))]

use std::collections::HashMap;
use std::sync::Arc;

use tcp_protocol::preboot::{
    self, PbBusy, PbPluginResult, PbPluginRun, PbQuery, PbQueryResult, PbStreamCtl, PreBootEvent,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Mutex, mpsc};

use crate::{PlatformSpawner, Spawner};

/// Default direct-link listen port on the console.
pub const DIRECT_PORT: u16 = 9209;

/// Relay port assumed when the configured base url carries none (axum's default).
const DEFAULT_RELAY_PORT: u16 = 8082;

/// Keepalive cadence toward the box; shorter than the firmware's own so either
/// side notices a dead peer within a few seconds.
const PING_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);

/// Lock attempts before a session read/send gives up.
const LOCK_RETRIES: usize = 64;

/// Consecutive `accept` failures tolerated before the listener rebinds.
const ACCEPT_ERROR_LIMIT: u32 = 10;

/// Pause between failed accepts, so a persistent fault cannot spin the runtime.
const ACCEPT_ERROR_BACKOFF: std::time::Duration = std::time::Duration::from_millis(200);

/// Per-session shared state, updated by the reader task and read by egui.
struct Session {
    /// Latest decoded frame bytes (bincode `PreBootFrame`).
    frame: Option<Vec<u8>>,
    frame_seq: u64,
    /// Outbound frame sender to this session's writer task.
    tx: mpsc::UnboundedSender<Vec<u8>>,
    /// Most recent plugin result pushed by the firmware.
    plugin_result: Option<PbPluginResult>,
    /// Most recent state-query answer pushed by the firmware.
    query_result: Option<PbQueryResult>,
    last_seen: std::time::Instant,
    peer: String,
}

/// A connected direct-link box, surfaced to the roster.
pub struct DirectAgent {
    pub serial: String,
    pub idle_secs: u64,
    pub peer: String,
}

/// Shared registry of direct sessions, keyed by firmware serial (HELLO body).
/// A box's last "entering/leaving a child image" report, kept keyed by serial.
///
/// This deliberately outlives the session: the box goes silent *because* the
/// child blocked its polled network stack, so the socket dies mid-run and the
/// session is torn down. Without a record that survives that, a machine part-way
/// through a firmware flash is indistinguishable from one that was unplugged.
#[derive(Clone, Debug)]
pub struct BusyRecord {
    pub busy: PbBusy,
    pub at: std::time::Instant,
}

impl BusyRecord {
    /// Still inside the window the firmware said to expect silence for.
    pub fn within_budget(&self) -> bool {
        self.busy.active && self.at.elapsed().as_secs() <= u64::from(self.busy.expect_secs)
    }
}

#[derive(Clone, Default)]
pub struct DirectHub {
    inner: Arc<Mutex<HashMap<String, Session>>>,
    /// Survives session teardown, unlike everything in `inner`.
    busy: Arc<Mutex<HashMap<String, BusyRecord>>>,
    started: Arc<std::sync::atomic::AtomicBool>,
    advertising: Arc<std::sync::atomic::AtomicBool>,
    port: Arc<std::sync::atomic::AtomicU16>,
}

impl DirectHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind the listener once (idempotent). Accepted connections register a
    /// session on HELLO and pump frames until the socket closes.
    ///
    /// The caller re-invokes this every UI frame, so releasing `started` on any
    /// exit is what makes the listener self-healing. Without that, a failed bind
    /// (the port is still in `TIME_WAIT` from a previous instance) or a single
    /// transient `accept` error retires the listener for the life of the process:
    /// the console keeps running, firmware's connect gets an RST, and only an
    /// app restart brings it back.
    pub fn start(&self, port: u16) {
        if self.started.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return;
        }
        self.port.store(port, std::sync::atomic::Ordering::Release);
        let inner = self.inner.clone();
        let busy = self.busy.clone();
        let started = self.started.clone();
        PlatformSpawner::spawn(async move {
            let listener = match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
                Ok(l) => l,
                Err(e) => {
                    log::warn!("preboot direct: bind :{port} failed: {e}; retrying");
                    started.store(false, std::sync::atomic::Ordering::Release);
                    return;
                }
            };
            log::info!("preboot direct: listening on 0.0.0.0:{port}");
            let mut errors = 0u32;
            loop {
                match listener.accept().await {
                    Ok((sock, peer)) => {
                        errors = 0;
                        let inner = inner.clone();
                        let busy = busy.clone();
                        PlatformSpawner::spawn(async move {
                            if let Err(e) = handle_conn(sock, peer.to_string(), inner, busy).await {
                                log::debug!("preboot direct: session {peer} ended: {e}");
                            }
                        });
                    }
                    Err(e) => {
                        // A peer that vanishes between SYN and accept, or a
                        // momentary descriptor shortage, is routine.
                        errors += 1;
                        log::warn!("preboot direct: accept failed ({errors}): {e}");
                        if errors >= ACCEPT_ERROR_LIMIT {
                            log::error!(
                                "preboot direct: {errors} consecutive accept failures; rebinding"
                            );
                            break;
                        }
                        tokio::time::sleep(ACCEPT_ERROR_BACKOFF).await;
                    }
                }
            }
            started.store(false, std::sync::atomic::Ordering::Release);
        });
    }

    /// Advertise this console's LAN direct-link endpoint to the relay once, then
    /// re-advertise every ~60s so firmware can discover it. LAN IP is derived by
    /// asking the OS which local interface routes toward the relay host. Also
    /// starts a LAN UDP beacon so firmware can discover this console without the
    /// relay at all.
    pub fn advertise(&self, base_url: String) {
        if base_url.trim().is_empty() {
            return;
        }
        if self.advertising.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return;
        }
        let port = self.port.load(std::sync::atomic::Ordering::Acquire);
        let base = base_url.trim().trim_end_matches('/').to_string();
        self.beacon(base.clone(), port);
        PlatformSpawner::spawn(async move {
            let url = format!("{base}/api/v1/qc/preboot/console");
            let client = reqwest::Client::new();
            loop {
                if let Some(ip) = lan_ip_toward(&base) {
                    let body = serde_json::json!({ "addr": format!("{ip}:{port}") });
                    let _ = client.post(&url).json(&body).send().await;
                }
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            }
        });
    }

    /// Broadcast a LAN UDP discovery beacon (`ip:port` plus the relay base url)
    /// every ~3s so firmware can find this console without any relay round-trip.
    /// One beacon per usable IPv4, each naming the address of the interface it
    /// leaves by: firmware reaches this console over the segment the beacon
    /// arrived on, so any other address in the payload is one it cannot ARP. A
    /// fresh bound socket each cycle also tracks DHCP address changes. Falls
    /// back to a v1 beacon when no relay url can be resolved.
    fn beacon(&self, base: String, port: u16) {
        PlatformSpawner::spawn(async move {
            let dest = format!("255.255.255.255:{}", preboot::DISCOVERY_PORT);
            loop {
                for v4 in beacon_source_ips(&base) {
                    let ip = std::net::IpAddr::V4(v4);
                    let Ok(sock) = std::net::UdpSocket::bind(std::net::SocketAddr::new(ip, 0))
                    else {
                        continue;
                    };
                    if sock.set_broadcast(true).is_err() {
                        continue;
                    }
                    let addr = format!("{ip}:{port}");
                    let msg = match relay_url_for(&base, ip) {
                        Some(relay) => preboot::encode_beacon_v2(&addr, &relay),
                        None => preboot::encode_beacon(&addr),
                    };
                    let _ = sock.send_to(&msg, &dest);
                }
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        });
    }

    /// Snapshot the live sessions for the roster. Read-only: a session lives
    /// until its socket dies, so an idle box is never swept out from under a
    /// working connection.
    pub fn agents(&self) -> Vec<DirectAgent> {
        let Ok(map) = self.inner.try_lock() else {
            return Vec::new();
        };
        let now = std::time::Instant::now();
        map.iter()
            .map(|(serial, s)| DirectAgent {
                serial: serial.clone(),
                idle_secs: now.duration_since(s.last_seen).as_secs(),
                peer: s.peer.clone(),
            })
            .collect()
    }

    pub fn latest_frame(&self, serial: &str) -> Option<Vec<u8>> {
        self.inner.try_lock().ok()?.get(serial)?.frame.clone()
    }

    pub fn frame_seq(&self, serial: &str) -> u64 {
        self.inner.try_lock().ok().and_then(|m| m.get(serial).map(|s| s.frame_seq)).unwrap_or(0)
    }

    /// Briefly retries the lock: a momentary contention must not read as a
    /// disconnected box, since callers gate real work on this.
    pub fn is_connected(&self, serial: &str) -> bool {
        for _ in 0..LOCK_RETRIES {
            if let Ok(m) = self.inner.try_lock() {
                return m.contains_key(serial);
            }
            std::thread::yield_now();
        }
        false
    }

    /// Take (and clear) the last plugin result for `serial`, if any.
    pub fn take_plugin_result(&self, serial: &str) -> Option<PbPluginResult> {
        self.inner.try_lock().ok()?.get_mut(serial)?.plugin_result.take()
    }

    /// Take (and clear) the last state-query answer for `serial`, if any.
    pub fn take_query_result(&self, serial: &str) -> Option<PbQueryResult> {
        self.inner.try_lock().ok()?.get_mut(serial)?.query_result.take()
    }

    /// Last child-image report for `serial`, kept across disconnects.
    pub fn busy_record(&self, serial: &str) -> Option<BusyRecord> {
        self.busy.try_lock().ok()?.get(serial).cloned()
    }

    /// Every retained child-image report, including boxes with no live session.
    pub fn busy_serials(&self) -> Vec<(String, BusyRecord)> {
        match self.busy.try_lock() {
            Ok(m) => m.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Retries the lock so a keystroke isn't silently dropped on contention.
    fn send_tagged(&self, serial: &str, tag: u8, body: &[u8]) -> bool {
        for _ in 0..LOCK_RETRIES {
            let Ok(map) = self.inner.try_lock() else {
                std::thread::yield_now();
                continue;
            };
            let Some(s) = map.get(serial) else {
                return false;
            };
            return s.tx.send(frame_bytes(tag, body)).is_ok();
        }
        log::warn!("preboot direct: '{serial}' send tag 0x{tag:02x} gave up on the session lock");
        false
    }

    pub fn send_input(&self, serial: &str, ev: &PreBootEvent) -> bool {
        self.send_tagged(serial, preboot::FRAME_TAG_PREBOOT_INPUT, &preboot::encode_event(ev))
    }

    pub fn send_stream_ctl(&self, serial: &str, stream: bool) -> bool {
        self.send_tagged(
            serial,
            preboot::FRAME_TAG_PREBOOT_STREAM_CTL,
            &preboot::encode_stream_ctl(&PbStreamCtl { stream }),
        )
    }

    pub fn send_query(&self, serial: &str, req: &PbQuery) -> bool {
        self.send_tagged(serial, preboot::FRAME_TAG_PREBOOT_QUERY, &preboot::encode_query(req))
    }

    pub fn run_plugin(&self, serial: &str, req: &PbPluginRun) -> bool {
        self.send_tagged(
            serial,
            preboot::FRAME_TAG_PREBOOT_PLUGIN_RUN,
            &preboot::encode_plugin_run(req),
        )
    }
}

/// Local interface IP that routes toward `base_url`'s host. A UDP `connect`
/// picks the outbound interface without any handshake; reading back
/// `local_addr` yields the LAN IP. Only IP-literal hosts are targeted directly;
/// a hostname would force a blocking DNS lookup on the async worker, so it falls
/// back to a public literal (learns the default-route interface). Loopback and
/// unspecified results are rejected so a `localhost` relay can't advertise an
/// unreachable `127.0.0.1` to firmware.
fn lan_ip_toward(base_url: &str) -> Option<std::net::IpAddr> {
    let host = base_url
        .split("://")
        .nth(1)
        .unwrap_or(base_url)
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");
    let target: std::net::SocketAddr = match host.parse::<std::net::IpAddr>() {
        Ok(ip) => std::net::SocketAddr::new(ip, 80),
        Err(_) => "8.8.8.8:80".parse().ok()?,
    };
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect(target).ok()?;
    let ip = sock.local_addr().ok()?.ip();
    if ip.is_loopback() || ip.is_unspecified() {
        return None;
    }
    Some(ip)
}

/// Every IPv4 this host can broadcast a beacon from, route-preferred first.
///
/// [`lan_ip_toward`] alone answers "which interface reaches the relay", which is
/// the wrong question for a beacon: with a hostname base it resolves against
/// 8.8.8.8 and so names whichever adapter holds the default route — a VPN, a
/// hypervisor switch, or the wrong NIC on a multi-homed console. Firmware on
/// any other segment then learns an address that never answers ARP.
fn beacon_source_ips(base: &str) -> Vec<std::net::Ipv4Addr> {
    let preferred = match lan_ip_toward(base) {
        Some(std::net::IpAddr::V4(v4)) => Some(v4),
        _ => None,
    };
    let mut out: Vec<std::net::Ipv4Addr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.ip() {
            std::net::IpAddr::V4(v4) => Some(v4),
            std::net::IpAddr::V6(_) => None,
        })
        // APIPA and 0.0.0.0 carry no reachable listener.
        .filter(|v4| !v4.is_link_local() && !v4.is_unspecified() && !v4.is_broadcast())
        .collect();
    if let Some(p) = preferred {
        out.retain(|v4| *v4 != p);
        out.insert(0, p);
    }
    out
}

/// Relay base url to advertise in a beacon.
///
/// Firmware has no DNS resolver and no TLS stack, so a beacon may only ever name
/// plain HTTP on a literal IPv4. Anything else — `https`, a hostname, loopback,
/// unspecified — is rewritten to this console's own LAN `ip` on
/// [`DEFAULT_RELAY_PORT`], which is where `preboot-relay` listens. Advertising
/// the configured public https host instead wedges every firmware upload behind
/// a name it cannot resolve. None when no valid url can be built.
fn relay_url_for(base: &str, ip: std::net::IpAddr) -> Option<String> {
    let std::net::IpAddr::V4(v4) = ip else { return None };
    let base = base.trim().trim_end_matches('/');
    let (scheme, rest) = base.split_once("://")?;
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let authority = rest.split('/').next().unwrap_or("");
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => (h, p.parse::<u16>().ok()),
        _ => (authority, None),
    };
    // Only a literal IPv4 over plain http is usable as-is; its port carries over.
    let host_ipv4 = host.parse::<std::net::Ipv4Addr>().ok();
    let usable = scheme == "http"
        && host_ipv4.is_some_and(|h| !h.is_loopback() && !h.is_unspecified());
    let url = if usable {
        format!("http://{host}:{}", port.unwrap_or(DEFAULT_RELAY_PORT))
    } else {
        // A rewritten host means the original port belonged to a different
        // service (443 for the public host), so fall back to the relay's.
        format!("http://{v4}:{DEFAULT_RELAY_PORT}")
    };
    preboot::is_valid_relay_url(&url).then_some(url)
}

/// Frame one payload as `[u32 LE total_len][tag][body]` (total_len counts tag).
fn frame_bytes(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(5 + body.len());
    v.extend_from_slice(&((1 + body.len()) as u32).to_le_bytes());
    v.push(tag);
    v.extend_from_slice(body);
    v
}

async fn handle_conn(
    mut sock: tokio::net::TcpStream,
    peer: String,
    inner: Arc<Mutex<HashMap<String, Session>>>,
    busy: Arc<Mutex<HashMap<String, BusyRecord>>>,
) -> std::io::Result<()> {
    sock.set_nodelay(true).ok();
    // First frame must be HELLO carrying the serial.
    let (tag, body) = read_frame(&mut sock).await?;
    if tag != preboot::FRAME_TAG_PREBOOT_HELLO {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "expected HELLO first"));
    }
    let serial = String::from_utf8_lossy(&body).trim().to_string();
    if serial.is_empty() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "empty serial in HELLO"));
    }
    log::info!("preboot direct: {peer} linked as '{serial}'");

    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    {
        let mut map = inner.lock().await;
        map.insert(
            serial.clone(),
            Session {
                frame: None,
                frame_seq: 0,
                tx,
                plugin_result: None,
                query_result: None,
                last_seen: std::time::Instant::now(),
                peer: peer.clone(),
            },
        );
    }
    // Ask the box to start streaming now that a session exists.
    let _ = sock
        .write_all(&frame_bytes(
            preboot::FRAME_TAG_PREBOOT_STREAM_CTL,
            &preboot::encode_stream_ctl(&PbStreamCtl { stream: true }),
        ))
        .await;

    let (mut rd, mut wr) = sock.into_split();
    let serial_r = serial.clone();
    let inner_r = inner.clone();
    let busy_r = busy.clone();
    let ping_tx = {
        let map = inner.lock().await;
        map.get(&serial).map(|s| s.tx.clone())
    };
    // Keepalive: pings the box so a half-open socket surfaces as a write error
    // rather than a session that lingers forever.
    let pinger = tokio::spawn(async move {
        let Some(tx) = ping_tx else { return };
        loop {
            tokio::time::sleep(PING_INTERVAL).await;
            if tx.send(frame_bytes(tcp_protocol::FRAME_TAG_PING, &[])).is_err() {
                break;
            }
        }
    });
    // Reader: decode inbound frames into the session. Any frame - including a
    // ping or pong - counts as liveness.
    let reader = tokio::spawn(async move {
        loop {
            let (tag, body) = match read_frame(&mut rd).await {
                Ok(v) => v,
                Err(_) => break,
            };
            let mut map = inner_r.lock().await;
            // A live socket whose session went missing is re-registered by the
            // writer's cleanup, so skip the frame rather than tearing it down.
            let Some(s) = map.get_mut(&serial_r) else { continue };
            s.last_seen = std::time::Instant::now();
            match tag {
                t if t == preboot::FRAME_TAG_PREBOOT_FRAME => {
                    s.frame = Some(body);
                    s.frame_seq = s.frame_seq.wrapping_add(1);
                }
                t if t == preboot::FRAME_TAG_PREBOOT_PLUGIN_RESULT => {
                    s.plugin_result = preboot::decode_plugin_result(&body);
                }
                t if t == preboot::FRAME_TAG_PREBOOT_QUERY_RESULT => {
                    s.query_result = preboot::decode_query_result(&body);
                }
                t if t == preboot::FRAME_TAG_PREBOOT_BUSY => {
                    if let Some(b) = preboot::decode_busy(&body) {
                        if b.active {
                            log::info!(
                                "preboot direct: '{serial_r}' entering child image '{}'; silence expected for up to {}s",
                                b.what, b.expect_secs
                            );
                        } else {
                            log::info!(
                                "preboot direct: '{serial_r}' returned from '{}': {}",
                                b.what, b.status
                            );
                        }
                        // Recorded outside the session map so it survives the
                        // teardown the child image is about to cause.
                        if let Ok(mut m) = busy_r.try_lock() {
                            m.insert(
                                serial_r.clone(),
                                BusyRecord { busy: b, at: std::time::Instant::now() },
                            );
                        }
                    }
                }
                t if t == tcp_protocol::FRAME_TAG_PING => {
                    let _ = s.tx.send(frame_bytes(tcp_protocol::FRAME_TAG_PONG, &[]));
                }
                _ => {}
            }
        }
    });
    // Writer: drain the outbound channel to the socket.
    while let Some(bytes) = rx.recv().await {
        if wr.write_all(&bytes).await.is_err() {
            break;
        }
    }
    pinger.abort();
    reader.abort();
    inner.lock().await.remove(&serial);
    log::info!("preboot direct: '{serial}' unlinked");
    Ok(())
}

/// Read one `[u32 LE total_len][tag][body]` frame. total_len counts the tag.
async fn read_frame<R: AsyncReadExt + Unpin>(r: &mut R) -> std::io::Result<(u8, Vec<u8>)> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf).await?;
    let total = u32::from_le_bytes(len_buf);
    if total == 0 || total > tcp_protocol::MAX_FRAME_BYTES {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "bad frame length"));
    }
    let mut buf = vec![0u8; total as usize];
    r.read_exact(&mut buf).await?;
    let tag = buf[0];
    Ok((tag, buf[1..].to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lan() -> std::net::IpAddr {
        "192.168.22.91".parse().unwrap()
    }

    /// A bind that loses the port must not latch `started`: the caller retries
    /// every UI frame, and a stuck latch means the console never listens again
    /// and firmware's connect gets an RST until the app is restarted.
    #[test]
    fn failed_bind_releases_the_start_latch() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let _g = rt.enter();

        // Must squat the same wildcard address the hub binds: on Windows a
        // 0.0.0.0 bind does not collide with one on 127.0.0.1.
        let squatter = std::net::TcpListener::bind(("0.0.0.0", 0)).unwrap();
        let port = squatter.local_addr().unwrap().port();

        let hub = DirectHub::new();
        hub.start(port);
        assert!(hub.started.load(std::sync::atomic::Ordering::Acquire), "latched while spawning");

        // Let the spawned task run its bind and fail.
        rt.block_on(async { tokio::time::sleep(std::time::Duration::from_millis(150)).await });

        assert!(
            !hub.started.load(std::sync::atomic::Ordering::Acquire),
            "latch stayed set after a failed bind - start() can never rebind"
        );
        drop(squatter);
    }

    #[test]
    fn https_public_host_is_rewritten_to_this_console() {
        assert_eq!(
            relay_url_for("https://axum.master-tech.app", lan()).unwrap(),
            "http://192.168.22.91:8082"
        );
    }

    #[test]
    fn a_hostname_over_plain_http_is_still_rewritten() {
        assert_eq!(
            relay_url_for("http://relay.local:8082", lan()).unwrap(),
            "http://192.168.22.91:8082"
        );
    }

    #[test]
    fn loopback_is_rewritten_keeping_the_relay_port() {
        assert_eq!(
            relay_url_for("http://localhost:8082", lan()).unwrap(),
            "http://192.168.22.91:8082"
        );
    }

    #[test]
    fn a_real_lan_relay_is_advertised_verbatim() {
        assert_eq!(
            relay_url_for("http://192.168.22.139:8082", lan()).unwrap(),
            "http://192.168.22.139:8082"
        );
    }

    #[test]
    fn a_portless_ipv4_gets_the_relay_port() {
        assert_eq!(
            relay_url_for("http://192.168.22.139", lan()).unwrap(),
            "http://192.168.22.139:8082"
        );
    }
}
