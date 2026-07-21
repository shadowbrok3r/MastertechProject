//! Host-embedded vendored copy of `mtech-plugin-sdk`.
//!
//! The SDK source is baked into the Mastertech binary at compile time and
//! materialized into the plugin store as a sibling path crate so scaffolded
//! plugins can depend on it (`mtech-plugin-sdk = { path = "../_mtech_sdk_vendor" }`)
//! on tech machines that have the binary but not the repo.

use std::path::PathBuf;

use super::mcp_bridge::plugin_store_root;

const SDK_DIR_NAME: &str = "_mtech_sdk_vendor";

const SDK_CARGO_TOML: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../mtech-plugin-sdk/Cargo.toml"));
const SDK_LIB_RS: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../mtech-plugin-sdk/src/lib.rs"));
const SDK_ERROR_RS: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../mtech-plugin-sdk/src/error.rs"));
const SDK_SCHEMA_RS: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../mtech-plugin-sdk/src/schema.rs"));
const SDK_MARSHAL_RS: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../mtech-plugin-sdk/src/marshal.rs"));
const SDK_HOST_RS: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../mtech-plugin-sdk/src/host.rs"));
const SDK_DISPATCH_RS: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../mtech-plugin-sdk/src/dispatch.rs"));

const SDK_SOURCES: &[(&str, &str)] = &[
    ("lib.rs", SDK_LIB_RS),
    ("error.rs", SDK_ERROR_RS),
    ("schema.rs", SDK_SCHEMA_RS),
    ("marshal.rs", SDK_MARSHAL_RS),
    ("host.rs", SDK_HOST_RS),
    ("dispatch.rs", SDK_DISPATCH_RS),
];

fn vendor_dir() -> PathBuf {
    plugin_store_root().join(SDK_DIR_NAME)
}

// FNV-1a-64 over all embedded SDK sources; changes whenever any source changes.
fn sdk_stamp() -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    };
    eat(SDK_CARGO_TOML.as_bytes());
    for (name, body) in SDK_SOURCES {
        eat(name.as_bytes());
        eat(body.as_bytes());
    }
    format!("{hash:016x}")
}

/// The vendored SDK crate as `(relative_path, content)` pairs.
/// `Cargo.toml` carries the appended `[workspace]` exactly as
/// [`ensure_vendored_sdk`] writes it; sources land under `src/`.
pub(crate) fn vendored_sdk_files() -> Vec<(String, String)> {
    let mut files = Vec::with_capacity(1 + SDK_SOURCES.len());
    files.push(("Cargo.toml".to_string(), format!("{SDK_CARGO_TOML}\n[workspace]\n")));
    for (name, body) in SDK_SOURCES {
        files.push((format!("src/{name}"), (*body).to_string()));
    }
    files
}

/// Materialize the vendored SDK crate into the plugin store; idempotent via `.stamp`.
pub(crate) fn ensure_vendored_sdk() -> Result<(), String> {
    let dir = vendor_dir();
    let stamp = dir.join(".stamp");
    let current = sdk_stamp();
    if let Ok(existing) = std::fs::read_to_string(&stamp) {
        if existing.trim() == current {
            return Ok(());
        }
    }

    for (rel, body) in vendored_sdk_files() {
        let path = dir.join(&rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        std::fs::write(&path, body).map_err(|e| format!("write {}: {e}", path.display()))?;
    }

    std::fs::write(&stamp, current).map_err(|e| format!("write vendor .stamp: {e}"))?;
    Ok(())
}

/// Whether a plugin's Cargo.toml depends on `mtech-plugin-sdk`.
pub(crate) fn cargo_toml_uses_sdk(cargo_toml: &str) -> bool {
    cargo_toml.contains("mtech-plugin-sdk")
}
