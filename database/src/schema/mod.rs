use serde::{Serialize, Deserialize};
use surrealdb::{opt::RecordId, sql::{Id, Thing}};
use uuid::Uuid;

pub const NS: &str = "Mastertech";
pub const DB: &str = "MastertechDB";
pub const USER_SCOPE: &str = "user";
pub const TICKET_TABLE: &str = "service_order";
pub const CUSTOMER_TABLE: &str = "customer";
pub const COMPUTER_TABLE: &str = "computer";
pub const TASK_TABLE: &str = "task";
pub const TASK_NOTE_TABLE: &str = "task_note";
pub const SEB_TABLE: &str = "seb_data";
pub const USER_TABLE: &str = "user";
pub const NOTIFICATION_TABLE: &str = "notification";
pub const CONNECTED_CLIENT_TABLE: &str = "connected_client";


#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Record {
    #[allow(dead_code)]
    pub id: Thing,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ClientId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ComputerId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CustomerId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TicketId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct UserId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TaskId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TaskNoteId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SebId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct NotificationId(pub RecordId);

#[derive(Serialize, Debug)]
pub struct RecordResult {
    pub result: bool,
    pub record: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RecordSuccess{
    pub success: bool
}

// A specific sentinel value for default initialization
const DEFAULT_USER_ID: RecordId = RecordId {
    tb: String::new(),
    id: Id::String(String::new()),
};

impl Default for UserId {
    fn default() -> Self {
        UserId(DEFAULT_USER_ID.clone())
    }
}


#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct TaskPayload{
    pub id: Option<TaskId>,
    pub task_name: String,
    pub service_ticket: Option<TicketPayload>,
    // #[serde(skip)]
    pub everest_initials: String,
    pub task_description: String, 
    pub assignee: UserId, // should i use a user id here or will email and name be enough for tracking?
    pub service_number: Option<String>,
    pub due_date: String, // optional because if not provided, set due date to creation date
    pub priority: Priority,
    pub task_note: Option<Vec<TaskNotePayload>>, // TaskNoteId
    pub completed: bool,
    pub status: Status,
    pub dep: String
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct LiveTaskPayload{
    pub id: Option<TaskId>,
    pub task_name: String,
    pub service_ticket: Option<TicketId>,
    // #[serde(skip)]
    pub everest_initials: String,
    pub task_description: String, 
    pub assignee: UserId, // should i use a user id here or will email and name be enough for tracking?
    pub service_number: Option<String>,
    pub due_date: String, // optional because if not provided, set due date to creation date
    pub priority: Priority,
    pub task_note: Option<Vec<TaskNoteId>>, // 
    pub completed: bool,
    pub status: Status,
    pub dep: String
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct TicketPayload{
    pub id: Option<TicketId>,
    pub created_at: Option<String>,
    pub customer: Option<CustomerData>,
    pub computer: Option<ComputerData>,
    pub service_ticket: Option<TaskId>,
    pub service_number: String,
    /// Person that checked computer in
    pub checkin_rep: String,
    /// This is main initials on ticket
    pub sales_rep: String,
    pub checkin_notes: String,
    pub tech: String,
    pub salesman: String,
    pub dep: String, // Store
    pub terms: String,
    pub ticket_total: String,
    pub doc_alias: String, // type of order (service,sales,transfer)
    pub current_antivirus: Option<Vec<String>>,
    pub hardware_test_results: HardwareTests,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct TicketData{
    pub id: Option<TicketId>,
    pub created_at: Option<String>,
    pub customer: Option<CustomerId>,
    pub computer: Option<ComputerId>,
    // pub service_task: Option<TaskId>,
    pub service_number: String,
    /// Person that checked computer in
    pub checkin_rep: String,
    /// This is main initials on ticket
    pub sales_rep: String,
    pub checkin_notes: String,
    pub tech: String,
    pub salesman: String,
    pub dep: String, // Store
    pub terms: String,
    pub ticket_total: String,
    pub doc_alias: String, // type of order (service,sales,transfer)
    pub current_antivirus: Option<Vec<String>>,
    pub hardware_test_results: HardwareTests,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct CustomerData{
    pub id: Option<CustomerId>, 
    pub part_order_links: Option<Vec<String>>,
    pub computers: Option<Vec<ComputerId>>,
    pub services: Option<Vec<TicketId>>,
    pub name: String,
    pub phone_number: String,
    pub phone_number_2: String, // Option<String>
    pub email: String,
    pub li_doc: String,
    pub li_amnt: String,
    pub num_inv: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ComputerData{
    pub id: Option<ComputerId>,
    pub customer: Option<CustomerId>,
    // pub seb_id: Option<SebId>,
    pub seb_info: Option<LocalSebData>,
    pub hostname: String,
    pub operating_system: String,
    pub cpu: String,
    pub gpu: String,
    pub ram: String,
    pub drives: Vec<DriveData>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[allow(non_snake_case)]
#[serde(rename_all(serialize = "PascalCase", deserialize = "snake_case"))]
#[serde(rename = "xml")]
pub struct LocalSebData {
    // pub id: Option<SebId>,
    pub InstalledDeviceId: String,
    pub InstallInstanceId: String,
    pub HasIssues: String,
    pub InstallationStage: String,
    pub ReasonCode: String,
    pub ActivationCode: String,
    pub InstallVersion: String,
    pub MachineName: String,
    pub ExtendedSeb: Option<ExtendedSeb>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct ExtendedSeb {
    pub email: String,
    pub phone: String,
    pub userid: String,
    pub device_name: String,
    pub device_id: String,
    pub state: String,
    pub usage_gb: String,
    pub date_device_created: String,
    pub activated: String,
    pub activation_code: String,
    pub last_complete_backup: String,
    pub last_client_status_update: String,
    pub id_recurly_account: String,
    pub date_last_scan: String,
    pub date_email_sent: String,
    pub date_canceled_account: String,
    pub date_deleted_account: String,
    pub current_period_ends_at: String,
    pub date_modified: String,
    pub date_created: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DriveData{
    pub drive_letter: String,
    pub drive_type: String,
    pub total_size: String,
    pub space_left: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct HardwareTests{
    pub hdd_test: String,
    pub ssd_test: String,
    pub ram_test: String
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct TaskNotePayload{
    pub id: Option<TaskNoteId>,
    pub task_id: Option<TaskId>,
    pub everest_initials: String,
    pub created_at: String,
    pub note: String,
}

// I will probably end up merging ModifyTask and TaskPayload since they contain most of the exact same data
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ModifyTask{
    /// unique id for tasks
    pub task_id: TaskId,
    /// ability to change store
    // pub dep: Option<String>, 
    /// change priority
    pub priority: Option<Priority>, 
    /// change which status task is part of
    pub status: Option<Status>, 
    /// change completed / incomplete
    pub completed: Option<bool>, 
    /// update due_date 
    pub due_date: Option<String>, 
    /// update task name 
    pub task_name: Option<String>, 
    /// modify description of task
    pub task_description: Option<String>, 
}

#[derive(Serialize, Debug, Clone, Deserialize, Default)]
pub struct ConnectedClient{ // <'a>
    pub id: Option<ClientId>,
    pub assigned_user: Option<UserId>,
    pub client_hash: String,
    pub connection_string: String,
    pub connected: bool,
}


#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Notification{
    /// receiver of notification
    pub user: UserId,
    /// description of notification
    pub notification_description: String, 
    /// type of notification
    pub notification_type: NotificationType,
    /// Has the notification been read?
    pub status: NotificationStatus, 
    pub user_initials: String
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum NotificationType {
    NewMessage,
    SpoStatusChange,
    NewTask,
    TaggedInComment,
    GroupTag,
    OverdueTask
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum NotificationStatus{
    Read,
    Unread
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ModifyNotification{
    pub id: NotificationId,
    pub everest_initials: Option<String>,
    /// either Read or Unread
    pub status: Option<NotificationStatus>,
    pub mark_all_read: Option<bool>,
    pub mark_all_unread: Option<bool>,
    pub archive: Option<bool>
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub enum Status{
    #[default]
    Todo,
    InRepair,
    Complete
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub enum Priority{
    Express,
    Rfs,
    CustomerFire,
    Qc,
    #[default]
    Normal,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Category{
    StoreTasks,
    MyTasks,
    CompletedTasks,
}

#[derive(Deserialize)]
struct CommandRequest {
    _client_id: String,
    _command: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Copy, Default)]
pub enum Store{
    #[default]
    RIV,
    LTN,
    MUR,
    AF,
    WJ, 
    ORE,
    SAN
}

impl Store{
    pub fn as_str(&mut self) -> &str{
        match self{
            Store::RIV => "RIV",
            Store::LTN => "LTN",
            Store::MUR => "MUR",
            Store::AF => "AF",
            Store::WJ => "WJ",
            Store::ORE => "ORE",
            Store::SAN => "SAN",
        }
    }
    pub const VALUES: [Self; 7] = [Self::RIV, Self::LTN, Self::MUR, Self::AF, Self::WJ, Self::ORE, Self::SAN];
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct User {
    pub id: UserId,
    pub name: String,
    pub everest_initials: String,
    // #[serde(skip)]
    pub email: String,
    pub store: Store,
    pub notifications: Option<Vec<NotificationId>>,
    pub connected_clients: Option<Vec<ClientId>>
}

impl Priority{
    pub fn as_str(&mut self) -> &str{
        match self{
            Priority::Normal => "Normal",
            Priority::Rfs => "Rfs",
            Priority::Qc => "Qc",
            Priority::Express => "Express",
            Priority::CustomerFire => "CustomerFire",
        }
    }
    pub const VALUES: [Self; 5] = [Self::Normal, Self::Rfs, Self::Qc, Self::Express, Self::CustomerFire];
}

impl Status{
    pub fn as_str(&mut self) -> &str{
        match self{
            Status::Todo => "Todo",
            Status::InRepair => "In Repair",
            Status::Complete => "Complete",
        }
    }
    pub const VALUES: [Self; 3] = [Self::Todo, Self::InRepair, Self::Complete];
}







///////////////////////////////                   PRESTA SHOP SCHEMA                   ///////////////////////////////
//                                                                                                                  //
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Address{
    pub id: String,
    pub id_customer: String,         // ❌     isNullOrUnsignedId  
    pub lastname: String,             // ✔️     isName  
    pub firstname: String,            // ✔️     isName  
    pub address1: String,             // ✔️     isAddress   
    pub address2: String,             // ❌     isAddress   
    pub postcode: String,                // ❌     isPostCode  
    pub city: String,                 // ✔️     isCityName  
    pub phone: String,                   // ❌     isPhoneNumber   
    pub phone_mobile: String, // ❌     isPhoneNumber   
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Employee{
    pub id: String,
    /// ✔️	isName	
    pub lastname: String, 
    /// ✔️	isName	
    pub firstname: String, 
    /// ✔️	isEmail	
    pub email: String, 
    /// ❌	isBool	
    pub active: String, 
    /// ✔️	isInt	
    pub id_profile: String, 
    /// ❌	isUnsignedInt	
    pub id_last_order: String, 
    /// ❌	isUnsignedInt	
    pub id_last_customer_message: String, 
    /// ❌	isUnsignedInt	
    pub id_last_customer: String, 
}


#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Order{
    pub order_type_name: String,
    pub id_address_delivery: String, // ✔️
    pub id_customer: String, // ✔️
    pub id_cart: String, // ✔️
    pub invoice_number: String, // ❌		
    pub invoice_date: String, // ❌		
    pub date_add: String, // ❌
    pub date_upd: String, // ❌
    pub id_employee_sales_rep: String,
    pub id_employee_split_rep: String,
    pub id_employee_editing: String,
    pub id_order_everest: String,
    pub id_store: String, // 1 = warehouse
    pub total_paid: String, // ✔️
    pub reference: String, // what prestashop sees since order id and reference are different...
    pub id_order_parent: String, // no idea
    // #[serde(flatten)]
    pub shipping_number: String, // Tracking number
    pub order_type: String, // Configurator / Sales Order
    // note: String, // ❌
    pub associations: Associations
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Associations{
    pub order_rows: Vec<OrderRow>,
    pub order_service: Option<Vec<ServiceOrder>>
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct OrderRow{
    pub id: String,
    pub id_order_config: String,
    pub product_id: String,
    pub product_quantity: String,
    pub product_name: String,
    pub product_price: String,
}


#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Customer { 	 	
    pub lastname: String,    //  	isCustomerName 	✔️ 	✔️ 	255
    pub firstname: String,   //  	isCustomerName 	✔️ 	✔️ 	255
    pub email: String, 	     //  	isEmail 	✔️ 	✔️ 	255
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct CustomerMessage { 	 	
    pub id_employee: String,        //  isUnsignedId   ❌ 		Employee ID
    pub id_customer_thread: String, //	               ❌ 		Customer Thread ID
    pub ip_address: String,         //  isIp2Long      ❌ 	    15 	
    pub message: String,            //  isCleanHtml    ✔️ 	     16777216 	
    pub file_name: String,          //		           ❌ 		
    pub user_agent: String,         //	               ❌ 		
    pub private: String,            //  isBool 	       ❌ 		
    pub date_add: String,           // 	isDate 	       ❌ 		
    pub date_upd: String,           // 	isDate 	       ❌ 		
    pub read: String,           
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct CustomerThread { 	
    pub id: String,     
    pub id_customer: String,    // isUnsignedId 	❌ 		Customer ID
    pub id_order: String,    	// isUnsignedId 	❌ 		Order ID
    pub date_add: String,    	// isDate 	        ❌ 		
    pub date_upd: String,    	// isDate 	        ❌ 		
    pub associations: CustMessageAssociation,    	     
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct CustMessageAssociation{
    pub customer_messages: Vec<CustMessage>
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct CustMessage{
    pub id: String
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ServiceOrder{
    pub id: String,
    pub id_order_service: String,
    pub id_cart: String,
    pub id_order: String,
    pub device_name: String,
    pub device_mfg: String,
    pub device_model: String,
    pub device_serial: String,
    pub device_password: String,
    pub id_status_service: String,
    pub device_power_supply: String,
    pub other_hardware_software: String,
    pub physical_damage: String,
    pub check_in_notes: String,
    pub intake_notes: String,
    pub id_employee_qc_tech: String,
    pub id_employee_qc_signoff: String,
}

#[derive(Serialize, Debug, Default)]
pub struct Resources {
    /// 	The Customer, Manufacturer and Customer addresses
    pub addresses: Address,          
    /// 	The product Attachments
    pub attachments: String,            
    /// 	The product Attachments files
    pub attachments_file: String,            
    /// 	Customer’s carts
    pub carts: String,          
    /// 	The product categories
    pub categories: String,             
    /// 	The product combinations
    pub combinations: String,
    /// 	Customer services messages
    pub customer_messages: String,          
    /// 	Customer services threads
    pub customer_threads: String,           
    /// 	The e-shop’s customers
    pub customers: String,
    /// 	The Employees
    pub employees: Employee,          
    /// 	The guests (customers not logged in)
    pub guests: String,             
    /// 	The product manufacturers
    pub manufacturers: String,          
    /// 	The customers messages
    pub messages: String,           
    /// 	The order carriers
    pub order_carriers: String,             
    /// 	Details of an order
    pub order_details: String,          
    /// 	The Order histories
    pub order_histories: String,            
    /// 	The Order invoices
    pub order_invoices: String,             
    /// 	The Order payments
    pub order_payments: String,   
    /// 	The Order states (Waiting for transfer, Payment accepted, …)
    pub order_states: String,           
    /// 	The Customers orders
    pub orders: String,    
    /// 	The Product customization fields
    pub product_customization_fields: String,           
    /// 	The product feature values (Ceramic, Polyester, … - Removable cover, Short sleeves, …)
    pub product_feature_values: String,             
    /// 	The product features (Composition, Property, …)
    pub product_features: String,           
    /// 	The product options value (S, M, L, … - White, Camel, …)
    pub product_option_values: String,          
    /// 	The product options (Size, Color, …)
    pub product_options: String,            
    /// 	Product Suppliers
    pub product_suppliers: String,          
    /// 	The products
    pub products: String,           
    /// 	Search
    pub search: String,             
    /// 	Available quantities of products
    pub stock_availables: String,  
    /// 	Stocks for products
    pub stocks: String,             
    /// 	The stores
    pub stores: String,
    /// 	The Products tags
    pub tags: String,           
}

pub trait SubResource {
    fn get_subresource(&self, field: &str) -> Option<String>;
    fn get_name(&self) -> String;
    fn get_resource_name(&self) -> String;
}

impl SubResource for Employee {
    fn get_subresource(&self, field: &str) -> Option<String> {
        match field {
            "id" => Some(self.id.to_string()),
            "lastname" => Some(self.lastname.clone()),
            "firstname" => Some(self.firstname.clone()),
            "email" => Some(self.email.clone()),
            "active" => Some(self.active.to_string()),
            "id_profile" => Some(self.id_profile.to_string()),
            "id_last_order" => Some(self.id_last_order.to_string()),
            "id_last_customer_message" => Some(self.id_last_customer_message.to_string()),
            "id_last_customer" => Some(self.id_last_customer.to_string()),
            _ => None,
        }
    }

    fn get_resource_name(&self) -> String {
        "employees".to_string()
    }

    fn get_name(&self) -> String {
        "employee".to_string()
    }
}