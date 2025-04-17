use anyhow::Context;
use rmcp::{
    model::{BuiltinToolDefinition, Schema, ToolResult},
    rust_sdk::ToolProvider,
    serve_server,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::HashMap, process::Command, sync::Arc};
use tokio::net::TcpListener;
use tracing::info;


// --- Tool Definitions ---

#[derive(Deserialize, Debug)]
struct MoveMouseParams {
    x: i32,
    y: i32,
}

#[derive(Deserialize, Debug)]
struct RunShellParams {
    command: String,
    args: Vec<String>,
}

// --- Tool Provider Implementation ---

#[derive(Clone)]
struct DesktopToolProvider;

// Use the derive macro for simplicity, or implement manually for more control
#[rmcp::macros::tool_provider]
impl DesktopToolProvider {
    // Define the 'move_mouse' tool
    #[tool(name = "move_mouse", description = "Moves the mouse cursor to the specified screen coordinates (X, Y).")]
    async fn move_mouse(&self, params: MoveMouseParams) -> ToolResult<serde_json::Value> {
        info!("Received request to move mouse to: {:?}", params);
        // TODO: Implement actual mouse movement using inputbot, windows-rs, etc.
        // Example (conceptual - requires a library):
        // inputbot::MouseCursor::move_abs(params.x, params.y);
        println!("Simulating mouse move to ({}, {})", params.x, params.y); // Placeholder
        Ok(json!({ "status": "success" }))
    }

    // Define the 'run_shell_command' tool
    #[tool(name = "run_shell_command", description = "Runs a command in the default system shell.")]
    async fn run_shell_command(&self, params: RunShellParams) -> ToolResult<serde_json::Value> {
        info!("Received request to run command: {:?}", params);
        // TODO: Implement robust command execution and output capturing
        let output = Command::new(&params.command)
            .args(&params.args)
            .output()
            .context("Failed to execute command")?; // Use anyhow::Context for errors

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let status_code = output.status.code();

        println!("Command executed. Status: {:?}, Stdout: {}, Stderr: {}", status_code, stdout, stderr); // Placeholder

        Ok(json!({
            "status": "success",
            "exit_code": status_code,
            "stdout": stdout,
            "stderr": stderr,
        }))
    }
}

// --- egui App Structure ---

struct MyApp;

impl Default for MyApp {
    fn default() -> Self {
        Self
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("MCP Server Running");
            ui.label("This application is exposing desktop control tools via MCP.");
            ui.label("Connect an MCP client to port 9001.");
            // Add any other UI elements you need
        });
    }
}

// --- Main Function ---

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init(); // Initialize logging

    // Spawn the MCP Server task
    tokio::spawn(async move {
        if let Err(e) = run_mcp_server().await {
            eprintln!("MCP Server error: {}", e);
        }
    });

    // Run the egui application
    let options = eframe::NativeOptions::default();
    let _ = eframe::run_native(
        "My MCP Server App",
        options,
        Box::new(|_cc| Box::<MyApp>::default()),
    ); // Result ignored for simplicity

    Ok(())
}

async fn run_mcp_server() -> anyhow::Result<()> {
    let addr = "127.0.0.1:9001";
    let listener = TcpListener::bind(addr).await?;
    info!("MCP Server listening on {}", addr);

    let tool_provider = DesktopToolProvider; // Create the tool provider instance

    loop {
        let (stream, client_addr) = listener.accept().await?;
        info!("Accepted connection from: {}", client_addr);
        let provider_clone = tool_provider.clone(); // Clone for the new task

        tokio::spawn(async move {
            // Serve this specific client connection
            let server_handle = serve_server(provider_clone, stream)
                .await
                .expect("Failed to serve server"); // Handle error better in production

            // Keep the connection alive until the client disconnects or an error occurs
            if let Err(e) = server_handle.waiting().await {
                 // Log disconnects or errors, but don't crash the whole server
                 if !e.to_string().contains("connection closed") { // Avoid logging expected disconnects as errors
                     tracing::error!("Client {} error: {:?}", client_addr, e);
                 } else {
                     info!("Client {} disconnected.", client_addr);
                 }
            }
        });
    }
}
