//! Always-on admin agent: holds sessions to connected clients, hosts the plugin
//! MCP server, and dispatches tech-confirmed assist requests.
//!
//! Runs headless on the shop server so remote diagnostics no longer depend on a
//! desktop console being open.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Stdout)
        .init();

    // Both rustls backends are compiled in on Linux; pick one before any TLS.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let _ = database::init_database().await;
    log::info!("admin-agent: database ready");

    if let Err(e) = sign_in().await {
        log::warn!("admin-agent: service signin failed, continuing as guest: {e}");
    }

    let http = std::env::var("MTECH_AGENT_MCP_STDIO").is_err();
    log::info!("admin-agent: starting session engine + MCP (http={http})");
    displays::headless::run(http).await
}

/// Signs in as the agent service account when credentials are present.
async fn sign_in() -> anyhow::Result<()> {
    let (Ok(user), Ok(pass)) = (
        std::env::var("MTECH_AGENT_USER"),
        std::env::var("MTECH_AGENT_PASS"),
    ) else {
        anyhow::bail!("MTECH_AGENT_USER / MTECH_AGENT_PASS not set");
    };
    database::login(user, pass).await?;
    if let Some(u) = database::get_current_user_from_auth() {
        log::info!("admin-agent: signed in as {}", u.get_email());
    }
    Ok(())
}
