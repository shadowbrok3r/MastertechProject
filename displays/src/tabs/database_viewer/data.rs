use database::schema::{get_data::get_services_by_status, helper_traits::{parse_email_user, EmployeeHelper}, prestashop_schema::{self, Employee, MissedCallOrder, PrestashopOrderType, PrestashopPayload}, utilities::{create_full_task_payload, get_prestashop_payload}, ComputerData, CustomerData, TaskNotePayload, TaskPayload, TicketPayload, User, TASK_TABLE, TICKET_TABLE};
use crossbeam::channel::Sender;
use egui_data_table::DataTable;
use itertools::Itertools;
use surrealdb::RecordId;
use chrono::Utc;

use crate::{PlatformSpawner, Spawner};

use super::{row_viewer::DatabaseRowViewer, TaskAudit, DatabaseViewer};

impl DatabaseViewer {
    pub fn receive(&mut self, store_users: Vec<User>, _frame: &mut eframe::Frame) {

    }
}

impl DatabaseRowViewer {

}
