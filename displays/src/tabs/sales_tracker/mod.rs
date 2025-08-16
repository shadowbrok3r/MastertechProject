

use std::collections::{HashMap, HashSet};

use crossbeam::channel::{unbounded, Receiver, Sender};
use database::schema::{
	prestashop::{get_order_payments, Order, OrderPayment, OrderState, OrderType, PayPeriod},
	sales_tracker::{get_sales_notes_for_user, upsert_sales_note, SalesNote},
	User,
};
use eframe::egui::{Button, CentralPanel, ComboBox, TextEdit, TopBottomPanel, Ui, Widget};
use egui_data_table::{DataTable, Renderer};

use crate::{get_current_user_from_auth, PlatformSpawner, Spawner};

mod data;
mod row_viewer;
mod codec;

use data::SalesTableData;
use row_viewer::SalesRowViewer;

pub struct SalesTracker {
	// async comms
	response_tx: Sender<Vec<Order>>,
	response_rx: Receiver<Vec<Order>>,
	order_payment_tx: Sender<OrderPayment>,
	order_payment_rx: Receiver<OrderPayment>,
	// notes pipeline from viewer edits -> persistence (sender lives in viewer)
	note_update_rx: Receiver<(String, String)>,
	// notes fetch
	notes_tx: Sender<Vec<SalesNote>>,
	notes_rx: Receiver<Vec<SalesNote>>,

	// data stores
	orders: HashMap<String, Vec<Order>>,          // key: my employee id (string)
	payments: HashMap<String, Vec<OrderPayment>>, // key: my employee id (string)
	notes: HashMap<String, String>,               // key: order_id => note

	// ui/state
	order_state: OrderState,
	pay_period: PayPeriod,
	user: User,
	pulling_all_orders: bool,
	import_order_number: String,

	// aggregates
	total: f64,
	total_w_tax: f64,
	total_spiffs: f64,

	// table
	table: DataTable<SalesTableData>,
	viewer: SalesRowViewer,
}

impl Default for SalesTracker {
	fn default() -> Self {
		let (response_tx, response_rx) = unbounded();
		let (order_payment_tx, order_payment_rx) = unbounded();
		let (note_update_tx, note_update_rx) = unbounded();
		let (notes_tx, notes_rx) = unbounded();

		let mut viewer = SalesRowViewer::default();
		viewer.note_update_tx = Some(note_update_tx.clone());

		Self {
			response_tx,
			response_rx,
			order_payment_tx,
			order_payment_rx,
			note_update_rx,
			notes_tx,
			notes_rx,
			orders: Default::default(),
			payments: Default::default(),
			notes: Default::default(),
			order_state: Default::default(),
			pay_period: Default::default(),
			user: get_current_user_from_auth().unwrap_or_default(),
			pulling_all_orders: false,
			import_order_number: String::new(),
			total: 0.0,
			total_w_tax: 0.0,
			total_spiffs: 0.0,
			table: DataTable::new(),
			viewer,
		}
	}
}

impl SalesTracker {
	pub fn ui(&mut self, ui: &mut Ui) {
		TopBottomPanel::top("SalesTopPanel").show_inside(ui, |ui| {
			ui.horizontal(|ui| {
				// search box
				TextEdit::singleline(&mut self.viewer.filter)
					.desired_width(150.)
					.hint_text(" Search")
					.ui(ui);

				// OrderState selection
				ComboBox::new("Sales OrderState", "")
					.selected_text(self.order_state.as_str())
					.show_ui(ui, |ui| {
						let selected = &mut self.order_state;
						ui.selectable_value(selected, OrderState::AcceptedByOdoo, OrderState::AcceptedByOdoo.as_str());
						ui.selectable_value(selected, OrderState::Shipped, OrderState::Shipped.as_str());
						ui.selectable_value(selected, OrderState::DeliveredToStore, OrderState::DeliveredToStore.as_str());
						ui.selectable_value(selected, OrderState::DoneShelf, OrderState::DoneShelf.as_str());
						ui.selectable_value(selected, OrderState::OrderPlaced, OrderState::OrderPlaced.as_str());
						ui.selectable_value(selected, OrderState::PrePulled, OrderState::PrePulled.as_str());
						ui.selectable_value(selected, OrderState::ReadyToBuild, OrderState::ReadyToBuild.as_str());
						ui.selectable_value(selected, OrderState::QcAndBurnin, OrderState::QcAndBurnin.as_str());
						ui.selectable_value(selected, OrderState::ShipToStore, OrderState::ShipToStore.as_str());
						ui.selectable_value(selected, OrderState::Returned, OrderState::Returned.as_str());
					});

				// Pay period
				ComboBox::new("Sales PayPeriod", "")
					.selected_text(self.pay_period.as_str())
					.show_ui(ui, |ui| {
						let selected = &mut self.pay_period;
						ui.selectable_value(selected, PayPeriod::Current, PayPeriod::Current.as_str());
						ui.selectable_value(selected, PayPeriod::Last, PayPeriod::Last.as_str());
					});

				if Button::new(format!("Pull Orders in '{}'", self.order_state.as_str())).ui(ui).clicked() {
					self.pull_orders(false);
				}

				if Button::new("Pull ALL orders").ui(ui).clicked() {
					self.pull_orders(true);
				}

				ui.separator();
				TextEdit::singleline(&mut self.import_order_number)
					.desired_width(120.)
					.hint_text(" Order #")
					.ui(ui);
				if Button::new("Import").ui(ui).clicked() {
					self.import_single_order();
				}

				ui.separator();
				if Button::new("Save Notes").ui(ui).clicked() {
					self.persist_all_notes();
				}
			});
		});

		// summary footer
		TopBottomPanel::bottom("SalesBottom").show_inside(ui, |ui| {
			ui.columns(9, |ui| {
				let uid = self.user.get_employee_id().map(|id| id.to_string()).unwrap_or_default();
				let my_orders = self.orders.get(&uid).cloned().unwrap_or_default();
				let my_payments = self.payments.get(&uid).cloned().unwrap_or_default();

				// sales counts
				let total_laptops = my_orders
					.iter()
					.filter(|o| o.id_order_type != "2")
					.flat_map(|o| o.associations.order_rows.iter())
					.filter(|a| a.product_reference.to_lowercase().starts_with("lap/"))
					.count();

				let total_desktops = my_orders
					.iter()
					.filter(|o| o.id_order_type != "2")
					.flat_map(|o| o.associations.order_rows.iter())
					.filter_map(|o| {
						let r = o.product_reference.to_lowercase();
						if !r.starts_with("lap/") && (r.starts_with("case/") || r.starts_with("bsd/") || r.starts_with("rci/") || r.starts_with("r2r/") || r.starts_with("rtr/")) && !r.starts_with("case/15") && !r.starts_with("case/17") {
							Some(())
						} else { None }
					})
					.count();

				let total_sales = total_desktops + total_laptops;
				let total_orders = self.orders.get(&uid).map(|v| v.len()).unwrap_or(0);

				let total_financed = my_orders
					.iter()
					.filter(|o| {
						my_payments
							.iter()
							.any(|p| p.payment_method == "Financing Payment" && p.id_order == o.id)
					})
					.map(|o| o.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0))
					.sum::<f64>();
				let ar_financing_ratio = if self.total > 0.0 { (total_financed / self.total) * 10.0 } else { 0.0 };

				ui[0].label(format!("Sales: {total_sales} / Orders: {total_orders}"));
				ui[1].label(format!("Laptops: {total_laptops} / Desktops: {total_desktops}"));
				ui[2].label("");
				ui[3].label("");
				ui[4].colored_label(ui[4].style().visuals.error_fg_color, format!("Finance ratio: {ar_financing_ratio:.2}%"));
				// warranty ratio
				let wty_count = my_orders.iter().filter(|order| {
					order.associations.order_rows.iter().any(|o| o.product_reference.to_lowercase().starts_with("wty/") && !o.product_price.starts_with("0.0"))
				}).count();
				ui[5].colored_label(ui[5].style().visuals.error_fg_color, format!("WTY's: {} out of {total_sales} sales", wty_count));
				ui[6].colored_label(ui[6].style().visuals.warn_fg_color, format!("Total W/Tax: $ {:.2}", self.total_w_tax));
				ui[7].colored_label(ui[7].style().visuals.warn_fg_color, format!("REVENUE: $ {:.2}", self.total));
				ui[8].label(format!("Spiffs: $ {:.2}", self.total_spiffs));
			});
		});
		
		// receive async events
		self.receive();

		// data table
		let date_label = match (self.order_state.clone(), self.pulling_all_orders) {
			(OrderState::AcceptedByOdoo, false) => "Delivery Date",
			_ => "Date Updated",
		};
		self.viewer.date_label = date_label.to_string();

		CentralPanel::default().show_inside(ui, |ui| {
			ui.group(|ui| {
				Renderer::new(&mut self.table, &mut self.viewer)
					.with_style_modify(|s| {
						s.auto_shrink = [false, false].into();
						s.single_click_edit_mode = true;
					})
					.ui(ui);
			});
		});
	}

	fn pull_orders(&mut self, all_states: bool) {
		self.pulling_all_orders = all_states;
		self.total = 0.0;
		self.total_w_tax = 0.0;
		self.total_spiffs = 0.0;
		self.orders.clear();
		self.payments.clear();

		let pay_period = self.pay_period.clone();
		let state = self.order_state.clone();
		let uid_num = if let Some(id) = self.user.get_employee_id() {
			id
		} else {
			self.user = get_current_user_from_auth().unwrap_or_default();
			self.user.get_employee_id().unwrap_or(0)
		};
		let emp_id = uid_num.to_string();
		let tx = self.response_tx.clone();

		PlatformSpawner::spawn(async move {
			if uid_num == 0 { return; }
			if all_states {
				for s in OrderState::VALUES.iter() {
					if *s == OrderState::Returned { continue; }
					let period = pay_period.clone();
					let state_id = s.id().to_string();
					match database::schema::prestashop::generate_orders_report(period, &state_id, &emp_id).await {
						Ok(orders) => { let _ = tx.try_send(orders); },
						Err(e) => log::error!("Error getting orders for sales tracker: {e:?}"),
					}
				}
			} else {
				match database::schema::prestashop::generate_orders_report(pay_period, &state.id().to_string(), &emp_id).await {
					Ok(orders) => { let _ = tx.try_send(orders); },
					Err(e) => log::error!("Error getting orders for sales tracker: {e:?}"),
				}
			}
		});
	}

	fn import_single_order(&mut self) {
		let order_number = self.import_order_number.trim().to_string();
		if order_number.is_empty() { return; }

		let tx = self.response_tx.clone();
		PlatformSpawner::spawn(async move {
			// Minimal pull: fetch the order itself
			let api = database::schema::prestashop::Prestashop::default();
			match api.request_subresources_by_id_wasm::<Order>("orders", "order", &order_number).await {
				Ok(order) => { let _ = tx.try_send(vec![order]); },
				Err(e) => log::error!("Import order failed: {e:?}"),
			}
		});
	}

	fn rebuild_rows(&mut self) {
		let uid = self.user.get_employee_id().map(|id| id.to_string()).unwrap_or_default();
		let my_orders = self.orders.get(&uid).cloned().unwrap_or_default();
		let my_payments = self.payments.get(&uid).cloned().unwrap_or_default();

		let mut rows: Vec<SalesTableData> = Vec::with_capacity(my_orders.len());
		let mut spiff_sum: f64 = 0.0;
		let mut total_sum_tax_excl: f64 = 0.0;
		let mut total_sum_tax_incl: f64 = 0.0;

		for order in my_orders.iter() {
			let state = OrderState::state_from_id_str(&order.current_state);
			let date_str = match state { OrderState::AcceptedByOdoo => order.delivery_date.clone(), _ => order.date_upd.clone() };

			let order_total_paid: f64 = order.total_paid.parse::<f64>().unwrap_or(0.0);
			let order_total_paid_tax_excl: f64 = order.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0);
			let attributed_paid: f64 = my_payments
				.iter()
				.filter(|p| p.id_order == order.id)
				.map(|p| p.amount.parse::<f64>().unwrap_or(0.0))
				.sum();
			if attributed_paid <= 0.0 || order_total_paid <= 0.0 { continue; }
			let share_ratio: f64 = (attributed_paid / order_total_paid).clamp(0.0, 1.0);
			let total_paid_num: f64 = order_total_paid * share_ratio;
			let total_paid_tax_excl_num: f64 = order_total_paid_tax_excl * share_ratio;

			// spiffs computation (copied from Koth)
			let mut spiffs_total: f64 = 0.0;
			let mut has_system_product = false;
			let mut cps_units: i32 = 0; // sw/cps (not plat)
			let mut has_sas = false;
			let mut has_wrav = false;

			for o in order.associations.order_rows.iter() {
				let r = o.product_reference.to_lowercase();
				let qty: i32 = o.product_quantity.parse::<i32>().unwrap_or(1);

				if r.starts_with("lap/") || (r.starts_with("case/") && !r.starts_with("case/15") && !r.starts_with("case/17")) {
					has_system_product = true;
				}
				if r.starts_with("sw/sas") { has_sas = true; }
				if r.starts_with("sw/wrav") { has_wrav = true; }
				if r.starts_with("sw/cps") && !r.starts_with("sw/cps-plat") { cps_units += qty; }
				if r.starts_with("sw/cps-plat") { spiffs_total += 25.0 * qty as f64; }
				if r == "seb/year" { spiffs_total += 15.0 * qty as f64; }
				if r.starts_with("mon/") || r.starts_with("kb/") || r.starts_with("mou/") || r.contains("/dock/") || r == "dvdrw/usb" || r.starts_with("case/15") || r.starts_with("case/17") || r.starts_with("spkr/") || r.starts_with("belk/") {
					spiffs_total += 2.0 * qty as f64;
				}
				if r.starts_with("wty/") {
					let ru = r.to_uppercase();
					if ru.contains("/R2R/2YR") { spiffs_total += 3.0 * qty as f64; }
					else if ru.contains("/R2R/3YR") { spiffs_total += 6.0 * qty as f64; }
					else if ru.contains("/R2R/4YR") { spiffs_total += 9.0 * qty as f64; }
					else if ru.contains("/R2R/5YR") { spiffs_total += 12.0 * qty as f64; }
					else if ru.contains("/R2R/LIFE") { spiffs_total += 15.0 * qty as f64; }
					if ru.contains("/LAP/CUST/2YR") { spiffs_total += 3.0 * qty as f64; }
					else if ru.contains("/LAP/CUST/3YR") { spiffs_total += 6.0 * qty as f64; }
					else if ru.contains("/LAP/CUST/4YR") { spiffs_total += 9.0 * qty as f64; }
					if ru.contains("/DSK/CUST/3YR") { spiffs_total += 3.0 * qty as f64; }
					else if ru.contains("/DSK/CUST/4YR") { spiffs_total += 6.0 * qty as f64; }
					else if ru.contains("/DSK/CUST/5YR") { spiffs_total += 9.0 * qty as f64; }
					else if ru.contains("/DSK/CUST/LIFE") { spiffs_total += 12.0 * qty as f64; }
					if matches!(OrderType::from_id_str(&order.id_order_type), OrderType::Bsd) {
						if ru.contains("/BSD/CUST/2YR") { spiffs_total += 3.0 * qty as f64; }
						else if ru.contains("/BSD/CUST/3YR") { spiffs_total += 6.0 * qty as f64; }
						else if ru.contains("/BSD/CUST/4YR") { spiffs_total += 9.0 * qty as f64; }
						else if ru.contains("/BSD/CUST/5YR") { spiffs_total += 12.0 * qty as f64; }
					}
				}
			}
			if cps_units > 0 { spiffs_total += 10.0 * cps_units as f64; } else if has_sas && has_wrav { spiffs_total += 10.0; }
			if has_system_product {
				match OrderType::from_id_str(&order.id_order_type) {
					OrderType::ReadyToRoll => spiffs_total += 5.0,
					OrderType::Rci => spiffs_total += 5.0,
					OrderType::Bsd => spiffs_total += 25.0,
					OrderType::SalesOrder | OrderType::ServiceOrder => {}
				}
			}

			let product: String = order.associations.order_rows
				.iter()
				.filter_map(|o| {
					let r = o.product_reference.to_lowercase();
					if r.starts_with("lap") || r.starts_with("case/") || r.starts_with("bsd/") || r.starts_with("rci/") || r.starts_with("r2r/") || r.starts_with("rtr/") && !r.starts_with("case/15") && !r.starts_with("case/17") {
						Some(o.product_reference.clone())
					} else { None }
				})
				.next()
				.unwrap_or_else(|| {
					order.associations.order_rows.first().map(|o| o.product_reference.clone()).unwrap_or_default()
				});

			let warranty = order.associations.order_rows
				.iter()
				.filter_map(|o| {
					if o.product_reference.to_lowercase().starts_with("wty/") && !o.product_price.starts_with("0.0") { Some(o.product_reference.clone()) } else { None }
				})
				.next()
				.unwrap_or_else(|| "-".to_string());

			let payment = my_payments.iter().find(|p| p.id_order == order.id).map(|p| p.payment_method.clone()).unwrap_or("-".to_string());

			let display_date = chrono::NaiveDateTime::parse_from_str(&date_str, "%Y-%m-%d %H:%M:%S")
				.map(|dt| dt.format("%m / %d / %Y").to_string())
				.unwrap_or_else(|_| String::new());

			let note = self.notes.get(&order.id).cloned().unwrap_or_default();

			spiff_sum += spiffs_total * share_ratio;
			total_sum_tax_incl += total_paid_num;
			total_sum_tax_excl += total_paid_tax_excl_num;
			rows.push(SalesTableData {
				order_id: order.id.clone(),
				date: display_date,
				order_state: OrderState::from_id_str(&order.current_state).to_string(),
				product,
				payment,
				warranty,
				total_paid: total_paid_num,
				total_without_tax: total_paid_tax_excl_num,
				spiffs: spiffs_total * share_ratio,
				notes: note,
			});
		}

		self.table.replace(rows);
		self.total_spiffs = spiff_sum;
		self.total = total_sum_tax_excl;
		self.total_w_tax = total_sum_tax_incl;
	}

	fn request_payments_for(&self, orders: &[Order]) {
		for order in orders.iter() {
			let tx = self.order_payment_tx.clone();
			let order = order.clone();
			PlatformSpawner::spawn(async move {
				match get_order_payments(&order.id).await {
					Ok(payments) => {
						for p in payments { let _ = tx.try_send(p); }
					}
					Err(e) => log::error!("Error getting payment details: {e:?}"),
				}
			});
		}
	}

	fn fetch_notes_for_orders(&self, order_ids: Vec<String>) {
		let tx = self.notes_tx.clone();
		let user = self.user.clone();
		PlatformSpawner::spawn(async move {
			match get_sales_notes_for_user(&user, order_ids).await {
				Ok(notes) => { let _ = tx.try_send(notes); },
				Err(e) => log::error!("Fetch sales notes failed: {e:?}"),
			}
		});
	}

	fn persist_all_notes(&self) {
		let user = self.user.clone();
		// best-effort: iterate current table snapshot
		for (order_id, note) in self.notes.iter() {
			if note.trim().is_empty() { continue; }
			let order_id = order_id.clone();
			let note = note.clone();
			PlatformSpawner::spawn({
				let user = user.clone();
				async move {
					let _ = upsert_sales_note(&user, &order_id, &note).await;
				}
			});
		}
	}

	pub fn receive(&mut self) {
		// Order batches
		if let Ok(orders) = self.response_rx.try_recv() {
			let sort = |a: &Order, b: &Order| {
				let a_total: f64 = a.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0);
				let b_total: f64 = b.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0);
				b_total.partial_cmp(&a_total).unwrap_or(std::cmp::Ordering::Equal)
			};

			let new_orders: Vec<Order> = orders
				.into_iter()
				.filter(|o| !o.id.is_empty())
				.collect();

			// request payments per order
			self.request_payments_for(&new_orders);

			let uid = self.user.get_employee_id().map(|id| id.to_string()).unwrap_or_default();
			if self.pulling_all_orders {
				let entry = self.orders.entry(uid.clone()).or_insert_with(Vec::new);
				// dedupe by id
				let existing: HashSet<String> = entry.iter().map(|o| o.id.clone()).collect();
				for o in new_orders.into_iter() { if !existing.contains(&o.id) { entry.push(o); } }
				entry.sort_by(sort);
			} else {
				// snapshot replace
				let mut v = new_orders;
				v.sort_by(sort);
				self.orders.insert(uid.clone(), v);
			}

			// fetch notes for these orders
			if let Some(my_orders) = self.orders.get(&uid) {
				let ids = my_orders.iter().map(|o| o.id.clone()).collect::<Vec<_>>();
				self.fetch_notes_for_orders(ids);
			}

			self.rebuild_rows();
		}

		// Payments
		if let Ok(payment) = self.order_payment_rx.try_recv() {
			let uid = self.user.get_employee_id().map(|id| id.to_string()).unwrap_or_default();
			// find order to determine split
			let maybe_order = self
				.orders
				.get(&uid)
				.and_then(|os| os.iter().find(|o| o.id == payment.id_order));
			if let Some(order) = maybe_order {
				let amt = payment.amount.parse::<f64>().unwrap_or(0.0);
				if amt > 0.0 {
					let mut p = payment.clone();
					let is_true_split = !order.id_employee_split_rep.trim().is_empty()
						&& order.id_employee_sales_rep != order.id_employee_split_rep
						&& (order.id_employee_sales_rep == uid || order.id_employee_split_rep == uid)
						&& order.id_employee_split_rep != "0".to_string();
					if is_true_split { p.amount = format!("{}", amt / 2.0); }
					self.payments.entry(uid.clone()).or_insert_with(Vec::new).push(p);
				}
			} else {
				let amt = payment.amount.parse::<f64>().unwrap_or(0.0);
				if amt > 0.0 {
					self.payments.entry(uid.clone()).or_insert_with(Vec::new).push(payment.clone());
				}
			}
			self.rebuild_rows();
		}

		// Note edits from viewer
		if let Ok((order_id, note)) = self.note_update_rx.try_recv() {
			self.notes.insert(order_id.clone(), note.clone());
			let user = self.user.clone();
			PlatformSpawner::spawn(async move {
				let _ = upsert_sales_note(&user, &order_id, &note).await;
			});
		}

		// Notes fetch results
		if let Ok(notes) = self.notes_rx.try_recv() {
			for n in notes.into_iter() {
				self.notes.insert(n.order_id.clone(), n.note.clone());
			}
			self.rebuild_rows();
		}
	}
}
