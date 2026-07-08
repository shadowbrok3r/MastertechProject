//! Duplicate detection and merge types for task creation flow.
//! 
//! This module provides types and utilities for detecting duplicate records
//! when creating tasks, service orders, customers, and computers.

use serde::{Deserialize, Serialize};

use super::{ComputerData, CustomerData, LiveTaskPayload, RecordIdExt, TicketData};

/// Represents a potential duplicate entity with both existing and new versions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicatePair<T> {
    /// The existing record found in the database
    pub existing: T,
    /// The new record being submitted
    pub new: T,
    /// Whether the two records are identical (no differences)
    pub is_identical: bool,
}

impl<T: PartialEq + Clone> DuplicatePair<T> {
    pub fn new(existing: T, new: T) -> Self {
        let is_identical = existing == new;
        Self {
            existing,
            new,
            is_identical,
        }
    }
}

/// Result of checking for duplicate entities across the cascade:
/// Task -> ServiceOrder -> Customer + Computer
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DuplicateCheckResult {
    /// Duplicate task found by service_number
    pub task: Option<DuplicatePair<LiveTaskPayload>>,
    /// Duplicate service order found
    pub service_order: Option<DuplicatePair<TicketData>>,
    /// Duplicate customer found by phone/email/cust_code
    pub customer: Option<DuplicatePair<CustomerData>>,
    /// Duplicate computer found by hostname/serial
    pub computer: Option<DuplicatePair<ComputerData>>,
    /// The service number being checked
    pub service_number: String,
}

impl DuplicateCheckResult {
    pub fn new(service_number: String) -> Self {
        Self {
            service_number,
            ..Default::default()
        }
    }

    /// Returns true if any non-identical duplicates were found
    pub fn has_conflicts(&self) -> bool {
        self.task.as_ref().map_or(false, |d| !d.is_identical)
            || self.service_order.as_ref().map_or(false, |d| !d.is_identical)
            || self.customer.as_ref().map_or(false, |d| !d.is_identical)
            || self.computer.as_ref().map_or(false, |d| !d.is_identical)
    }

    /// Returns true if any duplicates were found (even if identical)
    pub fn has_any_duplicates(&self) -> bool {
        self.task.is_some()
            || self.service_order.is_some()
            || self.customer.is_some()
            || self.computer.is_some()
    }

    /// Returns true if all found duplicates are identical (no user action needed)
    pub fn all_identical(&self) -> bool {
        let task_ok = self.task.as_ref().map_or(true, |d| d.is_identical);
        let service_ok = self.service_order.as_ref().map_or(true, |d| d.is_identical);
        let customer_ok = self.customer.as_ref().map_or(true, |d| d.is_identical);
        let computer_ok = self.computer.as_ref().map_or(true, |d| d.is_identical);
        task_ok && service_ok && customer_ok && computer_ok
    }
}

/// User's resolution choice for a duplicate conflict
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MergeResolution {
    /// Keep the existing record, discard the new one
    KeepExisting,
    /// Use the new record, overwrite existing
    UseNew,
    /// Merge fields - requires field selection
    Merge,
    /// Cancel the operation
    Cancel,
}

impl Default for MergeResolution {
    fn default() -> Self {
        Self::UseNew
    }
}

/// Tracks which fields the user wants to keep from each version during a merge
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FieldSelections {
    /// Map of field name to whether to use the new value (true) or existing (false)
    pub selections: std::collections::HashMap<String, bool>,
}

impl FieldSelections {
    pub fn new() -> Self {
        Self {
            selections: std::collections::HashMap::new(),
        }
    }

    /// Set whether to use the new value for a field
    pub fn set_use_new(&mut self, field: &str, use_new: bool) {
        self.selections.insert(field.to_string(), use_new);
    }

    /// Get whether to use the new value for a field (defaults to true)
    pub fn use_new(&self, field: &str) -> bool {
        *self.selections.get(field).unwrap_or(&true)
    }
}

/// The complete resolution for all entity types
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DuplicateResolution {
    pub task_resolution: MergeResolution,
    pub task_fields: FieldSelections,
    
    pub service_order_resolution: MergeResolution,
    pub service_order_fields: FieldSelections,
    
    pub customer_resolution: MergeResolution,
    pub customer_fields: FieldSelections,
    
    pub computer_resolution: MergeResolution,
    pub computer_fields: FieldSelections,

    /// Register the new device as an additional computer for the same customer
    /// instead of overwriting the existing one. Only meaningful when a duplicate
    /// computer was found.
    #[serde(default)]
    pub add_second_computer: bool,
}

/// Helper trait to get field names and values for display in the merge UI
pub trait FieldDisplay {
    /// Returns a list of (field_name, existing_value, new_value) tuples for differing fields
    fn get_differing_fields(&self, other: &Self) -> Vec<(String, String, String)>;
}

impl FieldDisplay for LiveTaskPayload {
    fn get_differing_fields(&self, other: &Self) -> Vec<(String, String, String)> {
        let mut fields = Vec::new();
        
        if self.task_name != other.task_name {
            fields.push(("task_name".to_string(), self.task_name.clone(), other.task_name.clone()));
        }
        if self.task_description != other.task_description {
            fields.push(("task_description".to_string(), self.task_description.clone(), other.task_description.clone()));
        }
        if self.assignee != other.assignee {
            fields.push(("assignee".to_string(), self.assignee.key_string(), other.assignee.key_string()));
        }
        if self.service_number != other.service_number {
            fields.push(("service_number".to_string(), 
                self.service_number.clone().unwrap_or_default(), 
                other.service_number.clone().unwrap_or_default()));
        }
        if self.priority != other.priority {
            fields.push(("priority".to_string(), 
                format!("{:?}", self.priority), 
                format!("{:?}", other.priority)));
        }
        if self.status != other.status {
            fields.push(("status".to_string(), self.status.as_str().to_string(), other.status.as_str().to_string()));
        }
        if self.completed != other.completed {
            fields.push(("completed".to_string(), self.completed.to_string(), other.completed.to_string()));
        }
        
        fields
    }
}

impl FieldDisplay for TicketData {
    fn get_differing_fields(&self, other: &Self) -> Vec<(String, String, String)> {
        let mut fields = Vec::new();
        
        if self.service_number != other.service_number {
            fields.push(("service_number".to_string(), self.service_number.clone(), other.service_number.clone()));
        }
        if self.checkin_rep != other.checkin_rep {
            fields.push(("checkin_rep".to_string(), self.checkin_rep.clone(), other.checkin_rep.clone()));
        }
        if self.sales_rep != other.sales_rep {
            fields.push(("sales_rep".to_string(), self.sales_rep.clone(), other.sales_rep.clone()));
        }
        if self.checkin_notes != other.checkin_notes {
            fields.push(("checkin_notes".to_string(), self.checkin_notes.clone(), other.checkin_notes.clone()));
        }
        if self.tech != other.tech {
            fields.push(("tech".to_string(), self.tech.clone(), other.tech.clone()));
        }
        if self.salesman != other.salesman {
            fields.push(("salesman".to_string(), self.salesman.clone(), other.salesman.clone()));
        }
        if self.terms != other.terms {
            fields.push(("terms".to_string(), self.terms.clone(), other.terms.clone()));
        }
        if self.ticket_total != other.ticket_total {
            fields.push(("ticket_total".to_string(), self.ticket_total.clone(), other.ticket_total.clone()));
        }
        // Additional fields that can cause conflicts
        if self.doc_alias != other.doc_alias {
            fields.push(("doc_alias".to_string(), self.doc_alias.clone(), other.doc_alias.clone()));
        }
        if self.created_at != other.created_at {
            fields.push(("created_at".to_string(), 
                self.created_at.to_string(), 
                other.created_at.to_string()));
        }
        // Customer link (as display only - usually shouldn't differ)
        if self.customer != other.customer {
            let self_cust = self.customer.as_ref().map_or_else(|| "—".to_string(), |id| id.key_string());
            let other_cust = other.customer.as_ref().map_or_else(|| "—".to_string(), |id| id.key_string());
            fields.push(("customer".to_string(), self_cust, other_cust));
        }
        // Computer link
        let self_computer = self.computer.as_ref().map(|c| c.key_string()).unwrap_or_default();
        let other_computer = other.computer.as_ref().map(|c| c.key_string()).unwrap_or_default();
        if self_computer != other_computer {
            fields.push(("computer".to_string(), self_computer, other_computer));
        }
        // Hardware test results
        if self.hardware_test_results != other.hardware_test_results {
            fields.push(("hardware_tests".to_string(),
                format!("HDD:{} SSD:{} RAM:{}", 
                    self.hardware_test_results.hdd_test,
                    self.hardware_test_results.ssd_test,
                    self.hardware_test_results.ram_test),
                format!("HDD:{} SSD:{} RAM:{}", 
                    other.hardware_test_results.hdd_test,
                    other.hardware_test_results.ssd_test,
                    other.hardware_test_results.ram_test)));
        }
        // Current antivirus
        let self_av = self.current_antivirus.as_ref().map(|v| v.join(", ")).unwrap_or_default();
        let other_av = other.current_antivirus.as_ref().map(|v| v.join(", ")).unwrap_or_default();
        if self_av != other_av {
            fields.push(("current_antivirus".to_string(), self_av, other_av));
        }
        
        fields
    }
}

impl FieldDisplay for CustomerData {
    fn get_differing_fields(&self, other: &Self) -> Vec<(String, String, String)> {
        let mut fields = Vec::new();
        
        if self.name != other.name {
            fields.push(("name".to_string(), self.name.clone(), other.name.clone()));
        }
        if self.phone_number != other.phone_number {
            fields.push(("phone_number".to_string(), self.phone_number.clone(), other.phone_number.clone()));
        }
        if self.phone_number_2 != other.phone_number_2 {
            fields.push(("phone_number_2".to_string(), self.phone_number_2.clone(), other.phone_number_2.clone()));
        }
        if self.email != other.email {
            fields.push(("email".to_string(), self.email.clone(), other.email.clone()));
        }
        if self.cust_code != other.cust_code {
            fields.push(("cust_code".to_string(), self.cust_code.clone(), other.cust_code.clone()));
        }
        
        fields
    }
}

impl FieldDisplay for ComputerData {
    fn get_differing_fields(&self, other: &Self) -> Vec<(String, String, String)> {
        let mut fields = Vec::new();
        
        if self.hostname != other.hostname {
            fields.push(("hostname".to_string(), self.hostname.clone(), other.hostname.clone()));
        }
        if self.operating_system != other.operating_system {
            fields.push(("operating_system".to_string(), self.operating_system.clone(), other.operating_system.clone()));
        }
        if self.cpu != other.cpu {
            fields.push(("cpu".to_string(), self.cpu.clone(), other.cpu.clone()));
        }
        if self.gpu != other.gpu {
            fields.push(("gpu".to_string(), self.gpu.clone(), other.gpu.clone()));
        }
        if self.ram != other.ram {
            fields.push(("ram".to_string(), self.ram.clone(), other.ram.clone()));
        }
        if self.motherboard_name != other.motherboard_name {
            fields.push(("motherboard_name".to_string(), self.motherboard_name.clone(), other.motherboard_name.clone()));
        }
        if self.product_name != other.product_name {
            fields.push(("product_name".to_string(), self.product_name.clone(), other.product_name.clone()));
        }
        if self.product_serial != other.product_serial {
            fields.push(("product_serial".to_string(), self.product_serial.clone(), other.product_serial.clone()));
        }
        
        fields
    }
}

/// Merge helper functions
pub fn merge_task(existing: &LiveTaskPayload, new: &LiveTaskPayload, selections: &FieldSelections) -> LiveTaskPayload {
    LiveTaskPayload {
        id: existing.id.clone(), // Always keep existing ID
        task_name: if selections.use_new("task_name") { new.task_name.clone() } else { existing.task_name.clone() },
        service_ticket: if selections.use_new("service_ticket") { new.service_ticket.clone() } else { existing.service_ticket.clone() },
        task_description: if selections.use_new("task_description") { new.task_description.clone() } else { existing.task_description.clone() },
        assignee: if selections.use_new("assignee") { new.assignee.clone() } else { existing.assignee.clone() },
        service_number: if selections.use_new("service_number") { new.service_number.clone() } else { existing.service_number.clone() },
        due_date: if selections.use_new("due_date") { new.due_date.clone() } else { existing.due_date.clone() },
        priority: if selections.use_new("priority") { new.priority.clone() } else { existing.priority.clone() },
        completed: if selections.use_new("completed") { new.completed } else { existing.completed },
        status: if selections.use_new("status") { new.status.clone() } else { existing.status.clone() },
        created_at: existing.created_at.clone(), // Always keep existing creation time
    }
}

pub fn merge_ticket(existing: &TicketData, new: &TicketData, selections: &FieldSelections) -> TicketData {
    TicketData {
        id: existing.id.clone(),
        created_at: existing.created_at.clone(),
        customer: if selections.use_new("customer") { new.customer.clone() } else { existing.customer.clone() },
        computer: if selections.use_new("computer") { new.computer.clone() } else { existing.computer.clone() },
        service_number: if selections.use_new("service_number") { new.service_number.clone() } else { existing.service_number.clone() },
        checkin_rep: if selections.use_new("checkin_rep") { new.checkin_rep.clone() } else { existing.checkin_rep.clone() },
        sales_rep: if selections.use_new("sales_rep") { new.sales_rep.clone() } else { existing.sales_rep.clone() },
        checkin_notes: if selections.use_new("checkin_notes") { new.checkin_notes.clone() } else { existing.checkin_notes.clone() },
        tech: if selections.use_new("tech") { new.tech.clone() } else { existing.tech.clone() },
        salesman: if selections.use_new("salesman") { new.salesman.clone() } else { existing.salesman.clone() },
        terms: if selections.use_new("terms") { new.terms.clone() } else { existing.terms.clone() },
        ticket_total: if selections.use_new("ticket_total") { new.ticket_total.clone() } else { existing.ticket_total.clone() },
        doc_alias: if selections.use_new("doc_alias") { new.doc_alias.clone() } else { existing.doc_alias.clone() },
        current_antivirus: if selections.use_new("current_antivirus") { new.current_antivirus.clone() } else { existing.current_antivirus.clone() },
        hardware_test_results: if selections.use_new("hardware_test_results") { new.hardware_test_results.clone() } else { existing.hardware_test_results.clone() },
        jobs: if selections.use_new("jobs") { new.jobs.clone() } else { existing.jobs.clone() },
    }
}

pub fn merge_customer(existing: &CustomerData, new: &CustomerData, selections: &FieldSelections) -> CustomerData {
    CustomerData {
        id: existing.id.clone(),
        cust_code: if selections.use_new("cust_code") { new.cust_code.clone() } else { existing.cust_code.clone() },
        part_order_links: if selections.use_new("part_order_links") { new.part_order_links.clone() } else { existing.part_order_links.clone() },
        name: if selections.use_new("name") { new.name.clone() } else { existing.name.clone() },
        phone_number: if selections.use_new("phone_number") { new.phone_number.clone() } else { existing.phone_number.clone() },
        phone_number_2: if selections.use_new("phone_number_2") { new.phone_number_2.clone() } else { existing.phone_number_2.clone() },
        email: if selections.use_new("email") { new.email.clone() } else { existing.email.clone() },
        li_doc: if selections.use_new("li_doc") { new.li_doc.clone() } else { existing.li_doc.clone() },
        li_amnt: if selections.use_new("li_amnt") { new.li_amnt.clone() } else { existing.li_amnt.clone() },
        num_inv: if selections.use_new("num_inv") { new.num_inv.clone() } else { existing.num_inv.clone() },
    }
}

pub fn merge_computer(existing: &ComputerData, new: &ComputerData, selections: &FieldSelections) -> ComputerData {
    ComputerData {
        id: existing.id.clone(),
        // Hardware-derived identity: the incoming reading wins when present.
        oa3_key: new.oa3_key.clone().or_else(|| existing.oa3_key.clone()),
        customer: if selections.use_new("customer") { new.customer.clone() } else { existing.customer.clone() },
        seb_info: if selections.use_new("seb_info") { new.seb_info.clone() } else { existing.seb_info.clone() },
        hostname: if selections.use_new("hostname") { new.hostname.clone() } else { existing.hostname.clone() },
        operating_system: if selections.use_new("operating_system") { new.operating_system.clone() } else { existing.operating_system.clone() },
        cpu: if selections.use_new("cpu") { new.cpu.clone() } else { existing.cpu.clone() },
        gpu: if selections.use_new("gpu") { new.gpu.clone() } else { existing.gpu.clone() },
        ram: if selections.use_new("ram") { new.ram.clone() } else { existing.ram.clone() },
        drives: if selections.use_new("drives") { new.drives.clone() } else { existing.drives.clone() },
        device_name: if selections.use_new("device_name") { new.device_name.clone() } else { existing.device_name.clone() },
        device_mfg: if selections.use_new("device_mfg") { new.device_mfg.clone() } else { existing.device_mfg.clone() },
        device_model: if selections.use_new("device_model") { new.device_model.clone() } else { existing.device_model.clone() },
        device_serial: if selections.use_new("device_serial") { new.device_serial.clone() } else { existing.device_serial.clone() },
        windows_active: if selections.use_new("windows_active") { new.windows_active } else { existing.windows_active },
        current_antivirus: if selections.use_new("current_antivirus") { new.current_antivirus.clone() } else { existing.current_antivirus.clone() },
        motherboard_name: if selections.use_new("motherboard_name") { new.motherboard_name.clone() } else { existing.motherboard_name.clone() },
        motherboard_serial: if selections.use_new("motherboard_serial") { new.motherboard_serial.clone() } else { existing.motherboard_serial.clone() },
        motherboard_asset_tag: if selections.use_new("motherboard_asset_tag") { new.motherboard_asset_tag.clone() } else { existing.motherboard_asset_tag.clone() },
        motherboard_vendor: if selections.use_new("motherboard_vendor") { new.motherboard_vendor.clone() } else { existing.motherboard_vendor.clone() },
        product_name: if selections.use_new("product_name") { new.product_name.clone() } else { existing.product_name.clone() },
        product_sku: if selections.use_new("product_sku") { new.product_sku.clone() } else { existing.product_sku.clone() },
        product_serial: if selections.use_new("product_serial") { new.product_serial.clone() } else { existing.product_serial.clone() },
        product_vendor: if selections.use_new("product_vendor") { new.product_vendor.clone() } else { existing.product_vendor.clone() },
        installed_programs: if selections.use_new("installed_programs") { new.installed_programs.clone() } else { existing.installed_programs.clone() },
    }
}

