
use database::schema::{prestashop_schema::PrestashopPayload, utilities::{create_full_task_payload, get_prestashop_payload, get_prestashop_payload_from_phone}, ComputerData, CustomerData, TaskNotePayload, TaskPayload, TicketPayload, TICKET_TABLE};
use displays::remote_viewer::ratagui::TerminalEvent;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use crate::filesystem::system_info::ComputerInfo;
use chrono::{DateTime, SecondsFormat, Utc};
use std::sync::{Arc, Condvar, Mutex};
use surrealdb::RecordId;
use egui::Key;
// use reqwest::Client;

use super::events::action_handler::{get_event_sender, ApiEvent, WidgetEvent};

pub mod first_run;


#[derive(Debug, Clone, Default)]
pub struct ServiceData {
    pub task_data: TaskPayload,
    pub ticket_data: TicketPayload,
    pub customer_data: CustomerData,
    pub computer_data: ComputerData,
    pub task_notes: Vec<TaskNotePayload>,
    send_specs: bool,
    // client: Client,
}

impl ServiceData {
    pub fn new() -> Self {
        let pair = Arc::new(
            (Mutex::new(ComputerData::default()), Condvar::new())
        );
        let pair_clone = Arc::clone(&pair);

        tokio::spawn(async move {
            match ComputerData::default().get_computer_data().await {
                // sysinfo_tx
                Ok(data) => {
                    let (lock, cvar) = &*pair_clone;
                    let mut comp_data = lock.lock().unwrap();
                    *comp_data = data;
                    log::info!("Computer Data: {comp_data:?}");
                    cvar.notify_one();
                }
                Err(e) => log::error!("Error getting specs: {e:?}"),
            }
        });

        // Wait for the spawned task to complete and notify the condition variable
        let (lock, cvar) = &*pair;
        let mut comp_data = lock.lock().unwrap();
        while comp_data.cpu.is_empty() {
            comp_data = cvar.wait(comp_data).unwrap();
        }

        Self {
            task_data: Default::default(),
            ticket_data: Default::default(),
            customer_data: Default::default(),
            computer_data: comp_data.clone(),
            task_notes: Default::default(),
            send_specs: true,
            // client: Client::new(),
        }
    }
    
    pub fn receive(&mut self, presta_data: PrestashopPayload) {
        log::info!("{:?}", serde_json::to_value(&presta_data).unwrap_or_default());
        let customer = &mut self.customer_data;
        let ticket = &mut self.ticket_data;
        let task = &mut self.task_data;
        let task_notes = &mut self.task_notes;

        let service_details = presta_data.order.associations.order_service.clone();
        let mut services: Vec<RecordId> = Vec::new();

        let sales_rep = presta_data.sales_rep.clone().unwrap_or_default();
        let split_rep = presta_data.split_rep.clone().unwrap_or_default();

        let sales_rep_initials = sales_rep.initials.clone();
        let split_initials = split_rep.initials.clone();

        let email = sales_rep
            .email
            .split_once("@")
            .clone()
            .unwrap_or((&sales_rep_initials, ""))
            .0
            .to_string();

        let email_split_rep = split_rep
            .email
            .split_once("@")
            .clone()
            .unwrap_or((&split_initials, ""))
            .0
            .to_string();

        for msg in presta_data.customer_messages.iter() {
            task_notes.push(TaskNotePayload {
                everest_initials: msg.id_employee.clone(),
                note: msg.message.clone(),
                ..Default::default()
            })
        }

        customer.id = presta_data.customer.id.clone();
        customer.cust_code = presta_data.customer.cust_code.clone();
        customer.email = presta_data.customer.email.clone();
        customer.name = presta_data.customer.name.clone();
        customer.phone_number = presta_data.customer.phone_number.clone();
        ticket.salesman = email_split_rep;
        ticket.sales_rep = email.clone();
        ticket.tech = email.clone();
        log::info!(
            "Salesman: {:?}\nTech: {:?}",
            ticket.salesman.clone(),
            ticket.tech.clone()
        );
        ticket.customer = Some(customer.clone());
        ticket.checkin_rep = email;
        ticket.terms = presta_data.order.payment.clone();
        ticket.ticket_total = presta_data.order.total_products_wt.clone();
        ticket.doc_alias = presta_data.order.order_type.clone();
        ticket.service_number = presta_data.order.id.clone();
        ticket.id = RecordId::from((
            TICKET_TABLE.to_string(),
            ticket.service_number.clone(),
        ));

        services.push(ticket.id.clone());
        
        if !service_details.is_empty() {
            if service_details.len() == 1 {
                let svc = service_details.get(0);
                if let Some(service) = svc {
                    ticket.checkin_notes = service.check_in_notes.clone();
                }
            } else {
                log::info!("Theres a couple.... {:?}", service_details);
            }
        }

        task.service_ticket = Some(ticket.clone());
    }
    
    pub fn get_ticket(&self) {
        let input = self.ticket_data.service_number.clone();
        let phone = self.customer_data.phone_number.clone();
        if !input.is_empty() {
            let tx = get_event_sender();
            tokio::spawn(async move {
                let prestashop_order = get_prestashop_payload(&input).await?;
                tx.try_send(WidgetEvent::Api(ApiEvent::GetTicketResponse(prestashop_order)))?;
                Ok::<(), anyhow::Error>(())
            });
        } else if !phone.is_empty() {
            let tx = get_event_sender();
            tokio::spawn(async move {
                let prestashop_order = get_prestashop_payload_from_phone(&phone).await?;
                tx.try_send(WidgetEvent::Api(ApiEvent::GetTicketResponse(prestashop_order)))?;
                Ok::<(), anyhow::Error>(())
            });
        }
    }

    pub fn submit_tur_mastertech(&mut self) {
        let mut task_data = self.task_data.clone();
        let customer_data = self.customer_data.clone();
        let ticket_data = self.ticket_data.clone();
        let computer_data = self.computer_data.clone();
        let task_notes = self.task_notes.clone();

        task_data.due_date = DateTime::<Utc>::default().to_rfc3339_opts(SecondsFormat::Secs, true);
        let send_specs = self.send_specs.clone();
        tokio::spawn(async move {
            let send_payload_result = create_full_task_payload(
                ticket_data.into(),
                customer_data,
                computer_data,
                task_data.into(),
                task_notes,
                send_specs,
            )
            .await;
            log::info!("send_payload_result: {send_payload_result:?}");
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

impl TryFrom<LocalTermEvent> for MouseEvent {
    type Error = anyhow::Error;
    
    fn try_from(value: LocalTermEvent) -> Result<Self, Self::Error> {
        if let TerminalEvent::MouseClick { x, y } = value.0 {
            Ok(
                MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: x,
                    row: y,
                    modifiers: KeyModifiers::NONE,
                }
            )
        } else {
            return Err(anyhow::anyhow!("Error converting TerminalEvent to MouseEvent"));
        }
    }
}


impl Into<TerminalEvent> for LocalTermEvent {
    fn into(self) -> TerminalEvent {
        match self.0 {
            TerminalEvent::MouseClick { x, y } => TerminalEvent::MouseClick { x, y },
            TerminalEvent::KeyPress { code, modifiers } => TerminalEvent::KeyPress { code, modifiers },
        }
    }
}

impl Into<LocalTermEvent> for TerminalEvent {
    fn into(self) -> LocalTermEvent {
        match self {
            TerminalEvent::MouseClick { x, y } => LocalTermEvent(TerminalEvent::MouseClick { x, y }),
            TerminalEvent::KeyPress { code, modifiers } => LocalTermEvent(TerminalEvent::KeyPress { code, modifiers }),
        }
    }
}