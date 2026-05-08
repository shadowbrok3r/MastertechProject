use rmcp::transport::stdio;
use tokio::net::TcpListener;
use rmcp::serve_server; // Keep McpError for type alias if needed internally
use tools::DesktopToolProvider;
use tracing::info; // Added warn

pub mod tools;

// --- TCP Server Function ---
pub async fn run_mcp_server_tcp() -> anyhow::Result<()> {
    let addr = "127.0.0.1:9001"; // The TCP address to listen on
    let listener = TcpListener::bind(addr).await?;
    info!("MCP Server listening on TCP {}", addr);

    let tool_provider = DesktopToolProvider::new(); // Create the tool provider instance

    loop {
        tokio::select! {
            biased;
            _ = displays::wait_for_shutdown() => {
                info!("MCP TCP server (:9001) -> shutdown signaled; stopping accept loop");
                return Ok(());
            }
            res = listener.accept() => {
                let (stream, client_addr) = res?;
                info!("Accepted TCP connection from: {}", client_addr);
                let provider_clone = tool_provider.clone();

                tokio::spawn(async move {
                    info!("Serving client {}...", client_addr);
                    match serve_server(provider_clone, stream).await {
                        Ok(server_handle) => {
                            if let Err(e) = server_handle.waiting().await {
                                if !e.to_string().contains("connection closed")
                                    && !e.to_string().contains("Connection reset by peer")
                                    && !e.to_string().contains("broken pipe")
                                   {
                                    tracing::error!("Client {} error: {:?}", client_addr, e);
                                } else {
                                    info!("Client {} disconnected.", client_addr);
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to start serving client {}: {:?}", client_addr, e);
                        }
                    }
                });
            }
        }
    }
}

// --- Stdio Server Function ---
// Renamed from run_mcp_server_tcp
pub async fn run_mcp_server_stdio() -> anyhow::Result<()> {
    info!("MCP Server starting in STDIO mode.");
    let tool_provider = DesktopToolProvider::new(); // Create the tool provider instance
    info!("Using stdio transport. Waiting for commands on stdin...");
    // Serve the server using the stdio transport.
    // This function will run until the stdio stream is closed (e.g., the parent process closes the pipes)
    // or an unrecoverable error occurs.
    match serve_server(tool_provider, stdio()).await {
        Ok(server_handle) => {
            // server_handle.waiting() waits for the server loop to exit.
            if let Err(e) = server_handle.waiting().await {
                log::error!("Server exited with unexpected error: {e}");
            } else {
                 info!("Server finished gracefully.");
            }
        }
        Err(e) => {
            log::error!("Failed to start serving on stdio: {:?}", e);
            return Err(e.into()); // Propagate the startup error
        }
    }

    Ok(())
}