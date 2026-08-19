//! Standalone ratatui terminal mode for qc-app, in 1:1 parity with the egui
//! GUI. Launched in Windows PE where no graphics driver is available. Built on
//! the shared `mtech-tui` infrastructure.

pub mod ai_backend;
pub mod charts;
pub mod context;
pub mod menu_bar;
pub mod modals;
pub mod tabs;

use std::cell::RefCell;
use std::io;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent},
    layout::{Constraint, Layout},
    prelude::{Backend, CrosstermBackend},
    style::Style,
    Frame, Terminal,
};

use mtech_tui::events::action_handler::{get_event_receiver, EventManager};
use mtech_tui::events::{Event, EventHandler};
use mtech_tui::styling::APP_BACKGROUND;
use mtech_tui::widgets::HandleWidget;
use stress_runner::RecordId;

use std::sync::OnceLock;

use crate::hw_sampler::HwSampler;
use context::QcContext;
use menu_bar::{MenuBar, Tab};
use modals::ReportModal;
use tabs::{
    AiTab, BugReportTab, HardwareTab, LogsTab, Oa3Tab, OrderQcTab, SettingsTab, StressTab,
    SwiftDbTab,
};

/// Owns every tab, the input source, the shared context, and the background
/// telemetry sampler. The standalone loop and (future) embedded driver both
/// funnel through the single `render_frame` + `handle_events` pair.
pub struct QcTerminalApp {
    ctx: Arc<Mutex<QcContext>>,
    event_handler: EventHandler,
    event_manager: EventManager<'static>,
    menu_bar: MenuBar<'static>,
    sampler: Option<HwSampler>,
    order: Rc<RefCell<OrderQcTab<'static>>>,
    stress: Rc<RefCell<StressTab<'static>>>,
    hardware: HardwareTab,
    settings: SettingsTab,
    logs: LogsTab,
    ai: AiTab<'static>,
    swift_db: Rc<RefCell<SwiftDbTab<'static>>>,
    oa3: Rc<RefCell<Oa3Tab<'static>>>,
    bug_report: Rc<RefCell<BugReportTab<'static>>>,
    report_modal: ReportModal,
    /// MCP server state (telemetry / report / run slot), spawned on first tick.
    mcp_state: Option<Arc<crate::mcp::QcMcpState>>,
    /// Orchestrator HTTP report sink; created once an orchestrator URL is set.
    report_sink: Option<crate::reporting::ReportSink>,
    /// Inbound fleet command client; twin of the report sink.
    fleet_client: Option<crate::fleet_client::FleetClient>,
    /// Last heartbeat send time (30 s throttle).
    last_heartbeat: Option<Instant>,
}

impl QcTerminalApp {
    pub fn new() -> Self {
        let ctx = Arc::new(Mutex::new(QcContext::default()));

        let swift_db = Rc::new(RefCell::new(SwiftDbTab::new()));
        let oa3 = Rc::new(RefCell::new(Oa3Tab::new()));
        let bug_report = Rc::new(RefCell::new(BugReportTab::new()));
        let stress = Rc::new(RefCell::new(StressTab::new(ctx.clone())));
        let order = Rc::new(RefCell::new(OrderQcTab::new(ctx.clone())));

        let mut event_manager = EventManager::new(get_event_receiver());
        event_manager.register_handler(swift_db.clone());
        event_manager.register_handler(oa3.clone());
        event_manager.register_handler(bug_report.clone());
        event_manager.register_handler(stress.clone());
        event_manager.register_handler(order.clone());

        Self {
            event_handler: EventHandler::new(),
            event_manager,
            menu_bar: MenuBar::new(),
            sampler: None,
            order,
            stress,
            hardware: HardwareTab::new(ctx.clone()),
            settings: SettingsTab,
            logs: LogsTab::default(),
            ai: AiTab::new(),
            swift_db,
            oa3,
            bug_report,
            report_modal: ReportModal::new(ctx.clone()),
            mcp_state: None,
            report_sink: None,
            fleet_client: None,
            last_heartbeat: None,
            ctx,
        }
    }

    /// Stable `computer:<key>` record for this machine, cached after first call.
    fn local_computer_record() -> RecordId {
        static CACHE: OnceLock<RecordId> = OnceLock::new();
        CACHE
            .get_or_init(|| {
                let (hostname, cpu) = crate::reporting::host_name_and_cpu_brand();
                RecordId::new(
                    database::schema::COMPUTER_TABLE,
                    stress_runner::computer_record_key(&hostname, &cpu),
                )
            })
            .clone()
    }

    /// Per-iteration headless work, the renderer-agnostic analog of
    /// `QcApp::logic`. Starts the sampler on first call and publishes each tick
    /// into the shared context.
    fn tick(&mut self) {
        if self.sampler.is_none() {
            let sampler = HwSampler::start(1000);
            if let Ok(mut ctx) = self.ctx.lock() {
                ctx.telemetry = Some(sampler.agent());
                ctx.computer = Some(Self::local_computer_record());
            }
            self.sampler = Some(sampler);
        }

        let current_cores = if let Some(sampler) = &self.sampler {
            let snapshot = sampler.snapshot();
            let rows = snapshot.cores.clone();
            if let Ok(mut ctx) = self.ctx.lock() {
                ctx.snapshot = Some(snapshot.clone());
            }
            let mut stress = self.stress.borrow_mut();
            stress.push_telemetry(&snapshot);
            stress.tick();
            rows
        } else {
            Vec::new()
        };
        self.order.borrow_mut().tick();

        // MCP servers + shared state: spawn once, feed every tick.
        if self.mcp_state.is_none() {
            let state = Arc::new(crate::mcp::QcMcpState {
                latest_cores: Arc::new(Mutex::new(Vec::new())),
                last_report: Arc::new(Mutex::new(None)),
                report_sink: Arc::new(Mutex::new(None)),
                telemetry: Arc::new(Mutex::new(None)),
                computer: Self::local_computer_record(),
                run_slot: Arc::new(Mutex::new(crate::mcp::RunSlot::default())),
            });
            crate::mcp::spawn_mcp_servers(state.clone());
            self.mcp_state = Some(state);
        }
        if let Some(state) = &self.mcp_state {
            if let Ok(mut g) = state.latest_cores.lock() {
                *g = current_cores.clone();
            }
            if let Ok(mut g) = state.report_sink.lock() {
                *g = self.report_sink.clone();
            }
            if let Some(sampler) = &self.sampler {
                if let Ok(mut g) = state.telemetry.lock() {
                    if g.is_none() {
                        *g = Some(sampler.agent());
                    }
                }
            }
        }

        // Report sink + fleet client once an orchestrator URL is configured.
        if self.report_sink.is_none() {
            let url = database::orchestrator_url();
            if !url.is_empty() {
                let mid = crate::reporting::machine_id();
                self.report_sink =
                    Some(crate::reporting::ReportSink::start(Some(url.to_string()), mid.clone()));
                self.fleet_client =
                    Some(crate::fleet_client::FleetClient::start(Some(url.to_string()), mid));
            }
        }

        // Drain orchestrator commands.
        if let Some(client) = self.fleet_client.clone() {
            for cmd in client.drain_commands(8) {
                self.dispatch_inbound_command(cmd, &client, &current_cores);
            }
        }

        // Heartbeat (30 s).
        const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
        if let Some(sink) = self.report_sink.as_ref() {
            let should_send = self
                .last_heartbeat
                .map(|t| t.elapsed() >= HEARTBEAT_INTERVAL)
                .unwrap_or(true);
            if should_send {
                let avg_pct = if current_cores.is_empty() {
                    0.0
                } else {
                    current_cores.iter().map(|c| c.usage_pct).sum::<f32>()
                        / current_cores.len() as f32
                };
                sink.send_heartbeat(crate::telemetry::Heartbeat::new(
                    sink.machine_id.as_str(),
                    avg_pct,
                ));
                self.last_heartbeat = Some(Instant::now());
            }
        }
    }

    /// Apply one inbound fleet command and ack it. Mirrors `QcApp::dispatch_inbound_command`.
    fn dispatch_inbound_command(
        &mut self,
        cmd: crate::fleet_client::InboundCommand,
        client: &crate::fleet_client::FleetClient,
        current_cores: &[crate::hw_sampler::CoreRow],
    ) {
        use crate::fleet_client::InboundCommandKind;
        let id = cmd.id.clone();
        match cmd.kind {
            InboundCommandKind::SendReport => {
                let snapshot = crate::telemetry::HwSnapshot::from_cores(current_cores);
                let mid = client.machine_id.as_ref().clone();
                let report = crate::telemetry::QcReport::new(&mid, snapshot);
                if let Some(sink) = self.report_sink.as_ref() {
                    sink.send_report(report.clone());
                }
                if let Some(state) = self.mcp_state.as_ref() {
                    if let Ok(mut g) = state.last_report.lock() {
                        *g = Some(report);
                    }
                }
                client.ack(id);
            }
            InboundCommandKind::Custom { payload } => {
                match payload.get("op").and_then(|v| v.as_str()) {
                    Some("run_stress_preset") => {
                        let preset = payload
                            .get("preset")
                            .and_then(|v| v.as_str())
                            .unwrap_or("bronze")
                            .to_string();
                        let mult = payload
                            .get("duration_multiplier")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(1.0) as f32;
                        match self.sampler.as_ref().map(|s| s.agent()) {
                            Some(telemetry) => {
                                let computer = Self::local_computer_record();
                                match self.stress.borrow_mut().start_certification_by_name(
                                    &preset, mult, telemetry, computer,
                                ) {
                                    Ok(()) => log::info!(
                                        "qc-app: fleet command {id} started preset '{preset}' at {mult}x"
                                    ),
                                    Err(err) => log::warn!(
                                        "qc-app: fleet command {id} preset '{preset}' refused: {err}"
                                    ),
                                }
                            }
                            None => log::warn!(
                                "qc-app: fleet command {id} refused — telemetry sampler not ready"
                            ),
                        }
                    }
                    Some("cancel_stress_run") => {
                        self.stress.borrow_mut().stop_active_run();
                        log::info!("qc-app: fleet command {id} cancelled the active run");
                    }
                    other => log::warn!(
                        "qc-app: fleet command {id} unhandled custom op {other:?}; payload={payload}"
                    ),
                }
                client.ack(id);
            }
        }
    }

    /// Draw one frame: menu bar, active tab content, then dropdown overlay.
    /// Shared by the standalone loop and the future embedded driver.
    pub fn render_frame<B: Backend>(&mut self, f: &mut Frame) {
        // Route widget-bus events (button clicks, field focus) to the tabs
        // registered as ActionHandlers before drawing this frame.
        self.event_manager.process_events();

        let area = f.area();
        f.buffer_mut().set_style(area, Style::new().bg(APP_BACKGROUND));

        let chunks = Layout::vertical([Constraint::Length(3), Constraint::Fill(1)]).split(area);
        self.menu_bar.draw::<B>(f, chunks[0]);

        let content = chunks[1];
        match self.menu_bar.current_tab() {
            Tab::OrderQc => self.order.borrow_mut().draw::<B>(f, content),
            Tab::Stress => self.stress.borrow_mut().draw::<B>(f, content),
            Tab::Hardware => self.hardware.draw::<B>(f, content),
            Tab::SwiftDb => self.swift_db.borrow_mut().draw::<B>(f, content),
            Tab::Oa3 => self.oa3.borrow_mut().draw::<B>(f, content),
            Tab::Settings => self.settings.draw::<B>(f, content),
            Tab::Logs => self.logs.draw::<B>(f, content),
            Tab::BugReport => self.bug_report.borrow_mut().draw::<B>(f, content),
            Tab::Ai => self.ai.draw::<B>(f, content),
        }

        self.menu_bar.draw_overlay(f);

        if let Some(id) = self.stress.borrow_mut().take_report_request() {
            self.report_modal.open_run(id);
        }
        self.report_modal.draw::<B>(f);
    }

    fn dispatch_key(&mut self, tab: Tab, key: KeyEvent) {
        match tab {
            Tab::OrderQc => self.order.borrow_mut().handle_key_event(key),
            Tab::Stress => self.stress.borrow_mut().handle_key_event(key),
            Tab::Hardware => self.hardware.handle_key_event(key),
            Tab::SwiftDb => self.swift_db.borrow_mut().handle_key_event(key),
            Tab::Oa3 => self.oa3.borrow_mut().handle_key_event(key),
            Tab::Settings => self.settings.handle_key_event(key),
            Tab::Logs => self.logs.handle_key_event(key),
            Tab::BugReport => self.bug_report.borrow_mut().handle_key_event(key),
            Tab::Ai => self.ai.handle_key_event(key),
        };
    }

    fn dispatch_mouse(&mut self, tab: Tab, ev: &MouseEvent) {
        match tab {
            Tab::OrderQc => self.order.borrow().handle_mouse_event(ev),
            Tab::Stress => self.stress.borrow().handle_mouse_event(ev),
            Tab::Hardware => self.hardware.handle_mouse_event(ev),
            Tab::SwiftDb => self.swift_db.borrow().handle_mouse_event(ev),
            Tab::Oa3 => self.oa3.borrow().handle_mouse_event(ev),
            Tab::Settings => self.settings.handle_mouse_event(ev),
            Tab::Logs => self.logs.handle_mouse_event(ev),
            Tab::BugReport => self.bug_report.borrow().handle_mouse_event(ev),
            Tab::Ai => self.ai.handle_mouse_event(ev),
        }
    }

    /// Drain queued input and route it: menu bar gets first refusal, then
    /// global shortcuts, then the active tab. Returns true to quit.
    pub fn handle_events(
        &mut self,
        _remote_mouse: Option<MouseEvent>,
        _remote_key: Option<KeyEvent>,
    ) -> bool {
        let mut quit = false;
        let mut drained = 0u16;
        while let Ok(event) = self.event_handler.next() {
            if self.report_modal.is_open() {
                match event {
                    Event::Key(key) => {
                        self.report_modal.handle_key_event(key);
                    }
                    Event::Mouse(mouse) => {
                        self.report_modal.handle_mouse_event(&mouse);
                    }
                    _ => {}
                }
                drained = drained.saturating_add(1);
                if drained >= 512 {
                    break;
                }
                continue;
            }
            match event {
                Event::Key(key) => {
                    if self.menu_bar.handle_menu_key(key) {
                        drained = drained.saturating_add(1);
                        if drained >= 512 {
                            break;
                        }
                        continue;
                    }
                    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                    match key.code {
                        KeyCode::Char('q') if ctrl => quit = true,
                        KeyCode::Right if ctrl => self.menu_bar.cycle_tab(1),
                        KeyCode::Left if ctrl => self.menu_bar.cycle_tab(-1),
                        _ => {
                            let tab = self.menu_bar.current_tab();
                            self.dispatch_key(tab, key);
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    if !self.menu_bar.handle_menu_mouse(&mouse) {
                        let tab = self.menu_bar.current_tab();
                        self.dispatch_mouse(tab, &mouse);
                    }
                }
                Event::Error => log::error!("terminal_mode: event loop error"),
                Event::Tick => {}
            }
            drained = drained.saturating_add(1);
            if quit || drained >= 512 {
                break;
            }
        }
        quit
    }

    async fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> anyhow::Result<()>
    where
        <B as Backend>::Error: Send + Sync + 'static,
    {
        loop {
            if self.handle_events(None, None) {
                break;
            }
            self.tick();
            terminal.draw(|f| self.render_frame::<B>(f))?;
            tokio::time::sleep(Duration::from_millis(16)).await;
        }
        Ok(())
    }
}

impl Default for QcTerminalApp {
    fn default() -> Self {
        Self::new()
    }
}

/// Standalone entry point: take over the TTY, run the loop, restore on exit.
pub async fn run_terminal_mode() -> anyhow::Result<()> {
    log::info!("qc-app: starting terminal mode");
    // Restores raw mode and the alternate screen on drop, so an unwinding
    // panic cannot leave the terminal in raw mode.
    let _terminal_guard = mtech_tui::panic_guard::TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut app = QcTerminalApp::new();
    let res = app.run(&mut terminal).await;

    if let Err(ref e) = res {
        log::error!("qc-app: terminal mode error: {e:?}");
    }
    res
}
