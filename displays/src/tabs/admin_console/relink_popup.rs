//! Manual customer re-link popup.
//!
//! Used machines we've resold inherit a `friendly_name` derived from the
//! Windows OA3 product key, which resolves to the *original* purchaser
//! and gets re-applied on every reconnect (see `create_client` in
//! `terminal_mode/websockets/mod.rs`). This popup lets an admin search
//! for the *current* customer by phone, email, or order number, then
//! commits the new linkage to:
//!
//!  - `connected_client` row: `customer` + `friendly_name` +
//!    `customer_locked = true` (the lock flag is what stops the
//!    auto-derived name from clobbering the admin's choice on next
//!    reconnect).
//!  - The associated `computer` row (if the connected_client has one
//!    linked), updating its `customer` field so downstream task /
//!    service-order creation picks up the right owner.
//!
//! Phone and email lookups can return multiple matches, so the popup
//! shows a results list and the admin picks the right one before the
//! Apply step runs the prestashop payload fetch + DB commit.

use crate::{PlatformSpawner, Spawner};
use crossbeam::channel::{unbounded, Receiver, Sender};
use database::{
    schema::{
        prestashop::{Address, Customer, PrestashopPayload},
        utilities::{get_prestashop_payload, get_prestashop_payload_from_phone},
        ConnectedClient, COMPUTER_TABLE, CONNECTED_CLIENT_TABLE,
    },
    db,
};
use eframe::egui::{
    self, Align, Button, Layout, RichText, ScrollArea, TextEdit, Ui, Window,
};
use crate::ui_tools::theme;

/// Which lookup the admin is performing. Only one input is active at a
/// time so we don't have to disambiguate when multiple are filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelinkSearchKind {
    Phone,
    Email,
    OrderNumber,
}

impl Default for RelinkSearchKind {
    fn default() -> Self {
        Self::Phone
    }
}

impl RelinkSearchKind {
    fn label(self) -> &'static str {
        match self {
            Self::Phone => "Phone",
            Self::Email => "Email",
            Self::OrderNumber => "Order #",
        }
    }
    fn hint(self) -> &'static str {
        match self {
            Self::Phone => "e.g. 555-123-4567",
            Self::Email => "customer@example.com",
            Self::OrderNumber => "e.g. 41234",
        }
    }
}

/// One row in the search-results list. Phone/email lookups produce multiple
/// of these (one per matched customer); order-# lookups produce exactly one.
#[derive(Debug, Clone)]
pub struct RelinkCandidate {
    pub display_label: String,
    pub customer_id: String,
    pub firstname: String,
    pub lastname: String,
    pub email: String,
    pub phone: String,
    /// Order number used for the friendly_name suffix. When the search
    /// kind was "order #" this is exactly what the admin typed; for
    /// phone/email lookups it's filled in only after the Apply step
    /// fetches the customer's most recent order.
    pub order_number: Option<String>,
}

impl RelinkCandidate {
    fn from_prestashop(c: &Customer, addr: &Address) -> Self {
        let phone = if !addr.phone.is_empty() {
            addr.phone.clone()
        } else {
            addr.phone_mobile.clone()
        };
        Self {
            display_label: format!(
                "{} {} <{}> {}",
                c.firstname.trim(),
                c.lastname.trim(),
                c.email,
                phone
            ),
            customer_id: c.id.clone(),
            firstname: c.firstname.trim().to_string(),
            lastname: c.lastname.trim().to_string(),
            email: c.email.clone(),
            phone,
            order_number: None,
        }
    }

    fn from_payload(p: &PrestashopPayload) -> Self {
        Self {
            display_label: format!(
                "{} <{}> — order {}",
                p.customer.name, p.customer.email, p.order.id
            ),
            customer_id: p.customer.cust_code.clone(),
            firstname: p.customer.name.split_whitespace().next().unwrap_or("").to_string(),
            lastname: p
                .customer
                .name
                .split_whitespace()
                .skip(1)
                .collect::<Vec<_>>()
                .join(" "),
            email: p.customer.email.clone(),
            phone: p.customer.phone_number.clone(),
            order_number: Some(p.order.id.clone()),
        }
    }
}

/// Background-task event surfaced to the popup's UI poll.
#[derive(Debug)]
pub enum RelinkEvent {
    SearchResults(Vec<RelinkCandidate>),
    SearchError(String),
    PayloadReady {
        candidate: RelinkCandidate,
        payload: PrestashopPayload,
    },
    PayloadError(String),
    ApplyOk,
    ApplyError(String),
}

/// Popup state. One instance per AdminConsole; `client` is `Some(_)` while
/// the popup is open, `None` when closed.
pub struct RelinkClientPopup {
    pub client: ConnectedClient,
    pub kind: RelinkSearchKind,
    pub query_input: String,
    pub results: Vec<RelinkCandidate>,
    pub selected: Option<usize>,
    pub status: String,
    pub busy: bool,
    pub channel: (Sender<RelinkEvent>, Receiver<RelinkEvent>),
    /// `true` after Apply succeeds, so the parent can close us.
    pub done: bool,
}

impl RelinkClientPopup {
    pub fn new(client: ConnectedClient) -> Self {
        Self {
            client,
            kind: RelinkSearchKind::default(),
            query_input: String::new(),
            results: Vec::new(),
            selected: None,
            status: String::new(),
            busy: false,
            channel: unbounded(),
            done: false,
        }
    }

    /// Drain background events. Call once per frame BEFORE rendering.
    pub fn poll(&mut self) {
        while let Ok(ev) = self.channel.1.try_recv() {
            match ev {
                RelinkEvent::SearchResults(rs) => {
                    self.busy = false;
                    self.results = rs;
                    self.selected = if self.results.len() == 1 { Some(0) } else { None };
                    self.status = if self.results.is_empty() {
                        "No matches.".to_string()
                    } else {
                        format!("{} match(es). Pick one and click Apply.", self.results.len())
                    };
                }
                RelinkEvent::SearchError(e) => {
                    self.busy = false;
                    self.status = format!("Search failed: {e}");
                }
                RelinkEvent::PayloadReady { candidate, payload } => {
                    // Stash the resolved order# back into the chosen result
                    // and proceed with the DB commit.
                    if let Some(idx) = self.selected {
                        if let Some(r) = self.results.get_mut(idx) {
                            r.order_number = Some(payload.order.id.clone());
                            r.firstname = candidate.firstname.clone();
                            r.lastname = candidate.lastname.clone();
                        }
                    }
                    self.commit_apply(&candidate, &payload);
                }
                RelinkEvent::PayloadError(e) => {
                    self.busy = false;
                    self.status = format!("Could not fetch order info: {e}");
                }
                RelinkEvent::ApplyOk => {
                    self.busy = false;
                    self.status = "Applied successfully.".to_string();
                    self.done = true;
                }
                RelinkEvent::ApplyError(e) => {
                    self.busy = false;
                    self.status = format!("Apply failed: {e}");
                }
            }
        }
    }

    /// Render the popup. Returns `true` while the popup is open; `false`
    /// when the admin closes it (caller drops the popup).
    pub fn ui(&mut self, ctx: &egui::Context) -> bool {
        let mut open = true;
        let title = format!(
            "Re-link customer for {}",
            self.client
                .friendly_name
                .clone()
                .unwrap_or_else(|| self.client.connection_string.clone())
        );

        Window::new(title)
            .open(&mut open)
            .resizable(true)
            .default_width(520.0)
            .show(ctx, |ui| {
                self.draw_body(ui);
            });

        open && !self.done
    }

    fn draw_body(&mut self, ui: &mut Ui) {
        ui.label(
            RichText::new(
                "Search by phone, email, or order # to re-link this client \
                 to the right customer. The 'customer_locked' flag will be \
                 set so the OA-key auto-detection no longer overwrites it.",
            )
            .small()
            .color(theme::weak_text(ui)),
        );
        ui.add_space(6.);

        ui.horizontal(|ui| {
            for kind in [
                RelinkSearchKind::Phone,
                RelinkSearchKind::Email,
                RelinkSearchKind::OrderNumber,
            ] {
                if ui
                    .selectable_label(self.kind == kind, kind.label())
                    .clicked()
                {
                    self.kind = kind;
                    self.results.clear();
                    self.selected = None;
                    self.status.clear();
                }
            }
        });

        ui.add_space(4.);
        ui.horizontal(|ui| {
            let response = ui.add(
                TextEdit::singleline(&mut self.query_input)
                    .hint_text(self.kind.hint())
                    .desired_width(300.),
            );
            let enter = response.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let look_up = ui
                .add_enabled(!self.busy && !self.query_input.trim().is_empty(), Button::new("Look Up"))
                .clicked();
            if look_up || enter {
                self.kick_off_search();
            }
        });

        if self.busy {
            ui.add_space(4.);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(if self.results.is_empty() {
                    "Searching…"
                } else {
                    "Applying…"
                });
            });
        }
        if !self.status.is_empty() {
            ui.add_space(4.);
            ui.colored_label(
                if self.status.contains("failed") || self.status.contains("Could not") {
                    theme::error(ui)
                } else if self.done {
                    theme::success(ui)
                } else {
                    theme::warn(ui)
                },
                &self.status,
            );
        }

        if !self.results.is_empty() {
            ui.add_space(8.);
            ui.separator();
            ui.label(RichText::new("Matches").strong());
            ScrollArea::vertical().max_height(180.).show(ui, |ui| {
                for (i, r) in self.results.iter().enumerate() {
                    let selected = self.selected == Some(i);
                    let label = if let Some(ord) = &r.order_number {
                        format!("{} — order {}", r.display_label, ord)
                    } else {
                        r.display_label.clone()
                    };
                    if ui.selectable_label(selected, label).clicked() {
                        self.selected = Some(i);
                    }
                }
            });
        }

        ui.add_space(10.);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let can_apply = !self.busy
                && self.selected.is_some()
                && self.results.get(self.selected.unwrap_or(usize::MAX)).is_some();
            if ui
                .add_enabled(
                    can_apply,
                    Button::new(RichText::new("Apply").color(theme::success(ui))),
                )
                .clicked()
            {
                if let Some(idx) = self.selected {
                    if let Some(cand) = self.results.get(idx).cloned() {
                        self.kick_off_apply(cand);
                    }
                }
            }
        });
    }

    fn kick_off_search(&mut self) {
        let kind = self.kind;
        let query = self.query_input.trim().to_string();
        let tx = self.channel.0.clone();
        self.busy = true;
        self.results.clear();
        self.selected = None;
        self.status = "Searching…".to_string();
        PlatformSpawner::spawn(async move {
            match kind {
                RelinkSearchKind::Phone => match Customer::find_customer_by_phone(&query).await {
                    Ok(matches) => {
                        let candidates = matches
                            .iter()
                            .map(|(c, a)| RelinkCandidate::from_prestashop(c, a))
                            .collect::<Vec<_>>();
                        let _ = tx.send(RelinkEvent::SearchResults(candidates));
                    }
                    Err(e) => {
                        let _ = tx.send(RelinkEvent::SearchError(e.to_string()));
                    }
                },
                RelinkSearchKind::Email => match Customer::find_customer_by_email(&query).await {
                    Ok(matches) => {
                        let candidates = matches
                            .iter()
                            .map(|(c, a)| RelinkCandidate::from_prestashop(c, a))
                            .collect::<Vec<_>>();
                        let _ = tx.send(RelinkEvent::SearchResults(candidates));
                    }
                    Err(e) => {
                        let _ = tx.send(RelinkEvent::SearchError(e.to_string()));
                    }
                },
                RelinkSearchKind::OrderNumber => match get_prestashop_payload(&query).await {
                    Ok(payload) => {
                        let cand = RelinkCandidate::from_payload(&payload);
                        let _ = tx.send(RelinkEvent::SearchResults(vec![cand]));
                    }
                    Err(e) => {
                        let _ = tx.send(RelinkEvent::SearchError(e.to_string()));
                    }
                },
            }
        });
    }

    fn kick_off_apply(&mut self, cand: RelinkCandidate) {
        let tx = self.channel.0.clone();
        self.busy = true;
        self.status = "Fetching order info…".to_string();
        let kind = self.kind;
        let phone = cand.phone.clone();

        PlatformSpawner::spawn(async move {
            // For phone/email lookups we still need the latest order
            // number so the admin's friendly_name follows the
            // "First Last - OrderID" convention. Order-# lookups already
            // have the payload embedded in the candidate, so reuse it.
            let payload_res: Result<PrestashopPayload, anyhow::Error> = match kind {
                RelinkSearchKind::OrderNumber => {
                    if let Some(order_id) = &cand.order_number {
                        get_prestashop_payload(order_id).await
                    } else {
                        Err(anyhow::anyhow!(
                            "Order-number candidate missing order id"
                        ))
                    }
                }
                _ => {
                    if phone.is_empty() {
                        Err(anyhow::anyhow!(
                            "Customer has no phone on file; \
                             try the order-# search instead"
                        ))
                    } else {
                        get_prestashop_payload_from_phone(&phone).await
                    }
                }
            };

            match payload_res {
                Ok(payload) => {
                    let _ = tx.send(RelinkEvent::PayloadReady {
                        candidate: cand,
                        payload,
                    });
                }
                Err(e) => {
                    let _ = tx.send(RelinkEvent::PayloadError(e.to_string()));
                }
            }
        });
    }

    /// Run the actual DB writes. Called from `poll()` once the
    /// prestashop payload arrives.
    fn commit_apply(&mut self, cand: &RelinkCandidate, payload: &PrestashopPayload) {
        let tx = self.channel.0.clone();
        let client_id = self.client.id.clone();
        let computer_link = self.client.computer.clone();
        let new_customer = payload.customer.clone();
        let new_customer_id = new_customer.id.clone();
        let new_friendly = format!(
            "{} {} - {}",
            cand.firstname.trim(),
            cand.lastname.trim(),
            payload.order.id
        );
        self.status = "Writing DB updates…".to_string();
        self.busy = true;

        PlatformSpawner::spawn(async move {
            // 1) Upsert the customer row so it exists / is fresh.
            let cust_upsert: Result<Option<database::schema::CustomerData>, surrealdb::Error> =
                db()
                    .upsert(new_customer_id.clone())
                    .content(new_customer.clone())
                    .await;
            if let Err(e) = cust_upsert {
                let _ = tx.send(RelinkEvent::ApplyError(format!(
                    "Customer upsert failed: {e}"
                )));
                return;
            }

            // 2) Patch the connected_client row: customer + friendly_name +
            //    customer_locked, leaving everything else (computer,
            //    local_ip, tcp_port, command_history, …) intact. We use a
            //    targeted UPDATE rather than `.content(...)` to avoid
            //    clobbering fields the popup doesn't know about.
            let cc_table = CONNECTED_CLIENT_TABLE;
            let cc_update = db()
                .query(
                    "UPDATE $id SET customer = $customer, \
                                    friendly_name = $name, \
                                    customer_locked = true, \
                                    last_update = time::now()",
                )
                .bind(("id", client_id.clone()))
                .bind(("customer", new_customer_id.clone()))
                .bind(("name", new_friendly.clone()))
                .await;
            if let Err(e) = cc_update {
                let _ = tx.send(RelinkEvent::ApplyError(format!(
                    "{cc_table} update failed: {e}"
                )));
                return;
            }

            // 3) If the connected_client has a computer link, repoint that
            //    row's owner too. Without this the customer→computer graph
            //    still resolves to the previous (wrong) owner.
            if let Some(computer_id) = computer_link {
                let comp_table = COMPUTER_TABLE;
                let comp_update = db()
                    .query("UPDATE $id SET customer = $customer")
                    .bind(("id", computer_id.clone()))
                    .bind(("customer", new_customer_id.clone()))
                    .await;
                if let Err(e) = comp_update {
                    let _ = tx.send(RelinkEvent::ApplyError(format!(
                        "{comp_table} update failed: {e}"
                    )));
                    return;
                }
            }

            let _ = tx.send(RelinkEvent::ApplyOk);
        });
    }
}
