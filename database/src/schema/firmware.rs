use crate::db;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{Datetime, RecordId, SurrealValue};

/// Bucket holding capsule artifacts.
pub const CAPSULE_BUCKET: &str = "capsules";

/// A vendor BIOS capsule published for the fleet to flash. Firmware never
/// receives an operator-supplied URL — it is handed a `capsule_id` and fetches
/// the bytes from us, so the set of flashable images is exactly this table.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct FirmwareCapsule {
    pub id: RecordId,
    /// Stable operator-facing key, e.g. `dell-latitude-5540-1.20.0`.
    pub capsule_id: String,
    /// SMBIOS baseboard/system product this capsule is built for.
    pub board_product: String,
    /// ESRT firmware-class GUID the capsule targets.
    pub fw_class: String,
    /// Version this capsule installs, compared against the ESRT `fw_version`.
    pub version: u32,
    /// ESRT lowest-supported version the vendor declares, if known.
    #[surreal(default)]
    pub lowest_supported: Option<u32>,
    /// Lowercase hex SHA-256 of the capsule bytes.
    pub sha256: String,
    pub size_bytes: u64,
    /// Path inside [`CAPSULE_BUCKET`]; `None` until the artifact is uploaded.
    #[surreal(default)]
    pub bucket_path: Option<String>,
    pub published_by: String,
    pub published_at: Datetime,
    #[surreal(default)]
    pub notes: Option<String>,
}

impl Default for FirmwareCapsule {
    fn default() -> Self {
        Self {
            id: super::random_record_id(super::FIRMWARE_CAPSULE_TABLE),
            capsule_id: String::new(),
            board_product: String::new(),
            fw_class: String::new(),
            version: 0,
            lowest_supported: None,
            sha256: String::new(),
            size_bytes: 0,
            bucket_path: None,
            published_by: String::new(),
            published_at: chrono::Utc::now().into(),
            notes: None,
        }
    }
}

impl FirmwareCapsule {
    /// Keyed by `capsule_id` so lookups are O(1) and duplicates impossible.
    pub async fn get(capsule_id: &str) -> anyhow::Result<Option<Self>> {
        let rid = RecordId::new(super::FIRMWARE_CAPSULE_TABLE, capsule_id);
        Ok(db().select(rid).await?)
    }

    pub async fn list() -> anyhow::Result<Vec<Self>> {
        Ok(db().select(super::FIRMWARE_CAPSULE_TABLE).await?)
    }

    /// Store the artifact then the row, so a row never points at missing bytes.
    pub async fn publish(mut entry: Self, bytes: Vec<u8>) -> anyhow::Result<Self> {
        if entry.capsule_id.trim().is_empty() {
            anyhow::bail!("capsule_id is required");
        }
        let digest = sha256_hex(&bytes);
        let path = format!("/{}/{}.cap", sanitize(&entry.capsule_id), entry.version);
        ensure_capsule_bucket().await?;
        super::put_file(CAPSULE_BUCKET, &path, bytes.clone()).await?;

        entry.id = RecordId::new(super::FIRMWARE_CAPSULE_TABLE, entry.capsule_id.clone());
        entry.sha256 = digest;
        entry.size_bytes = bytes.len() as u64;
        entry.bucket_path = Some(path);
        entry.published_at = chrono::Utc::now().into();

        let rid = entry.id.clone();
        let stored: Option<Self> = db().upsert(rid).content(entry.clone()).await?;
        Ok(stored.unwrap_or(entry))
    }

    /// Fetch the artifact and verify it still hashes to what was published.
    pub async fn fetch_bytes(&self) -> anyhow::Result<Vec<u8>> {
        let path = self
            .bucket_path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("capsule '{}' has no artifact", self.capsule_id))?;
        let bytes = super::get_file(CAPSULE_BUCKET, path)
            .await?
            .ok_or_else(|| anyhow::anyhow!("capsule missing from bucket at '{path}'"))?;
        let digest = sha256_hex(&bytes);
        if digest != self.sha256 {
            anyhow::bail!(
                "capsule '{}' failed integrity check (stored {}, got {digest})",
                self.capsule_id,
                self.sha256
            );
        }
        Ok(bytes)
    }
}

/// Define the capsule bucket if it does not exist. The backend path is
/// environment-dependent, so it lives here rather than in the schema files.
async fn ensure_capsule_bucket() -> anyhow::Result<()> {
    let base = if cfg!(debug_assertions) {
        if cfg!(target_os = "windows") {
            crate::BUCKET_DEV_WINDOWS_URL
        } else if cfg!(target_os = "linux") {
            crate::BUCKET_DEV_LINUX_URL
        } else {
            crate::BUCKET_URL
        }
    } else {
        crate::BUCKET_URL
    };
    let url = super::file_storage::join_bucket_path(base, CAPSULE_BUCKET);
    super::file_storage::define_bucket(CAPSULE_BUCKET, &url).await
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// Reduce an id to bucket-path-safe characters.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}
