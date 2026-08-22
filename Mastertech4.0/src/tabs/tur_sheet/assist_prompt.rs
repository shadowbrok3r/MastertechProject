//! Offers AI assistance after a service pull, and records the tech's
//! confirmation that the pulled order belongs to this machine.
//!
//! Confirming writes an `assist_request` the headless agent dispatches, and
//! links customer/computer/order so the machine-to-order match is recorded
//! rather than inferred.

use database::schema::Store;
use displays::plugins::push_widget_anchor;
use displays::ui_tools::icons;
use displays::{get_toast_sender, ToastMessage};
use eframe::egui::{Align2, Area, Context, Frame, Order, RichText};
use log::{info, warn};
use tokio::spawn;

use crate::app_state::MastertechContext;

/// Order and machine identity captured at pull time, pending the tech's answer.
#[derive(Debug, Clone, Default)]
pub struct PendingAssist {
    pub service_number: String,
    pub order_device: String,
    pub order_serial: String,
    pub customer_name: String,
    /// Check-in notes, shown when the agent resolved the match itself.
    pub checkin_notes: String,
    /// Set when the agent resolved this machine itself.
    pub offer_id: Option<database::schema::RecordId>,
}

/// Seconds between offer polls; an offer is not urgent.
const OFFER_POLL_SECS: u64 = 20;

impl MastertechContext {
    /// Picks up an assist offer the headless agent wrote for this machine.
    pub fn poll_assist_offer(&mut self) {
        while let Ok(pending) = self.assist_offer_rx.try_recv() {
            if self.pending_assist.is_none() {
                self.pending_assist = Some(pending);
            }
        }
        if self.pending_assist.is_some() || self.shared_ctx.current_user.is_none() {
            return;
        }
        let due = self
            .last_offer_poll
            .is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(OFFER_POLL_SECS));
        if !due {
            return;
        }
        self.last_offer_poll = Some(std::time::Instant::now());

        let cs = self.client_title.clone();
        let tx = self.assist_offer_tx.clone();
        spawn(async move {
            let sql = "SELECT id, service_number, customer_name, device, checkin_notes FROM assist_offer                        WHERE connection_string = $cs AND status = 'offered'                        AND created_at > time::now() - 4h LIMIT 1";
            let Ok(mut res) = database::db().query(sql).bind(("cs", cs)).await else { return };
            let rows: Vec<serde_json::Value> = res.take(0).unwrap_or_default();
            let Some(row) = rows.first() else { return };
            let Some(sn) = row.get("service_number").and_then(|v| v.as_str()) else { return };
            let id = row
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| {
                    database::schema::RecordId::new(
                        "assist_offer",
                        s.trim_start_matches("assist_offer:").trim_matches('`'),
                    )
                });
            let _ = tx.try_send(PendingAssist {
                service_number: sn.to_string(),
                order_device: row
                    .get("device")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                order_serial: String::new(),
                customer_name: row
                    .get("customer_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                checkin_notes: row
                    .get("checkin_notes")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                offer_id: id,
            });
        });
    }

    /// Closes out an offer the tech answered.
    fn answer_offer(offer_id: Option<database::schema::RecordId>, status: &str, by: String) {
        let Some(id) = offer_id else { return };
        let status = status.to_string();
        spawn(async move {
            let _ = database::db()
                .query("UPDATE $id SET status = $status, answered_by = $by")
                .bind(("id", id))
                .bind(("status", status))
                .bind(("by", by))
                .await;
        });
    }
}

impl MastertechContext {
    /// Arms the prompt from a fresh PrestaShop pull.
    pub fn arm_assist_prompt(&mut self, service_number: String, customer_name: String) {
        if service_number.trim().is_empty() {
            return;
        }
        let device = self.service_details.first();
        self.pending_assist = Some(PendingAssist {
            service_number,
            order_device: device
                .map(|d| format!("{} {}", d.device_mfg, d.device_model).trim().to_string())
                .unwrap_or_default(),
            order_serial: device.map(|d| d.device_serial.clone()).unwrap_or_default(),
            customer_name,
            checkin_notes: device.map(|d| d.check_in_notes.clone()).unwrap_or_default(),
            offer_id: None,
        });
    }

    /// Floating so it survives the tab's ScrollArea; only drawn while armed.
    pub fn render_assist_prompt(&mut self, ctx: &Context) {
        let Some(pending) = self.pending_assist.clone() else { return };
        let Some(user) = self.shared_ctx.current_user.clone() else {
            self.pending_assist = None;
            return;
        };

        let mut confirm = false;
        let mut dismiss = false;
        Area::new("assist_confirm_prompt".into())
            .anchor(Align2::RIGHT_BOTTOM, [-20.0, -80.0])
            .order(Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                let resp = Frame::popup(ui.style())
                    .inner_margin(12.0)
                    .fill(ui.style().visuals.window_fill)
                    .stroke(ui.style().visuals.window_stroke)
                    .show(ui, |ui| {
                        ui.set_min_width(320.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(icons::ROBOT).size(18.0));
                            ui.label(
                                RichText::new(if pending.offer_id.is_some() {
                                    format!(
                                        "Found service #{} for this computer - want AI help?",
                                        pending.service_number
                                    )
                                } else {
                                    format!(
                                        "Is service #{} for THIS computer?",
                                        pending.service_number
                                    )
                                })
                                .strong()
                                .size(14.0),
                            );
                        });
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(format!("Order: {} {}", pending.customer_name, pending.order_device))
                                .weak(),
                        );
                        if !pending.order_serial.is_empty() {
                            ui.label(RichText::new(format!("Order serial: {}", pending.order_serial)).weak());
                        }
                        if !pending.checkin_notes.is_empty() {
                            ui.label(
                                RichText::new(format!(
                                    "Checked in for: {}",
                                    pending.checkin_notes.chars().take(120).collect::<String>()
                                ))
                                .weak(),
                            );
                        }
                        ui.label(RichText::new(format!("This machine: {}", self.client_title)).weak());
                        if !self.computer_data.product_serial.is_empty() {
                            ui.label(
                                RichText::new(format!(
                                    "This serial: {}",
                                    self.computer_data.product_serial
                                ))
                                .weak(),
                            );
                        }
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui
                                .button(format!("{} Yes, get AI help", icons::CHECK))
                                .on_hover_text(
                                    "Links this machine to the order and asks the diagnostic agent to start.",
                                )
                                .clicked()
                            {
                                confirm = true;
                            }
                            if ui.button(format!("{} Not this one", icons::CLOSE)).clicked() {
                                dismiss = true;
                            }
                        });
                    });
                push_widget_anchor("tur.confirm_this_computer", resp.response.rect);
            });

        if dismiss {
            Self::answer_offer(pending.offer_id.clone(), "declined", user.get_email().to_string());
            self.pending_assist = None;
            return;
        }
        if !confirm {
            return;
        }
        self.pending_assist = None;
        Self::answer_offer(pending.offer_id.clone(), "accepted", user.get_email().to_string());

        // The confirmation is the ground truth the auto-link event cannot infer.
        self.create_and_link_only();

        // Give the tech something to watch: the first run took seven minutes
        // to open its session, with nothing on screen in the meantime.
        self.assist_progress = Some(
            crate::tabs::tur_sheet::assist_progress::AssistProgress::new(
                self.client_title.clone(),
                pending.service_number.clone(),
            ),
        );
        self.show_assist_viewport
            .store(true, std::sync::atomic::Ordering::Relaxed);

        let connection_string = crate::filesystem::get_client_hash().connection_string;
        let computer = crate::filesystem::local_computer_record();
        let store = user.get_store();
        let requested_by = user.get_email().to_string();
        let hostname = self.computer_data.hostname.clone();
        let service_number = pending.service_number.clone();
        let toast_tx = get_toast_sender();

        spawn(async move {
            let sql = "CREATE assist_request CONTENT { \
                 connection_string: $cs, hostname: $host, service_number: $sn, \
                 computer: $computer, requested_by: $by, store: $store, \
                 trigger_source: 'tur_sheet', machine_confirmed: true, status: 'pending' }";
            let res = database::db()
                .query(sql)
                .bind(("cs", connection_string))
                .bind(("host", hostname))
                .bind(("sn", service_number.clone()))
                .bind(("computer", computer))
                .bind(("by", requested_by))
                .bind(("store", store_code(store)))
                .await;
            match res {
                Ok(_) => {
                    info!("assist_request queued for service #{service_number}");
                    let _ = toast_tx.try_send(ToastMessage::Success(
                        format!("AI assistance requested for #{service_number}"),
                    ));
                }
                Err(e) => {
                    warn!("assist_request failed for #{service_number}: {e}");
                    let _ = toast_tx.try_send(ToastMessage::Error(
                        format!("Could not request AI assistance: {e}"),
                    ));
                }
            }
        });
    }
}

fn store_code(store: Store) -> String {
    format!("{store:?}")
}
