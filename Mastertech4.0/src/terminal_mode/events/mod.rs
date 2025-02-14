pub mod dispatcher;
pub mod exabind_event;
use crossbeam::channel::{unbounded, Receiver, Sender, TryRecvError};
use ratatui::crossterm::event::{KeyEvent, MouseEvent};

use tokio::task::JoinHandle;
use futures::{FutureExt, StreamExt};

#[derive(Clone, Copy, Debug)]
pub enum Event {
    Error,
    Tick,
    Key(KeyEvent),
    Mouse(MouseEvent)
}

pub struct EventHandler {
    pub tx: Sender<Event>,
    pub rx: Receiver<Event>,
    pub task: Option<JoinHandle<()>>,
}

impl EventHandler {
    pub fn new() -> Self {
        let (tx, rx) = unbounded();
        let mut reader = ratatui::crossterm::event::EventStream::new();
        let tick_rate = std::time::Duration::from_millis(250);

        let _tx = tx.clone();
        let task = tokio::spawn(async move {
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

        Self { tx, rx, task: Some(task) }
    }

    pub fn next(&mut self) -> anyhow::Result<Event, TryRecvError>{
        self.rx.try_recv()
    }
}