//! Raw IPv4 over SimpleNetwork for firmware that won't instantiate its UEFI
//! IPv4 stack (Ip4Dxe/Tcp4 absent). Hand-builds Ethernet/IPv4/UDP frames:
//! DHCP (DORA) for an address, ARP to resolve the next hop, then a chunked UDP
//! upload of the fingerprint. No Ip4Config2/Tcp4/Http dependency.

use core::sync::atomic::{AtomicU32, Ordering};
use core::time::Duration;

use uefi::boot;
use uefi::proto::network::snp::{NetworkState, ReceiveFlags, SimpleNetwork};

use crate::logln;
use crate::protoguard::{self, Held};

/// Orchestrator UDP port for the raw fingerprint upload (axum_server listener).
pub const UDP_PORT: u16 = 9202;
/// Per-datagram payload header magic.
const CHUNK_MAGIC: [u8; 4] = *b"MTUF";
/// Max fingerprint bytes per datagram (1500 MTU − 20 IP − 8 UDP − 12 header).
const CHUNK_DATA: usize = 1400;

static MSG_ID: AtomicU32 = AtomicU32::new(1);
static IP_ID: AtomicU32 = AtomicU32::new(0x4d54);

/// A DHCP-acquired IPv4 configuration on the raw SNP path.
#[derive(Clone, Copy)]
pub struct RawNet {
    pub mac: [u8; 6],
    pub ip: [u8; 4],
    pub mask: [u8; 4],
    pub gateway: [u8; 4],
}

pub fn ip_str(o: [u8; 4]) -> String {
    format!("{}.{}.{}.{}", o[0], o[1], o[2], o[3])
}

pub(crate) fn open_snp() -> Result<Held<SimpleNetwork>, String> {
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

/// BOOTP/DHCP message wrapped in a broadcast IPv4/UDP/Ethernet frame.
fn build_dhcp(mac: [u8; 6], xid: u32, msg_type: u8, request_ip: Option<[u8; 4]>, server_id: Option<[u8; 4]>) -> Vec<u8> {
    let mut d = Vec::with_capacity(300);
    d.extend_from_slice(&[1, 1, 6, 0]); // op=request, htype=eth, hlen=6, hops=0
    d.extend_from_slice(&xid.to_be_bytes());
    d.extend_from_slice(&[0, 0]); // secs
    d.extend_from_slice(&[0x80, 0x00]); // flags: broadcast
    d.extend_from_slice(&[0; 4]); // ciaddr
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

/// A DHCP reply's address fields: (yiaddr, server_id, mask, gateway).
type ReplyAddrs = ([u8; 4], [u8; 4], [u8; 4], [u8; 4]);

/// Parse a DHCP reply frame into its message type and address fields.
fn parse_dhcp(frame: &[u8], xid: u32) -> Option<(u8, ReplyAddrs)> {
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
    let mut yiaddr = [0u8; 4];
    yiaddr.copy_from_slice(&dhcp[16..20]);
    let (mut msg_type, mut server_id, mut mask, mut gateway) = (0u8, [0u8; 4], [0u8; 4], [0u8; 4]);
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
            54 if len == 4 => server_id.copy_from_slice(val),
            1 if len == 4 => mask.copy_from_slice(val),
            3 if len >= 4 => gateway.copy_from_slice(&val[..4]),
            _ => {}
        }
        i = val_start + len;
    }
    Some((msg_type, (yiaddr, server_id, mask, gateway)))
}

/// Retransmit windows for one DORA step. A single send loses the exchange
/// whenever the reply misses our receive window — the firmware's own MNP polls
/// the same queue, so one-shot is not enough.
const DORA_WINDOWS_MS: [u64; 3] = [2_000, 3_000, 5_000];

/// Broadcast `frame` and wait for a `want`-typed DHCP reply, retransmitting per
/// [`DORA_WINDOWS_MS`]. Logs the frame tally, which separates "nothing reaches
/// us at all" from "traffic flows but the server never answered".
fn await_reply(
    snp: &SimpleNetwork,
    frame: &[u8],
    xid: u32,
    want: u8,
    label: &str,
) -> Option<ReplyAddrs> {
    let mut buf = [0u8; 2048];
    let mut seen = 0usize;
    for (try_n, window) in DORA_WINDOWS_MS.iter().enumerate() {
        if let Err(e) = snp.transmit(0, frame, None, None, None) {
            logln(format!("netraw: {label} tx{try_n} ERR {e:?}"));
            continue;
        }
        let mut waited = 0u64;
        while waited < *window {
            let (got, spent) = recv_frame(snp, &mut buf, window - waited);
            // 1ms floor bounds the loop when frames arrive with no wait.
            waited += spent.max(1);
            let Some(n) = got else { break };
            seen += 1;
            if let Some((mt, addrs)) = parse_dhcp(&buf[..n], xid)
                && mt == want
            {
                logln(format!("netraw: {label} on try {try_n} ({seen} frames seen)"));
                return Some(addrs);
            }
        }
    }
    logln(format!("netraw: {label} timeout after {} tries, {seen} frames seen", DORA_WINDOWS_MS.len()));
    None
}

/// Acquire an IPv4 lease over raw SNP (DHCP DORA).
pub fn dhcp() -> Result<RawNet, String> {
    let snp = open_snp()?;
    let mode = snp.mode();
    let m = mode.current_address.0;
    let mac = [m[0], m[1], m[2], m[3], m[4], m[5]];
    logln(format!(
        "netraw: snp state={:?} media={} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mode.state,
        bool::from(mode.media_present),
        mac[0],
        mac[1],
        mac[2],
        mac[3],
        mac[4],
        mac[5]
    ));
    let xid = u32::from_be_bytes([mac[2], mac[3], mac[4], mac[5]]);

    let discover = build_dhcp(mac, xid, 1, None, None);
    let Some((yiaddr, server_id, mut mask, mut gateway)) =
        await_reply(&snp, &discover, xid, 2, "OFFER")
    else {
        return Err("no DHCP OFFER (raw SNP)".into());
    };
    logln(format!("netraw: OFFER {} from server {}", ip_str(yiaddr), ip_str(server_id)));

    let request = build_dhcp(mac, xid, 3, Some(yiaddr), Some(server_id));
    let Some((_, _, mk, gw)) = await_reply(&snp, &request, xid, 5, "ACK") else {
        return Err("no DHCP ACK (raw SNP)".into());
    };
    if mk != [0; 4] {
        mask = mk;
    }
    if gw != [0; 4] {
        gateway = gw;
    }
    logln(format!("netraw: ACK {} mask {} gw {}", ip_str(yiaddr), ip_str(mask), ip_str(gateway)));
    Ok(RawNet { mac, ip: yiaddr, mask, gateway })
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
    fn resolve(&self, snp: &SimpleNetwork, ip: [u8; 4]) -> Result<[u8; 6], String> {
        let target = if self.same_subnet(ip) { ip } else { self.gateway };
        let mut buf = [0u8; 2048];
        for _ in 0..4 {
            snp.transmit(0, &build_arp_request(self.mac, self.ip, target), None, None, None)
                .map_err(|e| format!("ARP tx: {e:?}"))?;
            let mut waited = 0u64;
            while waited < 1000 {
                let (got, spent) = recv_frame(snp, &mut buf, 1000 - waited);
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
        let snp = open_snp()?;
        let dst_mac = self.resolve(&snp, dst_ip)?;
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
            snp.transmit(0, &frame, None, None, None)
                .map_err(|e| format!("UDP tx seq {seq}: {e:?}"))?;
            let _ = boot::stall(Duration::from_millis(2));
        }
        Ok(total as usize)
    }
}

/// Extract the UDP payload from a raw Ethernet frame destined for `want_port`.
/// Mirrors `parse_dhcp`'s layering (Eth → IPv4 → UDP) without the DHCP specifics.
fn udp_payload_for_port(frame: &[u8], want_port: u16) -> Option<&[u8]> {
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
    Some(&udp[8..udp_len])
}

/// Listen for a console discovery beacon on `port` for up to `budget_ms` and
/// return the advertised console endpoint plus the relay url a v2 beacon
/// carries. Pure SNP receive (no UEFI IP4/UDP4 stack), so it works on the raw
/// path; best-effort when the UEFI stack owns the NIC. Returns None on timeout
/// or if no beacon parsed.
pub fn discover_console(port: u16, budget_ms: u64) -> Option<tcp_protocol::preboot::Beacon> {
    let snp = open_snp().ok()?;
    let mut buf = [0u8; 2048];
    let mut waited = 0u64;
    while waited < budget_ms {
        let (got, spent) = recv_frame(&snp, &mut buf, budget_ms - waited);
        // 1ms floor bounds the loop when frames arrive with no wait.
        waited += spent.max(1);
        let Some(n) = got else {
            break;
        };
        if let Some(payload) = udp_payload_for_port(&buf[..n], port) {
            if let Some(b) = tcp_protocol::preboot::parse_beacon_v2(payload) {
                match b.relay.as_deref() {
                    Some(relay) => logln(format!(
                        "netraw: discovery beacon -> {} (relay {relay})",
                        b.addr
                    )),
                    None => logln(format!("netraw: discovery beacon v1 -> {}", b.addr)),
                }
                return Some(b);
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
