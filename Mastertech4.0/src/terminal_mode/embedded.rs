use super::TerminalApp;
use crate::terminal_mode::data::LocalTermEvent;
use crossbeam::channel::{unbounded, Receiver, Sender};
use database::schema::User;
use displays::remote_viewer::ratagui::{RataguiBackend, TerminalEvent};
use eframe::egui::{Event, EventFilter, Frame, Id, Sense, Ui};
use ratatui::crossterm::event::{KeyEvent, MouseEvent};
use ratatui::Terminal;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

static NEXT_EMBEDDED_TERMINAL_NONCE: AtomicUsize = AtomicUsize::new(0);

/// Renders the full `terminal_mode` TUI inside an egui surface by driving a
/// `TerminalApp` against a `RataguiBackend` one frame per egui frame.
pub struct EmbeddedTerminal {
    app: TerminalApp<'static>,
    terminal: Terminal<RataguiBackend>,
    event_rx: Receiver<TerminalEvent>,
    _event_tx: Sender<TerminalEvent>,
    shutdown_tx: broadcast::Sender<()>,
    handles: Vec<JoinHandle<()>>,
    focus_id: Id,
    want_focus: bool,
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
        let nonce = NEXT_EMBEDDED_TERMINAL_NONCE.fetch_add(1, Ordering::Relaxed);
        Self {
            app,
            terminal,
            event_rx,
            _event_tx: event_tx,
            shutdown_tx,
            handles,
            focus_id: Id::new(("embedded_terminal_focus", nonce)),
            want_focus: false,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let focus_id = self.focus_id;

        // Keep the synthetic focus id alive and honor a pending focus request.
        let reg_rect = ui.available_rect_before_wrap();
        ctx.check_for_id_clash(focus_id, reg_rect, "embedded_terminal_focus");
        if self.want_focus {
            ctx.memory_mut(|m| m.request_focus(focus_id));
            self.want_focus = false;
        }
        let focused = ctx.memory(|m| m.has_focus(focus_id));
        // Lock Tab/arrows/Esc to the terminal whenever it owns focus; harmless
        // when it doesn't, so it's set every frame to avoid a one-frame gap.
        ctx.memory_mut(|m| {
            m.set_focus_lock_filter(
                focus_id,
                EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            );
        });

        // Feed input the backend captured last frame: pointer events always,
        // keys only while focused so unfocused keystrokes reach other widgets.
        while let Ok(event) = self.event_rx.try_recv() {
            let local = LocalTermEvent(event);
            if let Ok(mouse) = MouseEvent::try_from(local.clone()) {
                self.app.handle_events(Some(mouse), None);
            } else if focused {
                if let Ok(key) = KeyEvent::try_from(local) {
                    self.app.handle_events(None, Some(key));
                }
            }
        }

        let Self { app, terminal, .. } = self;
        let _ = terminal.draw(|f| app.render_frame::<RataguiBackend>(f));
        let inner = Frame::new().show(ui, |ui| {
            ui.add(terminal.backend_mut());
        });

        let area_resp = inner.response.interact(Sense::click_and_drag());
        if area_resp.clicked() || area_resp.is_pointer_button_down_on() {
            ctx.memory_mut(|m| m.request_focus(focus_id));
        }

        // While focused, swallow key/text so egui doesn't navigate dock tabs.
        if focused {
            ui.input_mut(|i| {
                i.events
                    .retain(|e| !matches!(e, Event::Key { .. } | Event::Text(_)));
            });
        }

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
