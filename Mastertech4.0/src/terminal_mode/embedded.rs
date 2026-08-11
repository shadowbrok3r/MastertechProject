use super::TerminalApp;
use crate::terminal_mode::data::LocalTermEvent;
use crossbeam::channel::{unbounded, Receiver, Sender};
use database::schema::User;
use displays::remote_viewer::ratagui::{RataguiBackend, TerminalEvent};
use eframe::egui::{Frame, Ui};
use ratatui::crossterm::event::{KeyEvent, MouseEvent};
use ratatui::Terminal;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

/// Renders the full `terminal_mode` TUI inside an egui surface by driving a
/// `TerminalApp` against a `RataguiBackend` one frame per egui frame.
pub struct EmbeddedTerminal {
    app: TerminalApp<'static>,
    terminal: Terminal<RataguiBackend>,
    event_rx: Receiver<TerminalEvent>,
    _event_tx: Sender<TerminalEvent>,
    shutdown_tx: broadcast::Sender<()>,
    handles: Vec<JoinHandle<()>>,
}

impl EmbeddedTerminal {
    pub fn new(current_user: Option<User>) -> Self {
        let (event_tx, event_rx) = unbounded::<TerminalEvent>();
        let mut backend = RataguiBackend::new(180, 50, event_tx.clone());
        backend.set_hover_events(true);
        let terminal = Terminal::new(backend).expect("ratatui terminal init");
        let app = TerminalApp::new_embedded();
        let (shutdown_tx, _) = broadcast::channel(1);
        let handles = app.spawn_core_systems(&shutdown_tx);
        if let Some(user) = current_user {
            app.seed_authenticated_user(user);
        }
        Self {
            app,
            terminal,
            event_rx,
            _event_tx: event_tx,
            shutdown_tx,
            handles,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();

        // Feed input the backend captured last frame. `RataguiBackend` owns the focus lock and only
        // emits key events while its grid holds focus, so nothing is gated here.
        while let Ok(event) = self.event_rx.try_recv() {
            let local = LocalTermEvent(event);
            if let Ok(mouse) = MouseEvent::try_from(local.clone()) {
                self.app.handle_events(Some(mouse), None);
            } else if let Ok(key) = KeyEvent::try_from(local) {
                self.app.handle_events(None, Some(key));
            }
        }

        let Self { app, terminal, .. } = self;
        let _ = terminal.draw(|f| app.render_frame::<RataguiBackend>(f));
        Frame::new().show(ui, |ui| {
            ui.add(terminal.backend_mut());
        });

        ctx.request_repaint();
    }
}

impl Drop for EmbeddedTerminal {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(());
        for handle in &self.handles {
            handle.abort();
        }
    }
}
