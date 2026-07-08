//! Injects the default upload endpoint at compile time.
//!
//! Reads `UEFI_TARGET_URL` from the process env first, then the workspace-root
//! `.env`, falling back to the LAN pre-boot relay. The shared ORCHESTRATOR_URL
//! is deliberately ignored: it is an https hostname, which firmware (no
//! DNS/TLS driver) can never reach. Exposed as `env!("UEFI_TARGET_URL")`.

use std::fs;

fn main() {
    println!("cargo:rerun-if-changed=../.env");
    println!("cargo:rerun-if-env-changed=UEFI_TARGET_URL");

    // Default: the LAN pre-boot relay (see ../preboot-relay), which
    // re-originates plain firmware HTTP to https://axum.master-tech.app.
    let mut url = String::from("http://192.168.22.139:8082");

    if let Ok(v) = std::env::var("UEFI_TARGET_URL") {
        if !v.trim().is_empty() {
            url = v.trim().to_string();
        }
    } else if let Ok(text) = fs::read_to_string("../.env") {
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("UEFI_TARGET_URL=") {
                let v = rest.trim().trim_matches('"').trim_matches('\'');
                if !v.is_empty() {
                    url = v.to_string();
                }
            }
        }
    }

    println!("cargo:rustc-env=UEFI_TARGET_URL={url}");
}
