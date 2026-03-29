use crate::remote_viewer::ratagui::{RataguiBackend, TerminalEvent};
use crossbeam::channel::{unbounded, Sender, Receiver};
use web_time::Instant;
use ratatui::prelude::*;

pub mod ui;
pub mod receive;

pub struct RemoteTerminal {
    pub terminal: Terminal<RataguiBackend>,
    cached_buffer: Buffer,
    buffer_rx: Receiver<(u64, Buffer)>, // Add frame_index to buffer updates
    pub buffer_tx: Sender<(u64, Buffer)>,
    event_rx: Receiver<TerminalEvent>,
    _event_tx: Sender<TerminalEvent>,
    size_tx: Sender<Rect>,
    buffer_count: usize,
    frame_count: usize,
    last_log: Instant,
    last_repaint: Instant,
    last_log_frame_count: usize,
    pub current_area: Rect,
    last_target_area: Rect,
    pub(crate) latest_frame_index: u64,
    msg_to_client: Sender<ewebsock::WsMessage>,
}

impl RemoteTerminal {
    pub fn new(msg_to_client: Sender<ewebsock::WsMessage>, size_tx: Sender<Rect>) -> Self {
        let (buffer_tx, buffer_rx) = unbounded();
        let (_event_tx, event_rx) = unbounded(); // New: Event channel
        let _ = msg_to_client.try_send(ewebsock::WsMessage::Text("READY".to_string()));

        let initial_area = Rect::new(0, 0, 250, 250);

        let mut backend = RataguiBackend::new(initial_area.width, initial_area.height, _event_tx.clone()); // Changed: Use initial_area size
        backend.set_frame_index(0);
        let terminal = Terminal::new(backend).unwrap();
        let initial_area = Rect::new(0, 0, 250, 250);

        Self {
            terminal,
            size_tx,
            msg_to_client,
            frame_count: 0,
            buffer_count: 0,
            event_rx, _event_tx,
            buffer_rx, buffer_tx,
            latest_frame_index: 0,
            last_log_frame_count: 0,
            last_log: Instant::now(),
            last_repaint: Instant::now(),
            last_target_area: initial_area,
            current_area: initial_area.clone(),
            cached_buffer: Buffer::empty(initial_area),
        }
    }
}