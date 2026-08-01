use crate::terminal_mode::{systems::{communication_system::CommunicationSystem, notification_system::{Notification, NotificationType}}, tabs::Tab};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use crossbeam::channel::{unbounded, Receiver, TryRecvError};
use futures::{FutureExt, StreamExt};

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

    /// Event handler with no crossterm reader. Embedded (egui) mode injects
    /// input through `TerminalApp::handle_events` instead of a TTY.
    pub fn new_inert() -> Self {
        let (_tx, rx) = unbounded();
        Self { rx }
    }

    pub fn next(&mut self) -> anyhow::Result<Event, TryRecvError>{
        self.rx.try_recv()
    }
}

impl <'a>TerminalApp<'a> {
    pub fn handle_events(&mut self, remote_mouse_event: Option<MouseEvent>, remote_key_event: Option<KeyEvent>) -> bool {
        let quit = &mut false;
        if let Ok(menu_bar) = self.menu_bar.try_borrow() {
            // Handle remote mouse event if provided
            if let Some(mouse_event) = remote_mouse_event {
                let menu_consumed = menu_bar.handle_menu_mouse(&mouse_event);
                let current_tab = menu_bar.current_tab.borrow().clone();
                if !menu_consumed {
                    match current_tab {
                        Tab::TurSheet => self.service_tab.borrow_mut().handle_mouse_event(&mouse_event),
                        Tab::Scripts => self.scripts_tab.borrow_mut().handle_mouse_event(&mouse_event),
                        Tab::SystemInfo => self.sysinfo_tab.handle_mouse_event(&mouse_event),
                        Tab::Login => self.login_tab.borrow_mut().handle_mouse_event(&mouse_event),
                        Tab::Tasks => self.tasks_tab.borrow_mut().handle_mouse_event(&mouse_event),
                        Tab::Stress => self.stress_tab.borrow_mut().handle_mouse_event(&mouse_event),
                        Tab::Webconsole => self.webconsole_tab.borrow_mut().handle_mouse_event(&mouse_event),
                        Tab::Logs => self.logger.handle_mouse_event(&mouse_event),
                        Tab::Settings => self.settings_tab.borrow_mut().handle_mouse_event(&mouse_event),
                        Tab::Assistant => self.assistant_tab.borrow_mut().handle_mouse_event(&mouse_event),
                        Tab::Ncdu => self.ncdu_tab.borrow_mut().handle_mouse_event(&mouse_event),
                    };
                }
            }

            if let Some(key_event) = remote_key_event {
                let ctrl_key = key_event.modifiers.contains(KeyModifiers::CONTROL);
                let current_tab = menu_bar.current_tab.borrow().clone();
                match key_event.code {
                    // Revoking remote control outranks whatever tab has focus.
                    KeyCode::F(12) => crate::remote_exec::banner_tui::end_session(),
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
                                        Tab::TurSheet => menu_bar.set_active_tab(Tab::Scripts),
                                        Tab::Scripts => menu_bar.set_active_tab(Tab::Tasks),
                                        Tab::Tasks => menu_bar.set_active_tab(Tab::SystemInfo),
                                        Tab::SystemInfo => menu_bar.set_active_tab(Tab::Webconsole),
                                        Tab::Webconsole => menu_bar.set_active_tab(Tab::Logs),
                                        Tab::Logs => menu_bar.set_active_tab(Tab::Login),
                                        Tab::Login => menu_bar.set_active_tab(Tab::Settings),
                                        Tab::Settings => menu_bar.set_active_tab(Tab::Assistant),
                                        Tab::Assistant => menu_bar.set_active_tab(Tab::TurSheet),
                                        Tab::Ncdu => menu_bar.set_active_tab(Tab::Ncdu),
                                        Tab::Stress => menu_bar.set_active_tab(Tab::Stress),
                                    };
                                }
                                KeyCode::Left => {
                                    match current_tab {
                                        Tab::TurSheet => menu_bar.set_active_tab(Tab::Assistant),
                                        Tab::Assistant => menu_bar.set_active_tab(Tab::Settings),
                                        Tab::Scripts => menu_bar.set_active_tab(Tab::TurSheet),
                                        Tab::Tasks => menu_bar.set_active_tab(Tab::Scripts),
                                        Tab::SystemInfo => menu_bar.set_active_tab(Tab::Tasks),
                                        Tab::Webconsole => menu_bar.set_active_tab(Tab::SystemInfo),
                                        Tab::Logs => menu_bar.set_active_tab(Tab::Webconsole),
                                        Tab::Login => menu_bar.set_active_tab(Tab::Logs),
                                        Tab::Settings => menu_bar.set_active_tab(Tab::Login),
                                        Tab::Ncdu => menu_bar.set_active_tab(Tab::Ncdu),
                                        Tab::Stress => menu_bar.set_active_tab(Tab::Stress),
                                    };
                                }
                                _ => {}
                            }
                        }

                        let consumed = match current_tab {
                            Tab::TurSheet => self.service_tab.borrow_mut().handle_key_event(key_event),
                            Tab::Scripts => self.scripts_tab.borrow_mut().handle_key_event(key_event),
                            Tab::Tasks => self.tasks_tab.borrow_mut().handle_key_event(key_event),
                            Tab::Stress => self.stress_tab.borrow_mut().handle_key_event(key_event),
                            Tab::SystemInfo => self.sysinfo_tab.handle_key_event(key_event),
                            Tab::Logs => self.logger.handle_key_event(key_event),
                            Tab::Login => self.login_tab.borrow_mut().handle_key_event(key_event),
                            Tab::Webconsole => self.webconsole_tab.borrow_mut().handle_key_event(key_event),
                            Tab::Settings => self.settings_tab.borrow_mut().handle_key_event(key_event),
                            Tab::Assistant => self.assistant_tab.borrow_mut().handle_key_event(key_event),
                            Tab::Ncdu => self.ncdu_tab.borrow_mut().handle_key_event(key_event),
                        };

                        if consumed {}
                    }
                };
            }

            // Drain all queued input each frame. Mouse motion floods Moved events far
            // faster than one-per-frame can consume, which starved hover highlighting.
            let mut drained = 0u16;
            while let Ok(events) = self.event_handler.next() {
                let current_tab = menu_bar.current_tab.borrow().clone();
                // An open dropdown consumes navigation/Esc keys before the content tab.
                let menu_consumed_key = matches!(events, Event::Key(ke) if menu_bar.handle_menu_key(ke));
                if !menu_consumed_key {
                match events {
                    Event::Key(key_event) => {
                        let ctrl_key = key_event.modifiers.contains(KeyModifiers::CONTROL);
                        match key_event.code {
                            // Revoking remote control outranks whatever tab has focus.
                            KeyCode::F(12) => crate::remote_exec::banner_tui::end_session(),
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
                                                Tab::TurSheet => menu_bar.set_active_tab(Tab::Scripts),
                                                Tab::Scripts => menu_bar.set_active_tab(Tab::Tasks),
                                                Tab::Tasks => menu_bar.set_active_tab(Tab::SystemInfo),
                                                Tab::SystemInfo => menu_bar.set_active_tab(Tab::Webconsole),
                                                Tab::Webconsole => menu_bar.set_active_tab(Tab::Logs),
                                                Tab::Logs => menu_bar.set_active_tab(Tab::Login),
                                                Tab::Login => menu_bar.set_active_tab(Tab::Settings),
                                                Tab::Settings => menu_bar.set_active_tab(Tab::Assistant),
                                                Tab::Assistant => menu_bar.set_active_tab(Tab::TurSheet),
                                                Tab::Ncdu => menu_bar.set_active_tab(Tab::Ncdu),
                                                Tab::Stress => menu_bar.set_active_tab(Tab::Stress),
                                            };
                                        }
                                        KeyCode::Left => if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                                            match current_tab {
                                                Tab::TurSheet => menu_bar.set_active_tab(Tab::Assistant),
                                                Tab::Assistant => menu_bar.set_active_tab(Tab::Settings),
                                                Tab::Scripts => menu_bar.set_active_tab(Tab::TurSheet),
                                                Tab::Tasks => menu_bar.set_active_tab(Tab::Scripts),
                                                Tab::SystemInfo => menu_bar.set_active_tab(Tab::Tasks),
                                                Tab::Webconsole => menu_bar.set_active_tab(Tab::SystemInfo),
                                                Tab::Logs => menu_bar.set_active_tab(Tab::Webconsole),
                                                Tab::Login => menu_bar.set_active_tab(Tab::Logs),
                                                Tab::Settings => menu_bar.set_active_tab(Tab::Login),
                                                Tab::Ncdu => menu_bar.set_active_tab(Tab::Ncdu),
                                                Tab::Stress => menu_bar.set_active_tab(Tab::Stress),
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
                                    Tab::Stress => self.stress_tab.borrow_mut().handle_key_event(key_event),
                                    Tab::SystemInfo => self.sysinfo_tab.handle_key_event(key_event),
                                    Tab::Logs => self.logger.handle_key_event(key_event),
                                    Tab::Login => self.login_tab.borrow_mut().handle_key_event(key_event),
                                    Tab::Webconsole => self.webconsole_tab.borrow_mut().handle_key_event(key_event),
                                    Tab::Settings => self.settings_tab.borrow_mut().handle_key_event(key_event),
                                    Tab::Assistant => self.assistant_tab.borrow_mut().handle_key_event(key_event),
                                    Tab::Ncdu => self.ncdu_tab.borrow_mut().handle_key_event(key_event),
                                };

                                if consumed {}
                            }
                        };
                    },
                    Event::Mouse(mouse_event) => {
                        let menu_consumed = menu_bar.handle_menu_mouse(&mouse_event);
                        if !menu_consumed {
                            match current_tab {
                                Tab::TurSheet => self.service_tab.borrow_mut().handle_mouse_event(&mouse_event),
                                Tab::Scripts => self.scripts_tab.borrow_mut().handle_mouse_event(&mouse_event),
                                Tab::SystemInfo => self.sysinfo_tab.handle_mouse_event(&mouse_event),
                                Tab::Login => self.login_tab.borrow_mut().handle_mouse_event(&mouse_event),
                                Tab::Tasks => self.tasks_tab.borrow_mut().handle_mouse_event(&mouse_event),
                                Tab::Stress => self.stress_tab.borrow_mut().handle_mouse_event(&mouse_event),
                                Tab::Webconsole => self.webconsole_tab.borrow_mut().handle_mouse_event(&mouse_event),
                                Tab::Logs => self.logger.handle_mouse_event(&mouse_event),
                                Tab::Settings => self.settings_tab.borrow_mut().handle_mouse_event(&mouse_event),
                                Tab::Assistant => self.assistant_tab.borrow_mut().handle_mouse_event(&mouse_event),
                                Tab::Ncdu => self.ncdu_tab.borrow_mut().handle_mouse_event(&mouse_event)
                            };
                        }
                    },
                    Event::Error => log::error!("Error in event loop"),
                    Event::Tick => {}
                }
                }
                drained = drained.saturating_add(1);
                if *quit || drained >= 512 {
                    break;
                }
            }
        }
        *quit
    }
}
