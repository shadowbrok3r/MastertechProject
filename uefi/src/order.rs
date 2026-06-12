//! Serial → order lookup: response contract + Order tab state.
//!
//! The axum_server endpoint (`/api/v1/qc/order-by-serial/{serial}`) resolves a
//! serial through PrestaShop / Shopify (XBS build serials, XBM scan history) /
//! Everest and returns this slim JSON. Every field defaults so additive server
//! changes never break firmware parsing.

use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct OrderResponse {
    pub found: bool,
    pub serial: String,
    pub matched_by: String,
    pub backend: String,
    pub order: OrderInfo,
    pub spec: Option<BuildSpec>,
    pub gate: Option<Gate>,
    pub odoo: Option<OdooLot>,
    pub error: Option<String>,
    pub tried: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct OrderInfo {
    pub id: String,
    pub reference: String,
    pub customer: String,
    pub kind: String,
    pub status: StatusInfo,
    pub total: String,
    pub build_serial: Option<String>,
    pub everest_doc: Option<String>,
    pub note: Option<String>,
    pub items: Vec<Item>,
    pub service: Option<ServiceInfo>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StatusInfo {
    pub legacy_id: i64,
    pub name: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Item {
    pub name: String,
    pub reference: String,
    pub qty: f64,
    pub unit_price: String,
    pub serials: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ServiceInfo {
    pub device: String,
    pub mfg: String,
    pub model: String,
    pub serial: String,
    pub notes: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct BuildSpec {
    pub model: String,
    pub cpu: String,
    pub gpu: String,
    pub ram: String,
    pub motherboard: Option<String>,
    pub os: Option<String>,
    pub drives: Vec<Drive>,
    pub extra: Vec<SlotPick>,
    pub device_serial: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Drive {
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct SlotPick {
    pub slot: String,
    pub name: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Gate {
    /// Externally-tagged `GateOutcome`: `"RefuseToMove"`, `"Neutral"`, or
    /// `{"GoodToMove":{"advance_to":N}}`.
    pub outcome: serde_json::Value,
    pub status_legacy_id: i64,
    pub status_name: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateKind {
    Good,
    Refuse,
    Neutral,
}

impl Gate {
    pub fn kind(&self) -> GateKind {
        if self.outcome.get("GoodToMove").is_some() {
            GateKind::Good
        } else if self.outcome.as_str() == Some("RefuseToMove") {
            GateKind::Refuse
        } else {
            GateKind::Neutral
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct OdooLot {
    pub lot_id: i64,
    pub name: String,
    pub product_name: Option<String>,
    pub r#ref: Option<String>,
}

pub enum LookupState {
    Idle,
    Busy,
    Done(Box<OrderResponse>),
    Failed(String),
}

pub struct OrderPanel {
    pub serial: String,
    pub state: LookupState,
}

impl OrderPanel {
    pub fn new(default_serial: String) -> Self {
        Self {
            serial: default_serial,
            state: LookupState::Idle,
        }
    }
}

pub fn parse_response(body: &[u8]) -> Result<OrderResponse, String> {
    serde_json::from_slice(body).map_err(|e| {
        let head: String = String::from_utf8_lossy(&body[..body.len().min(120)]).into();
        format!("bad response: {e} ({head})")
    })
}

/// Minimal percent-encoding for a serial inside a URL path segment.
pub fn encode_path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
