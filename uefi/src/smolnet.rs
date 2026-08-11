//! HTTP over raw SimpleNetwork via smoltcp, for firmware that never instantiated
//! its UEFI IPv4/TCP stack (Ip4Dxe/Tcp4 absent). smoltcp owns ARP/IPv4/TCP in
//! software over raw L2 frames; the IPv4 lease is seeded from [`netraw::dhcp`].

use core::sync::atomic::{AtomicU16, Ordering};
use core::time::Duration;

use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::tcp;
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr, IpListenEndpoint, Ipv4Address};

use uefi::boot;
use uefi::proto::network::snp::SimpleNetwork;

use crate::logln;
use crate::netraw::{self, RawNet};
use crate::protoguard::Held;

const RX_BUF: usize = 32 * 1024;
const TX_BUF: usize = 16 * 1024;
const DEADLINE_MS: i64 = 15_000;

static LOCAL_PORT: AtomicU16 = AtomicU16::new(0);
static LEASE: std::sync::Mutex<Option<RawNet>> = std::sync::Mutex::new(None);

fn v4(o: [u8; 4]) -> Ipv4Address {
    Ipv4Address::new(o[0], o[1], o[2], o[3])
}

/// DHCP lease, acquired once over raw SNP then cached for the session.
fn lease() -> Result<RawNet, String> {
    if let Ok(g) = LEASE.lock() {
        if let Some(rn) = *g {
            return Ok(rn);
        }
    }
    let rn = netraw::dhcp()?;
    if let Ok(mut g) = LEASE.lock() {
        *g = Some(rn);
    }
    Ok(rn)
}

struct SnpDevice {
    snp: Held<SimpleNetwork>,
}

struct SnpRx(Vec<u8>);
struct SnpTx<'a>(&'a SimpleNetwork);

impl Device for SnpDevice {
    type RxToken<'a> = SnpRx where Self: 'a;
    type TxToken<'a> = SnpTx<'a> where Self: 'a;

    fn receive(&mut self, _t: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let mut buf = [0u8; 2048];
        match self.snp.receive(&mut buf, None, None, None, None) {
            Ok(n) => Some((SnpRx(buf[..n].to_vec()), SnpTx(&self.snp))),
            Err(_) => None,
        }
    }

    fn transmit(&mut self, _t: Instant) -> Option<Self::TxToken<'_>> {
        Some(SnpTx(&self.snp))
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
        let _ = self.0.transmit(0, &buf, None, None, None);
        r
    }
}

/// Drive a single HTTP request/response over a fresh TCP connection. Reads until
/// the peer closes (request must carry `Connection: close`) or the deadline.
fn request(host_port: &str, ip: [u8; 4], port: u16, req: &[u8]) -> Result<Vec<u8>, String> {
    let rn = lease()?;
    let mut device = SnpDevice { snp: netraw::open_snp()? };

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

        boot::stall(Duration::from_millis(1));
        clock_ms += 1;
    }

    sockets.get_mut::<tcp::Socket>(handle).close();
    for _ in 0..50 {
        iface.poll(Instant::from_millis(clock_ms), &mut device, &mut sockets);
        boot::stall(Duration::from_millis(1));
        clock_ms += 1;
    }

    if !connected {
        return Err(format!("smol: no connection to {host_port}"));
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
