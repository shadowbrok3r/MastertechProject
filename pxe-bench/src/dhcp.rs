//! Minimal ProxyDHCP for PXE: offers boot-server info alongside the shop's
//! real DHCP without allocating addresses.
//!
//! Port 67: answers PXEClient DHCPDISCOVERs with an addressless OFFER carrying
//! next-server + boot file. Port 4011: answers the PXE client's follow-up
//! REQUEST with an ACK repeating the boot file. iPXE re-DHCP is detected via
//! user-class and redirected to the HTTP chain script.

use std::net::{Ipv4Addr, SocketAddrV4};

use tokio::net::UdpSocket;

use crate::config::Config;

const BOOTREQUEST: u8 = 1;
const BOOTREPLY: u8 = 2;
const DHCP_MAGIC: [u8; 4] = [99, 130, 83, 99];

const OPT_MSG_TYPE: u8 = 53;
const OPT_SERVER_ID: u8 = 54;
const OPT_VENDOR_CLASS: u8 = 60;
const OPT_USER_CLASS: u8 = 77;
const OPT_CLIENT_ARCH: u8 = 93;
const OPT_UUID: u8 = 97;
const OPT_END: u8 = 255;

const MSG_DISCOVER: u8 = 1;
const MSG_OFFER: u8 = 2;
const MSG_REQUEST: u8 = 3;
const MSG_ACK: u8 = 5;

#[derive(Debug, Clone)]
pub struct DhcpPacket {
    pub op: u8,
    pub htype: u8,
    pub hlen: u8,
    pub xid: [u8; 4],
    pub flags: [u8; 2],
    pub giaddr: [u8; 4],
    pub chaddr: [u8; 16],
    pub msg_type: Option<u8>,
    pub vendor_class: Option<String>,
    pub user_class: Option<String>,
    pub client_arch: Option<u16>,
    pub uuid: Option<Vec<u8>>,
}

impl DhcpPacket {
    pub fn parse(buf: &[u8]) -> Option<Self> {
        if buf.len() < 240 || buf[236..240] != DHCP_MAGIC {
            return None;
        }
        let mut p = Self {
            op: buf[0],
            htype: buf[1],
            hlen: buf[2],
            xid: buf[4..8].try_into().ok()?,
            flags: buf[10..12].try_into().ok()?,
            giaddr: buf[24..28].try_into().ok()?,
            chaddr: buf[28..44].try_into().ok()?,
            msg_type: None,
            vendor_class: None,
            user_class: None,
            client_arch: None,
            uuid: None,
        };
        let mut i = 240;
        while i + 1 < buf.len() {
            let code = buf[i];
            if code == OPT_END {
                break;
            }
            if code == 0 {
                i += 1;
                continue;
            }
            let len = buf[i + 1] as usize;
            if i + 2 + len > buf.len() {
                break;
            }
            let val = &buf[i + 2..i + 2 + len];
            match code {
                OPT_MSG_TYPE if len >= 1 => p.msg_type = Some(val[0]),
                OPT_VENDOR_CLASS => {
                    p.vendor_class = Some(String::from_utf8_lossy(val).into_owned())
                }
                OPT_USER_CLASS => p.user_class = Some(String::from_utf8_lossy(val).into_owned()),
                OPT_CLIENT_ARCH if len >= 2 => {
                    p.client_arch = Some(u16::from_be_bytes([val[0], val[1]]))
                }
                OPT_UUID => p.uuid = Some(val.to_vec()),
                _ => {}
            }
            i += 2 + len;
        }
        Some(p)
    }

    pub fn is_pxe_request(&self) -> bool {
        self.op == BOOTREQUEST
            && self
                .vendor_class
                .as_deref()
                .map(|v| v.starts_with("PXEClient"))
                .unwrap_or(false)
    }

    pub fn is_ipxe(&self) -> bool {
        self.user_class.as_deref() == Some("iPXE")
    }

    pub fn mac_string(&self) -> String {
        self.chaddr[..self.hlen.min(6) as usize]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(":")
    }
}

/// Boot file for this client: iPXE re-DHCP chains to HTTP, firmware gets a binary by arch.
pub fn boot_file_for(packet: &DhcpPacket, cfg: &Config) -> String {
    if packet.is_ipxe() {
        return cfg.ipxe_script_url();
    }
    match packet.client_arch.unwrap_or(0) {
        0 => cfg.bios_boot_file.clone(),
        _ => cfg.uefi_boot_file.clone(),
    }
}

/// Build a BOOTREPLY (OFFER/ACK) that carries boot info but no address lease.
pub fn build_reply(req: &DhcpPacket, msg_type: u8, server_ip: Ipv4Addr, boot_file: &str) -> Vec<u8> {
    let mut b = vec![0u8; 240];
    b[0] = BOOTREPLY;
    b[1] = req.htype;
    b[2] = req.hlen;
    b[4..8].copy_from_slice(&req.xid);
    b[10..12].copy_from_slice(&req.flags);
    b[20..24].copy_from_slice(&server_ip.octets()); // siaddr
    b[24..28].copy_from_slice(&req.giaddr);
    b[28..44].copy_from_slice(&req.chaddr);
    // sname empty; file field carries the boot file for firmware clients.
    let file_bytes = boot_file.as_bytes();
    let file_len = file_bytes.len().min(127);
    b[108..108 + file_len].copy_from_slice(&file_bytes[..file_len]);
    b[236..240].copy_from_slice(&DHCP_MAGIC);

    b.extend_from_slice(&[OPT_MSG_TYPE, 1, msg_type]);
    b.extend_from_slice(&[OPT_SERVER_ID, 4]);
    b.extend_from_slice(&server_ip.octets());
    let vc = b"PXEClient";
    b.extend_from_slice(&[OPT_VENDOR_CLASS, vc.len() as u8]);
    b.extend_from_slice(vc);
    if let Some(uuid) = &req.uuid {
        let len = uuid.len().min(255);
        b.extend_from_slice(&[OPT_UUID, len as u8]);
        b.extend_from_slice(&uuid[..len]);
    }
    // PXE vendor options: discovery control 8 = boot straight to the file.
    b.extend_from_slice(&[43, 4, 6, 1, 8, 255]);
    b.push(OPT_END);
    b
}

async fn reply_to(sock: &UdpSocket, req: &DhcpPacket, from: SocketAddrV4, reply: &[u8]) {
    // Relayed requests go back via the relay; broadcast otherwise.
    let dest = if req.giaddr != [0, 0, 0, 0] {
        SocketAddrV4::new(Ipv4Addr::from(req.giaddr), 67)
    } else if from.ip().is_unspecified() || from.ip() == &Ipv4Addr::BROADCAST {
        SocketAddrV4::new(Ipv4Addr::BROADCAST, 68)
    } else {
        SocketAddrV4::new(*from.ip(), from.port())
    };
    if let Err(e) = sock.send_to(reply, dest).await {
        log::warn!("dhcp: send to {dest} failed: {e}");
    }
}

/// ProxyDHCP listener on :67 — addressless OFFERs to PXE DISCOVERs.
pub async fn serve_proxy_dhcp(cfg: Config) -> anyhow::Result<()> {
    let sock = UdpSocket::bind(("0.0.0.0", 67)).await?;
    sock.set_broadcast(true)?;
    log::info!("proxyDHCP listening on :67 (next-server {})", cfg.server_ip);
    let mut buf = vec![0u8; 1500];
    loop {
        let (n, from) = sock.recv_from(&mut buf).await?;
        let std::net::SocketAddr::V4(from) = from else { continue };
        let Some(req) = DhcpPacket::parse(&buf[..n]) else { continue };
        if !req.is_pxe_request() || req.msg_type != Some(MSG_DISCOVER) {
            continue;
        }
        let boot_file = boot_file_for(&req, &cfg);
        log::info!(
            "PXE DISCOVER from {} (arch {:?}, ipxe {}) -> offering {}",
            req.mac_string(),
            req.client_arch,
            req.is_ipxe(),
            boot_file
        );
        let reply = build_reply(&req, MSG_OFFER, cfg.server_ip, &boot_file);
        reply_to(&sock, &req, from, &reply).await;
    }
}

/// PXE boot-server listener on :4011 — ACKs the follow-up REQUEST with the boot file.
pub async fn serve_pxe_ack(cfg: Config) -> anyhow::Result<()> {
    let sock = UdpSocket::bind(("0.0.0.0", 4011)).await?;
    sock.set_broadcast(true)?;
    log::info!("PXE boot server listening on :4011");
    let mut buf = vec![0u8; 1500];
    loop {
        let (n, from) = sock.recv_from(&mut buf).await?;
        let std::net::SocketAddr::V4(from) = from else { continue };
        let Some(req) = DhcpPacket::parse(&buf[..n]) else { continue };
        if !req.is_pxe_request() || !matches!(req.msg_type, Some(MSG_REQUEST) | Some(MSG_DISCOVER))
        {
            continue;
        }
        let boot_file = boot_file_for(&req, &cfg);
        log::info!(
            "PXE REQUEST from {} -> ACK {}",
            req.mac_string(),
            boot_file
        );
        let reply = build_reply(&req, MSG_ACK, cfg.server_ip, &boot_file);
        reply_to(&sock, &req, from, &reply).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn discover(vendor: &[u8], arch: Option<u16>, user_class: Option<&[u8]>) -> Vec<u8> {
        let mut b = vec![0u8; 240];
        b[0] = BOOTREQUEST;
        b[1] = 1;
        b[2] = 6;
        b[4..8].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        b[28..34].copy_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        b[236..240].copy_from_slice(&DHCP_MAGIC);
        b.extend_from_slice(&[OPT_MSG_TYPE, 1, MSG_DISCOVER]);
        b.extend_from_slice(&[OPT_VENDOR_CLASS, vendor.len() as u8]);
        b.extend_from_slice(vendor);
        if let Some(a) = arch {
            b.extend_from_slice(&[OPT_CLIENT_ARCH, 2]);
            b.extend_from_slice(&a.to_be_bytes());
        }
        if let Some(uc) = user_class {
            b.extend_from_slice(&[OPT_USER_CLASS, uc.len() as u8]);
            b.extend_from_slice(uc);
        }
        b.push(OPT_END);
        b
    }

    fn test_config() -> Config {
        toml::from_str(r#"server_ip = "10.10.0.2""#).unwrap()
    }

    #[test]
    fn parses_pxe_discover() {
        let raw = discover(b"PXEClient:Arch:00007", Some(7), None);
        let p = DhcpPacket::parse(&raw).unwrap();
        assert!(p.is_pxe_request());
        assert_eq!(p.msg_type, Some(MSG_DISCOVER));
        assert_eq!(p.client_arch, Some(7));
        assert_eq!(p.mac_string(), "aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn arch_selects_boot_file() {
        let cfg = test_config();
        let bios = DhcpPacket::parse(&discover(b"PXEClient", Some(0), None)).unwrap();
        assert_eq!(boot_file_for(&bios, &cfg), "undionly.kpxe");
        let uefi = DhcpPacket::parse(&discover(b"PXEClient", Some(7), None)).unwrap();
        assert_eq!(boot_file_for(&uefi, &cfg), "snponly.efi");
        let ipxe = DhcpPacket::parse(&discover(b"PXEClient", Some(7), Some(b"iPXE"))).unwrap();
        assert_eq!(boot_file_for(&ipxe, &cfg), "http://10.10.0.2:7777/boot.ipxe");
    }

    #[test]
    fn reply_round_trips() {
        let cfg = test_config();
        let req = DhcpPacket::parse(&discover(b"PXEClient", Some(7), None)).unwrap();
        let reply = build_reply(&req, MSG_OFFER, cfg.server_ip, "ipxe.efi");
        let parsed = DhcpPacket::parse(&reply).unwrap();
        assert_eq!(parsed.op, BOOTREPLY);
        assert_eq!(parsed.xid, req.xid);
        assert_eq!(parsed.msg_type, Some(MSG_OFFER));
        assert_eq!(&reply[108..116], b"ipxe.efi");
    }

    #[test]
    fn ignores_non_dhcp() {
        assert!(DhcpPacket::parse(&[0u8; 100]).is_none());
        assert!(DhcpPacket::parse(&[0u8; 300]).is_none());
    }
}
