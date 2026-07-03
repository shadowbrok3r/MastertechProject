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

fn write_default_boot_script(cfg: &Config) -> anyhow::Result<()> {
    let path = cfg.http_root.join(&cfg.ipxe_script);
    if path.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(&cfg.http_root)?;
    std::fs::create_dir_all(&cfg.tftp_root)?;
    let base = format!("http://{}:{}", cfg.server_ip, cfg.http_port);
    // Minimal wimboot recipe (ipxe.org/howto/winpe): wimboot auto-extracts the
    // boot manager + BCD from the WIM, so only boot.wim is served.
    let script = format!(
        "#!ipxe\necho Mastertech bench boot - ${{net0/mac}}\nkernel {base}/wimboot\ninitrd {base}/media/sources/boot.wim boot.wim\nboot\n"
    );
    std::fs::write(&path, script)?;
    log::info!("wrote default boot script {}", path.display());
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
