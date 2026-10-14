use crate::ui_tools::controls::IconLabel;
use eframe::egui::{Align, Frame, Id, Layout, Margin, RichText, Ui};
use crate::Cmd;
use crate::ui_tools::controls::{self, BarKind, ButtonKind, MenuButton, Metrics, TextButton};
use crate::ui_tools::icons::svg;
use crate::ui_tools::{glass_card, theme};
use crate::EGUI_INPUT_TAG;
use bincode::config::standard;
use ewebsock::WsMessage;
#[cfg(not(target_arch = "wasm32"))]
use crate::plugins::remote::EguiInputEvent;
use super::{AdminTransport, TransportKind, WebSocketClient};


#[derive(Debug)]
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

/// Whether a stream slider finished an adjustment this frame.
#[cfg(all(feature = "tokio", not(target_arch = "wasm32")))]
fn slider_settled(r: &eframe::egui::Response) -> bool {
    r.drag_stopped() || r.clicked() || (r.changed() && !r.is_pointer_button_down_on())
}

impl WsDisplayState {
    /// Pages that only mean something against a remote machine: two frame streams
    /// and an interactive remote shell.
    pub fn is_remote_only(&self) -> bool {
        matches!(self, Self::Shell | Self::Terminal | Self::RemoteDesktop)
    }
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

/// Track width of the remote-desktop stream sliders.
#[cfg(all(feature = "tokio", not(target_arch = "wasm32")))]
const SLIDER_W: f32 = 96.0;

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
        let bar_h = Metrics::get(ui.ctx()).bar_h;
        eframe::egui::Panel::top(Id::new(format!("ClientTopPanel-{}{}", self.id_prefix(), self.client.client_hash)))
        .exact_size(bar_h)
        .frame(Frame::side_top_panel(ui.style()).inner_margin(Margin::ZERO))
        .show_separator_line(false)
        .show(ui, |ui| {
            controls::bar(ui, BarKind::Tool, |ui| {
                let rule_h = Metrics::get(ui.ctx()).control_h;
                let btn_color = theme::primary(ui);
                let sys_color = theme::success(ui);

                // Home shortcut doubling as the machine identity — the only place this session
                // names its machine, so the Home page no longer repeats it.
                let machine_label = self
                    .client
                    .friendly_name
                    .clone()
                    .unwrap_or_else(|| self.client.connection_string.clone());
                if TextButton::new(&machine_label)
                    .icon(svg::HOUSE)
                    .show(ui)
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
                    controls::badge(ui, "WinPE", theme::primary(ui))
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
                MenuButton::new("View").icon(svg::LAYOUT).tint(btn_color).show(ui, |ui| {
                    if controls::menu_item(ui, svg::TOOLBOX, "My Tools").clicked() {
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::ToolBox);
                        let _ = self.toolbox.request_contents("/");
                        ui.close();
                    }
                    if controls::menu_item(ui, svg::FOLDER_OPEN, "Explorer").clicked() {
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
                    let show_remote_only = !self.local_only;
                    let notifs = if matches!(self.state, WsDisplayState::Shell) {
                        "Shell".to_string()
                    } else if self.notifications > 0 {
                        format!("Shell  ({})", self.notifications)
                    } else {
                        "Shell".to_string()
                    };
                    if show_remote_only
                        && controls::menu_item(ui, svg::TERMINAL, &notifs).clicked()
                    {
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::Shell);
                        ui.close();
                    }
                    if controls::menu_item(ui, svg::MICROSCOPE, "Fleet Intel")
                        .on_hover_text(
                            "Crash-signature intelligence (dump analysis + prior verdicts across the fleet) and driver snapshots/drift/blocklist for this client",
                        )
                        .clicked()
                    {
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::FleetIntel);
                        ui.close();
                    }
                    if controls::menu_item(ui, svg::BUG_BEETLE, "Crash Dumps")
                        .on_hover_text(
                            "Every crash recorded for THIS machine: minidumps, live-kernel reports, and GPU (Aftermath) dumps with their signatures, fleet prevalence, and verdicts",
                        )
                        .clicked()
                    {
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::CrashDumps);
                        ui.close();
                    }
                    if show_remote_only
                        && controls::menu_item(ui, svg::FILE_ARCHIVE, "Download crash dumps")
                        .on_hover_text(
                            "Zip and download this client's kernel dumps (MEMORY.DMP, Minidump, LiveKernelReports, WER), app crash dumps, WER crash and hang reports, and UE/GPU crash folders (last 30 days) in one archive, with an app crash summary and a manifest",
                        )
                        .clicked()
                    {
                        self.remote_explorer.start_crash_dump_download(&self.send_cmd_tx);
                        ui.close();
                    }
                    controls::menu_separator(ui);
                    // Live data feed toggle. Home's charts depend on this feed.
                    if self.live_stats_active {
                        if controls::menu_item(ui, svg::STOP, "Stop live data").clicked() {
                            let _ = self.send_cmd_tx.try_send(Cmd::Quit);
                            self.live_stats_active = false;
                            ui.close();
                        }
                    } else if controls::menu_item(ui, svg::PULSE, "Start live data")
                        .on_hover_text("Resume the live telemetry feed that drives the Home page charts.")
                        .clicked()
                    {
                        let _ = self.send_cmd_tx.try_send(Cmd::LiveData);
                        self.live_stats_active = true;
                        ui.close();
                    }
                    if show_remote_only {
                    if self.egui_viewer_active {
                        if controls::menu_item(ui, svg::STOP, "Stop Viewer").clicked() {
                            self.egui_viewer_active = false;
                            let _ = self.send_cmd_tx.try_send(Cmd::SetFrameCapture { enabled: false });
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::Shell);
                            ui.close();
                        }
                    } else if controls::menu_item(ui, svg::APP_WINDOW, "Start Viewer").clicked() {
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::Terminal);
                        self.egui_viewer_active = true;
                        let _ = self.send_cmd_tx.try_send(Cmd::SetFrameCapture { enabled: true });
                        ui.close();
                    }
                    if self.desktop_streaming {
                        if controls::menu_item(ui, svg::STOP, "Stop Remote Desktop").clicked() {
                            self.desktop_streaming = false;
                            let _ = self.send_cmd_tx.try_send(Cmd::DesktopStreamStop);
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::Home);
                            ui.close();
                        }
                    } else if controls::menu_item(ui, svg::MT_REMOTE, "Remote Desktop").clicked() {
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
                        controls::menu_separator(ui);
                        let quit = controls::menu_item(ui, svg::SIGN_OUT, "Quit interactive shell");
                        if quit.clicked() {
                            let _ = self.send_cmd_tx.try_send(Cmd::Quit);
                            ui.close();
                        }
                    }
                    }
                    controls::menu_separator(ui);
                    let pending = crate::mcp_tool_log::pending_count(&self.client.connection_string);
                    let label = if pending > 0 {
                        format!("MCP Tool Log  ({pending} running)")
                    } else {
                        "MCP Tool Log".to_string()
                    };
                    let icon = if pending > 0 { svg::HOURGLASS_SIMPLE } else { svg::PLUGS };
                    if controls::menu_item(ui, icon, &label).clicked() {
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::McpToolLog);
                        ui.close();
                    }
                });

                // ── Inspect ──────────────────────────────────────────
                // Read-only system surfaces: every entry kicks off the
                // initial list-fetch if it hasn't been loaded yet,
                // matching the old per-button behavior.
                MenuButton::new("Inspect").icon(svg::MICROSCOPE).tint(sys_color).show(
                    ui,
                    |ui| {
                        if controls::menu_item(ui, svg::LIST_BULLETS, "Event Log").clicked() {
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
                        if controls::menu_item(ui, svg::SCROLL, "Client Log").clicked() {
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::ClientLog);
                            if self.client_log_viewer.text.is_empty() {
                                let cmd = self.client_log_viewer.fetch_cmd();
                                if self.send_cmd_tx.try_send(cmd).is_ok() {
                                    self.client_log_viewer.loading = true;
                                }
                            }
                            ui.close();
                        }
                        if controls::menu_item(ui, svg::GEAR_SIX, "Services").clicked() {
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::Services);
                            if self.services_viewer.entries.is_empty() {
                                let _ = self.send_cmd_tx.try_send(Cmd::ListServices);
                                self.services_viewer.loading = true;
                            }
                            ui.close();
                        }
                        let tasks = controls::menu_item(ui, svg::CALENDAR_CHECK, "Task Scheduler");
                        if tasks.clicked() {
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::TaskScheduler);
                            if self.task_scheduler_viewer.entries.is_empty() {
                                let _ = self.send_cmd_tx.try_send(Cmd::ListScheduledTasks { folder: None });
                                self.task_scheduler_viewer.loading = true;
                            }
                            ui.close();
                        }
                        if controls::menu_item(ui, svg::TREE_STRUCTURE, "Registry").clicked() {
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::Registry);
                            ui.close();
                        }
                        if controls::menu_item(ui, svg::ROCKET_LAUNCH, "Startup Apps").clicked() {
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::StartupApps);
                            if self.startup_apps_viewer.entries.is_empty() {
                                let _ = self.send_cmd_tx.try_send(Cmd::ListStartupApps);
                                self.startup_apps_viewer.loading = true;
                            }
                            ui.close();
                        }
                        if controls::menu_item(ui, svg::CODE, "Scripts").clicked() {
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::Scripts);
                            if !(self.remote_scripts_viewer.loading || self.remote_scripts_viewer.running) {
                                self.remote_scripts_viewer.loading = true;
                                let _ = self.send_cmd_tx.try_send(Cmd::GetRemoteScriptList);
                            }
                            ui.close();
                        }
                        if controls::menu_item(ui, svg::PACKAGE, "Installed Programs").clicked() {
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
                    if !self.local_only {
                    let transfer = MenuButton::new("Transfer")
                        .icon(svg::ARROWS_LEFT_RIGHT)
                        .tint(transfer_color);
                    transfer.show(
                        ui,
                        |ui| {
                            #[cfg(not(target_arch = "wasm32"))]
                            #[cfg(not(any(target_os = "ios", target_os = "android")))]
                            {
                                if controls::MenuItem::new("Send File…")
                                    .icon(svg::PAPER_PLANE_TILT)
                                    .enabled(!transfer_busy)
                                    .show(ui)
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
                                let deploy = controls::MenuItem::new("Deploy MasterTech Update…")
                                    .icon(svg::ROCKET_LAUNCH)
                                    .enabled(self.self_update_rx.is_none())
                                    .show(ui)
                                    .on_disabled_hover_text("A MasterTech update is already being sent");
                                if deploy
                                    .on_hover_text(
                                        "Push a new MasterTech binary (MasterTech.exe or Mastertech-linux) to this remote client.\nIt will replace itself and relaunch automatically.",
                                    )
                                    .clicked()
                                {
                                    if let Some(path) = rfd::FileDialog::new()
                                        .set_title("Select the MasterTech binary to deploy (MasterTech.exe or Mastertech-linux)")
                                        .pick_file()
                                    {
                                        let path_str = path.display().to_string();
                                        let (tx, rx) = crossbeam::channel::bounded::<Cmd>(8);
                                        self.self_update_rx = Some(rx);
                                        self.self_update_file = path
                                            .file_name()
                                            .map(|n| n.to_string_lossy().into_owned())
                                            .unwrap_or_default();
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
                }

                // ── Power ────────────────────────────────────────────
                // Destructive OS-level operations. Lock is the only
                // non-destructive entry; the others either log the user
                // out, reboot, or shut down — order them from least to
                // most disruptive so the destructive ones don't sit at
                // the top of the menu.
                if !self.local_only {
                MenuButton::new("Power").icon(svg::POWER).show(
                    ui,
                    |ui| {
                        if controls::menu_item(ui, svg::LOCK, "Lock workstation").clicked() {
                            let _ = self.send_cmd_tx.try_send(Cmd::LockWorkstation);
                            ui.close();
                        }
                        if controls::menu_item(ui, svg::SIGN_OUT, "Log off user").clicked() {
                            let _ = self.send_cmd_tx.try_send(Cmd::LogOffUser);
                            ui.close();
                        }
                        controls::menu_separator(ui);
                        if controls::menu_item(ui, svg::ARROW_CLOCKWISE, "Reboot").clicked() {
                            let _ = self.send_cmd_tx.try_send(Cmd::RebootSystem {
                                persist_mastertech: true,
                                terminal_mode: false,
                            });
                            ui.close();
                        }
                        if controls::menu_item(ui, svg::TERMINAL_WINDOW, "Switch to Terminal Mode")
                            .clicked()
                        {
                            let _ = self.send_cmd_tx.try_send(Cmd::LaunchTerminalMode);
                            ui.close();
                        }
                        controls::menu_separator(ui);
                        if controls::menu_item(ui, svg::POWER, "Shutdown").clicked() {
                            let _ = self.send_cmd_tx.try_send(Cmd::ShutdownSystem);
                            ui.close();
                        }
                    },
                );
                }

                // ── Protections ──────────────────────────────────────
                // Disable/re-enable Memory Integrity + the driver blocklist.
                self.driver_protections_toolbar(ui);

                controls::vline(ui, rule_h);

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
                    controls::note(ui, svg::TERMINAL, "Persistent Shell", theme::warn(ui));
                }

                // Home's sub-tabs belong on this row, not on a second row inside the page body.
                if matches!(self.state, WsDisplayState::Home) {
                    self.home_page.sub_tab_toggles(ui);
                }

                // ── Right-aligned status indicator ───────────────────
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (status_color, status_icon, status_tooltip) = if !self.client.connected {
                        (theme::error(ui), svg::X_CIRCLE, "Disconnected")
                    } else if let Some(last_activity) = &self.client.last_update {
                        let now = chrono::Utc::now();
                        let activity_time = last_activity.to_utc();
                        let elapsed_secs = (now - activity_time).num_seconds();
                        if elapsed_secs < 30 {
                            (theme::success(ui), svg::CHECK_CIRCLE, "Active")
                        } else if elapsed_secs < 120 {
                            (theme::warn(ui), svg::WARNING, "Stale")
                        } else {
                            (theme::error(ui), svg::HOURGLASS_SIMPLE, "Inactive")
                        }
                    } else if self.is_connected {
                        (
                            theme::success(ui),
                            svg::CIRCLE,
                            "Connected (awaiting activity)",
                        )
                    } else {
                        (theme::error(ui), svg::X_CIRCLE, "Disconnected")
                    };

                    status_icon.show_tinted(ui, Metrics::get(ui.ctx()).icon_sm, status_color).on_hover_text(status_tooltip);

                    controls::vline(ui, rule_h);

                    // ── Transport badge: which path carries this session ──
                    let kind = self.transport.kind();
                    let (badge, badge_tip) = match kind {
                        TransportKind::Tcp => ("TCP", "Direct TCP (same network)"),
                        TransportKind::Relay => ("RELAY", "Relay tunnel via websocket server"),
                        TransportKind::WebSocket => ("WS", "Legacy WebSocket relay room"),
                        TransportKind::Local => ("LOCAL", "This machine, in-process"),
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
                    controls::badge(ui, badge, badge_color).on_hover_text(badge_hover);

                    controls::vline(ui, rule_h);

                    // ── Client build badge: MasterTech version the agent reported ──
                    // Shown in full, hash included: the release number sits still between builds,
                    // so the hash is the only part that identifies which build is out there.
                    if let Some(ver) = self.client_version.as_deref().filter(|_| !self.local_only) {
                        let admin_ver = crate::shape_fp::BUILD_VERSION;
                        use crate::shape_fp::release_of;
                        let (ver_color, ver_text, ver_hover) = if self.cmd_protocol_mismatch {
                            (
                                theme::warn(ui),
                                format!("v{ver}"),
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
                        if self.cmd_protocol_mismatch {
                            svg::WARNING.show_tinted(ui, Metrics::get(ui.ctx()).icon_sm, ver_color);
                        }
                        controls::vline(ui, rule_h);
                    }

                    // The one place this session's connection string is printed, and the fastest way
                    // to get it into an MCP call or a ticket.
                    if TextButton::new(self.client.connection_string.as_str())
                        .icon(svg::COPY)
                        .kind(ButtonKind::Ghost)
                        .show(ui)
                        .on_hover_text("Connection string (host:client hash) — click to copy")
                        .clicked()
                    {
                        ui.ctx().copy_text(self.client.connection_string.clone());
                        let _ = crate::get_toast_sender().try_send(crate::ToastMessage::Success(
                            format!("Copied {}", self.client.connection_string),
                        ));
                    }

                    if !self.local_only
                        && let Some((ref name, sent, total)) = self.file_transfer_progress
                    {
                        controls::vline(ui, rule_h);
                        let short = name.rsplit(['/', '\\']).next().unwrap_or(name);
                        let frac = if total > 0 {
                            (sent as f32 / total as f32).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        controls::meter(ui, frac, 80.0, theme::warn(ui))
                            .on_hover_text(format!("Sending {name} — chunk {sent} of {total}"));
                        IconLabel::new(svg::UPLOAD_SIMPLE, &short)
                            .tone(theme::warn(ui))
                            .small(true)
                            .wrap(true)
                            .show(ui);
                    }

                    // Live stress progress rides this row so it stays visible on every view, not
                    // just Home's inventory.
                    controls::vline(ui, rule_h);
                    self.home_page.active_run_strip(ui);
                });
            });
        });

        if self.local_only && self.state.is_remote_only() {
            log::debug!("client_interface -> local session refused a remote-only page");
            self.state = WsDisplayState::Home;
        }

        match self.state {
            WsDisplayState::Home => self.show_home(ui),
            WsDisplayState::Explorer => {
                let cmd_tx = self.send_cmd_tx.clone();
                glass_card::group(ui, |ui| self.remote_explorer.display(ui, &cmd_tx)).inner
            },
            WsDisplayState::ToolBox => glass_card::group(ui, |ui| self.toolbox.display(ui)).inner,
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
                            controls::toggle(
                                ui,
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
                                IconLabel::new(svg::PICTURE_IN_PICTURE, "Remote UI is in a separate window. Close that window or \
                                         switch off the toggle above to embed here.").tone(theme::weak_text(ui)).small(true).wrap(true).show(ui);
                            }
                        }
                        #[cfg(target_arch = "wasm32")]
                        {
                            self.egui_viewer.ui(ui, |_| {}, None);
                        }
                    } else {
                        ui.add_space(24.0);
                        controls::empty_state(
                            ui,
                            svg::MT_REMOTE,
                            "Connecting to remote viewer...",
                            "Waiting for terminal or egui frame data from the remote instance.",
                            &[],
                        );
                        ui.vertical_centered(|ui| {
                            ui.add_space(8.0);
                            controls::spinner(ui, controls::SpinnerSize::Md);
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
                    controls::empty_state(
                        ui,
                        svg::MICROSCOPE,
                        "Fleet Intel unavailable",
                        "Fleet Intel requires the native tokio build.",
                        &[],
                    );
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
                    controls::empty_state(
                        ui,
                        svg::BUG_BEETLE,
                        "Crash Dumps unavailable",
                        "Crash Dumps requires the native tokio build.",
                        &[],
                    );
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

                    let star = format!("{} ", crate::ui_tools::icons::STAR);
                    let label = |m: &crate::remote_desktop::DesktopMonitorInfo| {
                        format!(
                            "{}{} ({}x{})",
                            if m.is_primary { star.as_str() } else { "" },
                            m.name,
                            m.width,
                            m.height
                        )
                    };

                    ui.horizontal_wrapped(|ui| {
                        let rule_h = Metrics::get(ui.ctx()).control_h;
                        if !monitors.is_empty() {
                            let names: Vec<String> = monitors.iter().map(label).collect();
                            let options: Vec<_> = monitors
                                .iter()
                                .zip(&names)
                                .map(|(m, name)| (m.id, name.as_str()))
                                .collect();
                            svg::MONITOR.show_tinted(ui, Metrics::get(ui.ctx()).icon_sm, theme::weak_text(ui));
                            let id = "remote_desktop_monitor";
                            if controls::dropdown(ui, id, &mut monitor, &options, 220.0).changed() {
                                restart = true;
                            }
                        }
                        let fps_slider = controls::Slider::new(&mut fps, 1..=30)
                            .width(SLIDER_W)
                            .value("fps")
                            .show(ui);
                        if slider_settled(&fps_slider) {
                            restart = true;
                        }
                        let quality_slider = controls::Slider::new(&mut quality, 20..=90)
                            .width(SLIDER_W)
                            .value("q")
                            .show(ui)
                            .on_hover_text("JPEG quality");
                        if slider_settled(&quality_slider) {
                            restart = true;
                        }
                        let scale_slider = controls::Slider::new(&mut scale, 0.25..=1.0)
                            .width(SLIDER_W)
                            .step(0.05)
                            .value("x")
                            .show(ui)
                            .on_hover_text("Frame scale");
                        if slider_settled(&scale_slider) {
                            restart = true;
                        }
                        if streaming {
                            if controls::icon_text_button(ui, svg::STOP, "Stop").clicked() {
                                streaming = false;
                                stop = true;
                            }
                        } else if controls::icon_text_button(ui, svg::PLAY, "Start").clicked() {
                            streaming = true;
                            restart = true;
                        }
                        let pop = controls::icon_text_button(ui, svg::ARROW_SQUARE_OUT, "Pop out");
                        if pop.clicked() {
                            popout = true;
                        }
                        svg::CLIPBOARD.show_tinted(ui, Metrics::get(ui.ctx()).icon_sm, theme::weak_text(ui));
                        if controls::toggle(ui, &mut clipboard_sync, "Clipboard")
                            .on_hover_text(
                                "Mirror the clipboard both ways while streaming. Copy here and \
                                 paste there, or the reverse.",
                            )
                            .changed()
                        {
                            clipboard_changed = true;
                        }
                        controls::vline(ui, rule_h);
                        IconLabel::new(svg::GAUGE, &format!(
                                "{frames} frames | {latency} ms | {} KB/frame",
                                bytes / 1024
                            )).tone(theme::success(ui)).small(true).wrap(true).show(ui);
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

                    controls::hairline(ui);
                    if self.desktop_popout {
                        let back = controls::empty_state(
                            ui,
                            svg::PICTURE_IN_PICTURE,
                            "Remote desktop is popped out to its own window.",
                            "",
                            &[(controls::Role::Secondary, "Return to tab")],
                        );
                        if back.is_some() {
                            self.desktop_popout = false;
                        }
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

        controls::bar(ui, BarKind::Tool, |ui| {
            let (icon, label) = if fullscreen {
                (svg::CORNERS_IN, "Exit Full Screen")
            } else {
                (svg::CORNERS_OUT, "Full Screen")
            };
            if controls::icon_text_button(ui, icon, label).clicked() {
                toggle_fullscreen = true;
            }
            if controls::icon_text_button(ui, svg::ARROW_SQUARE_IN, "Return to tab").clicked() {
                return_to_tab = true;
            }
            controls::vline(ui, Metrics::get(ui.ctx()).control_h);
            IconLabel::new(svg::GAUGE, &format!("{frames} frames | {latency} ms")).tone(theme::success(ui)).small(true).wrap(true).show(ui);
        });

        if toggle_fullscreen {
            ui.ctx()
                .send_viewport_cmd(eframe::egui::ViewportCommand::Fullscreen(!fullscreen));
        }
        if return_to_tab {
            self.desktop_popout = false;
        }

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
#[cfg(test)]
mod local_gating_tests {
    use super::*;

    /// A local session must never land on a frame-stream or remote-shell page.
    /// Hiding the menu entries is the UI half; this is the half that holds even
    /// if a display-state message arrives from somewhere else.
    #[test]
    fn only_the_stream_pages_are_remote_only() {
        for state in [
            WsDisplayState::Shell,
            WsDisplayState::Terminal,
            WsDisplayState::RemoteDesktop,
        ] {
            assert!(state.is_remote_only(), "{state:?} must be refused locally");
        }
        for state in [
            WsDisplayState::Home,
            WsDisplayState::Explorer,
            WsDisplayState::ToolBox,
            WsDisplayState::EventLog,
            WsDisplayState::ClientLog,
            WsDisplayState::Services,
            WsDisplayState::TaskScheduler,
            WsDisplayState::Registry,
            WsDisplayState::StartupApps,
            WsDisplayState::Scripts,
            WsDisplayState::InstalledPrograms,
            WsDisplayState::McpToolLog,
            WsDisplayState::FleetIntel,
            WsDisplayState::CrashDumps,
        ] {
            assert!(
                !state.is_remote_only(),
                "{state:?} works against this machine and must stay available"
            );
        }
    }
}
