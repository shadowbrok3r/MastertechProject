//! Injects compile-time `env!(...)` values for the `database` crate.
//! Prefer a repo-root `.env`; otherwise reads matching keys from the process environment,
//! then applies non-secret defaults for bucket paths and integration base URLs.

use std::collections::HashMap;
use std::env;
use std::path::PathBuf;

include!("../build_hash.rs");

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
    // DB_URL_LOCAL, WS_CLIENT_URL_LOCAL, WS_MASTER_URL_LOCAL are local-dev only —
    // CI and production builds never select them at runtime. They get sensible
    // defaults below so the build works without those secrets.
    "DB_URL_BETA",
    "WS_CLIENT_URL",
    "WS_MASTER_URL",
    "ODOO_API_KEY",
    "ODOO_JSONRPC_URL",
    "ODOO_DB",
    "ODOO_UID",
    "PRESTASHOP_API_URL",
    "PRESTASHOP_API_URL_WASM",
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
    // Fleet orchestrator (axum_server) endpoints — qc-app posts heartbeats and
    // polls commands against these. Picked at runtime by
    // `database::orchestrator_url()` based on `cfg(debug_assertions)`. Safe
    // defaults applied below so missing .env entries don't fail the build;
    // an empty URL just disables fleet reporting at runtime.
    "ORCHESTRATOR_URL",
    "ORCHESTRATOR_URL_DEV",
    // PrestaShop employee credential-check endpoint (QC tech sign-off).
    // Empty disables tech authentication at runtime.
    "PRESTASHOP_AUTH_URL",
    // Shopify Admin API (read-only token). Empty disables the Shopify order
    // backend at runtime; writes always go through the Worker, never here.
    "SHOPIFY_STORE_URL",
    "SHOPIFY_ADMIN_TOKEN",
    "SHOPIFY_API_VERSION",
    // Xidax Build Management API (build-mgmt.xidax.com). Empty key disables
    // the XBM client at runtime.
    "XBM_API_URL",
    "XBM_API_KEY",
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
    // Local-dev URLs: only meaningful on the developer's own machine.
    // CI and production builds never select the Local DB environment at
    // runtime, so placeholder loopback values are safe to embed.
    if map.get("DB_URL_LOCAL").map_or(true, |s| s.is_empty()) {
        map.insert("DB_URL_LOCAL".into(), "127.0.0.1:8000".into());
    }
    if map.get("WS_CLIENT_URL_LOCAL").map_or(true, |s| s.is_empty()) {
        map.insert("WS_CLIENT_URL_LOCAL".into(), "ws://127.0.0.1:8081".into());
    }
    if map.get("WS_MASTER_URL_LOCAL").map_or(true, |s| s.is_empty()) {
        map.insert("WS_MASTER_URL_LOCAL".into(), "ws://127.0.0.1:8080".into());
    }
    // Fleet orchestrator: empty string is a valid value meaning "fleet reporting
    // disabled". `database::orchestrator_url()` short-circuits the sink + poller
    // when the active URL is empty.
    if map.get("ORCHESTRATOR_URL").is_none() {
        map.insert("ORCHESTRATOR_URL".into(), String::new());
    }
    if map.get("ORCHESTRATOR_URL_DEV").is_none() {
        map.insert("ORCHESTRATOR_URL_DEV".into(), "http://localhost:8082".into());
    }
    // Empty string = feature disabled at runtime; never fails the build.
    for key in [
        "PRESTASHOP_AUTH_URL",
        "SHOPIFY_STORE_URL",
        "SHOPIFY_ADMIN_TOKEN",
        "XBM_API_KEY",
    ] {
        if map.get(key).is_none() {
            map.insert(key.into(), String::new());
        }
    }
    if map.get("SHOPIFY_API_VERSION").map_or(true, |s| s.is_empty()) {
        map.insert("SHOPIFY_API_VERSION".into(), "2025-01".into());
    }
    if map.get("XBM_API_URL").map_or(true, |s| s.is_empty()) {
        map.insert(
            "XBM_API_URL".into(),
            "https://build-mgmt.xidax.com/api/v1".into(),
        );
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
    emit_build_hash();

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
