use crate::schema::{SortDirection, Sortable, CONNECTED_CLIENT_TABLE};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use structdiff::{Difference, StructDiff};
use std::cmp::Reverse;

use super::{random_record_id, Datetime, RecordId, SurrealValue};

/// Whether this `connected_client` row represents a customer machine
/// under service or a Rust-toolchain `plugin_builder` worker. The
/// admin/MCP `list_build_workers` tool filters on this; the default
/// Web Console view hides workers so they don't clutter the technician
/// surface. The `Machine` variant is the default for back-compat with
/// pre-existing rows that have no `client_kind` field.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, Difference, SurrealValue)]
#[serde(rename_all = "snake_case")]
#[surreal(untagged)]
pub enum ClientKind {
    #[surreal(value = "machine")]
    Machine,
    #[surreal(value = "build_worker")]
    BuildWorker,
    /// Pre-OS UEFI QC fingerprint agent — dials in over plain TCP, pushes a
    /// hardware fingerprint, and shows up as a live client for the duration.
    #[surreal(value = "qc_agent")]
    QcAgent,
    /// Pre-boot UEFI firmware app — relayed HTTP presence/streaming; no
    /// hostname or OS, identity is the chassis serial or OA3/MSDM key.
    #[surreal(value = "uefi")]
    Uefi,
}

impl Default for ClientKind {
    fn default() -> Self {
        Self::Machine
    }
}

impl ClientKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Machine => "machine",
            Self::BuildWorker => "build_worker",
            Self::QcAgent => "qc_agent",
            Self::Uefi => "uefi",
        }
    }
}

#[derive(serde::Serialize, Debug, Clone, serde::Deserialize, PartialEq, Difference, SurrealValue)]
pub struct ConnectedClient {
    pub id: RecordId,
    pub assigned_user: Option<RecordId>,
    // SurrealValue read path ignores serde(default); surreal(default) supplies the default when the field is absent.
    #[serde(default)]
    #[surreal(default)]
    pub client_hash: String,
    #[serde(default)]
    #[surreal(default)]
    pub connection_string: String,
    pub command_history: Option<Vec<String>>,
    #[serde(default)]
    #[surreal(default)]
    pub connected: bool,
    pub friendly_name: Option<String>,
    pub customer: Option<RecordId>,
    pub last_update: Option<Datetime>,
    pub created_at: Option<Datetime>,
    pub computer: Option<RecordId>,
    /// Non-loopback IPv4 the client is bound to for direct admin↔client TCP
    /// sessions. Populated by the client at startup; consumed by the admin
    /// console which dials this IP:port before falling back to the
    /// WebSocket relay. `None` means TCP transport is unavailable for this
    /// client (older build, bind failure, etc.) and admins should use the
    /// relay path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_ip: Option<String>,
    /// Port the client's direct-TCP listener is bound to. See `local_ip`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tcp_port: Option<u16>,
    /// Set to `true` when an admin has manually re-linked this client to a
    /// specific customer (typically because we sold the machine used and
    /// the auto-derived friendly_name from the OA3 product key still
    /// resolves to the original purchaser). When this flag is true, the
    /// customer-side `create_client` flow MUST NOT overwrite
    /// `friendly_name` or `customer` from any auto-lookup; admins may
    /// clear the flag to opt back into auto-detection.
    #[serde(default)]
    #[surreal(default)]
    pub customer_locked: bool,
    /// Distinguishes a customer-machine client from a Rust-toolchain
    /// `plugin_builder` worker. Default `Machine` so pre-existing rows
    /// deserialize cleanly without a migration backfill.
    #[serde(default)]
    #[surreal(default)]
    pub client_kind: ClientKind,
    /// Capability markers advertised by a `build_worker` (e.g.
    /// `multifile`). Empty on customer-machine rows and pre-multifile
    /// workers, so old rows deserialize without a backfill.
    #[serde(default)]
    #[surreal(default)]
    pub capabilities: Vec<String>,
}

impl Default for ConnectedClient {
    fn default() -> Self {
        Self {
            id: random_record_id(CONNECTED_CLIENT_TABLE),
            assigned_user: Default::default(),
            client_hash: Default::default(),
            connection_string: Default::default(),
            command_history: Default::default(),
            connected: Default::default(),
            friendly_name: Default::default(),
            customer: Default::default(),
            last_update: Default::default(),
            created_at: Default::default(),
            computer: Default::default(),
            local_ip: None,
            tcp_port: None,
            customer_locked: false,
            client_kind: ClientKind::Machine,
            capabilities: Vec::new(),
        }
    }
}

impl Sortable<ConnectedClient> for Vec<ConnectedClient> {
    fn default_sort(&mut self,  sort_direction: SortDirection) -> &mut Vec<ConnectedClient> {
        self.sort_by_date(sort_direction)
    }

    fn sort_by_date(&mut self, sort_direction: SortDirection) -> &mut Vec<ConnectedClient> {
        self.sort_by(|a: &ConnectedClient, b: &ConnectedClient| {
            let date_a = &a.last_update.as_ref().cloned().unwrap_or_default();
            let date_b = &b.last_update.as_ref().cloned().unwrap_or_default();
            
            let ordering = date_b.cmp(&date_a);
            
            match sort_direction {
                SortDirection::Asc => ordering,               // Use default ordering for ascending
                SortDirection::Desc => ordering.reverse(),    // Reverse ordering for descending
            }
        });
    
        self
    }

    fn sort_by_name(&mut self, sort_direction: SortDirection) -> &mut Vec<ConnectedClient> {
        self.sort_by(|a, b| {
            let name_a = &a.connection_string.to_lowercase();
            let name_b = &b.connection_string.to_lowercase();
            
            let ordering = name_a.cmp(name_b);
    
            match sort_direction {
                SortDirection::Asc => ordering,              // Default alphabetical ordering (A-Z)
                SortDirection::Desc => ordering.reverse(),   // Reverse alphabetical ordering (Z-A)
            }
        });
    
        self
    }
}

pub trait FilterClients {
    fn filter_by_client<T: IntoIterator<Item = S>, S: AsRef<str> + std::fmt::Debug>(
        &self,
        name: T,
        search_input: String,
    ) -> Vec<ConnectedClient>;
}

impl FilterClients for Vec<ConnectedClient> {
    fn filter_by_client<T: IntoIterator<Item = S>, S: AsRef<str> + std::fmt::Debug>(
        &self,
        name: T,
        search_input: String,
    ) -> Vec<ConnectedClient> {
        // Create a fuzzy matcher with default settings, ignoring case.
        let matcher = SkimMatcherV2::default().ignore_case();

        // Initialize a vector to hold the match results.
        let mut match_results = name
            // Convert the input iterator into an iterator of the items.
            .into_iter()
            // Filter and map the items based on the fuzzy match score.
            .filter_map(|s| {
                // Calculate the fuzzy match score and the matched indices.
                let score = matcher.fuzzy_indices(s.as_ref(), search_input.as_str());
                // If a match is found, map it to a tuple of (item, score, indices).
                score.map(|(score, indices)| (s, score, indices))
            })
            // Collect the filtered and mapped results into a vector.
            .collect::<Vec<_>>();

        // Sort the match results by score in descending order (higher scores first).
        match_results.sort_by_key(|k| Reverse(k.1));

        for (_i, (output, _, _match_indices)) in match_results.iter().take(6).enumerate() {
            return self
                .into_iter()
                .filter(|client| {
                    client.connection_string.contains(output.as_ref())
                        || client
                            .friendly_name
                            .clone()
                            .unwrap_or_default()
                            .contains(output.as_ref())
                })
                .cloned()
                .collect();
        }
        self.to_vec()
    }
}

#[cfg(test)]
mod deser_tests {
    use super::*;
    use crate::schema::COMPUTER_TABLE;
    use surrealdb::types::Value;

    fn sample() -> ConnectedClient {
        ConnectedClient {
            id: RecordId::new(CONNECTED_CLIENT_TABLE, "DeWittHome:0419a2598"),
            client_hash: "0419a2598deadbeefcafef00d".to_string(),
            connection_string: "DeWittHome:0419a2598".to_string(),
            connected: true,
            computer: Some(RecordId::new(COMPUTER_TABLE, "DeWittHome:0419a2598")),
            ..Default::default()
        }
    }

    fn sample_without(field: &str) -> Value {
        let mut v = sample().into_value();
        match &mut v {
            Value::Object(obj) => {
                obj.remove(field);
            }
            other => panic!("ConnectedClient should serialize to an object, got {other:?}"),
        }
        v
    }

    #[test]
    fn missing_client_hash_defaults_to_empty() {
        let parsed = ConnectedClient::from_value(sample_without("client_hash"))
            .expect("missing client_hash must not fail to deserialize");
        assert_eq!(parsed.client_hash, "");
    }

    #[test]
    fn missing_connection_string_defaults_to_empty() {
        let parsed = ConnectedClient::from_value(sample_without("connection_string"))
            .expect("missing connection_string must not fail to deserialize");
        assert_eq!(parsed.connection_string, "");
    }

    #[test]
    fn missing_connected_defaults_to_false() {
        let parsed = ConnectedClient::from_value(sample_without("connected"))
            .expect("missing connected must not fail to deserialize");
        assert!(!parsed.connected);
    }

    #[test]
    fn missing_all_undeclared_fields_deserializes() {
        let mut v = sample().into_value();
        if let Value::Object(obj) = &mut v {
            obj.remove("client_hash");
            obj.remove("connection_string");
            obj.remove("connected");
            obj.remove("computer");
        }
        let parsed = ConnectedClient::from_value(v)
            .expect("a row missing every undeclared field must still deserialize");
        assert_eq!(parsed.client_hash, "");
        assert_eq!(parsed.connection_string, "");
        assert!(!parsed.connected);
        assert!(parsed.computer.is_none());
    }

    #[test]
    fn reported_production_record_deserializes() {
        // The exact reported row: connection_string present, client_hash and
        // computer absent.
        let mut v = sample().into_value();
        if let Value::Object(obj) = &mut v {
            obj.remove("client_hash");
            obj.remove("computer");
        }
        let parsed =
            ConnectedClient::from_value(v).expect("the reported production record must deserialize");
        assert_eq!(parsed.client_hash, "");
        assert_eq!(parsed.connection_string, "DeWittHome:0419a2598");
        assert!(parsed.connected);
    }

    #[test]
    fn row_missing_every_non_option_field_deserializes() {
        let mut v = sample().into_value();
        if let Value::Object(obj) = &mut v {
            for f in [
                "client_hash",
                "connection_string",
                "connected",
                "customer_locked",
                "client_kind",
            ] {
                obj.remove(f);
            }
        }
        let parsed = ConnectedClient::from_value(v)
            .expect("a row with only id (plus optional fields) must deserialize");
        assert_eq!(parsed.client_hash, "");
        assert_eq!(parsed.connection_string, "");
        assert!(!parsed.connected);
        assert!(!parsed.customer_locked);
        assert_eq!(parsed.client_kind, ClientKind::Machine);
    }

    #[test]
    fn full_record_round_trips() {
        let original = sample();
        let parsed = ConnectedClient::from_value(original.clone().into_value())
            .expect("a fully-populated record round-trips");
        assert_eq!(parsed.client_hash, original.client_hash);
        assert_eq!(parsed.connection_string, original.connection_string);
        assert_eq!(parsed.connected, original.connected);
        assert_eq!(parsed.computer, original.computer);
    }
}
