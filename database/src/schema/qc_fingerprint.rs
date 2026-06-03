//! Pre-OS hardware fingerprint captured by the Mastertech UEFI agent.
//!
//! One upserted row per machine, keyed by SMBIOS system serial, so repeated
//! captures of the same machine overwrite in place. A handful of hot-path
//! fields are denormalized out of the document for cheap querying; the full
//! document the agent POSTed is kept verbatim in `raw`.

use serde::{Deserialize, Serialize};

use super::{Datetime, RecordId, SurrealValue};

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct HardwareFingerprint {
    pub id: RecordId,
    pub serial: String,
    pub uuid: String,
    pub captured_at: Datetime,
    pub cpu_model: String,
    pub cpu_cores: u32,
    pub ram_bytes: u64,
    pub dimm_count: u32,
    pub disk_count: u32,
    pub win11_ready: bool,
    /// The full fingerprint document exactly as the agent sent it.
    pub raw: serde_json::Value,
}

/// Deterministic id `qc_fingerprint:<serial>` so re-captures upsert in place.
pub fn fingerprint_record_id(serial: &str) -> RecordId {
    RecordId::new(super::QC_FINGERPRINT_TABLE, serial.to_string())
}
