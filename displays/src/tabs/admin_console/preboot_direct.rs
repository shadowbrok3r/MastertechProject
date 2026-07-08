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

use tcp_protocol::preboot::{self, PbPluginResult, PbPluginRun, PbStreamCtl, PreBootEvent};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Mutex, mpsc};

use crate::{PlatformSpawner, Spawner};

/// Default direct-link listen port on the console.
pub const DIRECT_PORT: u16 = 9209;

/// Per-session shared state, updated by the reader task and read by egui.
struct Session {
    /// Latest decoded frame bytes (bincode `PreBootFrame`).
    frame: Option<Vec<u8>>,
    frame_seq: u64,
    /// Outbound frame sender to this session's writer task.
    tx: mpsc::UnboundedSender<Vec<u8>>,
    /// Most recent plugin result pushed by the firmware.
    plugin_result: Option<PbPluginResult>,
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
#[derive(Clone, Default)]
pub struct DirectHub {
    inner: Arc<Mutex<HashMap<String, Session>>>,
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
    pub fn start(&self, port: u16) {
        if self.started.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return;
        }
        self.port.store(port, std::sync::atomic::Ordering::Release);
        let inner = self.inner.clone();
        PlatformSpawner::spawn(async move {
            let listener = match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
                Ok(l) => l,
                Err(e) => {
                    log::warn!("preboot direct: bind :{port} failed: {e}");
                    return;
                }
            };
            log::info!("preboot direct: listening on 0.0.0.0:{port}");
            loop {
                match listener.accept().await {
                    Ok((sock, peer)) => {
                        let inner = inner.clone();
                        PlatformSpawner::spawn(async move {
                            if let Err(e) = handle_conn(sock, peer.to_string(), inner).await {
                                log::debug!("preboot direct: session {peer} ended: {e}");
                            }
                        });
                    }
                    Err(e) => {
                        log::warn!("preboot direct: accept failed: {e}");
                        break;
                    }
                }
            }
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

    /// Broadcast a LAN UDP discovery beacon (`ip:port`) every ~3s so firmware
    /// can find this console without any relay round-trip.
    fn beacon(&self, base: String, port: u16) {
        PlatformSpawner::spawn(async move {
            let sock = match std::net::UdpSocket::bind("0.0.0.0:0") {
                Ok(s) => s,
                Err(e) => {
                    log::warn!("preboot direct: beacon bind failed: {e}");
                    return;
                }
            };
            if let Err(e) = sock.set_broadcast(true) {
                log::warn!("preboot direct: beacon set_broadcast failed: {e}");
                return;
            }
            let dest = format!("255.255.255.255:{}", tcp_protocol::preboot::DISCOVERY_PORT);
            loop {
                if let Some(ip) = lan_ip_toward(&base) {
                    let msg = tcp_protocol::preboot::encode_beacon(&format!("{ip}:{port}"));
                    let _ = sock.send_to(&msg, &dest);
                }
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        });
    }

    /// Snapshot the live sessions for the roster (sweeps sessions idle >90s).
    pub fn agents(&self) -> Vec<DirectAgent> {
        let Ok(mut map) = self.inner.try_lock() else {
            return Vec::new();
        };
        let now = std::time::Instant::now();
        map.retain(|_, s| now.duration_since(s.last_seen).as_secs() < 90);
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

    pub fn is_connected(&self, serial: &str) -> bool {
        self.inner.try_lock().ok().map(|m| m.contains_key(serial)).unwrap_or(false)
    }

    /// Take (and clear) the last plugin result for `serial`, if any.
    pub fn take_plugin_result(&self, serial: &str) -> Option<PbPluginResult> {
        self.inner.try_lock().ok()?.get_mut(serial)?.plugin_result.take()
    }

    fn send_tagged(&self, serial: &str, tag: u8, body: &[u8]) -> bool {
        let Ok(map) = self.inner.try_lock() else {
            return false;
        };
        let Some(s) = map.get(serial) else {
            return false;
        };
        s.tx.send(frame_bytes(tag, body)).is_ok()
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

    pub fn run_plugin(&self, serial: &str, req: &PbPluginRun) -> bool {
        self.send_tagged(
            serial,
            preboot::FRAME_TAG_PREBOOT_PLUGIN_RUN,
            &preboot::encode_plugin_run(req),
        )
    }
}

/// Local interface IP that routes toward `base_url`'s host. A UDP socket needs
/// no handshake, so `connect` just picks the outbound interface; reading back
/// `local_addr` yields the LAN IP without hardcoding it.
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
    let target = if host.is_empty() { "8.8.8.8:80".to_string() } else { format!("{host}:80") };
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    // Falls back to a public IP if the relay host doesn't resolve — either way
    // the point is only to learn our own outbound-interface address.
    if sock.connect(&target).is_err() {
        sock.connect("8.8.8.8:80").ok()?;
    }
    sock.local_addr().ok().map(|a| a.ip())
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
    // Reader: decode inbound frames into the session.
    let reader = tokio::spawn(async move {
        loop {
            let (tag, body) = match read_frame(&mut rd).await {
                Ok(v) => v,
                Err(_) => break,
            };
            let mut map = inner_r.lock().await;
            let Some(s) = map.get_mut(&serial_r) else { break };
            s.last_seen = std::time::Instant::now();
            match tag {
                t if t == preboot::FRAME_TAG_PREBOOT_FRAME => {
                    s.frame = Some(body);
                    s.frame_seq = s.frame_seq.wrapping_add(1);
                }
                t if t == preboot::FRAME_TAG_PREBOOT_PLUGIN_RESULT => {
                    s.plugin_result = preboot::decode_plugin_result(&body);
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
