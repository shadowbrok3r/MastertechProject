use crate::{tabs::admin_console::client_interface::serialize_command, virtual_filesystem::FileSysHelper, Cmd, FileSystemAction};
use database::schema::{Node, SystemInformation};
use ewebsock::{WsEvent, WsMessage};
use eframe::egui::Context;

use super::{deserialize_exact, deserializer, is_zstd_frame, ui::WsDisplayState, History, WebSocketClient};

impl WebSocketClient {
    pub fn receive(&mut self, ctx: &Context) {
        #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
        if let Some(rx) = self.remote_egui_mcp_rx.as_ref() {
            while let Ok(bin) = rx.try_recv() {
                self.transport.send(WsMessage::Binary(bin));
            }
        }

        // Application-layer heartbeat: fire an `AppPing` every 15 s
        // while the transport is open and log a warn if no `AppPong`
        // has been received for 60+ s.  Detects plugin-host wedges
        // that leave the kernel socket alive — the case in the
        // 16:13:01 log where the client's `display_connections` plugin
        // completed locally but the response Cmd never made it back to
        // the admin, and TCP keepalive stayed silent for the next
        // minute+.  Distinct from `last_pong_time`, which is the WS
        // Pong frame from `ewebsock`'s ping-pong (relay-only, doesn't
        // exercise the plugin dispatch path).
        if self.is_connected {
            let now = web_time::Instant::now();
            let due = self
                .last_app_ping_sent
                .map(|t| now.duration_since(t) >= std::time::Duration::from_secs(15))
                .unwrap_or(true);
            if due {
                self.app_ping_nonce = self.app_ping_nonce.wrapping_add(1);
                let sent_at_ms = web_time::SystemTime::now()
                    .duration_since(web_time::SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                let ping = Cmd::AppPing {
                    nonce: self.app_ping_nonce,
                    sent_at_ms,
                };
                self.transport.send(WsMessage::Binary(serialize_command(&ping)));
                self.last_app_ping_sent = Some(now);
            }
            // Wedge detector: if we have ever received an AppPong and
            // the last one is >60 s old, log once per minute so the
            // operator (or a future UI badge) can act.
            if let Some(last_pong) = self.last_app_pong_received {
                let age = now.duration_since(last_pong);
                let warn_due = self
                    .last_pong_silence_warn
                    .map(|t| now.duration_since(t) >= std::time::Duration::from_secs(60))
                    .unwrap_or(true);
                if age >= std::time::Duration::from_secs(60) && warn_due {
                    self.last_pong_silence_warn = Some(now);
                    log::warn!(
                        "AppPong silence: no application-layer pong from {} for {:?} \
                         — kernel TCP may still report alive while the plugin host is wedged",
                        self.client.connection_string,
                        age
                    );
                }
            }
        }

        self.explorer.receive();
        self.toolbox.receive();

        // Check if the toolbox wants to run a script on the remote client
        if let Ok((filename, content)) = self.toolbox.run_on_remote_rx.try_recv() {
            log::info!("Running script on remote: {filename}");
            let cmd = Cmd::RunScriptContent { filename, content };
            self.transport.send(WsMessage::Binary(serialize_command(&cmd)));
        }

        // Drain file-transfer chunks produced by background thread and send to remote
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(rx) = &self.file_transfer_rx {
            let mut done = false;
            while let Ok(cmd) = rx.try_recv() {
                if let Cmd::DirectFileTransfer { ref filename, chunk_index, total_chunks, .. } = cmd {
                    self.file_transfer_progress = Some((filename.clone(), chunk_index + 1, total_chunks));
                    if chunk_index + 1 == total_chunks {
                        done = true;
                    }
                }
                self.transport.send(WsMessage::Binary(serialize_command(&cmd)));
            }
            if done {
                self.file_transfer_rx = None;
                self.file_transfer_progress = None;
            }
        }

        // Drain self-update chunks produced by background thread and send to remote.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(rx) = &self.self_update_rx {
            let mut done = false;
            while let Ok(cmd) = rx.try_recv() {
                if let Cmd::MastertechSelfUpdateChunk { chunk_index, total_chunks, .. } = &cmd {
                    self.file_transfer_progress = Some((
                        "MasterTech.exe (self-update)".to_string(),
                        chunk_index + 1,
                        *total_chunks,
                    ));
                    if chunk_index + 1 == *total_chunks {
                        done = true;
                    }
                }
                self.transport.send(WsMessage::Binary(serialize_command(&cmd)));
            }
            if done {
                self.self_update_rx = None;
                // Keep file_transfer_progress visible briefly; it will be cleared
                // when MastertechSelfUpdateResult arrives (handled below).
            }
        }

        #[cfg(not(target_arch="wasm32"))]
        if let Ok(diagnostic_msg) = self.diagnostic_rx.try_recv() {
            // log::warn!("Diagnostic info: {diagnostic_msg:?}");
            match diagnostic_msg {
                crate::mcp::DiagnosticResponse::BsodAnalysis { 
                    summary: _summary, 
                    crash_reason: _crash_reason, 
                    driver_issues: _driver_issues, 
                    recommendations: _recommendations, 
                    dump_files_analyzed: _dump_files_analyzed
                } => {

                },
                crate::mcp::DiagnosticResponse::EventLogAnalysis { 
                    summary: _summary, 
                    critical_events: _critical_events, 
                    error_patterns: _error_patterns, 
                    recommendations: _recommendations, 
                    total_events_analyzed: _total_events_analyzed 
                } => {

                },
                crate::mcp::DiagnosticResponse::PerformanceReport { 
                    summary: _summary, 
                    cpu_analysis: _cpu_analysis, 
                    memory_analysis: _memory_analysis, 
                    disk_analysis: _disk_analysis, 
                    network_analysis: _network_analysis, 
                    recommendations: _recommendations, 
                    charts_data: _charts_data 
                } => {

                },
                crate::mcp::DiagnosticResponse::SystemSummary { 
                    overview: _overview, 
                    hardware_summary: _hardware_summary, 
                    software_summary: _software_summary, 
                    network_summary: _network_summary, 
                    health_score: _health_score, 
                    critical_issues: _critical_issues 
                } => {

                },
                crate::mcp::DiagnosticResponse::ScriptExecution { 
                    success: _success, 
                    output: _output, 
                    error: _error, 
                    approval_required: _approval_required, 
                    approved: _approved
                } => {

                },
                crate::mcp::DiagnosticResponse::CommandCompletions { 
                    completions, 
                    context_info: _context_info 
                } => {
                    // Replace existing suggestions with the new batch
                    self.command_suggestions.clear();
                    self.command_suggestions.extend_from_slice(&completions);
                    // Auto‑show suggestions if they came from a debounced background fetch
                    if !self.command_suggestions.is_empty() {
                        self.show_suggestions = true;
                    }
                    // Clear any in‑flight cancellation handle (request finished)
                    #[cfg(feature="tokio")]
                    { self.completion_cancel_tx = None; }
                    // Make sure UI updates
                    ctx.request_repaint();
                },
                crate::mcp::DiagnosticResponse::Error { 
                    message, 
                    details 
                } => {
                    // On error also clear spinner state
                    #[cfg(feature="tokio")]
                    { self.completion_cancel_tx = None; }
                    log::error!("AI Diagnostic error: {message} - {details:?}");
                    ctx.request_repaint();

                },
            }
        }

        while let Ok(msg) = self.msg_to_client_rx.try_recv() {
            self.transport.send(msg);
        }

        #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
        for notice in
            crate::plugins::crash_intel_hooks::drain_notices(&self.client.connection_string)
        {
            self.history.push(History {
                from: "Crash Intel".to_string(),
                message: notice,
                timestamp: chrono::Local::now().to_rfc3339(),
            });
            self.notifications += 1;
            ctx.request_repaint();
        }

        // Drain ALL pending websocket messages in one frame to avoid backlog.
        // Remote-viewer frames (zstd terminal buffers + tagged egui frames)
        // are coalesced to the latest to skip stale frames; everything else
        // is decoded immediately.
        let mut latest_viewer_frame: Option<Vec<u8>> = None;
        while let Some(event) = self.transport.try_recv() {
            match event {
                WsEvent::Message(msg) => {
                    match msg {
                        WsMessage::Binary(bin) => {
                            // Terminal-mode clients multiplex zstd remote-viewer
                            // buffers and egui frames onto the same channel as
                            // sysinfo/Cmd; route by content, not by view state.
                            let is_viewer_frame = bin.first() == Some(&crate::EGUI_FRAME_TAG)
                                || bin.first() == Some(&crate::DESKTOP_FRAME_TAG)
                                || is_zstd_frame(&bin);
                            // Intercept admin control-plane Cmd results before view-specific decoding.
                            let mut handled_as_admin_cmd = false;
                            if !is_viewer_frame
                            {
                                if let Some(decoded) = deserializer::<Cmd>(&bin) {
                                    match decoded {
                                        Cmd::RemotePluginToolResult { request_id, plugin_id, tool_name, success, result_json } => {
                                            log::info!("Remote plugin tool result: {plugin_id}::{tool_name} req={request_id} success={success}");
                                            #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
                                            if success && crate::plugins::crash_intel_hooks::is_dump_analysis_result(&plugin_id, &tool_name) {
                                                crate::plugins::crash_intel_hooks::ingest_dump_decode_result(
                                                    self.client.connection_string.clone(),
                                                    self.client.computer.clone(),
                                                    tool_name.clone(),
                                                    result_json.clone(),
                                                );
                                            }
                                            #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
                                            if success && crate::plugins::crash_intel_hooks::is_kernel_triage_result(&plugin_id, &tool_name) {
                                                crate::plugins::crash_intel_hooks::ingest_kernel_triage_result(
                                                    self.client.connection_string.clone(),
                                                    self.client.computer.clone(),
                                                    tool_name.clone(),
                                                    result_json.clone(),
                                                );
                                            }
                                            #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
                                            if success && crate::plugins::driver_intel_hooks::is_driver_snapshot_result(&plugin_id, &tool_name) {
                                                crate::plugins::driver_intel_hooks::ingest_driver_snapshot(
                                                    self.client.connection_string.clone(),
                                                    self.client.computer.clone(),
                                                    result_json.clone(),
                                                );
                                            }
                                            #[cfg(not(target_arch = "wasm32"))]
                                            crate::plugins::mcp_bridge::resolve_pending_request(&request_id, success, result_json);
                                            handled_as_admin_cmd = true;
                                        }
                                        Cmd::DirectFileTransferResult { filename, success, message } => {
                                            log::info!("File transfer result: {filename} success={success}");
                                            self.history.push(History {
                                                from: "System".to_string(),
                                                message: format!("File transfer '{filename}': {message}"),
                                                timestamp: chrono::Local::now().to_rfc3339(),
                                            });
                                            self.notifications += 1;
                                            handled_as_admin_cmd = true;
                                        }
                                        Cmd::LoadWasmPluginResult { plugin_id, success, message } => {
                                            log::info!("WASM plugin result: {plugin_id} success={success}");
                                            self.history.push(History {
                                                from: "System".to_string(),
                                                message: format!("WASM plugin '{plugin_id}': {message}"),
                                                timestamp: chrono::Local::now().to_rfc3339(),
                                            });
                                            self.notifications += 1;
                                            handled_as_admin_cmd = true;
                                        }
                                        Cmd::DesktopMonitorList(monitors) => {
                                            log::info!("Received {} monitor(s) from client", monitors.len());
                                            self.desktop_monitors = monitors;
                                            handled_as_admin_cmd = true;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            if !handled_as_admin_cmd {
                                if is_viewer_frame {
                                    latest_viewer_frame = Some(bin);
                                } else {
                                    self.handle_binary_message(bin, ctx);
                                }
                            }
                        },
                        WsMessage::Text(text) => self.handle_text_message(text),
                        WsMessage::Pong(_) => {
                            self.last_pong_time = Some(web_time::Instant::now());
                            self.is_connected = true;
                            self.connection_status = "Connected".to_string();
                        },
                        _ => {}
                    }
                },
                WsEvent::Opened => {
                    let is_redial = self.is_connected;
                    self.is_connected = true;
                    self.connection_status = "Connected".to_string();
                    if !is_redial {
                        self.history.push(History {
                            from: "Client".to_string(),
                            message: "Connection opened".to_string(),
                            timestamp:  chrono::Local::now().to_rfc3339()
                        });
                        self.notifications += 1;
                    } else {
                        log::debug!(
                            "Transport re-opened for {} (TCP redial — skipping duplicate history)",
                            self.client.connection_string
                        );
                    }
                    self.bootstrap_connected_session();
                },
                WsEvent::Closed => {
                    // Loud log so the operator (and us, reading logs)
                    // never has to guess WHEN the admin transport
                    // closed.  Pairs with the new in-flight stall
                    // warnings in `call_remote_plugin_tool` to make
                    // these silences self-explanatory.
                    log::warn!(
                        "admin transport CLOSED for {} (last_app_pong={:?}, last_ws_pong={:?})",
                        self.client.connection_string,
                        self.last_app_pong_received
                            .map(|t| web_time::Instant::now().duration_since(t)),
                        self.last_pong_time
                            .map(|t| web_time::Instant::now().duration_since(t)),
                    );
                    self.is_connected = false;
                    self.connection_status = "Disconnected".to_string();
                    self.last_pong_time = None;
                    self.last_app_pong_received = None;
                    self.history.push(History {
                        from: "Client".to_string(),
                        message: "Connection closed".to_string(),
                        timestamp:  chrono::Local::now().to_rfc3339()
                    });
                    self.notifications += 1;
                },
                WsEvent::Error(err) => {
                    let soft = err.contains("retrying")
                        || err.contains("reconnecting")
                        || err.contains("Reconnecting");
                    log::warn!(
                        "admin transport ERROR for {}: {err}",
                        self.client.connection_string
                    );
                    if soft {
                        self.connection_status = "Reconnecting…".to_string();
                    } else {
                        self.is_connected = false;
                        self.connection_status = format!("Error: {err}");
                        self.history.push(History {
                            from: "Client".to_string(),
                            message: format!("Connection error: {err}"),
                            timestamp:  chrono::Local::now().to_rfc3339()
                        });
                        self.notifications += 1;
                    }
                },
            }
        }

        // Forward only the latest remote-viewer frame, skipping stale ones.
        if let Some(bin) = latest_viewer_frame {
            let _ = self.msg_from_client_tx.try_send(WsMessage::Binary(bin));
        }
        
        if let Ok(state) = self.display_state_channel.1.try_recv() {
            self.state = state;
        }

        // Handle commands we are going to SEND to Mastertech
        while let Ok(command) = self.send_cmd_rx.try_recv() {
            self.handle_command(command);
        }

        // Handle commands we receive from Mastertech
        while let Ok(command) = self.receive_cmd_rx.try_recv() {
            ctx.request_repaint();
            if let Cmd::FileSystemAction(file_system_action) = command {
                self.helper_delegate.handle_filesystem_action(&file_system_action);
            }
        }
    }
    
    fn handle_command(&mut self, command: Cmd) {
        match command {
            Cmd::FileSystemAction(ref action) => {
                match action {
                    FileSystemAction::EnterDirectory(directory) => {
                        log::info!("web_console/websockets.rs -> EnterDirectory -> {directory:?}\nweb_console/websockets.rs -> EnterDirectory -> Root: {:?}", self.explorer.root);
                        log::info!("Prefix before double clicking folder: {}", self.explorer.current_prefix);
                        self.explorer.double_click_folder(&directory);
                        log::info!("After: {}", self.explorer.current_prefix);
                    },
                    FileSystemAction::GetNode(new_node) => {
                        log::info!("web_console/websockets.rs -> GetNode -> Root: {:?}", self.explorer.root); // {new_node:?}
                        if let Node::Folder(prefix, _) = new_node {
                            if &self.explorer.current_prefix == "current" {
                                self.explorer.current_prefix = prefix.clone();
                            }
                            log::info!("web_console/websockets.rs -> Current prefix: {}\nNew prefix: {}", self.explorer.current_prefix, prefix);
                        }
                        let insert_node = self.explorer.insert_node(new_node.clone());
                        log::info!("web_console/websockets.rs -> InsertNode -> {insert_node:?}");
                    },
                    FileSystemAction::RequestNewContents(directory) => {
                        log::info!("web_console/websockets.rs -> RequestNewContents -> {directory}");
                        log::info!("ACTION TO SEND: {}", crate::shape_fp::redacted(&command));
                        self.transport.send(WsMessage::Binary(serialize_command(&command)));
                    }
                    FileSystemAction::Execute(label) => { 
                        self.explorer.execute_file = label.clone(); 
                        if !label.is_empty() {
                            self.transport.send(WsMessage::Binary(serialize_command(&command)));
                            self.interactive = true;
                            self.history.push(History { 
                                from: "Client".to_string(), 
                                message: "Switching to interactive mode".to_string(), 
                                timestamp:  chrono::Local::now().to_rfc3339()
                            });
                            self.notifications += 1;
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::Shell);
                        }
                    },
                    FileSystemAction::Select((modifiers, label)) => {
                        if self.explorer.selected_items.borrow().contains(label) {
                            // If the item was already selected, deselect it
                            self.explorer.selected_items.borrow_mut().remove(label);
                        } 
                        if modifiers.ctrl { 
                            self.explorer.selected_items.borrow_mut().insert(label.clone());
                        } else { // If the control key is not down, clear previous selection and select the current item
                            self.explorer.selected_items.borrow_mut().clear();
                            self.explorer.selected_items.borrow_mut().insert(label.clone());
                        }
                        
                        
                        self.transport.send(WsMessage::Binary(serialize_command(&command)));
                    },
                    FileSystemAction::ExpandDirectory(directory) => self.explorer.expand_folder(&directory),
                    FileSystemAction::NavigateHome => {
                        log::info!("web_console/websockets.rs -> NavigateHome");
                        // self.explorer.navigation_stack.clear();
                        // self.explorer.current_prefix.clear();
                    }
                    // FileSystemAction::CopyToClient(_) => todo!(),
                    // FileSystemAction::CopyFromClient(_) => todo!(),
                    // FileSystemAction::Delete(_) => todo!(),
                    FileSystemAction::PreviewedFile(file) => {
                        self.explorer.previewed_file = Some(file.to_string());
                    },
                    _ => {
                        self.transport.send(WsMessage::Binary(serialize_command(&command)));
                    }
                }
            }
            Cmd::Quit => {
                self.interactive = false;
                self.transport.send(WsMessage::Binary(serialize_command(&Cmd::Quit)));
            },
            Cmd::ListDirectory(path) => {
                log::info!("Requesting directory listing for: {}", path);
                self.remote_explorer.loading = true;
                self.transport.send(WsMessage::Binary(serialize_command(&Cmd::ListDirectory(path))));
            },
            Cmd::DirectoryListing(entries, path) => {
                log::info!("Received directory listing with {} entries at path: {:?}", entries.len(), path);
                self.remote_explorer.set_entries(entries, path);
            },
            _ => self.transport.send(WsMessage::Binary(serialize_command(&command)))
        }
    }

    fn handle_binary_message(&mut self, bin: Vec<u8>, ctx: &Context) {
        if bin.first() == Some(&crate::EGUI_FRAME_TAG)
            || bin.first() == Some(&crate::DESKTOP_FRAME_TAG)
        {
            return;
        }
        // Builder traffic: workers prepend `BUILDER_WIRE_TAG` to every
        // frame. Decode here and dispatch into builder_transport so
        // the MCP tools see a coherent worker registry + job table.
        // No need to feed this into the Cmd path or the terminal state.
        #[cfg(not(target_arch = "wasm32"))]
        if bin.first() == Some(&plugin_builder::BUILDER_WIRE_TAG) {
            match plugin_builder::BuilderWire::decode_tagged(&bin) {
                Ok(Some(wire)) => {
                    use plugin_builder::BuilderWire;
                    match &wire {
                        BuilderWire::Hello { .. } => {
                            crate::plugins::builder_transport::register_worker(
                                &self.client.connection_string,
                                wire,
                            );
                        }
                        BuilderWire::CompileResult { job_id, .. } => {
                            let job_id = job_id.clone();
                            crate::plugins::builder_transport::resolve_job(&job_id, wire);
                            ctx.request_repaint();
                        }
                        BuilderWire::CompileProgress { job_id, .. } => {
                            let job_id = job_id.clone();
                            crate::plugins::builder_transport::record_progress(&job_id, wire);
                        }
                        BuilderWire::CompileRequest { .. } => {
                            log::warn!("admin received a CompileRequest from a worker; ignoring");
                        }
                    }
                }
                Ok(None) => {
                    // Tag byte matched but BuilderWire said no — impossible.
                }
                Err(e) => {
                    log::warn!("BuilderWire decode failed ({} bytes): {e}", bin.len());
                }
            }
            return;
        }
        // Live telemetry feeds the resource monitor regardless of which tab
        // is active, so the Home charts keep updating in the background.
        // Full-buffer decode so a control-plane Cmd whose prefix resembles
        // SystemInformation can't be misread as telemetry.
        if let Some(sysinfo) = deserialize_exact::<SystemInformation>(&bin) {
            log::debug!("[sysinfo] {} bytes -> resource monitor", bin.len());
            self.resource_monitor.set_sysinfo(sysinfo);
            return;
        }
        {
                if let Some(cmd) = deserializer::<Cmd>(&bin){
                    // Handle DirectoryListing directly here for the remote explorer
                    if let Cmd::DirectoryListing(entries, path) = &cmd {
                        log::info!("Received directory listing with {} entries at path: {:?}", entries.len(), path);
                        self.remote_explorer.set_entries(entries.clone(), path.clone());
                    } else if let Cmd::DriveList(drives) = &cmd {
                        log::info!("Received drive list with {} drives", drives.len());
                        self.remote_explorer.set_drives(drives.clone());
                    } else if let Cmd::FileChunk(data, is_last) = cmd {
                        log::info!("Received file chunk: {} bytes, is_last: {}", data.len(), is_last);
                        
                        // Check if this is for "Copy to My Tools" (native only)
                        #[cfg(not(target_arch = "wasm32"))]
                        if let Some((path, buffer)) = &mut self.remote_explorer.pending_tool_upload {
                            buffer.extend_from_slice(&data);
                            if is_last {
                                let path_clone = path.clone();
                                let data_clone = std::mem::take(buffer);
                                self.remote_explorer.copy_to_my_tools(&path_clone, data_clone);
                                self.remote_explorer.pending_tool_upload = None;
                                self.history.push(History {
                                    from: "System".to_string(),
                                    message: format!("Copied to My Tools: {}", path_clone),
                                    timestamp: chrono::Local::now().to_rfc3339(),
                                });
                            }
                        } else
                        
                        // Always handle normal file downloads
                        {
                            // MCP headless crash-dump fetch: open a writer at the
                            // registered dest on the first chunk when no UI
                            // download is active.
                            #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
                            if !self.remote_explorer.download_in_progress() {
                                if let Some(dest) = crate::plugins::mcp_bridge::peek_headless_dump_fetch(&self.client.connection_string) {
                                    self.remote_explorer.begin_headless_download(dest);
                                }
                            }
                            // Normal file download — streams straight to disk.
                            match self.remote_explorer.handle_file_download(data, is_last) {
                                Ok(Some(msg)) => {
                                    self.history.push(History {
                                        from: "System".to_string(),
                                        message: msg,
                                        timestamp: chrono::Local::now().to_rfc3339(),
                                    });
                                }
                                Ok(None) => {}
                                Err(msg) => {
                                    #[cfg(not(target_arch = "wasm32"))]
                                    self.remote_explorer.abort_download();
                                    #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
                                    if let Some((_, req)) = crate::plugins::mcp_bridge::take_headless_dump_fetch(&self.client.connection_string) {
                                        crate::plugins::mcp_bridge::resolve_pending_request(&req, false, format!("download failed: {msg}"));
                                    }
                                    self.history.push(History {
                                        from: "System".to_string(),
                                        message: format!("Download failed: {}", msg),
                                        timestamp: chrono::Local::now().to_rfc3339(),
                                    });
                                }
                            }
                            // Resolve a headless fetch + advance the queue on completion.
                            if is_last {
                                #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
                                if let Some((dest, req)) = crate::plugins::mcp_bridge::take_headless_dump_fetch(&self.client.connection_string) {
                                    crate::plugins::mcp_bridge::resolve_pending_request(&req, true, dest.to_string_lossy().to_string());
                                }
                                self.remote_explorer.advance_download_queue(&self.send_cmd_tx);
                            }
                        }
                    } else if let Cmd::DirectorySizeResult {
                        path,
                        total_bytes,
                        file_count,
                        dir_count,
                        error,
                    } = cmd
                    {
                        let message = self.remote_explorer.handle_directory_size_result(
                            path,
                            total_bytes,
                            file_count,
                            dir_count,
                            error,
                        );
                        self.history.push(History {
                            from: "System".to_string(),
                            message,
                            timestamp: chrono::Local::now().to_rfc3339(),
                        });
                    } else if let Cmd::FilePreviewContent(path, content) = cmd {
                        log::info!("Received file preview content for: {}", path);
                        self.remote_explorer.handle_preview_content(path, content);
                    } else if let Cmd::ThumbnailResponse(path, png_data) = cmd {
                        log::info!("Received thumbnail for: {} ({} bytes)", path, png_data.len());
                        self.remote_explorer.handle_thumbnail(path, png_data, ctx);
                    } else if let Cmd::SaveResult(success, message) = cmd {
                        log::info!("Save result: {} - {}", success, message);
                        if success {
                            self.remote_explorer.preview.modified = false;
                        }
                        self.history.push(History {
                            from: "System".to_string(),
                            message,
                            timestamp: chrono::Local::now().to_rfc3339(),
                        });
                    } else if let Cmd::EventLogResponse(entries) = cmd {
                        log::info!("Received {} event log entries", entries.len());
                        self.event_log_viewer.set_entries(entries);
                    } else if let Cmd::ServiceListResponse(services) = cmd {
                        log::info!("Received {} services", services.len());
                        self.services_viewer.set_entries(services);
                    } else if let Cmd::ServiceActionResponse { name, success, message } = cmd {
                        log::info!("Service action result: {} - {} - {}", name, success, message);
                        self.services_viewer.set_action_result(name, success, message);
                        // Refresh the service list after an action
                        let _ = self.send_cmd_tx.try_send(Cmd::ListServices);
                    } else if let Cmd::ScheduledTaskListResponse(tasks) = cmd {
                        log::info!("Received {} scheduled tasks", tasks.len());
                        self.task_scheduler_viewer.set_entries(tasks);
                    } else if let Cmd::ScheduledTaskActionResponse { success, message } = cmd {
                        log::info!("Task scheduler action result: {} - {}", success, message);
                        self.task_scheduler_viewer.set_action_result(success, message);
                        let _ = self.send_cmd_tx.try_send(Cmd::ListScheduledTasks { folder: None });
                    } else if let Cmd::RegistryKeyResponse { path, subkeys, values } = cmd {
                        log::info!("Received registry data for {} ({} subkeys, {} values)", path, subkeys.len(), values.len());
                        self.registry_editor.set_key_data(path, subkeys, values);
                    } else if let Cmd::RegistryBackupResponse { success, backup_path, message } = cmd {
                        log::info!("Registry backup result: {} - {}", success, message);
                        self.registry_editor.set_backup_result(success, backup_path, message);
                    } else if let Cmd::RegistryEditResponse { success, message } = cmd {
                        log::info!("Registry edit result: {} - {}", success, message);
                        self.registry_editor.set_edit_result(success, message);
                        if success && !self.registry_editor.selected_key.is_empty() {
                            let _ = self.send_cmd_tx.try_send(Cmd::ListRegistryKeys(self.registry_editor.selected_key.clone()));
                        }
                    } else if let Cmd::WindowsUpdateResult { success, summary } = cmd {
                        // Slice 4: batch Windows Update finished
                        // on this client. Surface the per-client
                        // outcome as a toast (success/error)
                        // tagged with the connection_string so the
                        // operator can tell which client just
                        // reported back from a batch fan-out.
                        log::info!(
                            "Windows Update result on {}: success={success} {summary}",
                            self.client.connection_string,
                        );
                        let cs = &self.client.connection_string;
                        let prefix = self
                            .client
                            .friendly_name
                            .clone()
                            .unwrap_or_else(|| cs.clone());
                        let toast_text = format!("{prefix} — {summary}");
                        let toast = if success {
                            crate::ToastMessage::Success(toast_text)
                        } else {
                            crate::ToastMessage::Error(toast_text)
                        };
                        let _ = crate::get_toast_sender().try_send(toast);
                    } else if let Cmd::InstalledProgramsResponse(programs) = cmd {
                        // Slice 3: client finished its registry
                        // walk and shipped the list. Drop it into
                        // the viewer's `entries` so the
                        // egui_data_table re-renders.
                        log::info!(
                            "Received {} installed programs for {}",
                            programs.len(),
                            self.client.connection_string,
                        );
                        self.installed_programs_viewer.set_entries(programs);
                    } else if let Cmd::UninstallProgramResult { id, success, message } = cmd {
                        // Slice 3: uninstall returned. Always
                        // surface the result in the viewer's
                        // status row so the admin sees which
                        // strategy fired (or why nothing
                        // happened). On success we re-fetch the
                        // program list so the row disappears.
                        log::info!(
                            "Uninstall result for {id} on {}: success={success} {message}",
                            self.client.connection_string,
                        );
                        self.installed_programs_viewer.set_action_result(id, success, message);
                        if success {
                            self.installed_programs_viewer.loading = true;
                            let _ = self.send_cmd_tx.try_send(Cmd::ListInstalledPrograms);
                        }
                    } else if let Cmd::SecurityInventoryResponse(products) = cmd {
                        // Slice 2 of the AV refactor: the remote
                        // client finished its WMI + registry walk
                        // and shipped us the structured list. Push
                        // it through the global channel — the
                        // `AdminConsole::receive` loop drains the
                        // channel, caches by connection_string for
                        // the row's expanded body to render, and
                        // upserts the `computer` row so the data
                        // survives this session.
                        log::info!(
                            "Received security inventory for {} ({} products)",
                            self.client.connection_string,
                            products.len(),
                        );
                        let _ = crate::get_security_inventory_sender().try_send(
                            crate::SecurityInventoryEvent {
                                connection_string: self.client.connection_string.clone(),
                                products,
                            },
                        );
                    } else if let Cmd::StartupAppsResponse(apps) = cmd {
                        log::info!("Received {} startup apps", apps.len());
                        self.startup_apps_viewer.set_entries(apps);
                    } else if let Cmd::StartupAppActionResponse { success, message } = cmd {
                        log::info!("Startup app action result: {} - {}", success, message);
                        self.startup_apps_viewer.set_action_result(success, message.clone());
                        if success {
                            let _ = self.send_cmd_tx.try_send(Cmd::ListStartupApps);
                        }
                    } else if let Cmd::RemoteScriptListResponse { categories } = cmd {
                        log::info!("Received {} script categories", categories.len());
                        crate::plugins::remote_script_notify::notify_script_list(categories.len());
                        self.remote_scripts_viewer.set_script_list(categories);
                        if let Some(user) = crate::get_current_user_from_auth() {
                            self.remote_scripts_viewer.load_custom_scripts(&user.get_user_bucket_name());
                        }
                    } else if let Cmd::RemoteScriptLog(msg) = cmd {
                        crate::plugins::remote_script_notify::notify_remote_script_log(&self.client.connection_string, msg.clone());
                        self.remote_scripts_viewer.append_log(msg);
                    } else if let Cmd::RemoteScriptResult { name, status } = cmd {
                        log::info!("Script result: {} - {:?}", name, status);
                        crate::plugins::remote_script_notify::notify_remote_script_result(
                            &self.client.connection_string,
                            name.clone(),
                            format!("{:?}", status),
                        );
                        self.remote_scripts_viewer.set_script_result(name, status);
                    } else if let Cmd::RemoteScriptsComplete = cmd {
                        log::info!("All remote scripts completed");
                        crate::plugins::remote_script_notify::notify_remote_scripts_complete(&self.client.connection_string);
                        self.remote_scripts_viewer.set_complete();
                    } else if let Cmd::LoadWasmPluginResult { plugin_id, success, message } = cmd {
                        log::info!("WASM plugin deploy result: {plugin_id} success={success} {message}");
                        crate::plugins::remote_script_notify::notify_deploy_ack(&plugin_id, success, &message);
                        self.history.push(History {
                            from: "System".to_string(),
                            message: format!("WASM plugin '{plugin_id}': {message}"),
                            timestamp: chrono::Local::now().to_rfc3339(),
                        });
                        self.notifications += 1;
                    } else if let Cmd::DirectFileTransferResult { filename, success, message } = cmd {
                        log::info!("File transfer result: {filename} success={success} {message}");
                        self.history.push(History {
                            from: "System".to_string(),
                            message: format!("File transfer '{filename}': {message}"),
                            timestamp: chrono::Local::now().to_rfc3339(),
                        });
                        self.notifications += 1;
                    } else if let Cmd::MastertechSelfUpdateRelaunching { reconnect_hint_secs } = cmd {
                        log::info!(
                            "Remote self-update relaunching; reconnect hint {reconnect_hint_secs}s"
                        );
                        let grace_secs = reconnect_hint_secs as u64 + 10;
                        self.transport.signal_relaunch_pending(grace_secs);
                        self.mark_session_rebootstrap_pending();
                        self.connection_status =
                            format!("Client relaunching (~{reconnect_hint_secs}s)…");
                        self.history.push(History {
                            from: "System".to_string(),
                            message: format!(
                                "Remote update applying — reconnect expected in ~{reconnect_hint_secs}s"
                            ),
                            timestamp: chrono::Local::now().to_rfc3339(),
                        });
                        self.notifications += 1;
                    } else if let Cmd::MastertechSelfUpdateResult { success, message } = cmd {
                        log::info!("Remote self-update result: success={success} {message}");
                        self.file_transfer_progress = None;
                        if success {
                            self.transport.signal_relaunch_pending(20);
                            self.mark_session_rebootstrap_pending();
                            self.connection_status = "Client relaunching (reconnecting…)".to_string();
                        }
                        let toast_msg = if success {
                            "Remote update applied — client is relaunching.".to_string()
                        } else {
                            format!("Remote update failed: {message}")
                        };
                        let toast = if success {
                            crate::ToastMessage::Success(toast_msg.clone())
                        } else {
                            crate::ToastMessage::Error(toast_msg.clone())
                        };
                        let _ = crate::get_toast_sender().try_send(toast);
                        self.history.push(History {
                            from: "System".to_string(),
                            message: toast_msg,
                            timestamp: chrono::Local::now().to_rfc3339(),
                        });
                        self.notifications += 1;
                    } else if let Cmd::RemotePluginToolResult { request_id, plugin_id, tool_name, success, result_json } = cmd {
                        log::info!("Remote plugin tool result: {plugin_id}::{tool_name} req={request_id} success={success}");
                        #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
                        if success && crate::plugins::crash_intel_hooks::is_dump_analysis_result(&plugin_id, &tool_name) {
                            crate::plugins::crash_intel_hooks::ingest_dump_decode_result(
                                self.client.connection_string.clone(),
                                self.client.computer.clone(),
                                tool_name.clone(),
                                result_json.clone(),
                            );
                        }
                        #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
                        if success && crate::plugins::crash_intel_hooks::is_kernel_triage_result(&plugin_id, &tool_name) {
                            crate::plugins::crash_intel_hooks::ingest_kernel_triage_result(
                                self.client.connection_string.clone(),
                                self.client.computer.clone(),
                                tool_name.clone(),
                                result_json.clone(),
                            );
                        }
                        #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
                        if success && crate::plugins::driver_intel_hooks::is_driver_snapshot_result(&plugin_id, &tool_name) {
                            crate::plugins::driver_intel_hooks::ingest_driver_snapshot(
                                self.client.connection_string.clone(),
                                self.client.computer.clone(),
                                result_json.clone(),
                            );
                        }
                        #[cfg(not(target_arch = "wasm32"))]
                        crate::plugins::mcp_bridge::resolve_pending_request(&request_id, success, result_json);
                    } else if let Cmd::AppPong { nonce, sent_at_ms } = cmd {
                        // Application-layer pong from the remote
                        // client.  Update the last-pong instant and
                        // compute the round-trip latency in ms (only
                        // for log clarity — no UI surface yet).  We
                        // don't strictly check the nonce matches our
                        // most-recent ping; if the client pongs an
                        // older nonce, that still proves the dispatch
                        // loop is alive, which is what we care about.
                        self.last_app_pong_received = Some(web_time::Instant::now());
                        let now_ms = web_time::SystemTime::now()
                            .duration_since(web_time::SystemTime::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(sent_at_ms);
                        let rtt_ms = now_ms.saturating_sub(sent_at_ms);
                        log::debug!(
                            "AppPong from {}: nonce={nonce} rtt={rtt_ms}ms",
                            self.client.connection_string
                        );
                    } else if let Cmd::OpenServiceCandidatesResponse { match_, candidates, live_specs } = cmd {
                        // Stash the suggestion in the global store keyed
                        // by this session's connection_string.  The card
                        // UI in tabs::tasks::client_cards reads from
                        // there each frame to render the chip; the
                        // Stage-4 confirmation modal reads the full
                        // payload (incl. live_specs) for the merge
                        // preview.
                        log::info!(
                            "OpenServiceCandidatesResponse for {}: match={} candidates={}",
                            self.client.connection_string,
                            match_.is_some(),
                            candidates.len()
                        );
                        crate::open_service_suggestions::put(
                            &self.client.connection_string,
                            crate::open_service_suggestions::OpenServiceSuggestion::from_cmd(
                                match_, candidates, live_specs,
                            ),
                        );
                    } else {
                        let _ = self.receive_cmd_tx.try_send(cmd);
                    }
                } else if bin.len() > 0 {
                    self.loading = false;
                    let msg = String::from_utf8_lossy(&bin).to_string();
                    let trimmed = msg.trim();

                    // Check if this message is or contains the "DONE" marker
                    if trimmed == "DONE" {
                        // DONE arrived as a separate message - finalize the buffer
                        if !self.buffer.is_empty() {
                            self.history.push(History {
                                from: "Client".to_string(),
                                message: self.buffer.trim().to_string(),
                                timestamp: chrono::Local::now().to_rfc3339(),
                            });
                            self.buffer.clear();
                            self.notifications += 1;
                        }
                    } else if trimmed.ends_with("DONE") {
                        // DONE is appended to content - extract content and finalize
                        let content = trimmed.trim_end_matches("DONE").trim();
                        if !content.is_empty() {
                            self.buffer.push_str(content);
                            self.buffer.push('\n');
                        }
                        
                        if !self.buffer.is_empty() {
                            self.history.push(History {
                                from: "Client".to_string(),
                                message: self.buffer.trim().to_string(),
                                timestamp: chrono::Local::now().to_rfc3339(),
                            });
                            self.buffer.clear();
                            self.notifications += 1;
                        }
                    } else if msg.is_ascii() {
                        // Regular output - append to buffer
                        self.buffer.push_str(&msg);
                        if !msg.ends_with('\n') {
                            self.buffer.push('\n');
                        }
                    }
                }
            }
    }

    fn handle_text_message(&mut self, text: String) {
        if text.eq("Closed") {
            self.transport.close();
            return;
        }

        // Drift sentinel: __SHAPE_FP_MISMATCH__|<peer_fp>|<peer_ver>|<local_fp>|<local_ver>.
        if let Some(rest) = text.strip_prefix("__SHAPE_FP_MISMATCH__|") {
            let parts: Vec<&str> = rest.split('|').collect();
            let (peer_fp, peer_ver, local_fp, local_ver) = match parts.as_slice() {
                [pf, pv, lf, lv] => (*pf, *pv, *lf, *lv),
                _ => ("?", "?", "?", "?"),
            };
            self.cmd_protocol_mismatch = true;
            self.history.push(History {
                from: "System".to_string(),
                message: format!(
                    "Client build is out of date (Cmd protocol mismatch): client fp={peer_fp} \
                     ver={peer_ver} vs admin fp={local_fp} ver={local_ver}. Push a self-update."
                ),
                timestamp: chrono::Local::now().to_rfc3339(),
            });
            let _ = crate::get_toast_sender().try_send(crate::ToastMessage::Warning(
                "Client build is out of date (Cmd protocol mismatch) — push a self-update"
                    .to_string(),
            ));
            return;
        }


        // Handle connection state notifications from WebSocket server
        match text.as_str() {
            "CLIENT_CONNECTED" => {
                log::info!("Client reconnected to room");
                self.is_connected = true;
                self.connection_status = "Client Connected".to_string();
                self.history.push(History {
                    from: "System".to_string(),
                    message: "Client reconnected".to_string(),
                    timestamp: chrono::Local::now().to_rfc3339(),
                });
                return;
            }
            "CLIENT_DISCONNECTED" => {
                log::info!("Client disconnected from room");
                self.is_connected = false;
                self.connection_status = "Client Disconnected".to_string();
                self.history.push(History {
                    from: "System".to_string(),
                    message: "Client disconnected".to_string(),
                    timestamp: chrono::Local::now().to_rfc3339(),
                });
                return;
            }
            "MASTER_CONNECTED" | "MASTER_DISCONNECTED" => {
                // These are for client-side, ignore on master
                return;
            }
            _ => {
                // Still filter out any legacy status messages if they somehow arrive
                if text.starts_with("CLIENT_STATUS:") || text.starts_with("MASTER_STATUS:") {
                    return;
                }
            }
        }
        
        self.loading = false;
        log::info!("Text data: {text:#?}");
    
        // Append the incoming text to the buffer
        self.buffer.push_str(&text);
    
        // Process the buffer for complete lines
        while let Some(pos) = self.buffer.find('\n') {
            // Extract the complete line up to the newline character
            let line = self.buffer.drain(..=pos).collect::<String>().trim_end().to_string();
    
            // Create a new history entry for the extracted line
            let history = History {
                from: "Client".to_string(),
                message: line,
                timestamp: chrono::Local::now().to_rfc3339(),
            };
    
            // Add to history
            self.history.push(history);
            self.notifications += 1;
        }
    }
}