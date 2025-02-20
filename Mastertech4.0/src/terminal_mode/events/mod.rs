use crossbeam::channel::{unbounded, Receiver, TryRecvError};
use ratatui::crossterm::event::{KeyEvent, MouseEvent};
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

            loop {
                let delay = interval.tick();
                let crossterm_event = reader.next().fuse();

                tokio::select! {
                    maybe_event = crossterm_event => {
                        match maybe_event {
                            Some(Ok(evt)) => {
                                match evt {
                                    ratatui::crossterm::event::Event::Key(key) => {let _ = _tx.try_send(Event::Key(key));},
                                    ratatui::crossterm::event::Event::Mouse(mouse) => {let _ = _tx.try_send(Event::Mouse(mouse));},
                                    _ => {}
                                }
                            }
                            Some(Err(e)) => {
                                log::info!("Error: {e:?}");
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

    pub fn next(&mut self) -> anyhow::Result<Event, TryRecvError>{
        self.rx.try_recv()
    }
}