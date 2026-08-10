//! Network helpers for direct admin↔client TCP transport.
//!
//! Provides:
//!  - [`detect_local_ipv4`]: best-effort, dependency-free local IPv4 lookup.
//!  - [`try_add_firewall_rule`]: Windows best-effort `netsh` rule that
//!    pre-authorizes the listener port; silently no-ops without admin.

use std::net::{IpAddr, SocketAddr, UdpSocket};

/// Best-effort local IPv4 lookup. Uses the "connectionless socket trick":
/// open a UDP socket and call `connect()` to a public address. No packets
/// are sent, but the kernel resolves the local routing table and binds the
/// socket to the IP of the interface it would have used. We then read that
/// IP back via `local_addr()`.
///
/// Returns `None` if the host has no routable IPv4 (e.g. fully air-gapped
/// machine, or only IPv6). Callers should fall back to binding `0.0.0.0`
/// and skipping DB registration in that case — the WebSocket relay path
/// remains available.
pub fn detect_local_ipv4() -> Option<std::net::Ipv4Addr> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    // Cloudflare's well-known DNS resolver. Any routable address works;
    // we never send a packet, the OS just consults the routing table.
    sock.connect("1.1.1.1:80").ok()?;
    match sock.local_addr().ok()? {
        SocketAddr::V4(a) => {
            let ip = *a.ip();
            if ip.is_unspecified() || ip.is_loopback() {
                None
            } else {
                Some(ip)
            }
        }
        SocketAddr::V6(_) => None,
    }
}

/// Same as [`detect_local_ipv4`] but returned as a generic `IpAddr` for
/// convenience when callers don't need the v4-specific API.
#[allow(dead_code)]
pub fn detect_local_ip() -> Option<IpAddr> {
    detect_local_ipv4().map(IpAddr::V4)
}

/// Best-effort Windows firewall rule for our direct-TCP listener port.
///
/// Deletes any existing rule with `rule_name` first (stale exe-path rules
/// from previous runs or self-update temp-exe launches) then adds a fresh
/// **port-only** inbound allow rule. Port-based rules are immune to the
/// exe-path changing on update; Mastertech is the only app using port 9101
/// so restricting by program is unnecessary.
///
/// Returns `Ok(true)` if `netsh` reported success for the add step,
/// `Ok(false)` otherwise (likely needs admin elevation — the first
/// inbound connection will still trigger Windows' standard allow popup).
/// `Err` only on spawn failure.
#[cfg(target_os = "windows")]
pub fn try_add_firewall_rule(port: u16, rule_name: &str) -> std::io::Result<bool> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let port_str = port.to_string();

    // Remove any stale rule with this name (ignore error — it may not exist).
    let _ = Command::new("netsh")
        .args([
            "advfirewall",
            "firewall",
            "delete",
            "rule",
            &format!("name={rule_name}"),
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    // Add fresh port-only inbound rule. No `program=` so it survives binary
    // replacement without needing a rule refresh after every self-update.
    let output = Command::new("netsh")
        .args([
            "advfirewall",
            "firewall",
            "add",
            "rule",
            &format!("name={rule_name}"),
            "dir=in",
            "action=allow",
            "protocol=TCP",
            &format!("localport={port_str}"),
            "profile=any",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;

    Ok(output.status.success())
}

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
pub fn try_add_firewall_rule(_port: u16, _rule_name: &str) -> std::io::Result<bool> {
    Ok(false)
}

/// Disable the WinPE firewall via `wpeutil DisableFirewall`.
///
/// WinPE's filter policy is not backed by the Windows Firewall service, so
/// `netsh advfirewall` rules do not open ports there — inbound stays blocked
/// and direct-TCP dials to the listener time out even on the same subnet.
/// `wpeutil` is the supported PE switch. Returns `Ok(true)` when wpeutil
/// reported success; `Err` only on spawn failure.
#[cfg(target_os = "windows")]
pub fn try_disable_winpe_firewall() -> std::io::Result<bool> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let output = Command::new("wpeutil")
        .arg("DisableFirewall")
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    Ok(output.status.success())
}
