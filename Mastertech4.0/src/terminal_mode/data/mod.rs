
use database::schema::{prestashop_schema::PrestashopPayload, utilities::{check_for_duplicates, create_full_task_payload}, ComputerData, CustomerData, DuplicateResolution, EntityDraft, OrderLookup, PrestaMapMode, PrestaMapOptions, TaskNotePayload, TaskPayload, TicketPayload, fetch_prestashop_order, apply_prestashop_payload};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use displays::remote_viewer::ratagui::TerminalEvent;
use crate::filesystem::system_info::ComputerInfo;
use crossbeam::channel::Receiver;
use egui::{Key, Modifiers};
use chrono::Utc;

use super::events::action_handler::{get_event_sender, ApiEvent, WidgetEvent};

pub mod first_run;
pub mod log_capture;


#[derive(Debug, Clone)]
pub struct ServiceData {
    pub task_data: TaskPayload,
    pub ticket_data: TicketPayload,
    pub customer_data: CustomerData,
    pub computer_data: ComputerData,
    pub task_notes: Vec<TaskNotePayload>,
    // pub computer_data_tx: Sender<ComputerData>,
    pub computer_data_rx: Receiver<ComputerData>,
    send_specs: bool,
    // client: Client,
}

impl ServiceData {
    pub fn new() -> Self {
        let (computer_data_tx, computer_data_rx) = crossbeam::channel::unbounded();
        let tx = computer_data_tx.clone();
        tokio::spawn(async move {
            match ComputerData::default().get_computer_data().await {
                Ok(data) => { let _ = tx.try_send(data); }
                Err(e) => log::error!("Error getting specs: {e:?}"),
            }
        });
        log::info!("COMPUTER DATA RETRIEVAL");
        Self {
            task_data: Default::default(),
            ticket_data: Default::default(),
            customer_data: Default::default(),
            computer_data: ComputerData::default(),
            task_notes: Default::default(),
            computer_data_rx,
            send_specs: true,
            // client: Client::new(),
        }
    }
    pub fn receive_computer_data(&mut self) {
        if let Ok(computer) = self.computer_data_rx.try_recv() {
            log::info!("GOT PC DATA");
            self.computer_data = computer;
        }
    }

    pub fn receive(&mut self, presta_data: PrestashopPayload) {
        log::info!("{:?}", serde_json::to_value(&presta_data).unwrap_or_default());

        let mut draft = EntityDraft {
            customer: self.customer_data.clone(),
            ticket: self.ticket_data.clone().into(),
            computer: self.computer_data.clone(),
            task: self.task_data.clone().into(),
            task_notes: self.task_notes.clone(),
        };
        apply_prestashop_payload(
            &presta_data,
            &mut draft,
            &PrestaMapOptions {
                mode: PrestaMapMode::Bench,
                ..Default::default()
            },
        );

        self.customer_data = draft.customer;
        self.ticket_data = TicketPayload {
            customer: Some(self.customer_data.clone()),
            computer: Some(draft.computer.clone()),
            ..draft.ticket.into()
        };
        self.computer_data = draft.computer;
        self.task_notes = draft.task_notes;
        self.task_data = TaskPayload {
            service_ticket: Some(self.ticket_data.clone()),
            task_note: self.task_notes.clone(),
            ..draft.task.into()
        };
        self.task_data.task_note = self.task_notes.clone();
    }
    
    pub fn get_ticket(&self) {
        let input = self.ticket_data.service_number.clone();
        let phone = self.customer_data.phone_number.clone();
        let lookup = if !input.is_empty() {
            Some(OrderLookup::ServiceNumber(input))
        } else if !phone.is_empty() {
            Some(OrderLookup::Phone(phone))
        } else {
            None
        };
        let Some(lookup) = lookup else { return };
        let tx = get_event_sender();
        tokio::spawn(async move {
            let prestashop_order = fetch_prestashop_order(lookup).await?;
            tx.try_send(WidgetEvent::Api(ApiEvent::GetTicketResponse(prestashop_order)))?;
            Ok::<(), anyhow::Error>(())
        });
    }

    /// First step: Check for duplicates before submitting
    pub fn submit_tur_mastertech(&mut self) {
        let mut task_data = self.task_data.clone();
        let customer_data = self.customer_data.clone();
        let ticket_data = self.ticket_data.clone();
        let computer_data = self.computer_data.clone();
        task_data.due_date = Utc::now().into();
        let send_specs = self.send_specs;
        let service_number = ticket_data.service_number.clone();

        // Populate task data fields
        task_data.service_number = Some(service_number.clone());
        task_data.task_name = format!("{} - {}", &customer_data.name, &service_number);

        let tx = get_event_sender();
        let computer_for_check = if send_specs { Some(computer_data.clone()) } else { None };

        log::info!("Starting duplicate check for service #{}", service_number);

        tokio::spawn(async move {
            let result = check_for_duplicates(
                &service_number,
                &task_data.clone().into(),
                &ticket_data.clone().into(),
                &customer_data,
                computer_for_check.as_ref(),
            ).await;

            match result {
                Ok(check_result) => {
                    log::info!("Duplicate check completed: has_conflicts={}", check_result.has_conflicts());
                    let _ = tx.try_send(WidgetEvent::Api(ApiEvent::DuplicateCheckResponse(check_result)));
                }
                Err(e) => {
                    log::error!("Error during duplicate check: {:?}", e);
                    // If check fails, proceed without showing modal (fallback behavior)
                    let empty_result = database::schema::DuplicateCheckResult::new(service_number);
                    let _ = tx.try_send(WidgetEvent::Api(ApiEvent::DuplicateCheckResponse(empty_result)));
                }
            }
        });
    }

    /// Second step: Actually submit after duplicate resolution (or if no conflicts)
    pub fn submit_after_resolution(&mut self, resolution: Option<DuplicateResolution>) {
        let mut task_data = self.task_data.clone();
        let customer_data = self.customer_data.clone();
        let ticket_data = self.ticket_data.clone();
        let computer_data = self.computer_data.clone();
        let task_notes = self.task_notes.clone();
        task_data.due_date = Utc::now().into();
        // Only persist hardware specs when the computer actually has them; without
        // real hardware, create_full_task_payload refuses to write a Presta-only
        // placeholder computer and the whole task creation errors out.
        let send_specs = self.send_specs
            && database::schema::entity_link::computer_has_minimal_hardware(&computer_data);

        let tx = get_event_sender();

        log::info!("Submitting TUR sheet after resolution: {:?}", resolution);

        tokio::spawn(async move {
            let send_payload_result = create_full_task_payload(
                ticket_data.into(),
                customer_data,
                computer_data,
                task_data.into(),
                task_notes,
                send_specs,
                false,
                None,
            )
            .await;
            log::info!("send_payload_result: {send_payload_result:?}");
            let _ = tx.try_send(WidgetEvent::Api(ApiEvent::TaskCreationResponse(send_payload_result)));
        });
    }
}

#[derive(Clone, serde::Deserialize, serde::Serialize, Debug)]
pub struct LocalTermEvent(pub TerminalEvent);

impl TryFrom<LocalTermEvent> for KeyEvent {
    type Error = anyhow::Error;
    
    fn try_from(value: LocalTermEvent) -> Result<Self, Self::Error> {
        if let TerminalEvent::KeyPress { code, modifiers } = value.0 {
            let mut rat_modifiers = KeyModifiers::NONE;
            if modifiers.ctrl { rat_modifiers.insert(KeyModifiers::CONTROL); }
            if modifiers.shift { rat_modifiers.insert(KeyModifiers::SHIFT); }
            if modifiers.alt { rat_modifiers.insert(KeyModifiers::ALT); }
    
            let code = match code {
                Key::Enter => KeyCode::Enter,
                Key::Tab => KeyCode::Tab,
                Key::Backspace => KeyCode::Backspace,
                Key::Escape => KeyCode::Esc,
                Key::Delete => KeyCode::Delete,
                Key::ArrowLeft => KeyCode::Left,
                Key::ArrowRight => KeyCode::Right,
                Key::ArrowUp => KeyCode::Up,
                Key::ArrowDown => KeyCode::Down,
                Key::Insert => KeyCode::Insert,
                Key::Home => KeyCode::Home,
                Key::End => KeyCode::End,
                Key::PageUp => KeyCode::PageUp,
                Key::PageDown => KeyCode::PageDown,
                Key::Space => KeyCode::Char(' '),
                Key::A => KeyCode::Char('a'),
                Key::B => KeyCode::Char('b'),
                Key::C => KeyCode::Char('c'),
                Key::D => KeyCode::Char('d'),
                Key::E => KeyCode::Char('e'),
                Key::F => KeyCode::Char('f'),
                Key::G => KeyCode::Char('g'),
                Key::H => KeyCode::Char('h'),
                Key::I => KeyCode::Char('i'),
                Key::J => KeyCode::Char('j'),
                Key::K => KeyCode::Char('k'),
                Key::L => KeyCode::Char('l'),
                Key::M => KeyCode::Char('m'),
                Key::N => KeyCode::Char('n'),
                Key::O => KeyCode::Char('o'),
                Key::P => KeyCode::Char('p'),
                Key::Q => KeyCode::Char('q'),
                Key::R => KeyCode::Char('r'),
                Key::S => KeyCode::Char('s'),
                Key::T => KeyCode::Char('t'),
                Key::U => KeyCode::Char('u'),
                Key::V => KeyCode::Char('v'),
                Key::W => KeyCode::Char('w'),
                Key::X => KeyCode::Char('x'),
                Key::Y => KeyCode::Char('y'),
                Key::Z => KeyCode::Char('z'),
                Key::Copy => KeyCode::Null, // No direct equivalent; could map to Ctrl+C if needed
                Key::Cut => KeyCode::Null,  // No direct equivalent; could map to Ctrl+X
                Key::Paste => KeyCode::Null,// No direct equivalent; could map to Ctrl+V
                Key::Colon => KeyCode::Char(':'),
                Key::Comma => KeyCode::Char(','),
                Key::Backslash => KeyCode::Char('\\'),
                Key::Slash => KeyCode::Char('/'),
                Key::Pipe => KeyCode::Char('|'),
                Key::Questionmark => KeyCode::Char('?'),
                Key::Exclamationmark => KeyCode::Char('!'),
                Key::OpenBracket => KeyCode::Char('['),
                Key::CloseBracket => KeyCode::Char(']'),
                Key::OpenCurlyBracket => KeyCode::Char('{'),
                Key::CloseCurlyBracket => KeyCode::Char('}'),
                Key::Backtick => KeyCode::Char('`'),
                Key::Minus => KeyCode::Char('-'),
                Key::Period => KeyCode::Char('.'),
                Key::Plus => KeyCode::Char('+'),
                Key::Equals => KeyCode::Char('='),
                Key::Semicolon => KeyCode::Char(';'),
                Key::Quote => KeyCode::Char('\''),
                Key::Num0 => KeyCode::Char('0'),
                Key::Num1 => KeyCode::Char('1'),
                Key::Num2 => KeyCode::Char('2'),
                Key::Num3 => KeyCode::Char('3'),
                Key::Num4 => KeyCode::Char('4'),
                Key::Num5 => KeyCode::Char('5'),
                Key::Num6 => KeyCode::Char('6'),
                Key::Num7 => KeyCode::Char('7'),
                Key::Num8 => KeyCode::Char('8'),
                Key::Num9 => KeyCode::Char('9'),
                Key::F1 => KeyCode::F(1),
                Key::F2 => KeyCode::F(2),
                Key::F3 => KeyCode::F(3),
                Key::F4 => KeyCode::F(4),
                Key::F5 => KeyCode::F(5),
                Key::F6 => KeyCode::F(6),
                Key::F7 => KeyCode::F(7),
                Key::F8 => KeyCode::F(8),
                Key::F9 => KeyCode::F(9),
                Key::F10 => KeyCode::F(10),
                Key::F11 => KeyCode::F(11),
                Key::F12 => KeyCode::F(12),
                Key::F13 => KeyCode::F(13),
                Key::F14 => KeyCode::F(14),
                Key::F15 => KeyCode::F(15),
                Key::F16 => KeyCode::F(16),
                Key::F17 => KeyCode::F(17),
                Key::F18 => KeyCode::F(18),
                Key::F19 => KeyCode::F(19),
                Key::F20 => KeyCode::F(20),
                Key::F21 => KeyCode::F(21),
                Key::F22 => KeyCode::F(22),
                Key::F23 => KeyCode::F(23),
                Key::F24 => KeyCode::F(24),
                Key::F25 => KeyCode::F(25),
                Key::F26 => KeyCode::F(26),
                Key::F27 => KeyCode::F(27),
                Key::F28 => KeyCode::F(28),
                Key::F29 => KeyCode::F(29),
                Key::F30 => KeyCode::F(30),
                Key::F31 => KeyCode::F(31),
                Key::F32 => KeyCode::F(32),
                Key::F33 => KeyCode::F(33),
                Key::F34 => KeyCode::F(34),
                Key::F35 => KeyCode::F(35),
                Key::BrowserBack => KeyCode::Backspace,
                _ => KeyCode::Null,
            };

            Ok(
                KeyEvent {
                    code,
                    modifiers: rat_modifiers,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }
            )
        } else {
            return Err(anyhow::anyhow!("Error converting TerminalEvent to KeyEvent"));
        }
    }
}

impl TryFrom<KeyEvent> for LocalTermEvent {
    type Error = anyhow::Error;

    fn try_from(value: KeyEvent) -> Result<Self, Self::Error> {
        let modifiers = match value.modifiers {
            KeyModifiers::ALT => Modifiers::ALT,
            KeyModifiers::CONTROL => Modifiers::CTRL,
            KeyModifiers::SHIFT => Modifiers::SHIFT,
            _ =>  Modifiers::NONE
        };
        
        let code = match value.code {
            KeyCode::Enter => Key::Enter,
            KeyCode::Tab => Key::Tab,
            KeyCode::Backspace => Key::Backspace,
            KeyCode::Esc => Key::Escape,
            KeyCode::Delete => Key::Delete,
            KeyCode::Left => Key::ArrowLeft,
            KeyCode::Right => Key::ArrowRight,
            KeyCode::Up => Key::ArrowUp,
            KeyCode::Down => Key::ArrowDown,
            KeyCode::Insert => Key::Insert,
            KeyCode::Home => Key::Home,
            KeyCode::End => Key::End,
            KeyCode::PageUp => Key::PageUp,
            KeyCode::PageDown => Key::PageDown,
            KeyCode::Char(' ') => Key::Space,
            KeyCode::Char('a') => Key::A,
            KeyCode::Char('b') => Key::B,
            KeyCode::Char('c') => Key::C,
            KeyCode::Char('d') => Key::D,
            KeyCode::Char('e') => Key::E,
            KeyCode::Char('f') => Key::F,
            KeyCode::Char('g') => Key::G,
            KeyCode::Char('h') => Key::H,
            KeyCode::Char('i') => Key::I,
            KeyCode::Char('j') => Key::J,
            KeyCode::Char('k') => Key::K,
            KeyCode::Char('l') => Key::L,
            KeyCode::Char('m') => Key::M,
            KeyCode::Char('n') => Key::N,
            KeyCode::Char('o') => Key::O,
            KeyCode::Char('p') => Key::P,
            KeyCode::Char('q') => Key::Q,
            KeyCode::Char('r') => Key::R,
            KeyCode::Char('s') => Key::S,
            KeyCode::Char('t') => Key::T,
            KeyCode::Char('u') => Key::U,
            KeyCode::Char('v') => Key::V,
            KeyCode::Char('w') => Key::W,
            KeyCode::Char('x') => Key::X,
            KeyCode::Char('y') => Key::Y,
            KeyCode::Char('z') => Key::Z,
            KeyCode::Char(':') => Key::Colon,
            KeyCode::Char(',') => Key::Comma,
            KeyCode::Char('\\') => Key::Backslash,
            KeyCode::Char('/') => Key::Slash,
            KeyCode::Char('|') => Key::Pipe,
            KeyCode::Char('?') => Key::Questionmark,
            KeyCode::Char('!') => Key::Exclamationmark,
            KeyCode::Char('[') => Key::OpenBracket,
            KeyCode::Char(']') => Key::CloseBracket,
            KeyCode::Char('{') => Key::OpenCurlyBracket,
            KeyCode::Char('}') => Key::CloseCurlyBracket,
            KeyCode::Char('`') => Key::Backtick,
            KeyCode::Char('-') => Key::Minus,
            KeyCode::Char('.') => Key::Period,
            KeyCode::Char('+') => Key::Plus,
            KeyCode::Char('=') => Key::Equals,
            KeyCode::Char(';') => Key::Semicolon,
            KeyCode::Char('\'') => Key::Quote,
            KeyCode::Char('0') => Key::Num0,
            KeyCode::Char('1') => Key::Num1,
            KeyCode::Char('2') => Key::Num2,
            KeyCode::Char('3') => Key::Num3,
            KeyCode::Char('4') => Key::Num4,
            KeyCode::Char('5') => Key::Num5,
            KeyCode::Char('6') => Key::Num6,
            KeyCode::Char('7') => Key::Num7,
            KeyCode::Char('8') => Key::Num8,
            KeyCode::Char('9') => Key::Num9,
            KeyCode::F(1) => Key::F1,
            KeyCode::F(2) => Key::F2,
            KeyCode::F(3) => Key::F3,
            KeyCode::F(4) => Key::F4,
            KeyCode::F(5) => Key::F5,
            KeyCode::F(6) => Key::F6,
            KeyCode::F(7) => Key::F7,
            KeyCode::F(8) => Key::F8,
            KeyCode::F(9) => Key::F9,
            KeyCode::F(10) => Key::F10,
            KeyCode::F(11) => Key::F11,
            KeyCode::F(12) => Key::F12,
            KeyCode::F(13) => Key::F13,
            KeyCode::F(14) => Key::F14,
            KeyCode::F(15) => Key::F15,
            KeyCode::F(16) => Key::F16,
            KeyCode::F(17) => Key::F17,
            KeyCode::F(18) => Key::F18,
            KeyCode::F(19) => Key::F19,
            KeyCode::F(20) => Key::F20,
            KeyCode::F(21) => Key::F21,
            KeyCode::F(22) => Key::F22,
            KeyCode::F(23) => Key::F23,
            KeyCode::F(24) => Key::F24,
            KeyCode::F(25) => Key::F25,
            KeyCode::F(26) => Key::F26,
            KeyCode::F(27) => Key::F27,
            KeyCode::F(28) => Key::F28,
            KeyCode::F(29) => Key::F29,
            KeyCode::F(30) => Key::F30,
            KeyCode::F(31) => Key::F31,
            KeyCode::F(32) => Key::F32,
            KeyCode::F(33) => Key::F33,
            KeyCode::F(34) => Key::F34,
            KeyCode::F(35) => Key::F35,
            _ => Key::Space,
        };

        Ok(
            LocalTermEvent(
                TerminalEvent::KeyPress { code, modifiers }
            )
        )
    }
}

impl TryFrom<LocalTermEvent> for MouseEvent {
    type Error = anyhow::Error;
    
    fn try_from(value: LocalTermEvent) -> Result<Self, Self::Error> {
        match value.0 {
            TerminalEvent::MouseClick { x, y } => Ok(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            }),
            TerminalEvent::MouseMove { x, y } => Ok(MouseEvent {
                kind: MouseEventKind::Moved,
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            }),
            TerminalEvent::MouseScroll { x, y, up } => Ok(MouseEvent {
                kind: if up { MouseEventKind::ScrollUp } else { MouseEventKind::ScrollDown },
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            }),
            _ => Err(anyhow::anyhow!("Error converting TerminalEvent to MouseEvent")),
        }
    }
}

impl Into<TerminalEvent> for LocalTermEvent {
    fn into(self) -> TerminalEvent {
        self.0
    }
}

impl Into<LocalTermEvent> for TerminalEvent {
    fn into(self) -> LocalTermEvent {
        LocalTermEvent(self)
    }
}