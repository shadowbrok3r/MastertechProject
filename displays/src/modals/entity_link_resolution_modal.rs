//! Operator modal to create or repair computer/customer links when MCP
//! validation fails or admin triggers manual linking.

use crate::modals::tabs::computer_page::display_computer_page;
use crate::open_service_suggestions::OpenServiceSuggestion;
use crate::{PlatformSpawner, Spawner};
use crossbeam::channel::{unbounded, Receiver, Sender};
use database::schema::entity_link::{
    canonical_computer_id, cascade_repoint_computer, delete_computer_if_unreferenced,
    parse_record_id,
};
use database::schema::service_match::PrestaSpecsSnapshot;
use database::schema::{
    utilities::get_prestashop_payload, ComputerData, RecordId, RecordIdExt, TicketData,
    COMPUTER_TABLE, CUSTOMER_TABLE,
};
use database::DATABASE;
use eframe::egui::{Color32, Context, RichText, ScrollArea, TextEdit, Ui, Vec2, Window};

use crate::plugins::entity_link_pending::{
    resolve_entity_link_request, EntityLinkOutcome, EntityLinkRequest,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Prompt,
    Lookup,
    Verify,
}

pub struct EntityLinkResolutionModal {
    pub request: EntityLinkRequest,
    pub is_open: bool,
    step: Step,
    service_number: String,
    computer: ComputerData,
    ticket: TicketData,
    customer_id: RecordId,
    old_computer_id: Option<RecordId>,
    status: String,
    error: String,
    commit_rx: Receiver<Result<(String, String), String>>,
    commit_tx: Sender<Result<(String, String), String>>,
}

impl EntityLinkResolutionModal {
    pub fn new(request: EntityLinkRequest) -> Self {
        let customer_id = parse_record_id(&request.customer_id, CUSTOMER_TABLE);
        let old_computer_id = parse_record_id(&request.computer_id, COMPUTER_TABLE);
        let connection_string = request.connection_string.clone().unwrap_or_default();

        let mut service_number = String::new();
        let mut computer = ComputerData::default();
        if !connection_string.is_empty() {
            computer.id = canonical_computer_id(&connection_string);
            if let Some((host, _)) = connection_string.split_once(':') {
                computer.hostname = host.to_string();
            }
            if let Some(suggestion) = crate::open_service_suggestions::get(&connection_string) {
                if let Some(c) = suggestion.candidates.first() {
                    service_number = c.service_number.clone();
                }
                merge_specs_into_computer(&mut computer, &suggestion);
            }
        }

        let (commit_tx, commit_rx) = unbounded();
        Self {
            request,
            is_open: true,
            step: Step::Prompt,
            service_number,
            computer,
            ticket: TicketData::default(),
            customer_id,
            old_computer_id: Some(old_computer_id),
            status: String::new(),
            error: String::new(),
            commit_rx,
            commit_tx,
        }
    }

    pub fn show(&mut self, ctx: &Context) -> Option<EntityLinkOutcome> {
        if !self.is_open {
            return None;
        }

        if let Ok(result) = self.commit_rx.try_recv() {
            return match result {
                Ok((customer_id, computer_id)) => {
                    let outcome = EntityLinkOutcome::Resolved {
                        customer_id,
                        computer_id,
                    };
                    resolve_entity_link_request(&self.request.request_id, outcome.clone());
                    Some(outcome)
                }
                Err(e) => {
                    self.error = e;
                    None
                }
            };
        }

        let mut close = false;
        let mut outcome = None;
        Window::new("Link customer / computer records")
            .collapsible(false)
            .resizable(true)
            .default_size([720.0, 640.0])
            .show(ctx, |ui| {
                outcome = self.draw(ui, &mut close);
            });
        if close {
            self.is_open = false;
            if outcome.is_none() {
                let cancelled = EntityLinkOutcome::Cancelled {
                    reason: "closed".into(),
                };
                resolve_entity_link_request(&self.request.request_id, cancelled.clone());
                outcome = Some(cancelled);
            }
        }
        outcome
    }

    fn draw(&mut self, ui: &mut Ui, close: &mut bool) -> Option<EntityLinkOutcome> {
        ui.label(
            RichText::new("Validation failed — link or create records before continuing.")
                .strong(),
        );
        for issue in &self.request.issues {
            ui.label(format!("• {issue:?}"));
        }
        ui.separator();

        match self.step {
            Step::Prompt => {
                let needs_computer = self.request.issues.iter().any(|i| {
                    use database::schema::entity_link::LinkValidationIssue::*;
                    matches!(
                        i,
                        MissingComputer
                            | ComputerNotFound
                            | ComputerKeyNotCanonical { .. }
                            | ConnectedClientComputerMismatch { .. }
                    )
                });
                if needs_computer {
                    ui.label(
                        "Create or repair the computer record for this connected client?",
                    );
                    if ui
                        .button("Yes — look up service order and use client hardware")
                        .clicked()
                    {
                        self.step = Step::Lookup;
                    }
                }
                if ui.button("Cancel").clicked() {
                    *close = true;
                    return Some(EntityLinkOutcome::Cancelled {
                        reason: "operator cancelled".into(),
                    });
                }
            }
            Step::Lookup => {
                ui.label("Service order # (PrestaShop):");
                ui.add(TextEdit::singleline(&mut self.service_number).desired_width(200.0));
                if ui.button("Fetch PrestaShop order").clicked()
                    && !self.service_number.is_empty()
                {
                    let sn = self.service_number.clone();
                    let commit_tx = self.commit_tx.clone();
                    PlatformSpawner::spawn(async move {
                        match get_prestashop_payload(&sn).await {
                            Ok(_payload) => {
                                let _ = commit_tx.try_send(Err(
                                    "Presta merge: edit specs on verify step (fetch ok)"
                                        .into(),
                                ));
                            }
                            Err(e) => {
                                let _ = commit_tx.try_send(Err(format!(
                                    "PrestaShop fetch failed: {e:?}"
                                )));
                            }
                        }
                    });
                    self.status =
                        "PrestaShop fetch started — review specs on the verify step.".into();
                    self.step = Step::Verify;
                }
                if ui.button("Skip Presta — use client hardware only").clicked() {
                    self.step = Step::Verify;
                }
                if ui.button("Back").clicked() {
                    self.step = Step::Prompt;
                }
            }
            Step::Verify => {
                let mut cancelled = false;
                ui.label(RichText::new("Verify this information looks correct").strong());
                if !self.status.is_empty() {
                    ui.label(RichText::new(&self.status).color(Color32::LIGHT_BLUE));
                }
                if !self.error.is_empty() {
                    ui.label(RichText::new(&self.error).color(Color32::LIGHT_RED));
                }
                ScrollArea::vertical()
                    .max_height(400.0)
                    .show(ui, |ui| {
                        display_computer_page(
                            ui,
                            Some(&mut self.ticket),
                            Some(&mut self.computer),
                            Vec2::new(ui.available_width(), 400.0),
                        );
                    });
                ui.horizontal(|ui| {
                    if ui.button("Confirm and save").clicked() {
                        self.spawn_commit();
                    }
                    if ui.button("Back").clicked() {
                        self.step = Step::Lookup;
                    }
                    if ui.button("Cancel").clicked() {
                        cancelled = true;
                        *close = true;
                    }
                });
                if cancelled {
                    return Some(EntityLinkOutcome::Cancelled {
                        reason: "operator cancelled".into(),
                    });
                }
            }
        }
        None
    }

    fn spawn_commit(&mut self) {
        let cs = match self.request.connection_string.clone() {
            Some(s) if !s.is_empty() => s,
            _ => {
                self.error =
                    "connection_string required to create canonical computer record".into();
                return;
            }
        };
        let mut computer = self.computer.clone();
        let customer_id = self.customer_id.clone();
        let old_computer_id = self.old_computer_id.clone();
        let commit_tx = self.commit_tx.clone();
        self.status = "Saving…".into();
        self.error.clear();

        PlatformSpawner::spawn(async move {
            computer.id = canonical_computer_id(&cs);
            if computer.hostname.is_empty() {
                if let Some((host, _)) = cs.split_once(':') {
                    computer.hostname = host.to_string();
                }
            }
            computer.customer = Some(customer_id.clone());

            let upsert: Result<Option<ComputerData>, surrealdb::Error> = DATABASE
                .upsert(computer.id.clone())
                .content(computer.clone())
                .await;

            if let Err(e) = upsert {
                let _ = commit_tx.try_send(Err(format!("computer upsert failed: {e}")));
                return;
            }

            if let Some(ref old) = old_computer_id {
                if old.key_string() != computer.id.key_string() {
                    let _ = cascade_repoint_computer(old, &computer.id).await;
                    let _ = delete_computer_if_unreferenced(old).await;
                }
            }

            let _: Result<(), surrealdb::Error> = DATABASE
                .query(
                    "UPDATE connected_client SET computer = $cid \
                     WHERE connection_string == $cs",
                )
                .bind(("cid", computer.id.clone()))
                .bind(("cs", cs.clone()))
                .await
                .map(|_| ());

            let _ = commit_tx.try_send(Ok((
                customer_id.key_string(),
                computer.id.key_string(),
            )));
        });
    }
}

fn merge_specs_into_computer(computer: &mut ComputerData, suggestion: &OpenServiceSuggestion) {
    if let Some(live) = suggestion.live_specs.as_ref() {
        if !live.hostname.is_empty() {
            computer.hostname = live.hostname.clone();
        }
        if !live.os_version.is_empty() {
            computer.operating_system = live.os_version.clone();
        }
        if !live.product_serial.is_empty() {
            computer.device_serial = Some(live.product_serial.clone());
        }
        if !live.product_vendor.is_empty() {
            computer.device_mfg = Some(live.product_vendor.clone());
        }
        if !live.product_name.is_empty() {
            computer.device_model = Some(live.product_name.clone());
        }
        if !live.motherboard_name.is_empty() {
            computer.motherboard_name = live.motherboard_name.clone();
        }
    }
    if let Some(c) = suggestion.candidates.first() {
        merge_presta_specs(computer, &c.specs);
    }
}

fn merge_presta_specs(computer: &mut ComputerData, specs: &PrestaSpecsSnapshot) {
    if computer.cpu.is_empty() && !specs.cpu.is_empty() {
        computer.cpu = specs.cpu.clone();
    }
    if computer.gpu.is_empty() && !specs.gpu.is_empty() {
        computer.gpu = specs.gpu.clone();
    }
    if computer.ram.is_empty() && !specs.ram.is_empty() {
        computer.ram = specs.ram.clone();
    }
    if computer
        .device_serial
        .as_ref()
        .is_none_or(|s| s.is_empty())
        && !specs.device_serial.is_empty()
    {
        computer.device_serial = Some(specs.device_serial.clone());
    }
    if computer
        .device_mfg
        .as_ref()
        .is_none_or(|s| s.is_empty())
        && !specs.device_mfg.is_empty()
    {
        computer.device_mfg = Some(specs.device_mfg.clone());
    }
    if computer
        .device_model
        .as_ref()
        .is_none_or(|s| s.is_empty())
        && !specs.device_model.is_empty()
    {
        computer.device_model = Some(specs.device_model.clone());
    }
    if computer.operating_system.is_empty() && !specs.operating_system.is_empty() {
        computer.operating_system = specs.operating_system.clone();
    }
    if computer.motherboard_name.is_empty() && !specs.motherboard_name.is_empty() {
        computer.motherboard_name = specs.motherboard_name.clone();
    }
}
