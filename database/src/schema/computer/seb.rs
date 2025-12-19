use crate::schema::SurrealValue;

#[derive(Debug, serde::Serialize, serde::Deserialize, Default, Clone, PartialEq, SurrealValue)]
#[allow(non_snake_case)]
#[serde(rename_all(serialize = "PascalCase", deserialize = "snake_case"))]
#[serde(rename = "xml")]
pub struct LocalSebData {
    // pub id: RecordId,
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

#[derive(Debug, serde::Serialize, serde::Deserialize, Default, Clone, PartialEq, SurrealValue)]
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
