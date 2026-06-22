use eframe::egui::{Align, Color32, Id, Layout, RichText, Ui};
use crate::Cmd;
use crate::ui_tools::icons::{self, menu_label};
use crate::EGUI_INPUT_TAG;
use bincode::config::standard;
use ewebsock::WsMessage;
#[cfg(not(target_arch = "wasm32"))]
use crate::plugins::remote::EguiInputEvent;
use super::WebSocketClient;


pub enum WsDisplayState {
    /// RMM-style overview for this connected client: hardware inventory,
    /// live stress-run status with countdown, live telemetry charts, and
    /// a Processes sub-tab. Default landing screen for a new session.
    /// Replaces the old `LiveStats` standalone tab.
    Home,
    Explorer,
    Shell,
    ToolBox,
    Terminal,
    EventLog,
    Services,
    TaskScheduler,
    Registry,
    StartupApps,
    Scripts,
    /// Slice 3: Installed Programs viewer (egui_data_table).
    InstalledPrograms,
    /// Per-client log of MCP tool calls proxied through the admin
    /// Web Console (read from the global `mcp_tool_log` store).
    McpToolLog,
    /// Customer service record for the machine linked to this client:
    /// the matched task's ticket, check-in notes, recommendations, task
    /// notes, diagnostic sessions, and history.
    ServiceRecord,
}

impl WebSocketClient {
    pub fn show(&mut self, ui: &mut Ui) {
        self.receive(ui.ctx());
        ui.set_min_height(600.);

        // ── Unified menu-button toolbar ──────────────────────────────────
        //
        // The old layout exposed ~15 inline buttons split across two rows
        // (View tabs on top, System Inspection + Transfer on the bottom)
        // plus a right-aligned cluster of OS power actions. With a wide
        // enough window it crowded out the connection status; on narrower
        // windows buttons wrapped messily. Now each functional group is
        // its own MenuButton on a single row:
        //
        //   [View ▾] [Inspect ▾] [Transfer ▾] [Power ▾]   <current state>
        //                                          (right-aligned status)
        //
        // The currently-active page is shown as a small badge after the
        // menus so the operator can see what they're looking at without
        // having to remember which tab they last clicked.
        eframe::egui::Panel::top(Id::new(format!("ClientTopPanel-{}", self.client.client_hash)))
        .exact_size(38.)
        .show_inside(ui, |ui| {
            ui.add_space(4.);
            ui.horizontal(|ui| {
                let btn_color = ui.style().visuals.error_fg_color;
                let sys_color = Color32::from_rgb(160, 200, 180);
                let os_btn_color = Color32::from_rgb(180, 180, 200);

                // ── View ─────────────────────────────────────────────
                // The remote-rendered "live" views (Charts, Viewer) are
                // stateful — they emit start/stop commands the operator
                // needs to be able to toggle from this menu, not just
                // open. The Stop variant only appears when the relevant
                // stream is already running.
                ui.menu_button(RichText::new(menu_label("View")).color(btn_color).strong(), |ui| {
                    // Home is the RMM-style overview — hardware inventory,
                    // live charts, running stress tests, processes. Replaces
                    // the old standalone "Charts" entry.
                    if ui
                        .button(format!("{} Home", icons::HOME))
                        .clicked()
                    {
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::Home);
                        // Home renders live charts; fire the LiveData feed if
                        // it isn't already running so the chart_board has data.
                        if !self.live_stats_active {
                            let _ = self.send_cmd_tx.try_send(Cmd::LiveData);
                            self.live_stats_active = true;
                        }
                        ui.close();
                    }
                    if ui.button("My Tools").clicked() {
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::ToolBox);
                        let _ = self.toolbox.request_contents("/");
                        ui.close();
                    }
                    if ui.button("Explorer").clicked() {
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::Explorer);
                        self.notifications = 0;
                        if !self.interactive {
                            let path = if self.remote_explorer.current_path.is_empty() {
                                "current".to_string()
                            } else {
                                self.remote_explorer.current_path.clone()
                            };
                            let _ = self.send_cmd_tx.try_send(Cmd::ListDirectory(path));
                            self.remote_explorer.loading = true;
                            if self.remote_explorer.drives.is_empty() {
                                let _ = self.send_cmd_tx.try_send(Cmd::GetDrives);
                            }
                        }
                        ui.close();
                    }
                    let notifs = if matches!(self.state, WsDisplayState::Shell) {
                        "Shell".to_string()
                    } else if self.notifications > 0 {
                        format!("Shell  ({})", self.notifications)
                    } else {
                        "Shell".to_string()
                    };
                    if ui.button(notifs).clicked() {
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::Shell);
                        ui.close();
                    }
                    if ui
                        .button(format!("{} Service Record", icons::TASK_EXISTS))
                        .on_hover_text(
                            "Service ticket, check-in notes, recommendations, task notes, and diagnostic sessions for the machine linked to this client",
                        )
                        .clicked()
                    {
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::ServiceRecord);
                        ui.close();
                    }
                    ui.separator();
                    // Live data feed toggle. Home's charts depend on this feed.
                    if self.live_stats_active {
                        if ui
                            .button(
                                RichText::new(format!("{} Stop live data", icons::STOP))
                                    .color(ui.style().visuals.error_fg_color),
                            )
                            .clicked()
                        {
                            let _ = self.send_cmd_tx.try_send(Cmd::Quit);
                            self.live_stats_active = false;
                            ui.close();
                        }
                    } else if ui
                        .button(format!("{} Start live data", icons::PLAY))
                        .on_hover_text("Resume the live telemetry feed that drives the Home page charts.")
                        .clicked()
                    {
                        let _ = self.send_cmd_tx.try_send(Cmd::LiveData);
                        self.live_stats_active = true;
                        ui.close();
                    }
                    if self.egui_viewer_active {
                        if ui.button(RichText::new(format!("{} Stop Viewer", icons::STOP)).color(ui.style().visuals.error_fg_color)).clicked() {
                            self.egui_viewer_active = false;
                            let _ = self.send_cmd_tx.try_send(Cmd::SetFrameCapture { enabled: false });
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::Shell);
                            ui.close();
                        }
                    } else if ui.button(format!("{} Start Viewer", icons::PLAY)).clicked() {
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::Terminal);
                        self.egui_viewer_active = true;
                        let _ = self.send_cmd_tx.try_send(Cmd::SetFrameCapture { enabled: true });
                        ui.close();
                    }
                    if self.interactive {
                        ui.separator();
                        if ui.button(RichText::new("Quit interactive shell").color(ui.style().visuals.error_fg_color)).clicked() {
                            let _ = self.send_cmd_tx.try_send(Cmd::Quit);
                            ui.close();
                        }
                    }
                    ui.separator();
                    let pending = crate::mcp_tool_log::pending_count(&self.client.connection_string);
                    let label = if pending > 0 {
                        format!("MCP Tool Log  ({pending} running)")
                    } else {
                        "MCP Tool Log".to_string()
                    };
                    let text = if pending > 0 {
                        RichText::new(label).color(Color32::from_rgb(255, 200, 80))
                    } else {
                        RichText::new(label)
                    };
                    if ui.button(text).clicked() {
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::McpToolLog);
                        ui.close();
                    }
                });

                // ── Inspect ──────────────────────────────────────────
                // Read-only system surfaces: every entry kicks off the
                // initial list-fetch if it hasn't been loaded yet,
                // matching the old per-button behavior.
                ui.menu_button(
                    RichText::new(menu_label("Inspect")).color(sys_color).strong(),
                    |ui| {
                        if ui.button("Event Log").clicked() {
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::EventLog);
                            if self.event_log_viewer.entries.is_empty() {
                                let _ = self.send_cmd_tx.try_send(Cmd::ReadEventLog {
                                    log_name: self.event_log_viewer.selected_log.clone(),
                                    max_entries: self.event_log_viewer.max_entries,
                                    level_filter: None,
                                });
                                self.event_log_viewer.loading = true;
                            }
                            ui.close();
                        }
                        if ui.button("Services").clicked() {
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::Services);
                            if self.services_viewer.entries.is_empty() {
                                let _ = self.send_cmd_tx.try_send(Cmd::ListServices);
                                self.services_viewer.loading = true;
                            }
                            ui.close();
                        }
                        if ui.button("Task Scheduler").clicked() {
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::TaskScheduler);
                            if self.task_scheduler_viewer.entries.is_empty() {
                                let _ = self.send_cmd_tx.try_send(Cmd::ListScheduledTasks { folder: None });
                                self.task_scheduler_viewer.loading = true;
                            }
                            ui.close();
                        }
                        if ui.button("Registry").clicked() {
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::Registry);
                            ui.close();
                        }
                        if ui.button("Startup Apps").clicked() {
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::StartupApps);
                            if self.startup_apps_viewer.entries.is_empty() {
                                let _ = self.send_cmd_tx.try_send(Cmd::ListStartupApps);
                                self.startup_apps_viewer.loading = true;
                            }
                            ui.close();
                        }
                        if ui.button("Scripts").clicked() {
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::Scripts);
                            if !(self.remote_scripts_viewer.loading || self.remote_scripts_viewer.running) {
                                self.remote_scripts_viewer.loading = true;
                                let _ = self.send_cmd_tx.try_send(Cmd::GetRemoteScriptList);
                            }
                            ui.close();
                        }
                        if ui.button("Installed Programs").clicked() {
                            // Slice 3: opens the registry-walk
                            // viewer. Lazy-load on first open
                            // (matches the other Inspect entries).
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::InstalledPrograms);
                            if self.installed_programs_viewer.entries.is_empty() {
                                self.installed_programs_viewer.loading = true;
                                let _ = self.send_cmd_tx.try_send(Cmd::ListInstalledPrograms);
                            }
                            ui.close();
                        }
                    },
                );

                // ── Transfer ─────────────────────────────────────────
                // While a transfer is in flight we replace the menu with
                // a progress label so the operator can see the chunk
                // counter without having to open the menu first.
                #[cfg(not(target_arch = "wasm32"))]
                if let Some((ref name, sent, total)) = self.file_transfer_progress {
                    let short = name.rsplit(['/', '\\']).next().unwrap_or(name);
                    ui.colored_label(
                        Color32::YELLOW,
                        format!("Sending {short}  {sent}/{total}"),
                    );
                } else {
                    ui.menu_button(
                        RichText::new(menu_label("Transfer")).color(sys_color).strong(),
                        |ui| {
                            #[cfg(not(target_arch = "wasm32"))]
                            {
                                if ui.button("Send File…").clicked() {
                                    if let Some(path) = rfd::FileDialog::new().pick_file() {
                                        let path_str = path.display().to_string();
                                        let (tx, rx) = crossbeam::channel::bounded::<Cmd>(8);
                                        self.file_transfer_rx = Some(rx);
                                        std::thread::spawn(move || {
                                            Self::chunk_and_send_file(&path_str, tx);
                                        });
                                    }
                                    ui.close();
                                }
                                if ui
                                    .button(
                                        RichText::new("Deploy MasterTech Update…")
                                            .color(Color32::from_rgb(80, 200, 255)),
                                    )
                                    .on_hover_text(
                                        "Push a new MasterTech.exe to this remote client.\nIt will replace itself and relaunch automatically.",
                                    )
                                    .clicked()
                                {
                                    if let Some(path) = rfd::FileDialog::new()
                                        .add_filter("Executable", &["exe"])
                                        .set_title("Select new MasterTech.exe to deploy")
                                        .pick_file()
                                    {
                                        let path_str = path.display().to_string();
                                        let (tx, rx) = crossbeam::channel::bounded::<Cmd>(8);
                                        self.self_update_rx = Some(rx);
                                        std::thread::spawn(move || {
                                            Self::chunk_and_send_self_update(&path_str, tx);
                                        });
                                    }
                                    ui.close();
                                }
                            }
                        },
                    );
                }

                // ── Power ────────────────────────────────────────────
                // Destructive OS-level operations. Lock is the only
                // non-destructive entry; the others either log the user
                // out, reboot, or shut down — order them from least to
                // most disruptive so the destructive ones don't sit at
                // the top of the menu.
                ui.menu_button(
                    RichText::new(menu_label("Power")).color(os_btn_color).strong(),
                    |ui| {
                        if ui.button(RichText::new("Lock workstation").color(os_btn_color)).clicked() {
                            let _ = self.send_cmd_tx.try_send(Cmd::LockWorkstation);
                            ui.close();
                        }
                        if ui.button(RichText::new("Log off user").color(os_btn_color)).clicked() {
                            let _ = self.send_cmd_tx.try_send(Cmd::LogOffUser);
                            ui.close();
                        }
                        ui.separator();
                        if ui.button(RichText::new("Reboot").color(os_btn_color)).clicked() {
                            let _ = self.send_cmd_tx.try_send(Cmd::RebootSystem {
                                persist_mastertech: true,
                                terminal_mode: false,
                            });
                            ui.close();
                        }
                        if ui
                            .button(RichText::new("Switch to Terminal Mode").color(os_btn_color))
                            .clicked()
                        {
                            let _ = self.send_cmd_tx.try_send(Cmd::LaunchTerminalMode);
                            ui.close();
                        }
                        ui.separator();
                        if ui.button(RichText::new("Shutdown").color(Color32::LIGHT_RED)).clicked() {
                            let _ = self.send_cmd_tx.try_send(Cmd::ShutdownSystem);
                            ui.close();
                        }
                    },
                );

                ui.separator();

                // Show which sub-page is currently active. Operators
                // arriving back at the window after switching tabs lose
                // their place otherwise.
                let current_view = match self.state {
                    WsDisplayState::Home          => "Home",
                    WsDisplayState::Explorer      => "Explorer",
                    WsDisplayState::Shell         => "Shell",
                    WsDisplayState::ToolBox       => "My Tools",
                    WsDisplayState::Terminal      => "Remote Viewer",
                    WsDisplayState::EventLog      => "Event Log",
                    WsDisplayState::Services      => "Services",
                    WsDisplayState::TaskScheduler => "Task Scheduler",
                    WsDisplayState::Registry      => "Registry",
                    WsDisplayState::StartupApps   => "Startup Apps",
                    WsDisplayState::Scripts       => "Scripts",
                    WsDisplayState::InstalledPrograms => "Installed Programs",
                    WsDisplayState::McpToolLog    => "MCP Tool Log",
                    WsDisplayState::ServiceRecord => "Service Record",
                };
                ui.label(
                    RichText::new(current_view)
                        .color(Color32::from_rgb(200, 200, 220))
                        .small(),
                );

                if self.persistent_shell_mode {
                    ui.separator();
                    ui.colored_label(Color32::YELLOW, "Persistent Shell");
                }

                // ── Right-aligned status indicator ───────────────────
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (status_color, status_text, status_tooltip) = if !self.client.connected {
                        (Color32::RED, icons::STATUS_ERR, "Disconnected")
                    } else if let Some(last_activity) = &self.client.last_update {
                        let now = chrono::Utc::now();
                        let activity_time = last_activity.to_utc();
                        let elapsed_secs = (now - activity_time).num_seconds();
                        if elapsed_secs < 30 {
                            (Color32::GREEN, icons::STATUS_ON, "Active")
                        } else if elapsed_secs < 120 {
                            (Color32::YELLOW, icons::STATUS_WARN, "Stale")
                        } else {
                            (Color32::LIGHT_RED, icons::STATUS_WAIT, "Inactive")
                        }
                    } else if self.is_connected {
                        (
                            Color32::from_rgb(100, 200, 100),
                            icons::STATUS_IDLE,
                            "Connected (awaiting activity)",
                        )
                    } else {
                        (ui.style().visuals.error_fg_color, icons::STATUS_ERR, "Disconnected")
                    };

                    ui.colored_label(status_color, status_text)
                        .on_hover_text(status_tooltip);
                });
            });
        });

        match self.state {
            WsDisplayState::Home => self.show_home(ui),
            WsDisplayState::Explorer => {
                let cmd_tx = self.send_cmd_tx.clone();
                ui.group(|ui| self.remote_explorer.display(ui, &cmd_tx)).inner
            },
            WsDisplayState::ToolBox => ui.group(|ui| self.toolbox.display(ui)).inner,
            WsDisplayState::Shell => self.show_shell(ui),
            WsDisplayState::Terminal => {
                #[cfg(feature="tokio")]
                {
                    self.egui_viewer.poll_frames();

                    let terminal_has_data = self.remote_terminal.latest_frame_index > 0;
                    let egui_has_data = self.egui_viewer.has_received_frame;

                    if terminal_has_data {
                        self.remote_terminal.ui(ui);
                    } else if egui_has_data {
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            let Self {
                                egui_remote_popout,
                                egui_viewer,
                                transport,
                                ..
                            } = self;
                            ui.checkbox(
                                egui_remote_popout,
                                "Open remote UI in separate window",
                            );
                            if !*egui_remote_popout {
                                let tag = EGUI_INPUT_TAG;
                                let mcp_sess = self.client.connection_string.as_str();
                                egui_viewer.ui(
                                    ui,
                                    |ev| {
                                    let loud = matches!(
                                        &ev,
                                        EguiInputEvent::PointerButton { .. }
                                            | EguiInputEvent::PointerLeave
                                            | EguiInputEvent::Scroll { .. }
                                            | EguiInputEvent::Key { .. }
                                            | EguiInputEvent::Text(_)
                                    );
                                    match bincode::serde::encode_to_vec(&ev, standard()) {
                                        Ok(ser) => {
                                            let mut v = vec![tag];
                                            v.extend(ser);
                                            if loud {
                                                log::error!(
                                                    target: "egui_remote",
                                                    "[admin_ws_embed] send {:?} ({} bytes)",
                                                    ev,
                                                    v.len()
                                                );
                                            } else {
                                                log::debug!(
                                                    target: "egui_remote",
                                                    "[admin_ws_embed] send PointerMoved ({} bytes)",
                                                    v.len()
                                                );
                                            }
                                            transport.send(WsMessage::Binary(v));
                                        }
                                        Err(e) => {
                                            log::error!(
                                                target: "egui_remote",
                                                "[admin_ws_embed] bincode encode failed for {ev:?}: {e}"
                                            );
                                        }
                                    }
                                    },
                                    Some(mcp_sess),
                                );
                            } else {
                                ui.label(
                                    RichText::new(
                                        "Remote UI is in a separate window. Close that window or uncheck above to embed here.",
                                    )
                                    .color(Color32::GRAY)
                                    .small(),
                                );
                            }
                        }
                        #[cfg(target_arch = "wasm32")]
                        {
                            self.egui_viewer.ui(ui, |_| {}, None);
                        }
                    } else {
                        ui.vertical_centered(|ui| {
                            ui.add_space(40.0);
                            ui.label(
                                RichText::new("Connecting to remote viewer...")
                                    .color(Color32::GRAY)
                                    .size(14.0),
                            );
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new("Waiting for terminal or egui frame data from the remote instance.")
                                    .color(Color32::from_rgb(120, 120, 140))
                                    .small(),
                            );
                            ui.spinner();
                        });
                    }
                }
            },
            WsDisplayState::EventLog => {
                let cmd_tx = self.send_cmd_tx.clone();
                self.event_log_viewer.display(ui, &cmd_tx);
            },
            WsDisplayState::Services => {
                let cmd_tx = self.send_cmd_tx.clone();
                self.services_viewer.display(ui, &cmd_tx);
            },
            WsDisplayState::TaskScheduler => {
                let cmd_tx = self.send_cmd_tx.clone();
                self.task_scheduler_viewer.display(ui, &cmd_tx);
            },
            WsDisplayState::Registry => {
                let cmd_tx = self.send_cmd_tx.clone();
                self.registry_editor.display(ui, &cmd_tx);
            },
            WsDisplayState::StartupApps => {
                let cmd_tx = self.send_cmd_tx.clone();
                self.startup_apps_viewer.display(ui, &cmd_tx);
            },
            WsDisplayState::Scripts => {
                let cmd_tx = self.send_cmd_tx.clone();
                self.remote_scripts_viewer.display(ui, &cmd_tx);
            },
            WsDisplayState::InstalledPrograms => {
                let cmd_tx = self.send_cmd_tx.clone();
                self.installed_programs_viewer.display(ui, &cmd_tx);
            },
            WsDisplayState::McpToolLog => {
                let cs = self.client.connection_string.clone();
                self.mcp_tool_log_viewer.display(ui, &cs);
            },
            WsDisplayState::ServiceRecord => {
                let client = self.client.clone();
                let state_tx = self.display_state_channel.0.clone();
                self.service_record.display(ui, &client, &state_tx);
            },
        };
    }

    /// Read a file in 512 KB chunks and send each as a `Cmd::DirectFileTransfer`.
    /// Runs on a background thread; chunks are picked up by `receive()` via the channel.
    #[cfg(not(target_arch = "wasm32"))]
    fn chunk_and_send_file(path: &str, tx: crossbeam::channel::Sender<Cmd>) {
        const CHUNK_SIZE: usize = 512 * 1024;

        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(e) => {
                log::error!("Failed to read file for transfer: {path}: {e}");
                return;
            }
        };

        let filename = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());

        let total_chunks = ((data.len() + CHUNK_SIZE - 1) / CHUNK_SIZE).max(1) as u32;

        for (i, chunk) in data.chunks(CHUNK_SIZE).enumerate() {
            let cmd = Cmd::DirectFileTransfer {
                filename: filename.clone(),
                chunk_index: i as u32,
                total_chunks,
                data: chunk.to_vec(),
            };
            if tx.send(cmd).is_err() {
                log::error!("File transfer channel closed at chunk {i}/{total_chunks}");
                return;
            }
        }

        log::info!("File transfer queued: {filename} ({} bytes, {total_chunks} chunks)", data.len());
    }

    /// Read a file in 512 KiB chunks and send each as a
    /// [`Cmd::MastertechSelfUpdateChunk`] for remote self-update.
    #[cfg(not(target_arch = "wasm32"))]
    fn chunk_and_send_self_update(path: &str, tx: crossbeam::channel::Sender<Cmd>) {
        const CHUNK_SIZE: usize = 512 * 1024;

        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(e) => {
                log::error!("Failed to read binary for self-update: {path}: {e}");
                return;
            }
        };

        let total_chunks = ((data.len() + CHUNK_SIZE - 1) / CHUNK_SIZE).max(1) as u32;

        for (i, chunk) in data.chunks(CHUNK_SIZE).enumerate() {
            let cmd = Cmd::MastertechSelfUpdateChunk {
                chunk_index: i as u32,
                total_chunks,
                data: chunk.to_vec(),
            };
            if tx.send(cmd).is_err() {
                log::error!("Self-update channel closed at chunk {i}/{total_chunks}");
                return;
            }
        }

        log::info!(
            "Self-update queued: {} bytes, {total_chunks} chunks",
            data.len()
        );
    }
}