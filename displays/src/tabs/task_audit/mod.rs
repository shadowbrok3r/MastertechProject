use database::schema::prestashop_schema::{self, MissedCallOrder, PrestashopPayload};
use database::schema::prestashop::{OrderState, OrderType};
use crossbeam::channel::{Receiver, Sender};
use crate::{app_state::SharedContext, channel_manager::ChannelManager, TaskUiActions};
use egui_data_table::DataTable;
use row_viewer::TaskRowViewer;
use std::collections::HashMap;
use eframe::egui::Ui;

pub mod ui;
pub mod data;
pub mod codec;
pub mod row_viewer;

impl SharedContext {
    pub fn task_table_viewer(&mut self, ui: &mut Ui, ui_actions_tx: Sender<TaskUiActions>) {
        self.task_audit_table.services_viewer.sync_existing_tasks(&self.tasks);
        self.task_audit_table.show(ui, self.current_user.clone(), ui_actions_tx);
    }
}

pub struct TaskAuditViewer {
    audit_selection: TaskAudit,
    order_channel: (Sender<prestashop_schema::PrestashopPayload>, Receiver<prestashop_schema::PrestashopPayload>),
    pub services_viewer: TaskRowViewer,
    loading: bool,
    index: HashMap<String, i32>,
    time: Option<web_time::Instant>,
    pub service_map: HashMap<String, DataTable<PrestashopPayload>>,
    /// Cache key the in-flight load was requested for; arriving orders land
    /// here even if the selection or store changed while they were pulling.
    loading_key: String,
    pub missed_calls_tx: Sender<Vec<MissedCallOrder>>,
    pub missed_calls_rx: Receiver<Vec<MissedCallOrder>>,
}

impl TaskAuditViewer {
    /// Cache key for what the top panel currently has selected.
    pub fn current_key(&self) -> String {
        self.audit_selection.cache_key(self.services_viewer.store_selection)
    }

    pub fn new() -> Self {
        let order_channel = <prestashop_schema::PrestashopPayload>::create_unbounded_channel();
        let (missed_calls_tx, missed_calls_rx) = <Vec<MissedCallOrder>>::create_unbounded_channel();

        Self {
            audit_selection: TaskAudit::default(),
            services_viewer: TaskRowViewer::default(),
            order_channel,
            loading: false,
            index: HashMap::new(),
            service_map: HashMap::new(),
            loading_key: String::new(),
            time: None,
            missed_calls_tx,
            missed_calls_rx,
        }
    }
}

#[derive(PartialEq, Debug, Clone)]
pub enum TaskAudit {
    MyInRepair,
    MyServices,
    Status(OrderState),
    AllExcept { order_type: OrderType, excluded: Vec<OrderState> },
    /// Check-in Shelf services that still need a call today: checked in on a
    /// prior day with no customer message dated today.
    NeedsCallToday,
}

impl Default for TaskAudit {
    fn default() -> Self {
        Self::AllExcept { order_type: OrderType::ServiceOrder, excluded: Vec::new() }
    }
}

impl TaskAudit {
    /// Stable key for caching pulled orders and pagination per selection and
    /// store, so switching stores loads into its own table instead of
    /// appending to the one on screen.
    pub fn cache_key(&self, store: u64) -> String {
        // The two "mine" selections query by employee, not by store, so they
        // key store-agnostically and survive a store switch.
        if matches!(self, Self::MyInRepair | Self::MyServices) {
            return match self {
                Self::MyInRepair => "my_in_repair".to_string(),
                _ => "my_services".to_string(),
            };
        }
        let selection = match self {
            Self::MyInRepair => "my_in_repair".to_string(),
            Self::MyServices => "my_services".to_string(),
            Self::NeedsCallToday => "needs_call_today".to_string(),
            Self::Status(state) => format!("status:{}", state.to_id_str()),
            Self::AllExcept { order_type, excluded } => {
                let mut ids: Vec<&str> = excluded.iter().map(|s| s.to_id_str()).collect();
                ids.sort_unstable();
                format!("all:{}:excl:{}", order_type.to_id_str(), ids.join(","))
            }
        };
        format!("store:{store}:{selection}")
    }
}

#[cfg(test)]
mod cache_key_tests {
    use super::*;
    use database::schema::prestashop::OrderState;

    #[test]
    fn the_same_selection_keys_differently_per_store() {
        let selection = TaskAudit::Status(OrderState::InRepair);
        assert_ne!(selection.cache_key(1), selection.cache_key(4));
    }

    #[test]
    fn the_mine_selections_survive_a_store_switch() {
        assert_eq!(TaskAudit::MyServices.cache_key(1), TaskAudit::MyServices.cache_key(4));
        assert_eq!(TaskAudit::MyInRepair.cache_key(1), TaskAudit::MyInRepair.cache_key(4));
    }

    #[test]
    fn one_store_keys_selections_apart() {
        let in_repair = TaskAudit::Status(OrderState::InRepair).cache_key(1);
        let done = TaskAudit::Status(OrderState::DoneShelf).cache_key(1);
        assert_ne!(in_repair, done);
    }
}
