use ratatui::crossterm::event::{KeyEvent, KeyEventKind, MouseButton, MouseEvent, MouseEventKind};
use crossbeam::channel::{unbounded, Receiver, TryRecvError};
use futures::{FutureExt, StreamExt};

pub mod action_handler;

#[derive(Clone, Copy, Debug)]
pub enum Event {
    Error,
    Tick,
    Key(KeyEvent),
    Mouse(MouseEvent)
}

pub struct EventHandler {
    rx: Receiver<Event>,
}

impl EventHandler {
    pub fn new() -> Self {
        let (tx, rx) = unbounded();
        let mut reader = ratatui::crossterm::event::EventStream::new();
        let tick_rate = std::time::Duration::from_millis(250);
        let _tx = tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tick_rate);
            // Track genuinely-held buttons. Some terminals report bare motion as
            // Drag (button-state word non-zero); we reinterpret those as Moved.
            let (mut left_held, mut right_held, mut middle_held) = (false, false, false);

            loop {
                let delay = interval.tick();
                let crossterm_event = reader.next().fuse();

                tokio::select! {
                    maybe_event = crossterm_event => {
                        match maybe_event {
                            Some(Ok(evt)) => {
                                match evt {
                                    ratatui::crossterm::event::Event::Key(key) => {
                                        if key.kind == KeyEventKind::Press {
                                            let _ = _tx.send(Event::Key(key));
                                        }
                                    },
                                    ratatui::crossterm::event::Event::Mouse(mut mouse) => {
                                        match mouse.kind {
                                            MouseEventKind::Down(MouseButton::Left) => left_held = true,
                                            MouseEventKind::Down(MouseButton::Right) => right_held = true,
                                            MouseEventKind::Down(MouseButton::Middle) => middle_held = true,
                                            MouseEventKind::Up(MouseButton::Left) => left_held = false,
                                            MouseEventKind::Up(MouseButton::Right) => right_held = false,
                                            MouseEventKind::Up(MouseButton::Middle) => middle_held = false,
                                            MouseEventKind::Drag(btn) => {
                                                let held = match btn {
                                                    MouseButton::Left => left_held,
                                                    MouseButton::Right => right_held,
                                                    MouseButton::Middle => middle_held,
                                                };
                                                if !held {
                                                    mouse.kind = MouseEventKind::Moved;
                                                }
                                            }
                                            _ => {}
                                        }
                                        let _ = _tx.try_send(Event::Mouse(mouse));
                                    },
                                    _ => {}
                                }
                            }
                            Some(Err(e)) => {
                                log::error!("Error: {e:?}");
                                let _ = _tx.try_send(Event::Error);
                            }
                            None => {},
                        }
                    },
                    _ = delay => {
                        let _ = _tx.try_send(Event::Tick);
                    },
                }
            }
        });

        Self { rx }
    }

    /// Event handler with no crossterm reader. Embedded mode injects input
    /// through the host instead of a TTY.
    pub fn new_inert() -> Self {
        let (_tx, rx) = unbounded();
        Self { rx }
    }

    pub fn next(&mut self) -> anyhow::Result<Event, TryRecvError>{
        self.rx.try_recv()
    }
}
