
use serde::{Deserialize, Serialize};

pub mod scheduler;
pub use scheduler::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct ScheduledTask {
    #[serde(rename = "CimClass")]
    cim_class: CimClass,
    #[serde(rename = "CimInstanceProperties")]
    cim_instance_properties: Vec<String>,
    #[serde(rename = "CimSystemProperties")]
    cim_system_properties: CimSystemProperties,
    #[serde(rename = "State")]
    state: Option<u8>,
    #[serde(rename = "Actions")]
    actions: Option<Vec<String>>,
    #[serde(rename = "Author")]
    author: Option<String>,
    #[serde(rename = "Date")]
    date: Option<String>,
    #[serde(rename = "Description")]
    description: Option<String>,
    #[serde(rename = "Documentation")]
    documentation: Option<String>,
    #[serde(rename = "Principal")]
    principal: Option<Principal>,
    #[serde(rename = "SecurityDescriptor")]
    security_descriptor: Option<String>,
    #[serde(rename = "Settings")]
    settings: Option<Settings>,
    #[serde(rename = "Source")]
    source: Option<String>,
    #[serde(rename = "TaskName")]
    task_name: Option<String>,
    #[serde(rename = "TaskPath")]
    task_path: Option<String>,
    #[serde(rename = "Triggers")]
    triggers: Option<Vec<String>>,
    #[serde(rename = "URI")]
    uri: Option<String>,
    #[serde(rename = "Version")]
    version: Option<String>,
    #[serde(rename = "PSComputerName")]
    ps_computer_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CimClass {
    #[serde(rename = "CimSuperClassName")]
    cim_super_class_name: Option<String>,
    #[serde(rename = "CimSuperClass")]
    cim_super_class: Option<String>,
    #[serde(rename = "CimClassProperties")]
    cim_class_properties: Option<String>,
    #[serde(rename = "CimClassQualifiers")]
    cim_class_qualifiers: Option<String>,
    #[serde(rename = "CimClassMethods")]
    cim_class_methods: Option<String>,
    #[serde(rename = "CimSystemProperties")]
    cim_system_properties: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CimSystemProperties {
    #[serde(rename = "Namespace")]
    namespace: Option<String>,
    #[serde(rename = "ServerName")]
    server_name: Option<String>,
    #[serde(rename = "ClassName")]
    class_name: Option<String>,
    #[serde(rename = "Path")]
    path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Principal {
    #[serde(rename = "CimClass")]
    cim_class: Option<String>,
    #[serde(rename = "CimInstanceProperties")]
    cim_instance_properties: Option<String>,
    #[serde(rename = "CimSystemProperties")]
    cim_system_properties: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Settings {
    #[serde(rename = "CimClass")]
    cim_class: Option<String>,
    #[serde(rename = "CimInstanceProperties")]
    cim_instance_properties: Option<String>,
    #[serde(rename = "CimSystemProperties")]
    cim_system_properties: Option<String>,
}

/// Enum for specifying possible task triggers
pub enum TaskTrigger {
    Daily { time: String },
    Weekly { days_of_week: String, time: String },
    Once { date_time: String },
    AtLogon,
    AtStartup,
}

