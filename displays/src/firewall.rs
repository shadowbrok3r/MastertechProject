//! Port-only inbound rules in the Windows firewall for this app's listeners.

use std::os::windows::process::CommandExt;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Replaces the inbound rule `rule_name` with a TCP allow for `port` on every profile; `Ok(false)` when netsh refused.
pub fn try_add_firewall_rule(port: u16, rule_name: &str) -> std::io::Result<bool> {
    let name = format!("name={rule_name}");
    let _ = Command::new("netsh")
        .args(["advfirewall", "firewall", "delete", "rule", &name])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    let output = Command::new("netsh")
        .args([
            "advfirewall",
            "firewall",
            "add",
            "rule",
            &name,
            "dir=in",
            "action=allow",
            "protocol=TCP",
            &format!("localport={port}"),
            "profile=any",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    Ok(output.status.success())
}
