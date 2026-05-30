//! Operator modal to create or repair computer/customer links when MCP
//! validation fails or admin triggers manual linking.

use crate::modals::tabs::computer_page::display_computer_page;
use crate::open_service_suggestions::OpenServiceSuggestion;
use crate::{PlatformSpawner, Spawner};
use crossbeam::channel::{unbounded, Receiver, Sender};
use database::schema::entity_link::{
    canonical_computer_id, cascade_repoint_computer, delete_computer_if_unreferenced,
    parse_record_id, LinkValidationIssue,
};
use database::schema::service_match::{PrestaSpecsSnapshot, PrestashopCustomerMatch};
use database::schema::{
    utilities::get_prestashop_payload, ComputerData, CustomerData, RecordId, RecordIdExt,
    TicketData, COMPUTER_TABLE, CUSTOMER_TABLE,
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
    /// Built up from the cached PrestaShop suggestion + operator edits
    /// when the request carries a `MissingCustomer` / `CustomerNotFound`
    /// issue. Empty on pure computer-linking flows.
    customer: CustomerData,
    /// `id_order` from the cached `PrestashopCustomerMatch`, used to
    /// compose the `connected_client.friendly_name` field on commit so
    /// re-link decisions survive the next auto-detect run.
    customer_match_order: String,
    ticket: TicketData,
    customer_id: RecordId,
    old_computer_id: Option<RecordId>,
    status: String,
    error: String,
    commit_rx: Receiver<Result<(String, String), String>>,
    commit_tx: Sender<Result<(String, String), String>>,
    /// `received_at` of the most recently consumed
    /// `OpenServiceSuggestion`. The `draw()` poll only re-merges when
    /// a strictly newer snapshot lands, so a fresh response that
    /// arrives after the operator opened the modal still flows into
    /// the form, but a stale cached snapshot can't keep re-clobbering
    /// the same fields every frame.
    last_merged_at: Option<web_time::Instant>,
    /// Receiver for an async PrestaShop customer fetch — we kick this
    /// off when the operator clicks "Fetch customer from order" so the
    /// customer's email/phone (which aren't in the cached
    /// `PrestashopCustomerMatch`) flow in once the API call returns.
    customer_fetch_rx: Option<Receiver<Result<CustomerData, String>>>,
    /// The customer row already present in SurrealDB at the canonical
    /// `customer:<id_customer>` key (if any). Stored as a flexible
    /// snapshot rather than a strict `CustomerData` because legacy
    /// rows can have nulls / missing fields that the strict deserializer
    /// rejects — when that happens the operator still needs to see
    /// whatever fields DO parse so they can compare against the ticket.
    existing_customer: Option<ExistingCustomer>,
    /// Reason the existing-row fetch came back without an
    /// `existing_customer` — DB error, deserialize error, or "not
    /// found". Surfaces as a warning row on the Verify step so a
    /// missing comparison panel never silently misleads the operator.
    existing_customer_fetch_error: Option<String>,
    /// Receiver for the existing-customer DB fetch. Drained each
    /// frame by `poll_existing_customer_fetch`. The inner `Option`
    /// distinguishes "fetched but not found" (Some(None)) from "still
    /// loading" (channel hasn't fired).
    existing_customer_fetch_rx:
        Option<Receiver<Result<Option<ExistingCustomer>, String>>>,
}

/// Display snapshot of an existing `customer:<id>` row. Every field
/// is optional / String so we never reject a row because of schema
/// drift — the comparison grid renders `—` for missing values.
#[derive(Clone, Debug, Default)]
pub struct ExistingCustomer {
    pub record_id: String,
    pub cust_code: String,
    pub name: String,
    pub email: String,
    pub phone_number: String,
    pub phone_number_2: String,
}

impl ExistingCustomer {
    fn from_value(v: &serde_json::Value) -> Self {
        let s = |key: &str| -> String {
            v.get(key)
                .and_then(|val| val.as_str())
                .unwrap_or_default()
                .to_string()
        };
        // SurrealDB returns ids either as `{ tb, id }` objects or as
        // stringified records — accept either shape.
        let record_id = match v.get("id") {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Object(obj)) => {
                let tb = obj.get("tb").and_then(|x| x.as_str()).unwrap_or_default();
                let key = obj
                    .get("id")
                    .and_then(|x| {
                        if let Some(s) = x.as_str() {
                            Some(s.to_string())
                        } else if let Some(o) = x.as_object() {
                            o.get("String")
                                .and_then(|y| y.as_str())
                                .map(|s| s.to_string())
                                .or_else(|| {
                                    o.get("Number")
                                        .and_then(|n| n.as_i64())
                                        .map(|n| n.to_string())
                                })
                        } else {
                            None
                        }
                    })
                    .unwrap_or_default();
                if tb.is_empty() {
                    key
                } else {
                    format!("{tb}:{key}")
                }
            }
            _ => String::new(),
        };
        Self {
            record_id,
            cust_code: s("cust_code"),
            name: s("name"),
            email: s("email"),
            phone_number: s("phone_number"),
            phone_number_2: s("phone_number_2"),
        }
    }
}

impl EntityLinkResolutionModal {
    pub fn new(request: EntityLinkRequest) -> Self {
        let customer_id = parse_record_id(&request.customer_id, CUSTOMER_TABLE);
        let old_computer_id = parse_record_id(&request.computer_id, COMPUTER_TABLE);
        let connection_string = request.connection_string.clone().unwrap_or_default();

        let mut service_number = String::new();
        let mut computer = ComputerData::default();
        let mut customer = CustomerData::default();
        let mut customer_match_order = String::new();
        let mut last_merged_at: Option<web_time::Instant> = None;
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
                if let Some(m) = suggestion.match_.as_ref() {
                    merge_customer_match_into_customer(&mut customer, m);
                    customer_match_order = m.id_order.clone();
                }
                last_merged_at = Some(suggestion.received_at);
            }
            // Fire a fresh fetch so the hardware fields populate from
            // the client's own `ComputerData` / `SystemInformation`
            // (same source the tur sheet renders from) even if no
            // suggestion was cached yet. Silent no-op when no admin
            // session is registered for this connection — operator
            // opens the Web Console session and the next `draw()`
            // poll picks up the response when it lands.
            request_live_specs_refresh(&connection_string);
        }

        let (commit_tx, commit_rx) = unbounded();
        let mut me = Self {
            request,
            is_open: true,
            step: Step::Prompt,
            service_number,
            computer,
            customer,
            customer_match_order,
            ticket: TicketData::default(),
            customer_id,
            old_computer_id: Some(old_computer_id),
            status: String::new(),
            error: String::new(),
            commit_rx,
            commit_tx,
            last_merged_at,
            customer_fetch_rx: None,
            existing_customer: None,
            existing_customer_fetch_error: None,
            existing_customer_fetch_rx: None,
        };
        // If the cached PrestaShop match already gave us a canonical
        // customer id, eagerly fetch the existing DB row so the Verify
        // step can show a current-vs-ticket comparison.
        me.fire_existing_customer_fetch_if_needed();
        me
    }

    /// Kick off the async fetch for the existing `customer:<id>` row
    /// (or refresh it). No-op when the customer record id is still
    /// the random default that `CustomerData::default()` hands out —
    /// in that case there's no canonical row to look up yet.
    fn fire_existing_customer_fetch_if_needed(&mut self) {
        if self.customer.id.key_string().is_empty() {
            return;
        }
        if self.existing_customer_fetch_rx.is_some() {
            return;
        }
        if let Some(existing) = &self.existing_customer {
            // Skip if we already have the row for THIS id.
            let want = self.customer.id.key_string();
            if existing.record_id.split(':').nth(1).unwrap_or(&existing.record_id) == want {
                return;
            }
        }
        let (tx, rx) = unbounded::<Result<Option<ExistingCustomer>, String>>();
        self.existing_customer_fetch_rx = Some(rx);
        // Query both record id forms because the canonical
        // `customer:<id>` row may have been written with either a
        // string key (`customer:`147424``) or a numeric key
        // (`customer:147424`) depending on which code path created it.
        // The first form is what `RecordId::new("customer", "147424")`
        // produces; the second is what raw SurrealQL like
        // `CREATE customer:147424 …` produces. They are NOT the same
        // row in SurrealDB — different `Id` variants — but to a human
        // operator they're indistinguishable, so the existing-row
        // lookup should find whichever variant is present.
        let key_str = self.customer.id.key_string();
        // Parse via serde_json::Value so legacy rows with null /
        // missing fields don't trip the strict CustomerData deserializer.
        PlatformSpawner::spawn(async move {
            let outcome = fetch_existing_customer_lenient(&key_str).await;
            let _ = tx.try_send(outcome);
        });
    }

    fn poll_existing_customer_fetch(&mut self) {
        let Some(rx) = self.existing_customer_fetch_rx.as_ref() else {
            return;
        };
        let Ok(res) = rx.try_recv() else {
            return;
        };
        self.existing_customer_fetch_rx = None;
        match res {
            Ok(found) => {
                self.existing_customer = found;
                self.existing_customer_fetch_error = None;
            }
            Err(e) => {
                log::warn!("entity_link_modal: existing customer fetch failed: {e}");
                self.existing_customer = None;
                self.existing_customer_fetch_error = Some(e);
            }
        }
    }

    /// True when the validation issues for this request name the
    /// customer FK as the problem (missing FK on `connected_client`
    /// OR a stale FK that no longer points at a real customer row).
    fn needs_customer(&self) -> bool {
        self.request.issues.iter().any(|i| {
            matches!(i, LinkValidationIssue::MissingCustomer | LinkValidationIssue::CustomerNotFound)
        })
    }

    /// True when the validation issues name the computer FK as the
    /// problem. Kept symmetrical with `needs_customer` so the Prompt
    /// step renders the right set of action buttons.
    fn needs_computer(&self) -> bool {
        self.request.issues.iter().any(|i| {
            matches!(
                i,
                LinkValidationIssue::MissingComputer
                    | LinkValidationIssue::ComputerNotFound
                    | LinkValidationIssue::ComputerKeyNotCanonical { .. }
                    | LinkValidationIssue::ConnectedClientComputerMismatch { .. }
            )
        })
    }

    /// Re-merge from the global `open_service_suggestions` cache when
    /// a strictly newer snapshot has landed since the last merge.
    /// Called at the top of `draw()` so a response arriving after the
    /// modal opened still flows into the form fields. The merge
    /// itself uses "only fill empty" semantics so operator edits in
    /// the form are never clobbered by a subsequent poll.
    fn poll_live_specs(&mut self) {
        let Some(cs) = self.request.connection_string.as_deref() else {
            return;
        };
        let Some(suggestion) = crate::open_service_suggestions::get(cs) else {
            return;
        };
        if let Some(prev) = self.last_merged_at {
            if prev >= suggestion.received_at {
                return;
            }
        }
        merge_specs_into_computer(&mut self.computer, &suggestion);
        if let Some(m) = suggestion.match_.as_ref() {
            merge_customer_match_into_customer(&mut self.customer, m);
            if self.customer_match_order.is_empty() {
                self.customer_match_order = m.id_order.clone();
            }
            // The match supplied a canonical customer id; refresh the
            // existing-DB-row fetch so the comparison panel can show
            // both sides on the Verify step.
            self.fire_existing_customer_fetch_if_needed();
        }
        if self.service_number.is_empty() {
            if let Some(c) = suggestion.candidates.first() {
                self.service_number = c.service_number.clone();
            }
        }
        self.last_merged_at = Some(suggestion.received_at);
    }

    /// Drain the customer-fetch channel — if the async PrestaShop
    /// lookup completed, copy its richer email / phone fields into the
    /// form using "fill only empty" semantics so the operator's edits
    /// are never clobbered.
    fn poll_customer_fetch(&mut self) {
        let Some(rx) = self.customer_fetch_rx.as_ref() else {
            return;
        };
        let Ok(result) = rx.try_recv() else {
            return;
        };
        self.customer_fetch_rx = None;
        match result {
            Ok(fetched) => {
                // Adopt the fetched canonical (cust_code-keyed) customer id,
                // replacing the random CustomerData::default() id.
                if !fetched.id.key_string().is_empty()
                    && self.customer.id.key_string() != fetched.id.key_string()
                {
                    self.customer.id = fetched.id;
                }
                if !fetched.cust_code.is_empty() && self.customer.cust_code.is_empty() {
                    self.customer.cust_code = fetched.cust_code;
                }
                if !fetched.name.is_empty() && self.customer.name.is_empty() {
                    self.customer.name = fetched.name;
                }
                if !fetched.email.is_empty() && self.customer.email.is_empty() {
                    self.customer.email = fetched.email;
                }
                if !fetched.phone_number.is_empty() && self.customer.phone_number.is_empty() {
                    self.customer.phone_number = fetched.phone_number;
                }
                if !fetched.phone_number_2.is_empty() && self.customer.phone_number_2.is_empty() {
                    self.customer.phone_number_2 = fetched.phone_number_2;
                }
                self.status = "Customer details fetched from PrestaShop.".into();
            }
            Err(e) => {
                self.error = format!("Customer fetch failed: {e}");
            }
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
        // Pick up any fresh `OpenServiceCandidatesResponse` that landed
        // since the last frame (operator may have opened the Web
        // Console session after the modal was already open).
        self.poll_live_specs();
        self.poll_customer_fetch();
        self.poll_existing_customer_fetch();

        ui.label(
            RichText::new("Validation failed — link or create records before continuing.")
                .strong(),
        );
        for issue in &self.request.issues {
            ui.label(format!("• {issue:?}"));
        }
        ui.separator();

        let needs_customer = self.needs_customer();
        let needs_computer = self.needs_computer();

        match self.step {
            Step::Prompt => {
                if needs_customer {
                    ui.label("Create or repair the customer record for this connected client?");
                    if ui
                        .button("Yes — look up customer from service order")
                        .clicked()
                    {
                        self.step = Step::Lookup;
                    }
                }
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
                if !needs_customer && !needs_computer {
                    ui.colored_label(
                        Color32::LIGHT_YELLOW,
                        "No actionable validation issues — close to dismiss.",
                    );
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
                    let need_customer = needs_customer;
                    let (customer_tx, customer_rx) = unbounded::<Result<CustomerData, String>>();
                    self.customer_fetch_rx = if need_customer { Some(customer_rx) } else { None };
                    PlatformSpawner::spawn(async move {
                        // `PrestashopPayload` already carries the
                        // resolved customer object, so a single fetch
                        // gives us both the customer (name, email,
                        // phone) AND the order metadata. No second
                        // round-trip to `customers/{id}` needed.
                        match get_prestashop_payload(&sn).await {
                            Ok(payload) => {
                                if need_customer {
                                    let _ = customer_tx.try_send(Ok(payload.customer));
                                }
                            }
                            Err(e) => {
                                if need_customer {
                                    let _ = customer_tx.try_send(Err(format!(
                                        "PrestaShop order fetch failed: {e:?}"
                                    )));
                                }
                            }
                        }
                    });
                    self.status =
                        "PrestaShop fetch started — review on the verify step.".into();
                    self.step = Step::Verify;
                }
                if ui.button("Skip Presta — use cached data only").clicked() {
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
                        if needs_customer {
                            display_customer_compare(
                                ui,
                                self.existing_customer.as_ref(),
                                self.existing_customer_fetch_error.as_deref(),
                            );
                            ui.add_space(4.0);
                            display_customer_fields(ui, &mut self.customer);
                            ui.separator();
                        }
                        if needs_computer {
                            display_computer_page(
                                ui,
                                Some(&mut self.ticket),
                                Some(&mut self.computer),
                                Vec2::new(ui.available_width(), 400.0),
                            );
                        }
                    });
                ui.horizontal(|ui| {
                    if needs_customer {
                        // Two-button commit: "Link only" patches the
                        // connected_client FK without rewriting the
                        // customer row, useful when the existing DB
                        // record is already correct. "Update + link"
                        // applies the operator-edited fields with an
                        // UPDATE MERGE before linking.
                        let has_existing = self.existing_customer.is_some();
                        let link_only_label = if has_existing {
                            "Link existing customer (no changes)"
                        } else {
                            "Create customer + link"
                        };
                        if ui.button(link_only_label).clicked() {
                            self.spawn_commit(false);
                        }
                        if has_existing
                            && ui
                                .button("Update customer fields + link")
                                .clicked()
                        {
                            self.spawn_commit(true);
                        }
                    } else if needs_computer {
                        if ui.button("Confirm and save computer").clicked() {
                            self.spawn_commit(false);
                        }
                    } else if ui.button("Confirm").clicked() {
                        self.spawn_commit(false);
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

    fn spawn_commit(&mut self, update_customer: bool) {
        let cs = match self.request.connection_string.clone() {
            Some(s) if !s.is_empty() => s,
            _ => {
                self.error =
                    "connection_string required to create canonical computer record".into();
                return;
            }
        };

        let needs_customer = self.needs_customer();
        let needs_computer = self.needs_computer();
        let mut computer = self.computer.clone();
        let mut customer = self.customer.clone();
        let existing_customer = self.existing_customer.clone();
        let order_for_friendly = self.customer_match_order.clone();
        let service_number = self.service_number.clone();
        let prior_customer_id = self.customer_id.clone();
        let old_computer_id = self.old_computer_id.clone();
        let commit_tx = self.commit_tx.clone();
        self.status = "Saving…".into();
        self.error.clear();

        PlatformSpawner::spawn(async move {
            // ── Customer side ─────────────────────────────────────────
            // Two sub-cases when the validation flagged a missing
            // customer FK:
            //
            //   A. operator chose "Link only" → use the canonical id
            //      but do NOT rewrite the row at all. Skips the unique
            //      index check entirely; the FK patch below is what
            //      actually links the customer to the connected_client.
            //   B. operator chose "Update + link" / "Create + link" →
            //      `UPSERT $id MERGE { … }`. MERGE updates only the
            //      named fields without re-asserting the primary key,
            //      and UPSERT creates the row when it doesn't exist
            //      yet. Either way the unique `customer_code_idx`
            //      stays consistent because the cust_code value
            //      either matches what's already there (re-link of an
            //      existing row) or this is a brand-new row.
            //
            //   The earlier `.upsert(id).content(customer)` path was
            //   the bug — `.content()` rewrites every column including
            //   re-asserting the unique-indexed cust_code, which
            //   SurrealDB validates as if it were a brand-new write
            //   and rejects against the existing index entry on the
            //   same record. MERGE doesn't trip that check.
            let resolved_customer_id = if needs_customer {
                if customer.name.trim().is_empty() {
                    let _ = commit_tx.try_send(Err(
                        "Customer name required — fill the field on the verify step".into(),
                    ));
                    return;
                }
                if customer.cust_code.trim().is_empty() {
                    customer.cust_code = customer.id.key_string();
                }
                let cust_id = customer.id.clone();
                if update_customer || existing_customer.is_none() {
                    let merge_res: Result<(), surrealdb::Error> = DATABASE
                        .query(
                            "UPSERT $id MERGE { \
                                cust_code: $cust_code, \
                                name: $name, \
                                email: $email, \
                                phone_number: $phone, \
                                phone_number_2: $phone2 \
                            }",
                        )
                        .bind(("id", cust_id.clone()))
                        .bind(("cust_code", customer.cust_code.clone()))
                        .bind(("name", customer.name.clone()))
                        .bind(("email", customer.email.clone()))
                        .bind(("phone", customer.phone_number.clone()))
                        .bind(("phone2", customer.phone_number_2.clone()))
                        .await
                        .map(|_| ());
                    if let Err(e) = merge_res {
                        let _ = commit_tx
                            .try_send(Err(format!("customer UPSERT MERGE failed: {e}")));
                        return;
                    }
                } else {
                    log::info!(
                        "[entity_link] linking existing customer {cust_id:?} without changes"
                    );
                }
                cust_id
            } else {
                prior_customer_id.clone()
            };

            // ── Computer side ─────────────────────────────────────────
            if needs_computer {
                computer.id = canonical_computer_id(&cs);
                if computer.hostname.is_empty() {
                    if let Some((host, _)) = cs.split_once(':') {
                        computer.hostname = host.to_string();
                    }
                }
                computer.customer = Some(resolved_customer_id.clone());

                // Merge over the existing (live-client) row so suggestion specs
                // fill gaps without wiping hardware the client already reported.
                let mut merged = match DATABASE
                    .select::<Option<ComputerData>>(computer.id.clone())
                    .await
                {
                    Ok(Some(existing)) => existing,
                    _ => ComputerData {
                        id: computer.id.clone(),
                        ..ComputerData::default()
                    },
                };
                overlay_computer_specs(&mut merged, &computer);
                merged.id = computer.id.clone();
                merged.customer = Some(resolved_customer_id.clone());

                let upsert: Result<Option<ComputerData>, surrealdb::Error> = DATABASE
                    .upsert(merged.id.clone())
                    .content(merged.clone())
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
            } else if needs_customer {
                // Customer-only repair: also propagate the new customer
                // FK onto the existing computer row (if any) so
                // downstream task / service-order creation picks up
                // the right owner — mirrors the relink_popup behaviour.
                if let Some(ref old) = old_computer_id {
                    if !old.key_string().is_empty() {
                        let _: Result<(), surrealdb::Error> = DATABASE
                            .query("UPDATE $id SET customer = $cid")
                            .bind(("id", old.clone()))
                            .bind(("cid", resolved_customer_id.clone()))
                            .await
                            .map(|_| ());
                    }
                }
            }

            // ── connected_client row patch ─────────────────────────────
            // Always update both FKs in one statement when either side
            // changed. `customer_locked = true` matches relink_popup so
            // the auto-derived friendly_name from OA3 doesn't clobber
            // this assignment on the next reconnect.
            // Always "Name - Service#"; fall back to the typed service_number
            // when the cached match carried no order. Empty when no number is
            // available, which preserves the existing friendly_name instead of
            // downgrading it to a bare name.
            let order_suffix = if !order_for_friendly.trim().is_empty() {
                order_for_friendly.trim().to_string()
            } else {
                service_number.trim().to_string()
            };
            let friendly_name = if needs_customer
                && !customer.name.trim().is_empty()
                && !order_suffix.is_empty()
            {
                format!("{} - {}", customer.name.trim(), order_suffix)
            } else {
                String::new()
            };
            let write_friendly = !friendly_name.is_empty();

            let cc_sql = match (needs_computer, needs_customer, write_friendly) {
                (true, true, true) => {
                    "UPDATE connected_client SET computer = $compid, customer = $custid, \
                     friendly_name = $name, customer_locked = true \
                     WHERE connection_string == $cs"
                }
                (true, true, false) => {
                    "UPDATE connected_client SET computer = $compid, customer = $custid, \
                     customer_locked = true WHERE connection_string == $cs"
                }
                (true, false, _) => {
                    "UPDATE connected_client SET computer = $compid WHERE connection_string == $cs"
                }
                (false, _, true) => {
                    "UPDATE connected_client SET customer = $custid, computer = $compid, \
                     friendly_name = $name, customer_locked = true \
                     WHERE connection_string == $cs"
                }
                (false, _, false) => {
                    "UPDATE connected_client SET customer = $custid, computer = $compid, \
                     customer_locked = true WHERE connection_string == $cs"
                }
            };
            let _: Result<(), surrealdb::Error> = DATABASE
                .query(cc_sql)
                .bind(("compid", computer.id.clone()))
                .bind(("custid", resolved_customer_id.clone()))
                .bind(("name", friendly_name))
                .bind(("cs", cs.clone()))
                .await
                .map(|_| ());

            let _ = commit_tx.try_send(Ok((
                resolved_customer_id.key_string(),
                computer.id.key_string(),
            )));
        });
    }
}

/// Overlay non-empty hardware fields from `src` onto `dst`, leaving
/// existing values where `src` carries none.
fn overlay_computer_specs(dst: &mut ComputerData, src: &ComputerData) {
    if !src.cpu.is_empty() {
        dst.cpu = src.cpu.clone();
    }
    if !src.gpu.is_empty() {
        dst.gpu = src.gpu.clone();
    }
    if !src.ram.is_empty() {
        dst.ram = src.ram.clone();
    }
    if !src.operating_system.is_empty() {
        dst.operating_system = src.operating_system.clone();
    }
    if !src.motherboard_name.is_empty() {
        dst.motherboard_name = src.motherboard_name.clone();
    }
    if !src.hostname.is_empty() {
        dst.hostname = src.hostname.clone();
    }
    if src.device_serial.as_ref().is_some_and(|s| !s.is_empty()) {
        dst.device_serial = src.device_serial.clone();
    }
    if src.device_mfg.as_ref().is_some_and(|s| !s.is_empty()) {
        dst.device_mfg = src.device_mfg.clone();
    }
    if src.device_model.as_ref().is_some_and(|s| !s.is_empty()) {
        dst.device_model = src.device_model.clone();
    }
}

/// Copy the cached `PrestashopCustomerMatch` into a fresh
/// `CustomerData`. Only fills empty fields (matches the spec-merge
/// pattern) so a re-merge after the operator edits the form doesn't
/// clobber their changes.
fn merge_customer_match_into_customer(
    customer: &mut CustomerData,
    m: &PrestashopCustomerMatch,
) {
    if !m.id_customer.is_empty() {
        let new_id = RecordId::new(CUSTOMER_TABLE, m.id_customer.as_str());
        // Replace the random default id with the canonical PrestaShop
        // customer key so a re-open of the same client doesn't fork.
        if customer.id.key_string() != m.id_customer {
            customer.id = new_id;
        }
    }
    if customer.cust_code.is_empty() {
        customer.cust_code = m.id_customer.clone();
    }
    if customer.name.is_empty() {
        let combined = format!("{} {}", m.first_name.trim(), m.last_name.trim())
            .trim()
            .to_string();
        if !combined.is_empty() {
            customer.name = combined;
        } else if !m.friendly_name.is_empty() {
            customer.name = m.friendly_name.clone();
        }
    }
}

/// Side-by-side panel that contrasts the existing `customer:<id>` row
/// in SurrealDB (if any) against the PrestaShop ticket data the
/// operator pulled. Lets them spot stale fields before deciding
/// whether to link as-is or MERGE the ticket values in.
fn display_customer_compare(
    ui: &mut Ui,
    existing: Option<&ExistingCustomer>,
    fetch_error: Option<&str>,
) {
    ui.label(RichText::new("Compare").strong());
    eframe::egui::Grid::new("entity_link_customer_compare")
        .num_columns(3)
        .spacing([12.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            ui.label("");
            ui.label(RichText::new("In SurrealDB").strong());
            ui.label(RichText::new("From PrestaShop ticket").strong());
            ui.end_row();

            for (label, db_val) in compare_rows(existing) {
                ui.label(label);
                ui.label(db_val);
                ui.label(
                    RichText::new("(see editable form below)")
                        .color(Color32::from_rgb(120, 120, 140))
                        .small(),
                );
                ui.end_row();
            }
        });
    if let Some(err) = fetch_error {
        ui.colored_label(
            Color32::LIGHT_RED,
            format!("Could not load existing customer row: {err}"),
        );
        ui.colored_label(
            Color32::from_rgb(180, 180, 90),
            "Falling back to UPSERT MERGE — Confirm will write only the listed fields, \
             so other columns on the existing row are preserved.",
        );
    } else if existing.is_none() {
        ui.colored_label(
            Color32::from_rgb(180, 180, 90),
            "No existing customer row found in SurrealDB at this id — Confirm will create one.",
        );
    }
}

fn compare_rows(existing: Option<&ExistingCustomer>) -> Vec<(&'static str, String)> {
    let blank = "—".to_string();
    let by = |f: fn(&ExistingCustomer) -> &str| -> String {
        existing
            .map(|c| {
                let v = f(c).trim();
                if v.is_empty() { blank.clone() } else { v.to_string() }
            })
            .unwrap_or_else(|| blank.clone())
    };
    vec![
        (
            "Record id",
            existing
                .map(|c| c.record_id.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| blank.clone()),
        ),
        ("Customer code", by(|c| &c.cust_code)),
        ("Name", by(|c| &c.name)),
        ("Email", by(|c| &c.email)),
        ("Phone", by(|c| &c.phone_number)),
        ("Phone (alt)", by(|c| &c.phone_number_2)),
    ]
}

/// Lenient SELECT against `customer:<key>` that survives schema drift
/// on the existing row (legacy customers with `null` fields, missing
/// `cust_code`, etc.). Returns:
///   Ok(Some(...)) when at least one row matched
///   Ok(None)      when no row matched at this id
///   Err(...)      only on a real DB-level failure (transport, syntax)
async fn fetch_existing_customer_lenient(
    key: &str,
) -> Result<Option<ExistingCustomer>, String> {
    // String key first — what `RecordId::new("customer", "147424")`
    // produces. We pass the key as a bound param so SurrealDB doesn't
    // interpret backticks / colons in the key as syntax.
    let rid = RecordId::new(CUSTOMER_TABLE, key);
    let rows: Vec<serde_json::Value> = DATABASE
        .query("SELECT * FROM $id")
        .bind(("id", rid))
        .await
        .map_err(|e| e.to_string())?
        .take(0)
        .map_err(|e| e.to_string())?;
    if let Some(row) = rows.first() {
        return Ok(Some(ExistingCustomer::from_value(row)));
    }
    // Fallback: try the numeric-key variant. PrestaShop ids are
    // numeric, and rows created from older code paths or raw SurrealQL
    // `CREATE customer:147424 …` land under `Id::Number(147424)`,
    // which is a DIFFERENT record from the string-keyed
    // `customer:`147424``. Try parsing the key as an integer and look
    // for that record id; this is the only way to find legacy rows.
    if let Ok(n) = key.parse::<i64>() {
        let q = format!("SELECT * FROM customer:{n}");
        let rows: Vec<serde_json::Value> = DATABASE
            .query(q)
            .await
            .map_err(|e| e.to_string())?
            .take(0)
            .map_err(|e| e.to_string())?;
        if let Some(row) = rows.first() {
            return Ok(Some(ExistingCustomer::from_value(row)));
        }
    }
    Ok(None)
}

/// Compact customer form for the Verify step. Mirrors the field set
/// the relink_popup commits (name / phone / email / cust_code) so the
/// operator has the same surface here without leaving the modal.
fn display_customer_fields(ui: &mut Ui, customer: &mut CustomerData) {
    ui.label(RichText::new("Customer").strong());
    let label_w = 110.0;
    let field_w = 360.0;
    ui.horizontal(|ui| {
        ui.add_sized([label_w, 20.0], eframe::egui::Label::new("Customer code"));
        ui.add_sized(
            [field_w, 20.0],
            TextEdit::singleline(&mut customer.cust_code),
        );
    });
    ui.horizontal(|ui| {
        ui.add_sized([label_w, 20.0], eframe::egui::Label::new("Name"));
        ui.add_sized(
            [field_w, 20.0],
            TextEdit::singleline(&mut customer.name),
        );
    });
    ui.horizontal(|ui| {
        ui.add_sized([label_w, 20.0], eframe::egui::Label::new("Email"));
        ui.add_sized(
            [field_w, 20.0],
            TextEdit::singleline(&mut customer.email),
        );
    });
    ui.horizontal(|ui| {
        ui.add_sized([label_w, 20.0], eframe::egui::Label::new("Phone"));
        ui.add_sized(
            [field_w, 20.0],
            TextEdit::singleline(&mut customer.phone_number),
        );
    });
    ui.horizontal(|ui| {
        ui.add_sized([label_w, 20.0], eframe::egui::Label::new("Phone (alt)"));
        ui.add_sized(
            [field_w, 20.0],
            TextEdit::singleline(&mut customer.phone_number_2),
        );
    });
}

fn merge_specs_into_computer(computer: &mut ComputerData, suggestion: &OpenServiceSuggestion) {
    if let Some(live) = suggestion.live_specs.as_ref() {
        merge_live_specs_into_computer(computer, live);
    }
    if let Some(c) = suggestion.candidates.first() {
        merge_presta_specs(computer, &c.specs);
    }
}

/// Copy the client's reported `SystemInformation` (same data source
/// the tur sheet renders from on the client) into a `ComputerData`
/// for the link/repair modal. "Only fill empty" so the operator's
/// edits in the form aren't overwritten when a later poll re-merges.
fn merge_live_specs_into_computer(
    computer: &mut ComputerData,
    live: &database::schema::SystemInformation,
) {
    if computer.hostname.is_empty() && !live.hostname.is_empty() {
        computer.hostname = live.hostname.clone();
    }
    if computer.operating_system.is_empty() {
        // `live.os_version` on Windows is just the build string —
        // e.g. `"11 (26200)"` — because `System::os_version()` from
        // sysinfo drops the OS name. Prepend `live.name` (which is
        // `"Windows"` / `"Linux"` / `"macOS"`) so the form shows
        // `"Windows 11 (26200)"` instead of a bare version number.
        let formatted = compose_os_label(&live.name, &live.os_version);
        if !formatted.is_empty() {
            computer.operating_system = formatted;
        }
    }
    if computer.cpu.is_empty() && !live.cpu.is_empty() {
        computer.cpu = live.cpu.clone();
    }
    if computer.gpu.is_empty() {
        if let Some(card) = live.gpu_info.card.first() {
            if !card.name.is_empty() {
                // Use the full marketing name. The old `take(3)`
                // formatting matched a now-removed client-side
                // truncation and chopped `NVIDIA GeForce RTX 5060`
                // down to `NVIDIA GeForce RTX`; the merged form is
                // operator-facing and needs the model number.
                computer.gpu = card.name.trim().to_string();
            }
        }
    }
    if computer.ram.is_empty() && live.total_memory > 0.0 {
        // `SystemInformation.total_memory` is MiB (see
        // `Mastertech4.0/src/filesystem/system_info.rs:497` —
        // `sys.total_memory() as f32 / (1024 * 1024)`). Old code
        // divided by 1024³ as if it were bytes, which gave `1 Gb`
        // for every machine under 1 PiB. Round to the nearest GiB.
        let gb = (live.total_memory as f64 / 1024.0).round().max(1.0) as u64;
        computer.ram = format!("{gb} GB");
    }
    // Device-* fields surface in the modal as "Device Mfg / Model /
    // Serial". The client reports these through `product_*` on
    // `SystemInformation` (BIOS DMI / OEM strings), so we map across
    // field names — the original `device_name`/`device_mfg` slots
    // were intended for OEM-reported identity.
    //
    // BIOS DMI strings are notorious for shipping placeholder text
    // ("To Be Filled By O.E.M.", "Default string", "Not Applicable",
    // etc.) instead of the real OEM serial. We treat those as empty
    // so the PrestaShop `device_serial` (which is the source of
    // truth — the operator scanned it at intake) wins the merge.
    if computer
        .device_serial
        .as_ref()
        .is_none_or(|s| s.is_empty())
        && !is_placeholder_dmi(&live.product_serial)
    {
        computer.device_serial = Some(live.product_serial.clone());
    }
    if computer.device_mfg.as_ref().is_none_or(|s| s.is_empty())
        && !is_placeholder_dmi(&live.product_vendor)
    {
        computer.device_mfg = Some(live.product_vendor.clone());
    }
    if computer
        .device_model
        .as_ref()
        .is_none_or(|s| s.is_empty())
        && !is_placeholder_dmi(&live.product_name)
    {
        computer.device_model = Some(live.product_name.clone());
    }
    if computer.product_name.is_empty() && !is_placeholder_dmi(&live.product_name) {
        computer.product_name = live.product_name.clone();
    }
    if computer.product_sku.is_empty() && !is_placeholder_dmi(&live.product_sku) {
        computer.product_sku = live.product_sku.clone();
    }
    if computer.product_serial.is_empty() && !is_placeholder_dmi(&live.product_serial) {
        computer.product_serial = live.product_serial.clone();
    }
    if computer.product_vendor.is_empty() && !is_placeholder_dmi(&live.product_vendor) {
        computer.product_vendor = live.product_vendor.clone();
    }
    if computer.motherboard_name.is_empty() && !live.motherboard_name.is_empty() {
        computer.motherboard_name = live.motherboard_name.clone();
    }
    if computer.motherboard_serial.is_empty() && !live.motherboard_serial.is_empty() {
        computer.motherboard_serial = live.motherboard_serial.clone();
    }
    if computer.motherboard_asset_tag.is_empty()
        && !live.motherboard_asset_tag.is_empty()
    {
        computer.motherboard_asset_tag = live.motherboard_asset_tag.clone();
    }
    if computer.motherboard_vendor.is_empty() && !live.motherboard_vendor.is_empty() {
        computer.motherboard_vendor = live.motherboard_vendor.clone();
    }
}

/// Fire `Cmd::RequestOpenServiceCandidates { refresh: false }` at the
/// connected client so it returns its current `SystemInformation`
/// (which carries live cpu/gpu/ram/etc) into the
/// `open_service_suggestions` cache. The next `draw()` poll picks it
/// up and merges into the form. Silent no-op when the hub has no
/// admin transport registered for `connection_string` — the operator
/// can open the Web Console session for that client and the response
/// will flow in then.
fn request_live_specs_refresh(connection_string: &str) {
    let cmd = crate::Cmd::RequestOpenServiceCandidates { refresh: false };
    let bytes = match bincode::serde::encode_to_vec(&cmd, bincode::config::standard()) {
        Ok(v) => v,
        Err(e) => {
            log::warn!(
                "entity_link_modal: encode RequestOpenServiceCandidates failed: {e}"
            );
            return;
        }
    };
    if let Err(e) = crate::plugins::remote_egui_control::hub()
        .send_raw_binary(connection_string, bytes)
    {
        log::debug!(
            "entity_link_modal: live-specs refresh skipped for {connection_string}: {e}"
        );
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

/// Combine `System::name()` ("Windows" / "Linux" / "macOS") with the
/// bare `os_version` string sysinfo returns ("11 (26200)") into a form
/// that's useful in the modal ("Windows 11 (26200)"). If the version
/// already starts with the OS name (some Linux distros embed it), we
/// skip the prepend so we don't get "Linux Linux 6.10".
fn compose_os_label(name: &str, version: &str) -> String {
    let name = name.trim();
    let version = version.trim();
    if version.is_empty() {
        return name.to_string();
    }
    if name.is_empty() {
        return version.to_string();
    }
    let v_lower = version.to_lowercase();
    let n_lower = name.to_lowercase();
    if v_lower.starts_with(&n_lower) {
        version.to_string()
    } else {
        format!("{name} {version}")
    }
}

/// BIOS DMI / OEM strings that mean "the OEM didn't fill this in" and
/// should be treated as empty so PrestaShop / operator-supplied values
/// win the merge instead of being blocked by a junk live reading.
///
/// Sourced from real-world OEM defaults (Lenovo, HP, ASUS, MSI, ASRock,
/// random whiteboxes). All comparisons are case-insensitive after
/// trimming whitespace.
fn is_placeholder_dmi(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "to be filled by o.e.m."
            | "to be filled by oem"
            | "default string"
            | "not applicable"
            | "not specified"
            | "no enclosure"
            | "none"
            | "n/a"
            | "na"
            | "system serial number"
            | "system product name"
            | "system manufacturer"
            | "system version"
            | "system sku"
            | "chassis manufacture"
            | "chassis version"
            | "chassis serial number"
            | "0"
            | "00000000"
            | "unknown"
    )
}
