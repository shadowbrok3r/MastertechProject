//! Injects compile-time `env!(...)` values for the `database` crate.
//! Prefer a repo-root `.env`; otherwise reads matching keys from the process environment,
//! then applies non-secret defaults for bucket paths and integration base URLs.

use std::collections::HashMap;
use std::env;
use std::path::PathBuf;

/// Keys that must be present (non-empty) after loading `.env` / host env. No defaults.
const REQUIRED_NO_DEFAULT: &[&str] = &[
    "USER_SCOPE",
    "DB",
    "NS",
    "SCAFFOLD_URL",
    "SCAFFOLD_USER",
    "SCAFFOLD_PASS",
    "DB_URL",
    "STORAGE_URL",
    "REGION",
    "DB_URL_DEV",
    "DB_URL_LOCAL",
    "DB_URL_BETA",
    "WS_CLIENT_URL_LOCAL",
    "WS_MASTER_URL_LOCAL",
    "WS_CLIENT_URL",
    "WS_MASTER_URL",
    "ODOO_API_KEY",
    "SURREAL_GUEST_PASSWORD",
    "XIDAX_ADMIN_URL",
];

/// Every key this crate's `env!()` macros may read (used when no `.env` to pull from `std::env`).
const ALL_INJECT_KEYS: &[&str] = &[
    "USER_SCOPE",
    "DB",
    "NS",
    "SCAFFOLD_URL",
    "SCAFFOLD_USER",
    "SCAFFOLD_PASS",
    "DB_URL",
    "STORAGE_URL",
    "REGION",
    "DB_URL_DEV",
    "DB_URL_LOCAL",
    "DB_URL_BETA",
    "WS_CLIENT_URL_LOCAL",
    "WS_MASTER_URL_LOCAL",
    "WS_CLIENT_URL",
    "WS_MASTER_URL",
    "ODOO_API_KEY",
    "SURREAL_GUEST_PASSWORD",
    "BUCKET_DEV_WINDOWS_URL",
    "BUCKET_DEV_LINUX_URL",
    "BUCKET_URL",
    "ODOO_JSONRPC_URL",
    "ODOO_DB",
    "ODOO_UID",
    "PRESTASHOP_API_URL",
    "PRESTASHOP_API_URL_WASM",
    "XIDAX_ADMIN_URL",
];

fn apply_defaults(map: &mut HashMap<String, String>) {
    if map.get("BUCKET_DEV_LINUX_URL").map_or(true, |s| s.is_empty()) {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        map.insert(
            "BUCKET_DEV_LINUX_URL".into(),
            format!("{home}/.local/share/mastertech/surrealkv/"),
        );
    }
    if map.get("BUCKET_DEV_WINDOWS_URL").map_or(true, |s| s.is_empty()) {
        map.insert(
            "BUCKET_DEV_WINDOWS_URL".into(),
            "C:/SurrealBuckets/".into(),
        );
    }
    if map.get("BUCKET_URL").map_or(true, |s| s.is_empty()) {
        map.insert("BUCKET_URL".into(), "/SurrealBuckets".into());
    }
}

/// `database` embeds `env!(...)` for every key in `REQUIRED_NO_DEFAULT`. WASM bins (MtechServer2.0
/// / trunk) still compile `database` but often omit Prestashop / guest-only secrets. Fill obvious
/// placeholders only for `wasm32` targets so `trunk serve` works; set real values in `.env` for
/// production WASM or use native targets for full enforcement.
fn apply_wasm_compile_placeholders(map: &mut HashMap<String, String>) {
    let target = env::var("TARGET").unwrap_or_default();
    if !target.contains("wasm32") {
        return;
    }
    if map
        .get("SURREAL_GUEST_PASSWORD")
        .map_or(true, |s| s.is_empty())
    {
        map.insert(
            "SURREAL_GUEST_PASSWORD".into(),
            "wasm_build_placeholder_guest_password".into(),
        );
    }
}

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let env_path = PathBuf::from(&manifest_dir).join("..").join(".env");

    println!("cargo:rerun-if-changed={}", env_path.display());

    let mut map = HashMap::<String, String>::new();

    if env_path.exists() {
        for item in dotenvy::from_path_iter(&env_path).expect("read .env") {
            let (key, val) = item.expect("parse .env");
            map.insert(key, val);
        }
        // Allow missing keys to be filled from the process environment (CI, or new vars like
        // `SURREAL_GUEST_PASSWORD` not yet added to an existing `.env`).
        for key in ALL_INJECT_KEYS {
            if map.get(*key).map_or(true, |s| s.is_empty()) {
                if let Ok(val) = env::var(key) {
                    if !val.is_empty() {
                        map.insert((*key).to_string(), val);
                    }
                }
            }
        }
    } else {
        println!("cargo:warning=database: no ../.env — using process environment for known keys");
        for key in ALL_INJECT_KEYS {
            if let Ok(val) = env::var(key) {
                if !val.is_empty() {
                    map.insert((*key).to_string(), val);
                }
            }
        }
    }

    apply_defaults(&mut map);
    apply_wasm_compile_placeholders(&mut map);

    let mut missing: Vec<&'static str> = Vec::new();
    for key in REQUIRED_NO_DEFAULT {
        let bad = match map.get(*key) {
            None => true,
            Some(s) => s.is_empty(),
        };
        if bad {
            missing.push(key);
        }
    }

    if !missing.is_empty() {
        panic!(
            "database crate: missing or empty compile-time env for: {}.\n\
             Add a repo-root `.env` (see `.env.example`) or export these variables before `cargo build`.",
            missing.join(", ")
        );
    }

    for key in ALL_INJECT_KEYS {
        if let Some(val) = map.get(*key) {
            println!("cargo:rustc-env={key}={val}");
        }
    }
}
