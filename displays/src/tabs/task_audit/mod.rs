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
    pub missed_calls_tx: Sender<Vec<MissedCallOrder>>,
    pub missed_calls_rx: Receiver<Vec<MissedCallOrder>>,
}

impl TaskAuditViewer {
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
    /// Stable key for caching pulled orders and pagination per selection.
    pub fn cache_key(&self) -> String {
        match self {
            Self::MyInRepair => "my_in_repair".to_string(),
            Self::MyServices => "my_services".to_string(),
            Self::NeedsCallToday => "needs_call_today".to_string(),
            Self::Status(state) => format!("status:{}", state.to_id_str()),
            Self::AllExcept { order_type, excluded } => {
                let mut ids: Vec<&str> = excluded.iter().map(|s| s.to_id_str()).collect();
                ids.sort_unstable();
                format!("all:{}:excl:{}", order_type.to_id_str(), ids.join(","))
            }
        }
    }
}
