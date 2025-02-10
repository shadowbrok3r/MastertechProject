use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use tui_input::backend::crossterm::EventHandler;

use super::{tabs::Tab, widgets::button::State, App};


impl <'a> App <'a> {
    pub fn handle_mouse_event(&mut self, mouse_event: MouseEvent) -> anyhow::Result<(), anyhow::Error> {
        match mouse_event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let c = mouse_event.column;
                let r = mouse_event.row;

                // for button in self.buttons.iter_mut() {
                //     if let Some(area) = button.area() {
                //         let state = if c >= area.x && c < area.x + area.width && r >= area.y && r < area.y + area.height {
                //             // let user_input = self.input.value();
                //             // self.log_message(&format!("(Click) 'Get Ticket' with input: {}", user_input));
                //             State::Active
                //         } else {
                //             State::Normal
                //         };
                //         button.state(state);
                //     }
                // }

                // 1) Check if 'Get Ticket' is clicked
                if let Some(area) = self.get_ticket_button_area {
                    if c >= area.x && c < area.x + area.width && r >= area.y && r < area.y + area.height {
                        let user_input = self.input.value();
                        self.log_message(&format!("(Click) 'Get Ticket' with input: {}", user_input));
                        self.get_ticket_button_state = State::Active;
                    } else {
                        self.get_ticket_button_state = State::Normal;
                    }
                }

                // 2) Check if 'Submit Ticket' is clicked
                if let Some(area) = self.submit_ticket_button_area {
                    if c >= area.x && c < area.x + area.width && r >= area.y && r < area.y + area.height {
                        self.log_message("(Click) 'Submit Ticket'");
                        self.submit_ticket_button_state = State::Active;
                    } else {
                        self.submit_ticket_button_state = State::Normal;
                    }
                }

                // 3) Check if 'Tuneup' is clicked
                if let Some(area) = self.tuneup_button_area {
                    if c >= area.x && c < area.x + area.width && r >= area.y && r < area.y + area.height {
                        self.log_message("(Click) 'Tuneup'");
                        self.tuneup_button_state = State::Active;
                    } else {
                        self.tuneup_button_state = State::Normal;
                    }
                }

                // 4) Check if 'QC' is clicked
                if let Some(area) = self.qc_button_area {
                    if c >= area.x && c < area.x + area.width && r >= area.y && r < area.y + area.height {
                        self.log_message("(Click) 'QC'");
                        self.qc_button_state = State::Active;
                    } else {
                        self.qc_button_state = State::Normal;
                    }
                }
            }
            MouseEventKind::Moved => {
                let c = mouse_event.column;
                let r = mouse_event.row;

                // 1) Hover 'Get Ticket'
                if let Some(area) = self.get_ticket_button_area {
                    if c >= area.x && c < area.x + area.width && r >= area.y && r < area.y + area.height {
                        self.get_ticket_button_state = State::Selected;
                    } else {
                        self.get_ticket_button_state = State::Normal;
                    }
                }

                // 2) Hover 'Submit'
                if let Some(area) = self.submit_ticket_button_area {
                    if c >= area.x && c < area.x + area.width && r >= area.y && r < area.y + area.height {
                        self.submit_ticket_button_state = State::Selected;
                    } else {
                        self.submit_ticket_button_state = State::Normal;
                    }
                }

                // 3) Hover 'Tuneup'
                if let Some(area) = self.tuneup_button_area {
                    if c >= area.x && c < area.x + area.width && r >= area.y && r < area.y + area.height {
                        self.tuneup_button_state = State::Selected;
                    } else {
                        self.tuneup_button_state = State::Normal;
                    }
                }

                // 4) Hover 'QC'
                if let Some(area) = self.qc_button_area {
                    if c >= area.x && c < area.x + area.width && r >= area.y && r < area.y + area.height {
                        self.qc_button_state = State::Selected;
                    } else {
                        self.qc_button_state = State::Normal;
                    }
                }
            }
            _ => {}
        };
        Ok(())
    }

    pub fn handle_key_event(&mut self, key_event: KeyEvent) -> anyhow::Result<(), anyhow::Error> {
        // Exit on Ctrl + C
        if key_event.code == KeyCode::Char('c')
            && key_event.modifiers.contains(KeyModifiers::CONTROL)
        {
            return Ok(());
        }

        // Send keystroke to input field
        self.input.handle_event(&Event::Key(key_event));

        match key_event.code {
            KeyCode::Enter => {
                let user_input = self.input.value();
                self.get_ticket(user_input);
                self.log_message(&format!("(Enter) 'Get Ticket' with input: {}", user_input));
            }
            // We'll let left/right arrow change tabs, just as an example
            KeyCode::Right => {
                self.selected_tab = match self.selected_tab {
                    Tab::TurSheet => Tab::Scripts,
                    Tab::Scripts => Tab::SystemInfo,
                    Tab::SystemInfo => Tab::Extra,
                    Tab::Extra => Tab::TurSheet,
                };
            }
            KeyCode::Left => {
                self.selected_tab = match self.selected_tab {
                    Tab::TurSheet => Tab::Extra,
                    Tab::Scripts => Tab::TurSheet,
                    Tab::SystemInfo => Tab::Scripts,
                    Tab::Extra => Tab::SystemInfo,
                };
            }
            KeyCode::Down => {
                // Move highlight in JSON widget, etc.
                self.json_widget.next_edit();
            }
            KeyCode::Up => {
                self.json_widget.prev_edit();
            }
            _ => {}
        };
        Ok(())
    }
}