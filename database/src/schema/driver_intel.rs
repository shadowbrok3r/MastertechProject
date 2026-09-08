//! Driver time machine.
//!
//! Point-in-time driver inventories (`driver_snapshot`) captured at intake and
//! before service work, diffable between visits, plus the fleet blocklist
//! (`known_bad_driver`) that triage matches inventories and crash modules against.

use serde::{Deserialize, Serialize};

use crate::db;

use super::{crash_intel::module_stem, Datetime, RecordId, SurrealValue};

/// One installed third-party driver package.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, SurrealValue)]
pub struct DriverRecord {
    #[serde(default)]
    pub published_name: String,
    #[serde(default)]
    pub original_name: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub class_name: String,
    #[serde(default)]
    pub driver_version: String,
    #[serde(default)]
    pub driver_date: String,
    #[serde(default)]
    pub signer: String,
    #[serde(default)]
    pub device_name: Option<String>,
}

impl DriverRecord {
    /// Blocklist-matching key: original INF stem, falling back to the published name.
    pub fn key(&self) -> String {
        let primary = if self.original_name.is_empty() {
            &self.published_name
        } else {
            &self.original_name
        };
        module_stem(primary)
    }

    /// Diff key: one installed package. Several packages can share an INF stem.
    pub fn package_key(&self) -> String {
        format!(
            "{}|{}",
            module_stem(&self.published_name),
            module_stem(&self.original_name)
        )
    }
}

/// Full driver inventory of one machine at one point in time.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct DriverSnapshot {
    pub id: RecordId,
    pub connection_string: String,
    #[serde(default)]
    pub computer: Option<RecordId>,
    #[serde(default)]
    pub session_ref: Option<RecordId>,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub source: String,
    pub taken_at: Datetime,
    #[serde(default)]
    pub driver_count: u32,
    #[serde(default)]
    pub drivers: Vec<DriverRecord>,
    #[serde(default)]
    pub notes: String,
}

/// Fleet blocklist entry matched during triage.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct KnownBadDriver {
    pub id: RecordId,
    pub module: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub vendor: String,
    #[serde(default)]
    pub bad_versions: Vec<String>,
    #[serde(default)]
    pub fixed_version: Option<String>,
    #[serde(default)]
    pub symptom: String,
    #[serde(default)]
    pub fix: String,
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub signature_ref: Option<RecordId>,
    #[serde(default)]
    pub active: bool,
    pub created_at: Datetime,
    pub updated_at: Datetime,
}

/// Diff between two snapshots of the same machine.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DriverDiff {
    pub added: Vec<DriverRecord>,
    pub removed: Vec<DriverRecord>,
    pub changed: Vec<DriverChange>,
}

/// Same package, different version between snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverChange {
    pub key: String,
    pub published_name: String,
    pub provider: String,
    pub class_name: String,
    pub old_version: String,
    pub new_version: String,
    pub old_date: String,
    pub new_date: String,
}

/// A blocklist hit against one installed driver.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownBadHit {
    pub driver: DriverRecord,
    pub entry: KnownBadDriver,
}

impl DriverDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

/// Parse `pnputil /enum-drivers` output into driver records.
pub fn parse_pnputil_enum(text: &str) -> Vec<DriverRecord> {
    let mut drivers = Vec::new();
    let mut current: Option<DriverRecord> = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        match key.as_str() {
            "published name" => {
                if let Some(done) = current.take() {
                    drivers.push(done);
                }
                current = Some(DriverRecord {
                    published_name: value.to_string(),
                    ..Default::default()
                });
            }
            "original name" | "original file name" => {
                if let Some(d) = current.as_mut() {
                    d.original_name = value.to_string();
                }
            }
            "provider name" => {
                if let Some(d) = current.as_mut() {
                    d.provider = value.to_string();
                }
            }
            "class name" => {
                if let Some(d) = current.as_mut() {
                    d.class_name = value.to_string();
                }
            }
            "driver version" => {
                if let Some(d) = current.as_mut() {
                    let mut parts = value.split_whitespace();
                    d.driver_date = parts.next().unwrap_or_default().to_string();
                    d.driver_version = parts.next().unwrap_or_default().to_string();
                }
            }
            "signer name" => {
                if let Some(d) = current.as_mut() {
                    d.signer = value.to_string();
                }
            }
            _ => {}
        }
    }
    if let Some(done) = current.take() {
        drivers.push(done);
    }
    drivers
}

/// Parse the dump-decode `drivers` tool payload (WMI Win32_PnPSignedDriver rows).
pub fn parse_wmi_driver_payload(payload: &serde_json::Value) -> Vec<DriverRecord> {
    let data = payload.get("data").unwrap_or(payload);
    let Some(rows) = data.get("drivers").and_then(|d| d.as_array()) else {
        return Vec::new();
    };
    rows.iter()
        .map(|r| {
            let s = |k: &str| {
                r.get(k)
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            };
            DriverRecord {
                published_name: String::new(),
                original_name: s("inf"),
                provider: s("prov"),
                class_name: String::new(),
                driver_version: s("ver"),
                driver_date: s("date"),
                signer: String::new(),
                device_name: Some(s("dev")).filter(|v| !v.is_empty()),
            }
        })
        .filter(|d| !d.original_name.is_empty() || d.device_name.is_some())
        .collect()
}

/// Diff two inventories per installed package, pairing leftovers within a package as version changes.
pub fn diff_driver_sets(older: &[DriverRecord], newer: &[DriverRecord]) -> DriverDiff {
    use std::collections::BTreeMap;

    type Bucket<'a> = (Vec<&'a DriverRecord>, Vec<&'a DriverRecord>);
    let mut buckets: BTreeMap<String, Bucket<'_>> = BTreeMap::new();
    for d in older {
        buckets.entry(d.package_key()).or_default().0.push(d);
    }
    for d in newer {
        buckets.entry(d.package_key()).or_default().1.push(d);
    }

    let mut diff = DriverDiff::default();
    for (olds, news) in buckets.into_values() {
        // Drop version matches pairwise, leaving only the drift on each side.
        let mut new_left = news;
        let mut old_left = Vec::new();
        for old_d in olds {
            match new_left
                .iter()
                .position(|n| n.driver_version == old_d.driver_version)
            {
                Some(i) => {
                    new_left.remove(i);
                }
                None => old_left.push(old_d),
            }
        }
        old_left.sort_by(|a, b| a.driver_version.cmp(&b.driver_version));
        new_left.sort_by(|a, b| a.driver_version.cmp(&b.driver_version));

        for (old_d, new_d) in old_left.iter().zip(new_left.iter()) {
            diff.changed.push(DriverChange {
                key: new_d.key(),
                published_name: new_d.published_name.clone(),
                provider: new_d.provider.clone(),
                class_name: new_d.class_name.clone(),
                old_version: old_d.driver_version.clone(),
                new_version: new_d.driver_version.clone(),
                old_date: old_d.driver_date.clone(),
                new_date: new_d.driver_date.clone(),
            });
        }
        let paired = old_left.len().min(new_left.len());
        diff.removed
            .extend(old_left[paired..].iter().map(|d| (*d).clone()));
        diff.added
            .extend(new_left[paired..].iter().map(|d| (*d).clone()));
    }

    let by_package = |a: &DriverRecord, b: &DriverRecord| {
        a.package_key()
            .cmp(&b.package_key())
            .then_with(|| a.driver_version.cmp(&b.driver_version))
    };
    diff.added.sort_by(by_package);
    diff.removed.sort_by(by_package);
    diff.changed.sort_by(|a, b| {
        a.key
            .cmp(&b.key)
            .then_with(|| a.published_name.cmp(&b.published_name))
    });
    diff
}

fn version_matches(matcher: &str, version: &str) -> bool {
    let m = matcher.trim();
    let v = version.trim();
    !m.is_empty() && (v == m || v.starts_with(&format!("{m}.")))
}

impl DriverSnapshot {
    pub async fn create(snapshot: &Self) -> anyhow::Result<RecordId> {
        let mut s = snapshot.clone();
        s.id = super::random_record_id(super::DRIVER_SNAPSHOT_TABLE);
        s.taken_at = chrono::Utc::now().into();
        s.driver_count = s.drivers.len() as u32;
        let created: Option<Self> = db().create(s.id.clone()).content(s.clone()).await?;
        Ok(created.map(|c| c.id).unwrap_or(s.id))
    }

    /// Snapshots for a machine, newest first (inventories included).
    pub async fn list_for_connection(
        connection_string: &str,
        limit: u32,
    ) -> anyhow::Result<Vec<Self>> {
        let rows: Vec<Self> = db()
            .query("SELECT * FROM driver_snapshot WHERE connection_string == $cs ORDER BY taken_at DESC LIMIT $limit")
            .bind(("cs", connection_string.to_string()))
            .bind(("limit", limit as i64))
            .await?
            .take(0)?;
        Ok(rows)
    }

    pub async fn get(id: &RecordId) -> anyhow::Result<Option<Self>> {
        Ok(db().select(id.clone()).await?)
    }

    /// Snapshot metadata (no inventories) for a machine, newest first.
    pub async fn list_meta_for_connection(
        connection_string: &str,
        limit: u32,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        let rows: Vec<serde_json::Value> = db()
            .query(
                "SELECT id, label, source, taken_at, driver_count, notes \
                 FROM driver_snapshot WHERE connection_string == $cs \
                 ORDER BY taken_at DESC LIMIT $limit",
            )
            .bind(("cs", connection_string.to_string()))
            .bind(("limit", limit as i64))
            .await?
            .take(0)?;
        Ok(rows)
    }
}

impl KnownBadDriver {
    /// All active blocklist entries.
    pub async fn active() -> anyhow::Result<Vec<Self>> {
        let rows: Vec<Self> = db()
            .query("SELECT * FROM known_bad_driver WHERE active == true ORDER BY module ASC")
            .await?
            .take(0)?;
        Ok(rows)
    }

    pub async fn create(entry: &Self) -> anyhow::Result<RecordId> {
        let mut e = entry.clone();
        e.id = super::random_record_id(super::KNOWN_BAD_DRIVER_TABLE);
        e.module = module_stem(&e.module);
        e.created_at = chrono::Utc::now().into();
        e.updated_at = e.created_at.clone();
        let created: Option<Self> = db().create(e.id.clone()).content(e.clone()).await?;
        Ok(created.map(|c| c.id).unwrap_or(e.id))
    }

    pub async fn set_active(id: &RecordId, active: bool) -> anyhow::Result<()> {
        db()
            .query("UPDATE $id SET active = $active, updated_at = time::now()")
            .bind(("id", id.clone()))
            .bind(("active", active))
            .await?;
        Ok(())
    }

    /// Blocklist entries whose module stem matches a crash module.
    pub async fn matching_module(module: &str) -> anyhow::Result<Vec<Self>> {
        let stem = module_stem(module);
        let rows: Vec<Self> = db()
            .query("SELECT * FROM known_bad_driver WHERE active == true AND module == $stem")
            .bind(("stem", stem))
            .await?
            .take(0)?;
        Ok(rows)
    }

    /// Match an inventory against a blocklist; empty `bad_versions` hits every version.
    pub fn match_inventory(entries: &[Self], drivers: &[DriverRecord]) -> Vec<KnownBadHit> {
        let mut hits = Vec::new();
        for entry in entries.iter().filter(|e| e.active) {
            for driver in drivers {
                if driver.key() != entry.module {
                    continue;
                }
                let version_hit = entry.bad_versions.is_empty()
                    || entry
                        .bad_versions
                        .iter()
                        .any(|m| version_matches(m, &driver.driver_version));
                if version_hit {
                    hits.push(KnownBadHit {
                        driver: driver.clone(),
                        entry: entry.clone(),
                    });
                }
            }
        }
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNPUTIL_TEXT: &str = "\
Microsoft PnP Utility\n\
\n\
Published Name:     oem12.inf\n\
Original Name:      rtwlane.inf\n\
Provider Name:      Realtek Semiconductor Corp.\n\
Class Name:         Network adapters\n\
Class GUID:         {4d36e972-e325-11ce-bfc1-08002be10318}\n\
Driver Version:     12/06/2023 6001.15.128.1029\n\
Signer Name:        Microsoft Windows Hardware Compatibility Publisher\n\
\n\
Published Name:     oem43.inf\n\
Original Name:      u0396530.inf\n\
Provider Name:      Advanced Micro Devices, Inc.\n\
Class Name:         Display adapters\n\
Class GUID:         {4d36e968-e325-11ce-bfc1-08002be10318}\n\
Driver Version:     11/14/2023 31.0.22023.1014\n\
Signer Name:        Microsoft Windows Hardware Compatibility Publisher\n";

    fn record(original: &str, version: &str) -> DriverRecord {
        DriverRecord {
            original_name: original.to_string(),
            driver_version: version.to_string(),
            ..Default::default()
        }
    }

    /// `(published_name, original_name, driver_version, driver_date)` fixture rows.
    fn packages(rows: &[(&str, &str, &str, &str)]) -> Vec<DriverRecord> {
        rows.iter()
            .map(|(published, original, version, date)| DriverRecord {
                published_name: published.to_string(),
                original_name: original.to_string(),
                provider: "Realtek".to_string(),
                class_name: "Net".to_string(),
                driver_version: version.to_string(),
                driver_date: date.to_string(),
                ..Default::default()
            })
            .collect()
    }

    #[test]
    fn parses_pnputil_output() {
        let drivers = parse_pnputil_enum(PNPUTIL_TEXT);
        assert_eq!(drivers.len(), 2);
        assert_eq!(drivers[0].published_name, "oem12.inf");
        assert_eq!(drivers[0].original_name, "rtwlane.inf");
        assert_eq!(drivers[0].driver_date, "12/06/2023");
        assert_eq!(drivers[0].driver_version, "6001.15.128.1029");
        assert_eq!(drivers[1].provider, "Advanced Micro Devices, Inc.");
    }

    #[test]
    fn diffs_driver_sets() {
        let old = vec![record("rtwlane.inf", "6001.15.128.1029"), record("gone.inf", "1.0")];
        let new = vec![record("rtwlane.inf", "6001.80.132.0"), record("added.inf", "2.0")];
        let diff = diff_driver_sets(&old, &new);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.removed.len(), 1);
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].old_version, "6001.15.128.1029");
        assert_eq!(diff.changed[0].new_version, "6001.80.132.0");
    }

    #[test]
    fn diffs_packages_sharing_an_inf_stem() {
        let intake = packages(&[
            ("oem35.inf", "rt640x64.inf", "10.7.107.2016", "01/07/2016"),
            ("oem119.inf", "rt640x64.inf", "10.74.1128.2024", "11/28/2024"),
            ("oem64.inf", "rt640x64.inf", "10.38.1118.2019", "11/18/2019"),
        ]);
        let post_service = packages(&[
            ("oem35.inf", "rt640x64.inf", "10.7.107.2016", "01/07/2016"),
            ("oem119.inf", "rt640x64.inf", "10.74.1128.2024", "11/28/2024"),
            ("oem46.inf", "rt640x64.inf", "10.79.50.1003", "10/03/2025"),
            ("oem64.inf", "rt640x64.inf", "10.38.1118.2019", "11/18/2019"),
        ]);

        let diff = diff_driver_sets(&intake, &post_service);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.added[0].published_name, "oem46.inf");
        assert_eq!(diff.added[0].driver_version, "10.79.50.1003");
        assert!(diff.removed.is_empty());
        assert!(diff.changed.is_empty());
    }

    #[test]
    fn version_bump_of_one_package_sharing_a_stem_is_a_change() {
        let older = packages(&[
            ("oem35.inf", "rt640x64.inf", "10.7.107.2016", "01/07/2016"),
            ("oem119.inf", "rt640x64.inf", "10.74.1128.2024", "11/28/2024"),
        ]);
        let newer = packages(&[
            ("oem35.inf", "rt640x64.inf", "10.7.107.2016", "01/07/2016"),
            ("oem119.inf", "rt640x64.inf", "10.79.50.1003", "10/03/2025"),
        ]);

        let diff = diff_driver_sets(&older, &newer);
        assert!(diff.added.is_empty());
        assert!(diff.removed.is_empty());
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].key, "rt640x64");
        assert_eq!(diff.changed[0].published_name, "oem119.inf");
        assert_eq!(diff.changed[0].old_version, "10.74.1128.2024");
        assert_eq!(diff.changed[0].new_version, "10.79.50.1003");
        assert_eq!(diff.changed[0].new_date, "10/03/2025");
    }

    #[test]
    fn removes_one_package_of_a_shared_stem() {
        let older = packages(&[
            ("oem35.inf", "rt640x64.inf", "10.7.107.2016", "01/07/2016"),
            ("oem64.inf", "rt640x64.inf", "10.38.1118.2019", "11/18/2019"),
        ]);
        let newer = packages(&[("oem35.inf", "rt640x64.inf", "10.7.107.2016", "01/07/2016")]);

        let diff = diff_driver_sets(&older, &newer);
        assert!(diff.added.is_empty());
        assert!(diff.changed.is_empty());
        assert_eq!(diff.removed.len(), 1);
        assert_eq!(diff.removed[0].published_name, "oem64.inf");
    }

    #[test]
    fn matches_known_bad() {
        let entry = KnownBadDriver {
            id: RecordId::new("known_bad_driver", "t"),
            module: "rtwlane".to_string(),
            display_name: String::new(),
            vendor: String::new(),
            bad_versions: vec!["6001.15".to_string()],
            fixed_version: None,
            symptom: String::new(),
            fix: String::new(),
            severity: "critical".to_string(),
            signature_ref: None,
            active: true,
            created_at: chrono::Utc::now().into(),
            updated_at: chrono::Utc::now().into(),
        };
        let inventory = vec![record("rtwlane.inf", "6001.15.128.1029")];
        let hits = KnownBadDriver::match_inventory(&[entry.clone()], &inventory);
        assert_eq!(hits.len(), 1);

        let newer = vec![record("rtwlane.inf", "6001.80.132.0")];
        assert!(KnownBadDriver::match_inventory(&[entry], &newer).is_empty());
    }

    #[test]
    fn wmi_payload_parses() {
        let payload = serde_json::json!({
            "tool": "drivers",
            "data": { "count": 1, "drivers": [
                { "dev": "Realtek 8821CE", "prov": "Realtek", "ver": "6001.15.128.1029", "date": "2023-12-06", "inf": "oem12.inf" }
            ]}
        });
        let drivers = parse_wmi_driver_payload(&payload);
        assert_eq!(drivers.len(), 1);
        assert_eq!(drivers[0].device_name.as_deref(), Some("Realtek 8821CE"));
    }
}
