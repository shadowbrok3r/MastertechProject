use crate::{tabs::admin_console::client_interface::serialize_command, virtual_filesystem::FileSysHelper, Cmd, FileSystemAction};
use database::schema::{Node, SystemInformation};
use ewebsock::{WsEvent, WsMessage};
use eframe::egui::Context;

use super::{deserializer, ui::WsDisplayState, History, WebSocketClient};

impl WebSocketClient {
    pub fn receive(&mut self, ctx: &Context) {
        #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
        if let Some(rx) = self.remote_egui_mcp_rx.as_ref() {
            while let Ok(bin) = rx.try_recv() {
                self.transport.send(WsMessage::Binary(bin));
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

        // Drain ALL pending websocket messages in one frame to avoid backlog.
        // For terminal binary buffers, keep only the latest to skip stale frames.
        let mut latest_terminal_bin: Option<Vec<u8>> = None;
        while let Some(event) = self.transport.try_recv() {
            match event {
                WsEvent::Message(msg) => {
                    match msg {
                        WsMessage::Binary(bin) => {
                            // When showing the terminal viewer, raw frames (zstd) go to the
                            // terminal buffer. But admin-side Cmds (RemotePluginToolResult,
                            // DirectFileTransferResult, LoadWasmPluginResult) must NEVER be
                            // swallowed as terminal data — check for those first.
                            let mut handled_as_admin_cmd = false;
                            if matches!(self.state, WsDisplayState::Terminal)
                                && bin.first() != Some(&crate::EGUI_FRAME_TAG)
                            {
                                if let Some(decoded) = deserializer::<Cmd>(&bin) {
                                    match decoded {
                                        Cmd::RemotePluginToolResult { request_id, plugin_id, tool_name, success, result_json } => {
                                            log::info!("Remote plugin tool result (terminal state): {plugin_id}::{tool_name} req={request_id} success={success}");
                                            #[cfg(not(target_arch = "wasm32"))]
                                            crate::plugins::mcp_bridge::resolve_pending_request(&request_id, success, result_json);
                                            handled_as_admin_cmd = true;
                                        }
                                        Cmd::DirectFileTransferResult { filename, success, message } => {
                                            log::info!("File transfer result (terminal state): {filename} success={success}");
                                            self.history.push(History {
                                                from: "System".to_string(),
                                                message: format!("File transfer '{filename}': {message}"),
                                                timestamp: chrono::Local::now().to_rfc3339(),
                                            });
                                            self.notifications += 1;
                                            handled_as_admin_cmd = true;
                                        }
                                        Cmd::LoadWasmPluginResult { plugin_id, success, message } => {
                                            log::info!("WASM plugin result (terminal state): {plugin_id} success={success}");
                                            self.history.push(History {
                                                from: "System".to_string(),
                                                message: format!("WASM plugin '{plugin_id}': {message}"),
                                                timestamp: chrono::Local::now().to_rfc3339(),
                                            });
                                            self.notifications += 1;
                                            handled_as_admin_cmd = true;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            if !handled_as_admin_cmd {
                                if matches!(self.state, WsDisplayState::Terminal) {
                                    latest_terminal_bin = Some(bin);
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
                    self.is_connected = true;
                    self.connection_status = "Connected".to_string();
                    self.history.push(History { 
                        from: "Client".to_string(), 
                        message: "Connection opened".to_string(), 
                        timestamp:  chrono::Local::now().to_rfc3339()
                    });
                    self.notifications += 1;
                },
                WsEvent::Closed => {
                    self.is_connected = false;
                    self.connection_status = "Disconnected".to_string();
                    self.last_pong_time = None;
                    self.history.push(History { 
                        from: "Client".to_string(), 
                        message: "Connection closed".to_string(), 
                        timestamp:  chrono::Local::now().to_rfc3339()
                    });
                    self.notifications += 1;
                },
                WsEvent::Error(err) => {
                    self.is_connected = false;
                    self.connection_status = format!("Error: {}", err);
                    self.history.push(History { 
                        from: "Client".to_string(), 
                        message: format!("Connection error: {}", err), 
                        timestamp:  chrono::Local::now().to_rfc3339()
                    });
                    self.notifications += 1;
                },
            }
        }

        // Forward only the latest terminal buffer, skipping all stale frames
        if let Some(bin) = latest_terminal_bin {
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
                        log::info!("ACTION TO SEND: {command:?}");
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
        if bin.first() == Some(&crate::EGUI_FRAME_TAG) {
            return;
        }
        match self.state {
            WsDisplayState::LiveStats => {
                // let bin = &self.handle_binary_message(bin);
                if let Some(sysinfo) = deserializer::<SystemInformation>(&bin){
                    log::info!("Got sysinfo from admin console");
                    self.resource_monitor.set_sysinfo(sysinfo);
                }
            },
            WsDisplayState::Terminal => {
                let _ = self.msg_from_client_tx.try_send(WsMessage::Binary(bin));
            },
            _ => {
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
                            // Normal file download
                            match self.remote_explorer.handle_file_download(data, is_last, &mut self.download_buffer) {
                                Ok(Some(msg)) => {
                                    self.history.push(History {
                                        from: "System".to_string(),
                                        message: msg,
                                        timestamp: chrono::Local::now().to_rfc3339(),
                                    });
                                }
                                Ok(None) => {}
                                Err(msg) => {
                                    self.download_buffer.clear();
                                    self.history.push(History {
                                        from: "System".to_string(),
                                        message: format!("Download failed: {}", msg),
                                        timestamp: chrono::Local::now().to_rfc3339(),
                                    });
                                }
                            }
                        }
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
                        self.remote_scripts_viewer.set_script_list(categories);
                        if let Some(user) = crate::get_current_user_from_auth() {
                            self.remote_scripts_viewer.load_custom_scripts(&user.get_user_bucket_name());
                        }
                    } else if let Cmd::RemoteScriptLog(msg) = cmd {
                        crate::plugins::remote_script_notify::notify_remote_script_log(msg.clone());
                        self.remote_scripts_viewer.append_log(msg);
                    } else if let Cmd::RemoteScriptResult { name, status } = cmd {
                        log::info!("Script result: {} - {:?}", name, status);
                        crate::plugins::remote_script_notify::notify_remote_script_result(
                            name.clone(),
                            format!("{:?}", status),
                        );
                        self.remote_scripts_viewer.set_script_result(name, status);
                    } else if let Cmd::RemoteScriptsComplete = cmd {
                        log::info!("All remote scripts completed");
                        crate::plugins::remote_script_notify::notify_remote_scripts_complete();
                        self.remote_scripts_viewer.set_complete();
                    } else if let Cmd::LoadWasmPluginResult { plugin_id, success, message } = cmd {
                        log::info!("WASM plugin deploy result: {plugin_id} success={success} {message}");
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
                    } else if let Cmd::MastertechSelfUpdateResult { success, message } = cmd {
                        log::info!("Remote self-update result: success={success} {message}");
                        self.file_transfer_progress = None;
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
                        #[cfg(not(target_arch = "wasm32"))]
                        crate::plugins::mcp_bridge::resolve_pending_request(&request_id, success, result_json);
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
    }

    fn handle_text_message(&mut self, text: String) {
        if text.eq("Closed") {
            self.transport.close();
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