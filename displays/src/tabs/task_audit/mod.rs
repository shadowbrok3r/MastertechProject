use database::schema::prestashop_schema::{self, MissedCallOrder, PrestashopPayload};
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

#[derive(PartialEq, Debug, Clone, Default)]
pub enum TaskAudit {
    #[default]
    AllServices,
    CheckinShelf,
    MyInRepair,
    InRepair,
    DoneShelf,
    MyServices,
    NeedsCall
}

impl TaskAudit {
    fn as_str(&self) -> &str {
        match self {
            TaskAudit::CheckinShelf => "Check-in Shelf",
            TaskAudit::MyInRepair => "My In Repair",
            TaskAudit::InRepair => "In Repair",
            TaskAudit::DoneShelf => "Done Shelf",
            TaskAudit::AllServices => "All Services",
            TaskAudit::MyServices => "My Services",
            TaskAudit::NeedsCall => "Needs Call",
        }
    }
}
