use crossbeam::channel::{unbounded, Receiver, TryRecvError};
use displays::remote_viewer::ratagui::TerminalEvent;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent};
use futures::{FutureExt, StreamExt};

use crate::terminal_mode::{systems::{communication_system::CommunicationSystem, notification_system::{Notification, NotificationType}}, tabs::Tab};

use super::{widgets::HandleWidget, TerminalApp};

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

impl <'a>TerminalApp<'a> {
    pub fn handle_events(&mut self, remote_mouse_event: Option<MouseEvent>, remote_key_event: Option<KeyEvent>) -> bool {
        let quit = &mut false;
        // Handle remote mouse event if provided
        if let Some(mouse_event) = remote_mouse_event {
            self.menu_bar.handle_mouse_event(&mouse_event);
            let current_tab = self.menu_bar.current_tab.borrow().clone();
            match current_tab {
                Tab::TurSheet => self.service_tab.borrow_mut().handle_mouse_event(&mouse_event),
                Tab::Scripts => self.scripts_tab.borrow_mut().handle_mouse_event(&mouse_event),
                Tab::SystemInfo => self.sysinfo_tab.handle_mouse_event(&mouse_event),
                Tab::Logs => {}
                Tab::Login => self.login_tab.borrow_mut().handle_mouse_event(&mouse_event),
                Tab::Tasks => self.tasks_tab.borrow_mut().handle_mouse_event(&mouse_event),
            };
        }

        if let Some(key_event) = remote_key_event {
            let ctrl_key = key_event.modifiers.contains(KeyModifiers::CONTROL);
            let current_tab = self.menu_bar.current_tab.borrow().clone();
            match key_event.code {
                KeyCode::Char('q') if ctrl_key => {
                    log::info!("Quitting from remote key event");
                    *quit = true;
                }
                KeyCode::Char('n') if ctrl_key => {
                    let notification = Notification::new(
                        NotificationType::Info,
                        "Remote Key Event",
                        "You pressed 'n' remotely.",
                        3,
                    );
                    let x = self.data_system.send(Box::new(notification));
                    log::info!("Result: {x:?}");
                }
                _ => {
                    if ctrl_key {
                        match key_event.code {
                            KeyCode::Right => {
                                match current_tab {
                                    Tab::TurSheet => self.menu_bar.set_active_tab(Tab::Scripts),
                                    Tab::Scripts => self.menu_bar.set_active_tab(Tab::Tasks),
                                    Tab::Tasks => self.menu_bar.set_active_tab(Tab::SystemInfo),
                                    Tab::SystemInfo => self.menu_bar.set_active_tab(Tab::Logs),
                                    Tab::Logs => self.menu_bar.set_active_tab(Tab::Login),
                                    Tab::Login => self.menu_bar.set_active_tab(Tab::TurSheet),
                                };
                            }
                            KeyCode::Left => {
                                match current_tab {
                                    Tab::TurSheet => self.menu_bar.set_active_tab(Tab::Login),
                                    Tab::Scripts => self.menu_bar.set_active_tab(Tab::TurSheet),
                                    Tab::Tasks => self.menu_bar.set_active_tab(Tab::Scripts),
                                    Tab::SystemInfo => self.menu_bar.set_active_tab(Tab::Tasks),
                                    Tab::Logs => self.menu_bar.set_active_tab(Tab::SystemInfo),
                                    Tab::Login => self.menu_bar.set_active_tab(Tab::Logs),
                                };
                            }
                            _ => {}
                        }
                    }

                    let consumed = match current_tab {
                        Tab::TurSheet => self.service_tab.borrow_mut().handle_key_event(key_event),
                        Tab::Scripts => self.scripts_tab.borrow_mut().handle_key_event(key_event),
                        Tab::Tasks => self.tasks_tab.borrow_mut().handle_key_event(key_event),
                        Tab::SystemInfo => self.sysinfo_tab.handle_key_event(key_event),
                        Tab::Logs => self.logger.handle_key_event(key_event),
                        Tab::Login => self.login_tab.borrow_mut().handle_key_event(key_event),
                    };

                    if consumed {}
                }
            };
        }
        
        if let Ok(events) = self.event_handler.next() {
            let current_tab = self.menu_bar.current_tab.borrow().clone();
            match events {
                Event::Key(key_event) => {
                    let ctrl_key = key_event.modifiers.contains(KeyModifiers::CONTROL);
                    match key_event.code {
                        KeyCode::Char('q') if ctrl_key => {
                            log::info!("Quitting");
                            *quit = true;
                        }
                        KeyCode::Char('n') if ctrl_key => { // Pressing 'n' triggers a notification
                            let notification = Notification::new(
                                NotificationType::Info, 
                                "Some Shit Has Happened.", 
                                "You pressed the notification key.", 
                                3
                            );
            
                            let x = self.data_system.send(Box::new(notification));
                            log::info!("Result: {x:?}");
                        }
                        _ => {
                            if ctrl_key {
                                match key_event.code {
                                    // We'll let left/right arrow change tabs
                                    KeyCode::Right => if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                                        log::info!("Current tab: {current_tab:?}");
                                        match current_tab {
                                            Tab::TurSheet => self.menu_bar.set_active_tab(Tab::Scripts),
                                            Tab::Scripts => self.menu_bar.set_active_tab(Tab::Tasks),
                                            Tab::Tasks => self.menu_bar.set_active_tab(Tab::SystemInfo),
                                            Tab::SystemInfo => self.menu_bar.set_active_tab(Tab::Logs),
                                            Tab::Logs => self.menu_bar.set_active_tab(Tab::Login),
                                            Tab::Login => self.menu_bar.set_active_tab(Tab::TurSheet),
                                        };
                                    }
                                    KeyCode::Left => if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                                        match current_tab {
                                            Tab::TurSheet => self.menu_bar.set_active_tab(Tab::Login),
                                            Tab::Scripts => self.menu_bar.set_active_tab(Tab::TurSheet),
                                            Tab::Tasks => self.menu_bar.set_active_tab(Tab::Scripts),
                                            Tab::SystemInfo => self.menu_bar.set_active_tab(Tab::Tasks),
                                            Tab::Logs => self.menu_bar.set_active_tab(Tab::SystemInfo),
                                            Tab::Login => self.menu_bar.set_active_tab(Tab::Logs),
                                        };
                                    }
                                    _ => {}
                                }
                            }

                            // Now dispatch key event to the active widget, and only one widget:
                            let consumed = match current_tab {
                                Tab::TurSheet => self.service_tab.borrow_mut().handle_key_event(key_event),
                                Tab::Scripts => self.scripts_tab.borrow_mut().handle_key_event(key_event),
                                Tab::Tasks => self.tasks_tab.borrow_mut().handle_key_event(key_event),
                                Tab::SystemInfo => self.sysinfo_tab.handle_key_event(key_event),
                                Tab::Logs => self.logger.handle_key_event(key_event),
                                Tab::Login => self.login_tab.borrow_mut().handle_key_event(key_event),
                            };

                            if consumed {}
                        }
                    };
                },
                Event::Mouse(mouse_event) => {
                    self.menu_bar.handle_mouse_event(&mouse_event);
                     match current_tab {
                        Tab::TurSheet => self.service_tab.borrow_mut().handle_mouse_event(&mouse_event),
                        Tab::Scripts => self.scripts_tab.borrow_mut().handle_mouse_event(&mouse_event),
                        Tab::SystemInfo => self.sysinfo_tab.handle_mouse_event(&mouse_event),
                        Tab::Logs => {}
                        Tab::Login => self.login_tab.borrow_mut().handle_mouse_event(&mouse_event),
                        Tab::Tasks => self.tasks_tab.borrow_mut().handle_mouse_event(&mouse_event),
                    };
                },
                Event::Error => log::info!("Error in event loop"),
                Event::Tick => {}
            }
        }

        *quit
    }
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
                                    ratatui::crossterm::event::Event::Key(key) => {
                                        if key.kind == KeyEventKind::Press {
                                            let _ = _tx.send(Event::Key(key));
                                        }
                                    },
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