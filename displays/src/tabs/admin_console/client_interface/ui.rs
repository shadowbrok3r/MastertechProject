use eframe::egui::{Align, Id, Layout, ProgressBar, RichText, Ui};
use crate::Cmd;
use crate::ui_tools::icons::{self, menu_label};
use crate::ui_tools::theme;
use crate::EGUI_INPUT_TAG;
use bincode::config::standard;
use ewebsock::WsMessage;
#[cfg(not(target_arch = "wasm32"))]
use crate::plugins::remote::EguiInputEvent;
use super::{AdminTransport, TransportKind, WebSocketClient};


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
    /// The client's own MasterTech log ring, tail or full.
    ClientLog,
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
    /// Full remote-desktop control: live raster screen view with keyboard and
    /// mouse injection into the client's OS.
    RemoteDesktop,
    /// Fleet Intel: crash-signature intelligence (dump analysis, prior
    /// verdicts) and the driver time machine (snapshots, drift, blocklist).
    FleetIntel,
    /// Crash Dumps: this machine's own sightings, signatures, and verdicts.
    CrashDumps,
}

/// Sends one remote-desktop input event on the tagged binary path.
/// Ordering is the point: everything here reaches the client's injection thread
/// in the order it was queued.
///
/// Free function so the desktop viewer's callback can borrow `transport` out of
/// a destructured `WebSocketClient`.
pub fn send_desktop_input_on(
    transport: &mut AdminTransport,
    ev: crate::remote_desktop::DesktopInputEvent,
) {
    match bincode::serde::encode_to_vec(&ev, standard()) {
        Ok(ser) => {
            let mut v = vec![crate::DESKTOP_INPUT_TAG];
            v.extend(ser);
            transport.send(WsMessage::Binary(v));
        }
        Err(e) => log::warn!(target: "remote_desktop", "encode input failed: {e}"),
    }
}

impl WebSocketClient {
    pub fn send_desktop_input(&mut self, ev: crate::remote_desktop::DesktopInputEvent) {
        send_desktop_input_on(&mut self.transport, ev);
    }

    /// Turns clipboard mirroring on or off on both ends. The admin-side poller
    /// follows in `receive`, which reconciles it against the stream state.
    pub fn set_clipboard_sync(&mut self, enabled: bool) {
        self.clipboard_sync = enabled;
        let _ = self.send_cmd_tx.try_send(Cmd::ClipboardSyncEnable { enabled });
    }

    pub fn show(&mut self, ui: &mut Ui) {
        self.receive(ui.ctx());
        ui.set_min_height(600.);

        // Active-run detection drives the nav row's live strip, so it has to run on every view, not
        // only while Home is open. Both calls are interval-gated internally.
        self.home_page
            .ingest_script_log(&self.remote_scripts_viewer.log_messages);
        self.home_page
            .maybe_poll_active_runs(self.client.computer.as_ref());

        // Auto-stop remote-desktop streaming when the operator navigates away.
        if self.desktop_streaming && !matches!(self.state, WsDisplayState::RemoteDesktop) {
            self.desktop_streaming = false;
            let _ = self.send_cmd_tx.try_send(Cmd::DesktopStreamStop);
        }

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
        .show(ui, |ui| {
            ui.add_space(4.);
            ui.horizontal(|ui| {
                let btn_color = ui.style().visuals.error_fg_color;
                // Read-only surfaces take the calm accent; OS-level actions plain text, since the
                // destructive ones in that menu already carry the error color.
                let sys_color = theme::success(ui);
                let os_btn_color = theme::text(ui);

                // Home shortcut doubling as the machine identity — the only place this session
                // names its machine, so the Home page no longer repeats it.
                let machine_label = self
                    .client
                    .friendly_name
                    .clone()
                    .unwrap_or_else(|| self.client.connection_string.clone());
                if ui
                    .button(
                        RichText::new(format!("{} {machine_label}", icons::HOME))
                            .color(btn_color)
                            .strong(),
                    )
                    .on_hover_text(format!("Home — {}", self.client.connection_string))
                    .clicked()
                {
                    let _ = self.display_state_channel.0.try_send(WsDisplayState::Home);
                    if !self.live_stats_active {
                        let _ = self.send_cmd_tx.try_send(Cmd::LiveData);
                        self.live_stats_active = true;
                    }
                }
                if self.client.boot_environment.is_preboot() {
                    ui.label(
                        RichText::new(format!("{} WinPE", icons::POWER))
                            .small()
                            .strong()
                            .color(theme::accent(ui)),
                    )
                    .on_hover_text(
                        "This client booted into WinPE. Its key is the offline Windows \
                         install's; live readings come from the PE session.",
                    );
                }

                // ── View ─────────────────────────────────────────────
                // The remote-rendered "live" views (Charts, Viewer) are
                // stateful — they emit start/stop commands the operator
                // needs to be able to toggle from this menu, not just
                // open. The Stop variant only appears when the relevant
                // stream is already running.
                ui.menu_button(RichText::new(menu_label("View")).color(btn_color).strong(), |ui| {
                    if ui.button(format!("{} My Tools", icons::WRENCH)).clicked() {
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::ToolBox);
                        let _ = self.toolbox.request_contents("/");
                        ui.close();
                    }
                    if ui.button(format!("{} Explorer", icons::FOLDER)).clicked() {
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
                        format!("{} Shell", icons::TERMINAL)
                    } else if self.notifications > 0 {
                        format!("{} Shell  ({})", icons::TERMINAL, self.notifications)
                    } else {
                        format!("{} Shell", icons::TERMINAL)
                    };
                    if ui.button(notifs).clicked() {
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::Shell);
                        ui.close();
                    }
                    if ui
                        .button(format!("{} Fleet Intel", icons::DIAGNOSTICS))
                        .on_hover_text(
                            "Crash-signature intelligence (dump analysis + prior verdicts across the fleet) and driver snapshots/drift/blocklist for this client",
                        )
                        .clicked()
                    {
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::FleetIntel);
                        ui.close();
                    }
                    if ui
                        .button(format!("{} Crash Dumps", icons::CRITICAL))
                        .on_hover_text(
                            "Every crash recorded for THIS machine: minidumps, live-kernel reports, and GPU (Aftermath) dumps with their signatures, fleet prevalence, and verdicts",
                        )
                        .clicked()
                    {
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::CrashDumps);
                        ui.close();
                    }
                    if ui
                        .button(format!("{} Download crash dumps", icons::DOWNLOAD))
                        .on_hover_text(
                            "Zip and download this client's MEMORY.DMP, Minidump\\*, LiveKernelReports\\*, and UE/GPU crash folders (Aftermath dumps + crash context, last 30 days) in one archive",
                        )
                        .clicked()
                    {
                        self.remote_explorer.start_crash_dump_download(&self.send_cmd_tx);
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
                    if self.desktop_streaming {
                        if ui.button(RichText::new(format!("{} Stop Remote Desktop", icons::STOP)).color(ui.style().visuals.error_fg_color)).clicked() {
                            self.desktop_streaming = false;
                            let _ = self.send_cmd_tx.try_send(Cmd::DesktopStreamStop);
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::Home);
                            ui.close();
                        }
                    } else if ui.button(format!("{} Remote Desktop", icons::DESKTOP)).clicked() {
                        self.desktop_streaming = true;
                        let _ = self.send_cmd_tx.try_send(Cmd::DesktopListMonitors);
                        let _ = self.send_cmd_tx.try_send(Cmd::DesktopStreamStart {
                            monitor: self.desktop_monitor,
                            fps: self.desktop_fps,
                            quality: self.desktop_quality,
                            scale: self.desktop_scale,
                        });
                        // The client mirrors by default on stream start; correct
                        // it if the operator had turned mirroring off.
                        self.set_clipboard_sync(self.clipboard_sync);
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::RemoteDesktop);
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
                        RichText::new(label).color(theme::warn(ui))
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
                        if ui.button("Client Log").clicked() {
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::ClientLog);
                            if self.client_log_viewer.text.is_empty() {
                                let cmd = self.client_log_viewer.fetch_cmd();
                                if self.send_cmd_tx.try_send(cmd).is_ok() {
                                    self.client_log_viewer.loading = true;
                                }
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
                #[cfg(not(target_arch = "wasm32"))]
                {
                    // Disabled while a transfer is in flight; a second one would truncate it.
                    let transfer_busy = self.file_transfer_progress.is_some();
                    let transfer_color = if self.cmd_protocol_mismatch {
                        theme::warn(ui)
                    } else {
                        sys_color
                    };
                    ui.menu_button(
                        RichText::new(menu_label("Transfer")).color(transfer_color).strong(),
                        |ui| {
                            #[cfg(not(target_arch = "wasm32"))]
                            #[cfg(not(any(target_os = "ios", target_os = "android")))]
                            {
                                if ui
                                    .add_enabled(
                                        !transfer_busy,
                                        eframe::egui::Button::new("Send File…"),
                                    )
                                    .on_disabled_hover_text("A transfer is already in flight")
                                    .clicked()
                                {
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
                                            .color(theme::info(ui)),
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
                        if ui.button(RichText::new("Shutdown").color(theme::error(ui))).clicked() {
                            let _ = self.send_cmd_tx.try_send(Cmd::ShutdownSystem);
                            ui.close();
                        }
                    },
                );

                // ── Protections ──────────────────────────────────────
                // Disable/re-enable Memory Integrity + the driver blocklist.
                self.driver_protections_toolbar(ui);

                ui.separator();

                // Show which sub-page is currently active. Operators
                // arriving back at the window after switching tabs lose
                // their place otherwise.
                // let current_view = match self.state {
                //     WsDisplayState::Home          => "Home",
                //     WsDisplayState::Explorer      => "Explorer",
                //     WsDisplayState::Shell         => "Shell",
                //     WsDisplayState::ToolBox       => "My Tools",
                //     WsDisplayState::Terminal      => "Remote Viewer",
                //     WsDisplayState::EventLog      => "Event Log",
                //     WsDisplayState::ClientLog     => "Client Log",
                //     WsDisplayState::Services      => "Services",
                //     WsDisplayState::TaskScheduler => "Task Scheduler",
                //     WsDisplayState::Registry      => "Registry",
                //     WsDisplayState::StartupApps   => "Startup Apps",
                //     WsDisplayState::Scripts       => "Scripts",
                //     WsDisplayState::InstalledPrograms => "Installed Programs",
                //     WsDisplayState::McpToolLog    => "MCP Tool Log",
                //     WsDisplayState::RemoteDesktop => "Remote Desktop",
                //     WsDisplayState::FleetIntel    => "Fleet Intel",
                //     WsDisplayState::CrashDumps    => "Crash Dumps",
                // };
                // ui.label(
                //     RichText::new(current_view).color(theme::text(ui)).small(),
                // );

                if self.persistent_shell_mode {
                    ui.colored_label(theme::warn(ui), "Persistent Shell");
                }

                // Home's sub-tabs belong on this row, not on a second row inside the page body.
                if matches!(self.state, WsDisplayState::Home) {
                    self.home_page.sub_tab_toggles(ui);
                }

                // ── Right-aligned status indicator ───────────────────
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (status_color, status_text, status_tooltip) = if !self.client.connected {
                        (theme::error(ui), icons::STATUS_ERR, "Disconnected")
                    } else if let Some(last_activity) = &self.client.last_update {
                        let now = chrono::Utc::now();
                        let activity_time = last_activity.to_utc();
                        let elapsed_secs = (now - activity_time).num_seconds();
                        if elapsed_secs < 30 {
                            (theme::success(ui), icons::STATUS_ON, "Active")
                        } else if elapsed_secs < 120 {
                            (theme::warn(ui), icons::STATUS_WARN, "Stale")
                        } else {
                            (theme::error(ui), icons::STATUS_WAIT, "Inactive")
                        }
                    } else if self.is_connected {
                        (
                            theme::success(ui),
                            icons::STATUS_IDLE,
                            "Connected (awaiting activity)",
                        )
                    } else {
                        (ui.style().visuals.error_fg_color, icons::STATUS_ERR, "Disconnected")
                    };

                    ui.colored_label(status_color, status_text)
                        .on_hover_text(status_tooltip);

                    ui.separator();

                    // ── Transport badge: which path carries this session ──
                    let kind = self.transport.kind();
                    let (badge, badge_tip) = match kind {
                        TransportKind::Tcp => ("TCP", "Direct TCP (same network)"),
                        TransportKind::Relay => ("RELAY", "Relay tunnel via websocket server"),
                        TransportKind::WebSocket => ("WS", "Legacy WebSocket relay room"),
                    };
                    let badge_color = if !self.is_connected {
                        theme::weak_text(ui)
                    } else {
                        crate::tabs::admin_console::ui::transport_color(ui, kind)
                    };
                    let badge_hover = if self.is_connected {
                        badge_tip.to_string()
                    } else {
                        format!("{badge_tip} — {}", self.connection_status)
                    };
                    ui.colored_label(badge_color, RichText::new(badge).small().strong())
                        .on_hover_text(badge_hover);

                    ui.separator();

                    // ── Client build badge: MasterTech version the agent reported ──
                    // Shown in full, hash included: the release number sits still between builds,
                    // so the hash is the only part that identifies which build is out there.
                    if let Some(ver) = self.client_version.as_deref() {
                        let admin_ver = crate::shape_fp::BUILD_VERSION;
                        use crate::shape_fp::release_of;
                        let (ver_color, ver_text, ver_hover) = if self.cmd_protocol_mismatch {
                            (
                                theme::warn(ui),
                                format!("{} v{ver}", icons::STATUS_WARN),
                                format!(
                                    "Client MasterTech build v{ver} — out of date against this \
                                     console's v{admin_ver} (Cmd protocol mismatch). \
                                     Push a self-update."
                                ),
                            )
                        } else if release_of(ver) != release_of(admin_ver) {
                            // Shape matches, so commands still work; the release still differs.
                            (
                                theme::info(ui),
                                format!("v{ver}"),
                                format!(
                                    "Client MasterTech release differs from this console's: \
                                     v{ver} vs v{admin_ver}. The Cmd protocol matches."
                                ),
                            )
                        } else if ver != admin_ver {
                            (
                                theme::weak_text(ui),
                                format!("v{ver}"),
                                format!(
                                    "Same release as this console, different build: \
                                     v{ver} vs v{admin_ver}."
                                ),
                            )
                        } else {
                            (
                                theme::weak_text(ui),
                                format!("v{ver}"),
                                format!("Client MasterTech build v{ver} — identical to this console"),
                            )
                        };
                        ui.colored_label(ver_color, RichText::new(ver_text).small().monospace())
                            .on_hover_text(ver_hover);
                        ui.separator();
                    }

                    // The one place this session's connection string is printed, and the fastest way
                    // to get it into an MCP call or a ticket.
                    if ui
                        .button(
                            RichText::new(self.client.connection_string.as_str())
                                .small()
                                .monospace()
                                .color(theme::weak_text(ui)),
                        )
                        .on_hover_text("Connection string (host:client hash) — click to copy")
                        .clicked()
                    {
                        ui.ctx().copy_text(self.client.connection_string.clone());
                        let _ = crate::get_toast_sender().try_send(crate::ToastMessage::Success(
                            format!("Copied {}", self.client.connection_string),
                        ));
                    }

                    if let Some((ref name, sent, total)) = self.file_transfer_progress {
                        ui.separator();
                        let short = name.rsplit(['/', '\\']).next().unwrap_or(name);
                        let frac = if total > 0 {
                            (sent as f32 / total as f32).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        ui.add(
                            ProgressBar::new(frac)
                                .desired_width(80.0)
                                .desired_height(6.0),
                        )
                        .on_hover_text(format!("Sending {name} — chunk {sent} of {total}"));
                        ui.colored_label(theme::warn(ui), RichText::new(short).small());
                    }

                    // Live stress progress rides this row so it stays visible on every view, not
                    // just Home's inventory.
                    ui.separator();
                    self.home_page.active_run_strip(ui);
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

                    let terminal_has_data = self.remote_terminal.has_received_frame;
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
                                                log::debug!(
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
                                    .color(theme::weak_text(ui))
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
                                    .color(theme::weak_text(ui))
                                    .size(14.0),
                            );
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new("Waiting for terminal or egui frame data from the remote instance.")
                                    .color(theme::faint_text(ui))
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
            WsDisplayState::ClientLog => {
                let cmd_tx = self.send_cmd_tx.clone();
                self.client_log_viewer.display(ui, &cmd_tx);
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
            WsDisplayState::FleetIntel => {
                #[cfg(all(feature = "tokio", not(target_arch = "wasm32")))]
                {
                    let client = self.client.clone();
                    let cmd_tx = self.send_cmd_tx.clone();
                    self.fleet_intel.display(ui, &client, &cmd_tx);
                }
                #[cfg(not(all(feature = "tokio", not(target_arch = "wasm32"))))]
                {
                    ui.label("Fleet Intel requires the native tokio build.");
                }
            },
            WsDisplayState::CrashDumps => {
                #[cfg(all(feature = "tokio", not(target_arch = "wasm32")))]
                {
                    let client = self.client.clone();
                    self.crash_dumps.display(ui, &client);
                }
                #[cfg(not(all(feature = "tokio", not(target_arch = "wasm32"))))]
                {
                    ui.label("Crash Dumps requires the native tokio build.");
                }
            },
            WsDisplayState::RemoteDesktop => {
                #[cfg(all(feature = "tokio", not(target_arch = "wasm32")))]
                {
                    let monitors = self.desktop_monitors.clone();
                    let mut monitor = self.desktop_monitor;
                    let mut fps = self.desktop_fps;
                    let mut quality = self.desktop_quality;
                    let mut scale = self.desktop_scale;
                    let mut streaming = self.desktop_streaming;
                    let mut popout = self.desktop_popout;
                    let mut clipboard_sync = self.clipboard_sync;
                    let mut clipboard_changed = false;
                    let mut restart = false;
                    let mut stop = false;
                    let frames = self.desktop_viewer.frames_shown;
                    let latency = self.desktop_viewer.last_latency_ms;
                    let bytes = self.desktop_viewer.last_frame_bytes;

                    let label = |m: &crate::remote_desktop::DesktopMonitorInfo| {
                        format!(
                            "{}{} ({}x{})",
                            if m.is_primary { "★ " } else { "" },
                            m.name,
                            m.width,
                            m.height
                        )
                    };

                    ui.horizontal_wrapped(|ui| {
                        if !monitors.is_empty() {
                            let selected = monitors
                                .iter()
                                .find(|m| m.id == monitor)
                                .map(|m| label(m))
                                .unwrap_or_else(|| "Primary".to_string());
                            eframe::egui::ComboBox::from_id_salt("remote_desktop_monitor")
                                .selected_text(selected)
                                .show_ui(ui, |ui| {
                                    for m in &monitors {
                                        if ui
                                            .selectable_label(monitor == m.id, label(m))
                                            .clicked()
                                        {
                                            monitor = m.id;
                                            restart = true;
                                        }
                                    }
                                });
                        }
                        if ui
                            .add(eframe::egui::Slider::new(&mut fps, 1..=30).text("fps"))
                            .drag_stopped()
                        {
                            restart = true;
                        }
                        if ui
                            .add(eframe::egui::Slider::new(&mut quality, 20..=90).text("quality"))
                            .drag_stopped()
                        {
                            restart = true;
                        }
                        if ui
                            .add(eframe::egui::Slider::new(&mut scale, 0.25..=1.0).text("scale"))
                            .drag_stopped()
                        {
                            restart = true;
                        }
                        if streaming {
                            if ui.button(format!("{} Stop", icons::STOP)).clicked() {
                                streaming = false;
                                stop = true;
                            }
                        } else if ui.button(format!("{} Start", icons::PLAY)).clicked() {
                            streaming = true;
                            restart = true;
                        }
                        if ui.button(format!("{} Pop out", icons::POPOUT)).clicked() {
                            popout = true;
                        }
                        if ui
                            .checkbox(&mut clipboard_sync, format!("{} Clipboard", icons::CLIPBOARD))
                            .on_hover_text(
                                "Mirror the clipboard both ways while streaming. Copy here and \
                                 paste there, or the reverse.",
                            )
                            .changed()
                        {
                            clipboard_changed = true;
                        }
                        ui.separator();
                        ui.label(
                            RichText::new(format!(
                                "{frames} frames | {latency} ms | {} KB/frame",
                                bytes / 1024
                            ))
                            .small()
                            .color(theme::success(ui)),
                        );
                    });

                    self.desktop_monitor = monitor;
                    self.desktop_fps = fps;
                    self.desktop_quality = quality;
                    self.desktop_scale = scale;
                    self.desktop_streaming = streaming;
                    self.desktop_popout = popout;
                    if clipboard_changed {
                        self.set_clipboard_sync(clipboard_sync);
                    }
                    if stop {
                        let _ = self.send_cmd_tx.try_send(Cmd::DesktopStreamStop);
                    }
                    if restart && streaming {
                        let _ = self.send_cmd_tx.try_send(Cmd::DesktopStreamStart {
                            monitor,
                            fps,
                            quality,
                            scale,
                        });
                        self.set_clipboard_sync(self.clipboard_sync);
                    }

                    ui.separator();
                    if self.desktop_popout {
                        ui.vertical_centered(|ui| {
                            ui.add_space(30.0);
                            ui.label(
                                RichText::new("Remote desktop is popped out to its own window.")
                                    .color(theme::weak_text(ui)),
                            );
                            ui.add_space(8.0);
                            if ui.button("Return to tab").clicked() {
                                self.desktop_popout = false;
                            }
                        });
                    } else {
                        self.desktop_viewer.clipboard_sync = self.clipboard_sync;
                        let Self { desktop_viewer, transport, .. } = self;
                        desktop_viewer.ui(ui, |ev| {
                            send_desktop_input_on(transport, ev);
                        });
                    }
                }
            },
        };
    }

    /// Remote-desktop content for the popped-out OS window.
    #[cfg(all(feature = "tokio", not(target_arch = "wasm32")))]
    pub fn desktop_popout_ui(&mut self, ui: &mut eframe::egui::Ui) {
        let fullscreen = ui
            .ctx()
            .input(|i| i.viewport().fullscreen.unwrap_or(false));
        let mut toggle_fullscreen = false;
        let mut return_to_tab = false;
        let frames = self.desktop_viewer.frames_shown;
        let latency = self.desktop_viewer.last_latency_ms;

        ui.horizontal(|ui| {
            let (icon, label) = if fullscreen {
                (icons::FULLSCREEN_EXIT, "Exit Full Screen")
            } else {
                (icons::FULLSCREEN_ENTER, "Full Screen")
            };
            if ui.button(format!("{icon} {label}")).clicked() {
                toggle_fullscreen = true;
            }
            if ui.button("Return to tab").clicked() {
                return_to_tab = true;
            }
            ui.separator();
            ui.label(
                RichText::new(format!("{frames} frames | {latency} ms"))
                    .small()
                    .color(theme::success(ui)),
            );
        });

        if toggle_fullscreen {
            ui.ctx()
                .send_viewport_cmd(eframe::egui::ViewportCommand::Fullscreen(!fullscreen));
        }
        if return_to_tab {
            self.desktop_popout = false;
        }

        ui.separator();
        self.desktop_viewer.clipboard_sync = self.clipboard_sync;
        let Self { desktop_viewer, transport, .. } = self;
        desktop_viewer.ui(ui, |ev| {
            send_desktop_input_on(transport, ev);
        });
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