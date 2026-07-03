//! `pxe-bench.toml` appliance configuration.

use std::net::Ipv4Addr;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// IPv4 of the bench NIC to advertise as next-server. PXE clients TFTP/HTTP here.
    pub server_ip: Ipv4Addr,
    /// Directory served over TFTP (iPXE binaries live here).
    #[serde(default = "default_tftp_root")]
    pub tftp_root: PathBuf,
    /// Directory served over HTTP (wimboot, boot.ipxe, WinPE media tree).
    #[serde(default = "default_http_root")]
    pub http_root: PathBuf,
    #[serde(default = "default_http_port")]
    pub http_port: u16,
    /// Boot file handed to legacy BIOS clients (client arch 0).
    #[serde(default = "default_bios_boot_file")]
    pub bios_boot_file: String,
    /// Boot file handed to x64 UEFI clients (client arch 7/9).
    #[serde(default = "default_uefi_boot_file")]
    pub uefi_boot_file: String,
    /// iPXE script path (relative to http_root) chained once iPXE is running.
    #[serde(default = "default_ipxe_script")]
    pub ipxe_script: String,
}

fn default_tftp_root() -> PathBuf {
    PathBuf::from("tftp")
}
fn default_http_root() -> PathBuf {
    PathBuf::from("http")
}
fn default_http_port() -> u16 {
    7777
}
fn default_bios_boot_file() -> String {
    "undionly.kpxe".to_string()
}
fn default_uefi_boot_file() -> String {
    "snponly.efi".to_string()
}
fn default_ipxe_script() -> String {
    "boot.ipxe".to_string()
}

impl Config {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("cannot read {path}: {e}"))?;
        Ok(toml::from_str(&text)?)
    }

    /// HTTP URL of the iPXE chain script.
    pub fn ipxe_script_url(&self) -> String {
        format!(
            "http://{}:{}/{}",
            self.server_ip, self.http_port, self.ipxe_script
        )
    }
}
