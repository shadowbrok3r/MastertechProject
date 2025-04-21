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

    let tool_provider = DesktopToolProvider; // Create the tool provider instance

    loop {
        let (stream, client_addr) = listener.accept().await?;
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
    // Ok(()) // Unreachable
}

// --- Stdio Server Function ---
// Renamed from run_mcp_server_tcp
pub async fn run_mcp_server_stdio() -> anyhow::Result<()> {
    info!("MCP Server starting in STDIO mode.");
    let tool_provider = DesktopToolProvider; // Create the tool provider instance
    info!("Using stdio transport. Waiting for commands on stdin...");
    // Serve the server using the stdio transport.
    // This function will run until the stdio stream is closed (e.g., the parent process closes the pipes)
    // or an unrecoverable error occurs.
    match serve_server(tool_provider, stdio()).await {
        Ok(server_handle) => {
            // server_handle.waiting() waits for the server loop to exit.
            if let Err(e) = server_handle.waiting().await {
                // Log errors unless they are expected closure types
                // (These string checks might need adjustment based on exact errors seen)
                let err_str = e.to_string();
                if !err_str.contains("EOF") // Standard end-of-file
                    && !err_str.contains("Broken pipe") // Pipe closed by the other end
                    && !err_str.contains("Connection reset by peer") // Another common pipe closure error
                    && !err_str.contains("os error 109") // Windows specific pipe error
                    && !err_str.contains("The pipe is being closed") // Another Windows pipe error
                    && !err_str.contains("Input/output error") // Can happen on pipe close
                {
                    log::error!("Server exited with unexpected error: {:?}", e);
                    // Consider propagating the error if the calling context needs it
                    // return Err(e.into());
                } else {
                    info!("Stdio stream closed (EOF or broken pipe). Server shutting down gracefully.");
                }
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