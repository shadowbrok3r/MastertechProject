//! Bench PXE appliance: plug a dead machine into the bench VLAN and it
//! netboots straight into Mastertech PE — no USB sticks.
//!
//! Three services: ProxyDHCP (:67 + :4011, boot info only, coexists with the
//! shop DHCP), TFTP (:69, iPXE binaries), and HTTP (boot.ipxe + wimboot +
//! WinPE media, fast path for the big .wim). See PXE.md for payload setup.

mod config;
mod dhcp;
mod http;
mod tftp;

use config::Config;

const DEFAULT_CONFIG: &str = "pxe-bench.toml";

fn write_default_config() -> anyhow::Result<()> {
    let template = r#"# pxe-bench appliance configuration.
# IPv4 of the bench NIC — PXE clients fetch boot files from this address.
server_ip = "192.168.1.2"
tftp_root = "tftp"
http_root = "http"
http_port = 7777
bios_boot_file = "undionly.kpxe"
uefi_boot_file = "snponly.efi"
ipxe_script = "boot.ipxe"
"#;
    std::fs::write(DEFAULT_CONFIG, template)?;
    Ok(())
}

// Relative URIs resolve against the fetched-script URL, so no server IP appears.
const WINPE_BLOCK: &str = "kernel wimboot\n\
initrd MasterTech.exe MasterTech.exe\n\
initrd media/sources/boot.wim boot.wim\n\
boot\n";

fn boot_script(cfg: &Config) -> String {
    match cfg.boot_mode.as_str() {
        "winpe" => format!("#!ipxe\n{WINPE_BLOCK}"),
        "uefi" => format!("#!ipxe\nchain {}\n", cfg.uefi_efi),
        _ => format!(
            "#!ipxe\n\
menu Mastertech Bench Boot\n\
item winpe   Mastertech WinPE (terminal)\n\
item uefi    Mastertech UEFI diagnostics\n\
item local   Boot local disk\n\
item shell   iPXE shell\n\
choose --default winpe --timeout 15000 target && goto ${{target}} || goto winpe\n\
\n\
:winpe\n\
echo Booting Mastertech WinPE ...\n\
kernel wimboot\n\
initrd MasterTech.exe MasterTech.exe\n\
initrd media/sources/boot.wim boot.wim\n\
boot || goto failed\n\
\n\
:uefi\n\
echo Booting Mastertech UEFI diagnostics ...\n\
chain {efi} || goto failed\n\
\n\
:local\n\
echo Booting local disk ...\n\
exit\n\
\n\
:shell\n\
shell\n\
\n\
:failed\n\
echo Boot failed - dropping to iPXE shell\n\
shell\n",
            efi = cfg.uefi_efi
        ),
    }
}

fn write_default_boot_script(cfg: &Config) -> anyhow::Result<()> {
    let path = cfg.http_root.join(&cfg.ipxe_script);
    std::fs::create_dir_all(&cfg.http_root)?;
    std::fs::create_dir_all(&cfg.tftp_root)?;
    std::fs::write(&path, boot_script(cfg))?;
    log::info!("wrote boot script {} (mode {})", path.display(), cfg.boot_mode);
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let config_path = std::env::args().nth(1).unwrap_or_else(|| DEFAULT_CONFIG.to_string());
    if !std::path::Path::new(&config_path).exists() {
        write_default_config()?;
        log::warn!(
            "no config found — wrote template {DEFAULT_CONFIG}. Set server_ip to the bench NIC and restart."
        );
        return Ok(());
    }
    let cfg = Config::load(&config_path)?;
    write_default_boot_script(&cfg)?;

    log::info!(
        "pxe-bench up: next-server {} | tftp {} | http :{} {}",
        cfg.server_ip,
        cfg.tftp_root.display(),
        cfg.http_port,
        cfg.http_root.display()
    );

    let dhcp_task = tokio::spawn(dhcp::serve_proxy_dhcp(cfg.clone()));
    let ack_task = tokio::spawn(dhcp::serve_pxe_ack(cfg.clone()));
    let tftp_task = tokio::spawn(tftp::serve_tftp(cfg.tftp_root.clone()));
    let http_task = tokio::spawn(http::serve_http(cfg.http_root.clone(), cfg.http_port));

    tokio::select! {
        r = dhcp_task => log::error!("proxyDHCP exited: {r:?}"),
        r = ack_task  => log::error!("PXE ack server exited: {r:?}"),
        r = tftp_task => log::error!("TFTP exited: {r:?}"),
        r = http_task => log::error!("HTTP exited: {r:?}"),
        _ = tokio::signal::ctrl_c() => log::info!("shutting down"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(mode: &str) -> Config {
        toml::from_str(&format!("server_ip = \"192.168.22.15\"\nboot_mode = \"{mode}\"")).unwrap()
    }

    #[test]
    fn menu_offers_both_targets_with_relative_uris() {
        let s = boot_script(&cfg("menu"));
        assert!(s.contains("item winpe"));
        assert!(s.contains("item uefi"));
        assert!(s.contains("chain mtech.efi"));
        assert!(s.contains("initrd media/sources/boot.wim boot.wim"));
        assert!(s.contains("goto ${target}"));
        assert!(!s.contains("192.168"), "no server IP should appear in the script");
    }

    #[test]
    fn winpe_mode_is_bare_wimboot() {
        let s = boot_script(&cfg("winpe"));
        assert!(!s.contains("menu "));
        assert!(s.contains("kernel wimboot"));
        assert!(s.contains("initrd media/sources/boot.wim boot.wim"));
    }

    #[test]
    fn uefi_mode_chains_the_efi() {
        let s = boot_script(&cfg("uefi"));
        assert!(!s.contains("menu "));
        assert!(s.contains("chain mtech.efi"));
    }
}
