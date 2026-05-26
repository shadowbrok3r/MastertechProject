use super::entity_link::{computer_has_minimal_hardware, is_placeholder_computer};
use super::helper_traits::parse_email_user;
use super::prestashop::order::ExtractedOrderSpecs;
use super::prestashop_schema::{PrestashopPayload, ServiceOrder};
use super::random_record_id;
use super::{
    ComputerData, CustomerData, DriveData, HardwareTests, LiveTaskPayload, RecordId,
    TaskNotePayload, TicketData, COMPUTER_TABLE, TASK_TABLE, TICKET_TABLE,
};

#[derive(Debug, Clone)]
pub enum OrderLookup {
    ServiceNumber(String),
    Phone(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TaskIdStrategy {
    #[default]
    Random,
    MatchServiceNumber,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PrestaMapMode {
    /// Preserve local hardware scan; fill device metadata from order_service.
    #[default]
    Bench,
    /// Parse order rows (drives, model, OS) and guard against placeholder computers.
    Web,
    /// Minimal device fields for audit auto-create (`send_specs: false`).
    Audit,
}

#[derive(Debug, Clone, Default)]
pub struct PrestaMapOptions {
    pub mode: PrestaMapMode,
    pub hardware_tests: Option<HardwareTests>,
    pub guard_placeholder_computer: bool,
    pub task_id_strategy: TaskIdStrategy,
}

#[derive(Debug, Clone, Default)]
pub struct EntityDraft {
    pub customer: CustomerData,
    pub ticket: TicketData,
    pub computer: ComputerData,
    pub task: LiveTaskPayload,
    pub task_notes: Vec<TaskNotePayload>,
}

pub async fn fetch_prestashop_order(
    lookup: OrderLookup,
) -> anyhow::Result<PrestashopPayload, anyhow::Error> {
    match lookup {
        OrderLookup::ServiceNumber(n) if !n.is_empty() => {
            super::utilities::get_prestashop_payload(&n).await
        }
        OrderLookup::Phone(p) if !p.is_empty() => {
            super::utilities::get_prestashop_payload_from_phone(&p).await
        }
        _ => Err(anyhow::anyhow!("Service number or phone required")),
    }
}

pub fn apply_prestashop_payload(
    data: &PrestashopPayload,
    draft: &mut EntityDraft,
    options: &PrestaMapOptions,
) {
    let service_details = data.order.associations.order_service.clone();
    let sales_rep = data.sales_rep.clone().unwrap_or_default();
    let split_rep = data.split_rep.clone().unwrap_or_default();
    let email = parse_email_user(&sales_rep.email).to_string();
    let email_split_rep = parse_email_user(&split_rep.email).to_string();

    draft.task.id = match options.task_id_strategy {
        TaskIdStrategy::Random => random_record_id(TASK_TABLE),
        TaskIdStrategy::MatchServiceNumber => {
            RecordId::new(TASK_TABLE, data.order.id.clone())
        }
    };

    draft.task_notes.clear();
    for msg in data.task_notes.iter() {
        draft.task_notes.push(TaskNotePayload {
            task_id: Some(draft.task.id.clone()),
            ..msg.clone()
        });
    }

    draft.customer.id = data.customer.id.clone();
    draft.customer.cust_code = data.customer.cust_code.clone();
    draft.customer.email = data.customer.email.clone();
    draft.customer.name = data.customer.name.clone();
    draft.customer.phone_number = data.customer.phone_number.clone();

    draft.ticket.salesman = if email_split_rep.is_empty() && !email.is_empty() {
        email.clone()
    } else {
        email_split_rep
    };
    draft.ticket.sales_rep = email.clone();
    draft.ticket.tech = email.clone();
    draft.ticket.customer = Some(draft.customer.id.clone());
    draft.ticket.checkin_rep = email;
    draft.ticket.terms = data.order.payment.clone();
    draft.ticket.ticket_total = data.order.total_products_wt.clone();
    draft.ticket.doc_alias = data.order.order_type.clone();
    draft.ticket.service_number = data.order.id.clone();
    draft.ticket.id = RecordId::new(TICKET_TABLE, draft.ticket.service_number.clone());
    if let Some(tests) = &options.hardware_tests {
        draft.ticket.hardware_test_results = tests.clone();
    }

    if let Some(service) = service_details.first() {
        draft.ticket.checkin_notes = service.check_in_notes.clone();
    }

    match options.mode {
        PrestaMapMode::Web => apply_web_computer(data, draft, &service_details),
        PrestaMapMode::Audit => {
            if let Some(service) = service_details.first() {
                apply_service_order_device(draft, service);
            }
        }
        PrestaMapMode::Bench => {
            if let Some(service) = service_details.first() {
                let local = draft.computer.clone();
                draft.computer = ComputerData {
                    device_name: Some(service.device_name.clone()),
                    device_mfg: Some(service.device_mfg.clone()),
                    device_model: Some(service.device_model.clone()),
                    device_serial: Some(service.device_serial.clone()),
                    customer: Some(draft.customer.id.clone()),
                    ..local
                };
            }
        }
    }

    draft.computer.customer = Some(draft.customer.id.clone());

    let guard = options.guard_placeholder_computer || options.mode == PrestaMapMode::Web;
    if guard {
        if computer_has_minimal_hardware(&draft.computer) && !is_placeholder_computer(&draft.computer) {
            draft.ticket.computer = Some(draft.computer.id.clone());
        }
    } else {
        draft.ticket.computer = Some(draft.computer.id.clone());
    }

    draft.task.service_ticket = Some(draft.ticket.id.clone());
}

pub fn apply_extracted_specs(draft: &mut EntityDraft, specs: &ExtractedOrderSpecs) {
    if !specs.cpu.is_empty() {
        draft.computer.cpu = specs.cpu.clone();
    }
    if !specs.gpu.is_empty() {
        draft.computer.gpu = specs.gpu.clone();
    }
    if !specs.ram.is_empty() {
        draft.computer.ram = specs.ram.clone();
    }
    if !specs.device_serial.is_empty() {
        draft.computer.id = RecordId::new(COMPUTER_TABLE, specs.device_serial.clone());
        draft.computer.device_serial = Some(specs.device_serial.clone());
    }
    if !specs.device_mfg.is_empty() {
        draft.computer.device_mfg = Some(specs.device_mfg.clone());
    }
    if computer_has_minimal_hardware(&draft.computer) && !is_placeholder_computer(&draft.computer) {
        draft.ticket.computer = Some(draft.computer.id.clone());
    }
}

fn apply_web_computer(
    data: &PrestashopPayload,
    draft: &mut EntityDraft,
    service_details: &[ServiceOrder],
) {
    let model = data.order.extract_model();
    if !model.is_empty() {
        draft.computer.device_model = Some(model);
    }
    for (name, drive_type) in data.order.extract_drives() {
        draft.computer.add_disk(DriveData {
            drive_letter: name.clone(),
            drive_type: drive_type.clone(),
            total_size: name.split('/').last().unwrap_or("").to_string(),
            space_left: String::new(),
        });
    }
    if let Some(mb) = data.order.extract_motherboard() {
        draft.computer.motherboard_name = mb;
    }
    if let Some(os) = data.order.extract_os() {
        draft.computer.operating_system = os;
    }
    if let Some(service) = service_details.first() {
        if draft
            .computer
            .device_mfg
            .as_ref()
            .map(|s| s.is_empty())
            .unwrap_or(true)
        {
            draft.computer.device_mfg = Some(service.device_mfg.clone());
        }
        if draft
            .computer
            .device_model
            .as_ref()
            .map(|s| s.is_empty())
            .unwrap_or(true)
        {
            draft.computer.device_model = Some(service.device_model.clone());
        }
    }
}

fn apply_service_order_device(draft: &mut EntityDraft, service: &ServiceOrder) {
    draft.computer.device_name = Some(service.device_name.clone());
    draft.computer.device_mfg = Some(service.device_mfg.clone());
    draft.computer.device_model = Some(service.device_model.clone());
    draft.computer.device_serial = Some(service.device_serial.clone());
}
