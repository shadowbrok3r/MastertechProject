//! HTTP over raw L2 frames via smoltcp, for firmware that never instantiated
//! its UEFI IPv4/TCP stack (Ip4Dxe/Tcp4 absent). smoltcp owns ARP/IPv4/TCP in
//! software; frames move over [`netraw::Link`] (MNP when bound, raw SNP
//! otherwise) and the IPv4 lease is seeded from [`netraw::dhcp`].

use core::sync::atomic::{AtomicU16, Ordering};
use core::time::Duration;

use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::tcp;
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr, IpListenEndpoint, Ipv4Address};

use uefi::boot;

use crate::logln;
use crate::netraw::{self, Link, RawNet};

const RX_BUF: usize = 32 * 1024;
const TX_BUF: usize = 16 * 1024;
const DEADLINE_MS: i64 = 15_000;

/// Budget for the handshake alone. A peer that never answers ARP cannot
/// complete one, and would otherwise burn the whole-request deadline on it.
const CONNECT_DEADLINE_MS: i64 = 2_500;

static LOCAL_PORT: AtomicU16 = AtomicU16::new(0);

fn v4(o: [u8; 4]) -> Ipv4Address {
    Ipv4Address::new(o[0], o[1], o[2], o[3])
}

/// The shared raw lease, acquiring one when none is held yet. Reading
/// [`netraw::current`] (not a module-local copy) picks up background renewals.
fn lease() -> Result<RawNet, String> {
    match netraw::current() {
        Some(rn) => Ok(rn),
        None => netraw::dhcp(),
    }
}

struct SnpDevice {
    link: Link,
    /// Handed to smoltcp ahead of the NIC, once.
    primed: Option<Vec<u8>>,
}

struct SnpRx(Vec<u8>);
struct SnpTx<'a>(&'a Link);

impl Device for SnpDevice {
    type RxToken<'a> = SnpRx where Self: 'a;
    type TxToken<'a> = SnpTx<'a> where Self: 'a;

    fn receive(&mut self, _t: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        if let Some(frame) = self.primed.take() {
            return Some((SnpRx(frame), SnpTx(&self.link)));
        }
        let mut buf = [0u8; 2048];
        match self.link.recv(&mut buf, 0) {
            (Some(n), _) => Some((SnpRx(buf[..n].to_vec()), SnpTx(&self.link))),
            (None, _) => None,
        }
    }

    fn transmit(&mut self, _t: Instant) -> Option<Self::TxToken<'_>> {
        Some(SnpTx(&self.link))
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut c = DeviceCapabilities::default();
        c.medium = Medium::Ethernet;
        c.max_transmission_unit = 1514;
        c
    }
}

impl RxToken for SnpRx {
    fn consume<R, F: FnOnce(&[u8]) -> R>(self, f: F) -> R {
        f(&self.0)
    }
}

impl TxToken for SnpTx<'_> {
    fn consume<R, F: FnOnce(&mut [u8]) -> R>(self, len: usize, f: F) -> R {
        let mut buf = vec![0u8; len];
        let r = f(&mut buf);
        let _ = self.0.transmit(&buf);
        r
    }
}

/// Drive a single HTTP request/response over a fresh TCP connection. Reads until
/// the peer closes (request must carry `Connection: close`) or the deadline.
fn request(host_port: &str, ip: [u8; 4], port: u16, req: &[u8]) -> Result<Vec<u8>, String> {
    let rn = lease()?;
    let mut device = SnpDevice { link: Link::open()?, primed: None };

    let mac = rn.mac;
    let seed = u64::from_le_bytes([mac[0], mac[1], mac[2], mac[3], mac[4], mac[5], 0, 0]);
    let mut cfg = Config::new(HardwareAddress::Ethernet(EthernetAddress(mac)));
    cfg.random_seed = seed;

    let mut iface = Interface::new(cfg, &mut device, Instant::from_millis(0));
    let prefix = rn.mask.iter().map(|b| b.count_ones()).sum::<u32>() as u8;
    iface.update_ip_addrs(|v| {
        let _ = v.push(IpCidr::new(IpAddress::Ipv4(v4(rn.ip)), prefix));
    });
    iface
        .routes_mut()
        .add_default_ipv4_route(v4(rn.gateway))
        .map_err(|e| format!("smol route: {e:?}"))?;

    // smoltcp's ARP request can go unanswered indefinitely: the reply is unicast,
    // and the firmware's own network drivers drain the same receive queue. A
    // reply synthesized from an address already seen on the wire fills the
    // neighbor cache without one. smoltcp accepts it because the target is us
    // and the sender is unicast and on-subnet.
    if let Some(mac) = netraw::neighbor_mac(ip) {
        device.primed = Some(netraw::build_arp_reply(
            netraw::Origin { mac, ip },
            netraw::Origin { mac: rn.mac, ip: rn.ip },
        ));
        logln(format!("smol: seeded {} from a seen frame", netraw::ip_str(ip)));
    }

    let mut sockets = SocketSet::new(vec![]);
    let rx = tcp::SocketBuffer::new(vec![0u8; RX_BUF]);
    let tx = tcp::SocketBuffer::new(vec![0u8; TX_BUF]);
    let handle = sockets.add(tcp::Socket::new(rx, tx));

    let lp = 0xC000 | (LOCAL_PORT.fetch_add(1, Ordering::Relaxed) & 0x3FFF);
    sockets
        .get_mut::<tcp::Socket>(handle)
        .connect(
            iface.context(),
            (IpAddress::Ipv4(v4(ip)), port),
            IpListenEndpoint { addr: None, port: lp },
        )
        .map_err(|e| format!("smol connect {host_port}: {e:?}"))?;

    // Elapsed wall time, not iterations: a poll costs several ms, so counting
    // passes stretched both this deadline and smoltcp's own timers.
    let epoch = crate::mono::now_ms();
    let elapsed = move |stalls: i64| match (epoch, crate::mono::now_ms()) {
        (Some(a), Some(b)) => b.saturating_sub(a) as i64,
        _ => stalls,
    };

    let mut stalls: i64 = 0;
    let mut clock_ms: i64 = 0;
    let mut sent = 0usize;
    let mut resp: Vec<u8> = Vec::new();
    let mut connected = false;

    while clock_ms < DEADLINE_MS {
        iface.poll(Instant::from_millis(clock_ms), &mut device, &mut sockets);
        let sock = sockets.get_mut::<tcp::Socket>(handle);

        if sock.may_send() {
            connected = true;
        }
        if connected && sent < req.len() && sock.can_send() {
            if let Ok(n) = sock.send_slice(&req[sent..]) {
                sent += n;
            }
        }
        while sock.can_recv() {
            let mut tmp = [0u8; 2048];
            match sock.recv_slice(&mut tmp) {
                Ok(0) | Err(_) => break,
                Ok(n) => resp.extend_from_slice(&tmp[..n]),
            }
        }
        if connected && sent >= req.len() && !sock.may_recv() {
            break;
        }
        if !connected && clock_ms >= CONNECT_DEADLINE_MS {
            break;
        }

        boot::stall(Duration::from_millis(1));
        stalls += 1;
        clock_ms = elapsed(stalls);
    }

    sockets.get_mut::<tcp::Socket>(handle).close();
    for _ in 0..50 {
        iface.poll(Instant::from_millis(clock_ms), &mut device, &mut sockets);
        boot::stall(Duration::from_millis(1));
        stalls += 1;
        clock_ms = elapsed(stalls);
    }

    if !connected {
        return Err(format!("smol: no connection to {host_port} in {clock_ms}ms"));
    }
    if resp.is_empty() {
        return Err("smol: empty response".into());
    }
    Ok(resp)
}

/// Split an HTTP response into (status code, body). Assumes Content-Length /
/// connection-close framing (no chunked decode).
fn parse_http(raw: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let sep = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("smol: no HTTP header end")?;
    let line_end = raw.windows(2).position(|w| w == b"\r\n").unwrap_or(sep);
    let status = core::str::from_utf8(&raw[..line_end]).unwrap_or("");
    let code = status
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse::<u16>().ok())
        .unwrap_or(0);
    Ok((code, raw[sep + 4..].to_vec()))
}

fn host_of(host_port: &str) -> &str {
    host_port.split(':').next().unwrap_or(host_port)
}

/// HTTP GET over smoltcp; returns (status, body). IPv4-literal hosts only.
pub fn get(host_port: &str, path: &str) -> Result<(u16, Vec<u8>), String> {
    let ip = netraw::parse_ipv4(host_port).ok_or_else(|| format!("smol: non-IPv4 host {host_port}"))?;
    let port = host_port.rsplit_once(':').and_then(|(_, p)| p.parse().ok()).unwrap_or(80);
    logln(format!("smol: GET {host_port}{path}"));
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAccept: */*\r\n\r\n",
        host_of(host_port)
    );
    let raw = request(host_port, ip, port, req.as_bytes())?;
    let (code, body) = parse_http(&raw)?;
    logln(format!("smol: GET {code} ({}B)", body.len()));
    Ok((code, body))
}

/// HTTP POST over smoltcp; returns a status summary string. IPv4-literal hosts only.
pub fn post(host_port: &str, path: &str, body: &[u8]) -> Result<String, String> {
    let ip = netraw::parse_ipv4(host_port).ok_or_else(|| format!("smol: non-IPv4 host {host_port}"))?;
    let port = host_port.rsplit_once(':').and_then(|(_, p)| p.parse().ok()).unwrap_or(80);
    logln(format!("smol: POST {host_port}{path} ({}B)", body.len()));
    let mut req = format!(
        "POST {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        host_of(host_port),
        body.len()
    )
    .into_bytes();
    req.extend_from_slice(body);
    let raw = request(host_port, ip, port, &req)?;
    let (code, rbody) = parse_http(&raw)?;
    logln(format!("smol: POST {code} ({}B)", rbody.len()));
    Ok(format!("HTTP {code} ({}B)", rbody.len()))
}
