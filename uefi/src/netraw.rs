//! Raw IPv4 for firmware that won't instantiate its UEFI IPv4 stack
//! (Ip4Dxe/Tcp4 absent). Hand-builds Ethernet/IPv4/UDP frames: DHCP (DORA) for
//! an address, ARP to resolve the next hop, then a chunked UDP upload of the
//! fingerprint. No Ip4Config2/Tcp4/Http dependency.
//!
//! Frames move over [`Link`]: MNP when MnpDxe is bound (each consumer gets a
//! copy of every matching frame, so this app and the firmware stack stop
//! stealing each other's unicast replies), raw SimpleNetwork otherwise.

use core::sync::atomic::{AtomicU32, Ordering};
use core::time::Duration;

use uefi::boot;
use uefi::proto::network::snp::{NetworkState, ReceiveFlags, SimpleNetwork};

use crate::logln;
use crate::mnp::MnpNet;
use crate::protoguard::{self, Held};

/// Orchestrator UDP port for the raw fingerprint upload (axum_server listener).
pub const UDP_PORT: u16 = 9202;
/// Per-datagram payload header magic.
const CHUNK_MAGIC: [u8; 4] = *b"MTUF";
/// Max fingerprint bytes per datagram (1500 MTU − 20 IP − 8 UDP − 12 header).
const CHUNK_DATA: usize = 1400;

static MSG_ID: AtomicU32 = AtomicU32::new(1);
static IP_ID: AtomicU32 = AtomicU32::new(0x4d54);

/// The observed layer-2/3 origin of a received frame.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Origin {
    pub mac: [u8; 6],
    pub ip: [u8; 4],
}

/// Origins seen on the wire. A host that has sent us a frame is reachable and
/// its MAC is known, so a stack whose own ARP goes unanswered can still address
/// it. Small and append-only: the console and its relay are the only entries.
static NEIGHBORS: std::sync::Mutex<Vec<Origin>> = std::sync::Mutex::new(Vec::new());

/// Record an origin observed on a received frame.
pub fn note_neighbor(o: Origin) {
    if o.ip == [0; 4] || o.mac == [0; 6] {
        return;
    }
    if let Ok(mut g) = NEIGHBORS.lock() {
        match g.iter_mut().find(|n| n.ip == o.ip) {
            Some(n) => n.mac = o.mac,
            None => g.push(o),
        }
    }
}

/// The MAC last seen sending from `ip`, if any.
pub fn neighbor_mac(ip: [u8; 4]) -> Option<[u8; 6]> {
    NEIGHBORS.lock().ok()?.iter().find(|n| n.ip == ip).map(|n| n.mac)
}

/// A DHCP-acquired IPv4 configuration on the raw SNP path.
#[derive(Clone, Copy)]
pub struct RawNet {
    pub mac: [u8; 6],
    pub ip: [u8; 4],
    pub mask: [u8; 4],
    pub gateway: [u8; 4],
    /// DHCP server that granted the lease (option 54).
    pub server: [u8; 4],
    /// Mono clock at grant, None when the TSC is unusable.
    pub obtained_ms: Option<u64>,
    /// Lease duration (option 51); 0 = unknown, never renewed.
    pub lease_secs: u32,
}

impl RawNet {
    /// Past the T1 midpoint, where a client renews.
    pub fn renew_due(&self, now_ms: u64) -> bool {
        match self.obtained_ms {
            Some(t0) if self.lease_secs > 0 => {
                now_ms.saturating_sub(t0) >= u64::from(self.lease_secs) * 500
            }
            _ => false,
        }
    }

    /// Past the full lease duration; the address is no longer ours to use.
    pub fn expired(&self, now_ms: u64) -> bool {
        match self.obtained_ms {
            Some(t0) if self.lease_secs > 0 => {
                now_ms.saturating_sub(t0) >= u64::from(self.lease_secs) * 1000
            }
            _ => false,
        }
    }
}

/// The live raw lease. `dhcp()`/`renew()` update it on success; consumers that
/// cannot reach `App` state (smolnet) read it here, so a background renewal is
/// picked up by every later request instead of a stale per-module copy.
static CURRENT: std::sync::Mutex<Option<RawNet>> = std::sync::Mutex::new(None);

/// The last successfully acquired or renewed raw lease.
pub fn current() -> Option<RawNet> {
    CURRENT.lock().ok().and_then(|g| *g)
}

fn set_current(rn: RawNet) {
    if let Ok(mut g) = CURRENT.lock() {
        *g = Some(rn);
    }
}

pub fn ip_str(o: [u8; 4]) -> String {
    format!("{}.{}.{}.{}", o[0], o[1], o[2], o[3])
}

fn open_snp() -> Result<Held<SimpleNetwork>, String> {
    let handle = boot::find_handles::<SimpleNetwork>()
        .map_err(|e| format!("no SNP: {e:?}"))?
        .into_iter()
        .next()
        .ok_or_else(|| "no SNP handle".to_string())?;
    // Held: rebinding the NIC drivers drops this open record under us.
    let snp = protoguard::get::<SimpleNetwork>(handle)
        .map_err(|e| format!("open SNP: {e:?}"))?;
    if snp.mode().state == NetworkState::STOPPED {
        let _ = snp.start();
    }
    if snp.mode().state != NetworkState::INITIALIZED {
        let _ = snp.initialize(0, 0);
    }
    let _ = snp.receive_filters(
        ReceiveFlags::UNICAST | ReceiveFlags::BROADCAST,
        ReceiveFlags::empty(),
        false,
        None,
    );
    Ok(snp)
}

/// The frame transport: an MNP child when the managed stack exists, raw
/// SimpleNetwork otherwise. Same three primitives either way, so the DHCP,
/// ARP and beacon logic above it never cares which one is live.
pub(crate) enum Link {
    Mnp(MnpNet),
    Snp(Held<SimpleNetwork>),
}

impl Link {
    /// Prefer MNP; fall back to raw SNP where MnpDxe never bound. Logs the
    /// choice once, and again only when it changes.
    pub fn open() -> Result<Self, String> {
        match MnpNet::open() {
            Ok(net) => {
                let m = net.mac;
                crate::mnp::announce(
                    1,
                    &format!(
                        "MNP (demuxed) mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} media={}",
                        m[0], m[1], m[2], m[3], m[4], m[5], net.media_present
                    ),
                );
                Ok(Link::Mnp(net))
            }
            Err(e) => {
                let snp = open_snp()?;
                crate::mnp::announce(2, &format!("raw SNP (MNP: {e})"));
                Ok(Link::Snp(snp))
            }
        }
    }

    pub fn mac(&self) -> [u8; 6] {
        match self {
            Link::Mnp(n) => n.mac,
            Link::Snp(s) => {
                let a = s.mode().current_address.0;
                [a[0], a[1], a[2], a[3], a[4], a[5]]
            }
        }
    }

    pub fn media_present(&self) -> bool {
        match self {
            Link::Mnp(n) => n.media_present,
            Link::Snp(s) => bool::from(s.mode().media_present),
        }
    }

    /// Send one fully built Ethernet frame.
    pub fn transmit(&self, frame: &[u8]) -> Result<(), String> {
        match self {
            Link::Mnp(n) => n.transmit(frame),
            Link::Snp(s) => s
                .transmit(0, frame, None, None, None)
                .map_err(|e| format!("SNP tx: {e:?}")),
        }
    }

    /// Poll for one frame, up to `timeout_ms` (0 = single non-blocking pass).
    /// Returns the frame length and the milliseconds actually spent waiting.
    pub fn recv(&mut self, buf: &mut [u8], timeout_ms: u64) -> (Option<usize>, u64) {
        match self {
            Link::Mnp(n) => n.recv(buf, timeout_ms),
            Link::Snp(s) => recv_frame(s, buf, timeout_ms),
        }
    }
}

/// Poll the NIC for one frame, up to `timeout_ms`. Returns the frame length and
/// the milliseconds actually spent waiting.
fn recv_frame(snp: &SimpleNetwork, buf: &mut [u8], timeout_ms: u64) -> (Option<usize>, u64) {
    let mut waited = 0u64;
    loop {
        if let Ok(n) = snp.receive(buf, None, None, None, None) {
            return (Some(n), waited);
        }
        if waited >= timeout_ms {
            return (None, waited);
        }
        let _ = boot::stall(Duration::from_millis(5));
        waited += 5;
    }
}

/// One's-complement 16-bit checksum (IPv4 header).
fn checksum16(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut i = 0;
    while i + 1 < data.len() {
        sum += u16::from_be_bytes([data[i], data[i + 1]]) as u32;
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

fn eth_header(out: &mut Vec<u8>, dst: [u8; 6], src: [u8; 6], ethertype: u16) {
    out.extend_from_slice(&dst);
    out.extend_from_slice(&src);
    out.extend_from_slice(&ethertype.to_be_bytes());
}

/// IPv4 + UDP datagram (checksum-0 UDP, valid for IPv4) wrapped in Ethernet.
fn build_udp_frame(
    src_mac: [u8; 6],
    dst_mac: [u8; 6],
    src_ip: [u8; 4],
    dst_ip: [u8; 4],
    src_port: u16,
    dst_port: u16,
    payload: &[u8],
) -> Vec<u8> {
    let udp_len = 8 + payload.len();
    let total_len = 20 + udp_len;
    let mut frame = Vec::with_capacity(14 + total_len);
    eth_header(&mut frame, dst_mac, src_mac, 0x0800);

    let ip_start = frame.len();
    let id = (IP_ID.fetch_add(1, Ordering::Relaxed) & 0xFFFF) as u16;
    frame.extend_from_slice(&[0x45, 0x00]);
    frame.extend_from_slice(&(total_len as u16).to_be_bytes());
    frame.extend_from_slice(&id.to_be_bytes());
    frame.extend_from_slice(&[0x00, 0x00]); // flags/frag
    frame.extend_from_slice(&[64, 17]); // ttl, proto=UDP
    frame.extend_from_slice(&[0x00, 0x00]); // checksum placeholder
    frame.extend_from_slice(&src_ip);
    frame.extend_from_slice(&dst_ip);
    let csum = checksum16(&frame[ip_start..ip_start + 20]);
    frame[ip_start + 10..ip_start + 12].copy_from_slice(&csum.to_be_bytes());

    frame.extend_from_slice(&src_port.to_be_bytes());
    frame.extend_from_slice(&dst_port.to_be_bytes());
    frame.extend_from_slice(&(udp_len as u16).to_be_bytes());
    frame.extend_from_slice(&[0x00, 0x00]); // UDP checksum 0 (disabled, IPv4)
    frame.extend_from_slice(payload);
    frame
}

/// BOOTP/DHCP message wrapped in a broadcast IPv4/UDP/Ethernet frame. A set
/// `ciaddr` is the RENEWING/REBINDING request form (no option 50/54 with it).
fn build_dhcp(
    mac: [u8; 6],
    xid: u32,
    msg_type: u8,
    request_ip: Option<[u8; 4]>,
    server_id: Option<[u8; 4]>,
    ciaddr: Option<[u8; 4]>,
) -> Vec<u8> {
    let mut d = Vec::with_capacity(300);
    d.extend_from_slice(&[1, 1, 6, 0]); // op=request, htype=eth, hlen=6, hops=0
    d.extend_from_slice(&xid.to_be_bytes());
    d.extend_from_slice(&[0, 0]); // secs
    d.extend_from_slice(&[0x80, 0x00]); // flags: broadcast
    d.extend_from_slice(&ciaddr.unwrap_or([0; 4]));
    d.extend_from_slice(&[0; 4]); // yiaddr
    d.extend_from_slice(&[0; 4]); // siaddr
    d.extend_from_slice(&[0; 4]); // giaddr
    d.extend_from_slice(&mac);
    d.extend_from_slice(&[0; 10]); // chaddr padding
    d.extend_from_slice(&[0; 64]); // sname
    d.extend_from_slice(&[0; 128]); // file
    d.extend_from_slice(&[0x63, 0x82, 0x53, 0x63]); // magic cookie
    d.extend_from_slice(&[53, 1, msg_type]); // DHCP message type
    if let Some(ip) = request_ip {
        d.push(50);
        d.push(4);
        d.extend_from_slice(&ip);
    }
    if let Some(sid) = server_id {
        d.push(54);
        d.push(4);
        d.extend_from_slice(&sid);
    }
    d.extend_from_slice(&[55, 3, 1, 3, 6]); // param request: mask, router, dns
    d.push(255); // end

    // UDP 68 -> 67, broadcast.
    build_udp_frame(mac, [0xFF; 6], [0; 4], [255; 4], 68, 67, &d)
}

/// A DHCP reply's address fields and lease duration.
#[derive(Clone, Copy)]
struct Lease4 {
    yiaddr: [u8; 4],
    server: [u8; 4],
    mask: [u8; 4],
    gateway: [u8; 4],
    lease_secs: u32,
}

/// Parse a DHCP reply frame into its message type and address fields.
fn parse_dhcp(frame: &[u8], xid: u32) -> Option<(u8, Lease4)> {
    if frame.len() < 14 + 20 + 8 + 240 {
        return None;
    }
    if u16::from_be_bytes([frame[12], frame[13]]) != 0x0800 {
        return None; // not IPv4
    }
    let ip = &frame[14..];
    let ihl = ((ip[0] & 0x0F) as usize) * 4;
    if ip[9] != 17 {
        return None; // not UDP
    }
    let udp = &ip[ihl..];
    let dport = u16::from_be_bytes([udp[2], udp[3]]);
    if dport != 68 {
        return None; // not a DHCP client reply
    }
    let dhcp = &udp[8..];
    if dhcp.len() < 240 || u32::from_be_bytes([dhcp[4], dhcp[5], dhcp[6], dhcp[7]]) != xid {
        return None;
    }
    if dhcp[236..240] != [0x63, 0x82, 0x53, 0x63] {
        return None;
    }
    let mut l = Lease4 {
        yiaddr: [0; 4],
        server: [0; 4],
        mask: [0; 4],
        gateway: [0; 4],
        lease_secs: 0,
    };
    l.yiaddr.copy_from_slice(&dhcp[16..20]);
    let mut msg_type = 0u8;
    let mut i = 240;
    while i < dhcp.len() {
        let opt = dhcp[i];
        if opt == 255 {
            break;
        }
        if opt == 0 {
            i += 1;
            continue;
        }
        if i + 1 >= dhcp.len() {
            break;
        }
        let len = dhcp[i + 1] as usize;
        let val_start = i + 2;
        if val_start + len > dhcp.len() {
            break;
        }
        let val = &dhcp[val_start..val_start + len];
        match opt {
            53 if len == 1 => msg_type = val[0],
            54 if len == 4 => l.server.copy_from_slice(val),
            1 if len == 4 => l.mask.copy_from_slice(val),
            3 if len >= 4 => l.gateway.copy_from_slice(&val[..4]),
            51 if len == 4 => l.lease_secs = u32::from_be_bytes([val[0], val[1], val[2], val[3]]),
            _ => {}
        }
        i = val_start + len;
    }
    Some((msg_type, l))
}

/// Retransmit windows for one DORA step. A single send loses the exchange
/// whenever the reply misses our receive window — the firmware's own MNP polls
/// the same queue, so one-shot is not enough.
const DORA_WINDOWS_MS: [u64; 3] = [2_000, 3_000, 5_000];

/// Shorter windows for a background renewal, which holds the run loop.
const RENEW_WINDOWS_MS: [u64; 2] = [1_500, 2_500];

/// Broadcast `frame` and wait for a `want`-typed DHCP reply, retransmitting per
/// `windows`. Logs the frame tally, which separates "nothing reaches us at all"
/// from "traffic flows but the server never answered".
fn await_reply(
    link: &mut Link,
    frame: &[u8],
    xid: u32,
    want: u8,
    label: &str,
    windows: &[u64],
) -> Option<Lease4> {
    let mut buf = [0u8; 2048];
    let mut seen = 0usize;
    for (try_n, window) in windows.iter().enumerate() {
        if let Err(e) = link.transmit(frame) {
            logln(format!("netraw: {label} tx{try_n} ERR {e}"));
            continue;
        }
        let mut waited = 0u64;
        while waited < *window {
            let (got, spent) = link.recv(&mut buf, window - waited);
            // 1ms floor bounds the loop when frames arrive with no wait.
            waited += spent.max(1);
            let Some(n) = got else { break };
            seen += 1;
            if let Some((mt, lease)) = parse_dhcp(&buf[..n], xid)
                && mt == want
            {
                logln(format!("netraw: {label} on try {try_n} ({seen} frames seen)"));
                return Some(lease);
            }
        }
    }
    logln(format!("netraw: {label} timeout after {} tries, {seen} frames seen", windows.len()));
    None
}

/// Acquire an IPv4 lease over the raw path (DHCP DORA).
pub fn dhcp() -> Result<RawNet, String> {
    let mut link = Link::open()?;
    let mac = link.mac();
    logln(format!(
        "netraw: dhcp media={} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        link.media_present(),
        mac[0],
        mac[1],
        mac[2],
        mac[3],
        mac[4],
        mac[5]
    ));
    let xid = u32::from_be_bytes([mac[2], mac[3], mac[4], mac[5]]);

    let discover = build_dhcp(mac, xid, 1, None, None, None);
    let Some(offer) = await_reply(&mut link, &discover, xid, 2, "OFFER", &DORA_WINDOWS_MS) else {
        return Err("no DHCP OFFER (raw path)".into());
    };
    logln(format!("netraw: OFFER {} from server {}", ip_str(offer.yiaddr), ip_str(offer.server)));

    let request = build_dhcp(mac, xid, 3, Some(offer.yiaddr), Some(offer.server), None);
    let Some(ack) = await_reply(&mut link, &request, xid, 5, "ACK", &DORA_WINDOWS_MS) else {
        return Err("no DHCP ACK (raw path)".into());
    };
    let pick = |a: [u8; 4], o: [u8; 4]| if a != [0; 4] { a } else { o };
    let rn = RawNet {
        mac,
        ip: offer.yiaddr,
        mask: pick(ack.mask, offer.mask),
        gateway: pick(ack.gateway, offer.gateway),
        server: pick(ack.server, offer.server),
        obtained_ms: crate::mono::now_ms(),
        lease_secs: if ack.lease_secs != 0 { ack.lease_secs } else { offer.lease_secs },
    };
    logln(format!(
        "netraw: ACK {} mask {} gw {} lease {}s",
        ip_str(rn.ip),
        ip_str(rn.mask),
        ip_str(rn.gateway),
        rn.lease_secs
    ));
    set_current(rn);
    Ok(rn)
}

/// Renew an existing lease: a REQUEST in the RENEWING form (ciaddr set, no
/// option 50/54). Broadcast, with the broadcast reply flag set, so the ACK
/// survives the firmware stack draining unicast frames from the same queue.
pub fn renew(rn: &RawNet) -> Result<RawNet, String> {
    let mut link = Link::open()?;
    let xid = u32::from_be_bytes([rn.mac[2], rn.mac[3], rn.mac[4], rn.mac[5]]);
    let request = build_dhcp(rn.mac, xid, 3, None, None, Some(rn.ip));
    let Some(ack) = await_reply(&mut link, &request, xid, 5, "RENEW", &RENEW_WINDOWS_MS) else {
        return Err("no renewal ACK".into());
    };
    let keep = |a: [u8; 4], old: [u8; 4]| if a != [0; 4] { a } else { old };
    let renewed = RawNet {
        mac: rn.mac,
        ip: keep(ack.yiaddr, rn.ip),
        mask: keep(ack.mask, rn.mask),
        gateway: keep(ack.gateway, rn.gateway),
        server: keep(ack.server, rn.server),
        obtained_ms: crate::mono::now_ms(),
        lease_secs: if ack.lease_secs != 0 { ack.lease_secs } else { rn.lease_secs },
    };
    if renewed.ip != rn.ip {
        logln(format!("netraw: renewal moved {} -> {}", ip_str(rn.ip), ip_str(renewed.ip)));
    } else {
        logln(format!("netraw: lease renewed {} for {}s", ip_str(renewed.ip), renewed.lease_secs));
    }
    set_current(renewed);
    Ok(renewed)
}

fn build_arp_request(src_mac: [u8; 6], src_ip: [u8; 4], target_ip: [u8; 4]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(42);
    eth_header(&mut frame, [0xFF; 6], src_mac, 0x0806);
    frame.extend_from_slice(&[0x00, 0x01]); // htype: ethernet
    frame.extend_from_slice(&[0x08, 0x00]); // ptype: IPv4
    frame.extend_from_slice(&[6, 4]); // hlen, plen
    frame.extend_from_slice(&[0x00, 0x01]); // op: request
    frame.extend_from_slice(&src_mac);
    frame.extend_from_slice(&src_ip);
    frame.extend_from_slice(&[0; 6]); // target mac (unknown)
    frame.extend_from_slice(&target_ip);
    frame
}

/// An ARP reply asserting `from.ip is at from.mac`, addressed to `to`. Injected
/// into a local stack whose own ARP request never draws a reply back.
pub fn build_arp_reply(from: Origin, to: Origin) -> Vec<u8> {
    let mut frame = Vec::with_capacity(42);
    eth_header(&mut frame, to.mac, from.mac, 0x0806);
    frame.extend_from_slice(&[0x00, 0x01]); // htype: ethernet
    frame.extend_from_slice(&[0x08, 0x00]); // ptype: IPv4
    frame.extend_from_slice(&[6, 4]); // hlen, plen
    frame.extend_from_slice(&[0x00, 0x02]); // op: reply
    frame.extend_from_slice(&from.mac);
    frame.extend_from_slice(&from.ip);
    frame.extend_from_slice(&to.mac);
    frame.extend_from_slice(&to.ip);
    frame
}

/// Parse an ARP reply for `want_ip`, returning its MAC.
fn parse_arp_reply(frame: &[u8], want_ip: [u8; 4]) -> Option<[u8; 6]> {
    if frame.len() < 42 || u16::from_be_bytes([frame[12], frame[13]]) != 0x0806 {
        return None;
    }
    let a = &frame[14..];
    if u16::from_be_bytes([a[6], a[7]]) != 2 {
        return None; // not a reply
    }
    if a[14..18] != want_ip {
        return None; // sender ip != who we asked
    }
    let mut mac = [0u8; 6];
    mac.copy_from_slice(&a[8..14]);
    Some(mac)
}

impl RawNet {
    fn same_subnet(&self, ip: [u8; 4]) -> bool {
        (0..4).all(|i| (ip[i] & self.mask[i]) == (self.ip[i] & self.mask[i]))
    }

    /// ARP the next hop toward `ip` (the host on-subnet, else the gateway).
    fn resolve(&self, link: &mut Link, ip: [u8; 4]) -> Result<[u8; 6], String> {
        let target = if self.same_subnet(ip) { ip } else { self.gateway };
        let mut buf = [0u8; 2048];
        for _ in 0..4 {
            link.transmit(&build_arp_request(self.mac, self.ip, target))
                .map_err(|e| format!("ARP tx: {e}"))?;
            let mut waited = 0u64;
            while waited < 1000 {
                let (got, spent) = link.recv(&mut buf, 1000 - waited);
                // 1ms floor bounds the loop when frames arrive with no wait.
                waited += spent.max(1);
                let Some(n) = got else { break };
                if let Some(mac) = parse_arp_reply(&buf[..n], target) {
                    return Ok(mac);
                }
            }
        }
        Err(format!("ARP timeout for {}", ip_str(target)))
    }

    /// Send `payload` to `dst_ip:dst_port` as chunked UDP. Returns chunk count.
    pub fn send_udp(&self, dst_ip: [u8; 4], dst_port: u16, payload: &[u8]) -> Result<usize, String> {
        let mut link = Link::open()?;
        let dst_mac = self.resolve(&mut link, dst_ip)?;
        let total = payload.len().div_ceil(CHUNK_DATA).max(1) as u16;
        let msg_id = MSG_ID.fetch_add(1, Ordering::Relaxed);
        for (seq, chunk) in payload.chunks(CHUNK_DATA).enumerate() {
            let mut body = Vec::with_capacity(12 + chunk.len());
            body.extend_from_slice(&CHUNK_MAGIC);
            body.extend_from_slice(&msg_id.to_be_bytes());
            body.extend_from_slice(&(seq as u16).to_be_bytes());
            body.extend_from_slice(&total.to_be_bytes());
            body.extend_from_slice(chunk);
            let frame = build_udp_frame(self.mac, dst_mac, self.ip, dst_ip, dst_port, dst_port, &body);
            link.transmit(&frame).map_err(|e| format!("UDP tx seq {seq}: {e}"))?;
            boot::stall(Duration::from_millis(2));
        }
        Ok(total as usize)
    }
}

/// Extract the sender's origin and the UDP payload from a raw Ethernet frame
/// destined for `want_port`. Mirrors `parse_dhcp`'s layering (Eth → IPv4 → UDP)
/// without the DHCP specifics. The origin is observed, not claimed, so it is
/// the one host in a datagram known to be reachable at layer 2.
fn udp_payload_for_port(frame: &[u8], want_port: u16) -> Option<(Origin, &[u8])> {
    if frame.len() < 14 + 20 + 8 {
        return None;
    }
    if u16::from_be_bytes([frame[12], frame[13]]) != 0x0800 {
        return None; // not IPv4
    }
    let ip = &frame[14..];
    let ihl = ((ip[0] & 0x0F) as usize) * 4;
    if ihl < 20 || ip.len() < ihl + 8 || ip[9] != 17 {
        return None; // not UDP / truncated
    }
    let total_len = u16::from_be_bytes([ip[2], ip[3]]) as usize;
    if total_len < ihl + 8 || total_len > ip.len() {
        return None;
    }
    let udp = &ip[ihl..];
    if u16::from_be_bytes([udp[2], udp[3]]) != want_port {
        return None; // dst port mismatch
    }
    let udp_len = u16::from_be_bytes([udp[4], udp[5]]) as usize;
    if udp_len < 8 || udp_len > udp.len() {
        return None;
    }
    let mut src_ip = [0u8; 4];
    src_ip.copy_from_slice(&ip[12..16]);
    let mut mac = [0u8; 6];
    mac.copy_from_slice(&frame[6..12]);
    Some((Origin { mac, ip: src_ip }, &udp[8..udp_len]))
}

/// A beacon and the origin its frame actually came from.
pub struct Sighting {
    pub beacon: tcp_protocol::preboot::Beacon,
    pub src: Origin,
}

/// Listen for a console discovery beacon on `port` for up to `budget_ms` and
/// return the advertised console endpoint plus the relay url a v2 beacon
/// carries, alongside the sender's observed address. Pure SNP receive (no UEFI
/// IP4/UDP4 stack), so it works on the raw path; best-effort when the UEFI
/// stack owns the NIC. Returns None on timeout or if no beacon parsed.
pub fn discover_console(port: u16, budget_ms: u64) -> Option<Sighting> {
    let mut link = Link::open().ok()?;
    let mut buf = [0u8; 2048];
    let mut waited = 0u64;
    while waited < budget_ms {
        let (got, spent) = link.recv(&mut buf, budget_ms - waited);
        // 1ms floor bounds the loop when frames arrive with no wait.
        waited += spent.max(1);
        let Some(n) = got else {
            break;
        };
        if let Some((src, payload)) = udp_payload_for_port(&buf[..n], port) {
            if let Some(b) = tcp_protocol::preboot::parse_beacon_v2(payload) {
                note_neighbor(src);
                match b.relay.as_deref() {
                    Some(relay) => logln(format!(
                        "netraw: discovery beacon from {} -> {} (relay {relay})",
                        ip_str(src.ip),
                        b.addr
                    )),
                    None => logln(format!(
                        "netraw: discovery beacon v1 from {} -> {}",
                        ip_str(src.ip),
                        b.addr
                    )),
                }
                return Some(Sighting { beacon: b, src });
            }
        }
    }
    None
}

/// Parse an IPv4 literal `a.b.c.d` (ignores any trailing `:port`).
pub fn parse_ipv4(host: &str) -> Option<[u8; 4]> {
    let host = host.split(':').next().unwrap_or(host);
    let mut out = [0u8; 4];
    let mut parts = host.split('.');
    for o in out.iter_mut() {
        *o = parts.next()?.parse::<u8>().ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(out)
}
