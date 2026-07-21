//! Native integration test for the multifile sibling layout.
//!
//! Materializes a stub SDK-dependent job (a `_mtech_sdk_vendor` sibling
//! crate + a plugin that depends on it via `path = "../_mtech_sdk_vendor"`)
//! and asserts `compile_one` builds it to wasm through the sibling
//! layout. Needs the `wasm32-wasip1` target, so it is `#[ignore]`.
//!
//! Run manually:
//!   rustup target add wasm32-wasip1
//!   cargo test -p plugin_builder --test compile_multifile -- --ignored

use std::path::PathBuf;

use database::schema::BuildFile;
use plugin_builder::compile::{compile_one, Config};
use url::Url;

const VENDOR_CARGO: &str = r#"[package]
name = "mtech-sdk-stub"
version = "0.0.0"
edition = "2021"

[lib]

[workspace]
"#;

const VENDOR_LIB: &str = "pub fn sdk_marker() -> u32 { 42 }\n";

const PLUGIN_CARGO: &str = r#"[package]
name = "stub-plugin"
version = "0.1.0"
edition = "2021"

[workspace]

[lib]
crate-type = ["cdylib"]

[dependencies]
mtech-sdk-stub = { path = "../_mtech_sdk_vendor" }
"#;

const PLUGIN_LIB: &str = r#"use mtech_sdk_stub::sdk_marker;

#[no_mangle]
pub extern "C" fn go() -> u32 {
    sdk_marker()
}
"#;

fn temp_root(tag: &str) -> PathBuf {
    let unique = format!(
        "mtech-builder-test-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    std::env::temp_dir().join(unique)
}

#[tokio::test]
#[ignore = "needs the wasm32-wasip1 target; run with -- --ignored"]
async fn sibling_layout_builds_sdk_dependent_plugin() {
    let scratch_root = temp_root("scratch");
    let target_cache_root = temp_root("cache");

    let cfg = Config {
        ws_url: Url::parse("ws://127.0.0.1:8081/websocket").unwrap(),
        hostname: "test-host".to_string(),
        target_triples: vec!["wasm32-wasip1".to_string()],
        scratch_root: scratch_root.clone(),
        target_cache_root: target_cache_root.clone(),
    };

    let extra_files = vec![
        BuildFile {
            path: "_mtech_sdk_vendor/Cargo.toml".to_string(),
            content: VENDOR_CARGO.to_string(),
        },
        BuildFile {
            path: "_mtech_sdk_vendor/src/lib.rs".to_string(),
            content: VENDOR_LIB.to_string(),
        },
    ];

    let artifact = compile_one(
        &cfg,
        "job-multifile-1",
        "stub-plugin",
        PLUGIN_CARGO,
        PLUGIN_LIB,
        "wasm32-wasip1",
        "release",
        &extra_files,
    )
    .await
    .expect("sibling-layout SDK plugin should build to wasm");

    assert!(
        !artifact.wasm_bytes.is_empty(),
        "expected a non-empty .wasm artifact"
    );
    assert_eq!(
        &artifact.wasm_bytes[..4],
        b"\0asm",
        "artifact should carry the wasm magic header"
    );

    // The sibling crate must have been materialized next to the plugin.
    let job_dir = scratch_root.join("stub-plugin").join("job-multifile-1");
    assert!(job_dir.join("plugin").join("Cargo.toml").exists());
    assert!(job_dir.join("_mtech_sdk_vendor").join("Cargo.toml").exists());
    assert!(job_dir.join("_mtech_sdk_vendor").join("src").join("lib.rs").exists());

    let _ = std::fs::remove_dir_all(&scratch_root);
    let _ = std::fs::remove_dir_all(&target_cache_root);
}
