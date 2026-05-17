//! Shared `cargo build` orchestration used by both transport modes
//! (WS in `main.rs`, DB in [`crate::db_mode`]).
//!
//! A worker's job is always the same: drop the supplied `Cargo.toml`
//! + `lib.rs` onto disk, shell out to `cargo build --target <triple>`
//! with a persistent per-plugin `CARGO_TARGET_DIR`, read the resulting
//! `.wasm`. The only difference between transports is how the request
//! arrives and where the bytes are sent on completion.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use url::Url;

#[derive(Debug)]
pub struct Config {
    pub ws_url: Url,
    pub hostname: String,
    pub target_triples: Vec<String>,
    pub scratch_root: PathBuf,
    pub target_cache_root: PathBuf,
}

impl Clone for Config {
    fn clone(&self) -> Self {
        Self {
            ws_url: self.ws_url.clone(),
            hostname: self.hostname.clone(),
            target_triples: self.target_triples.clone(),
            scratch_root: self.scratch_root.clone(),
            target_cache_root: self.target_cache_root.clone(),
        }
    }
}

#[derive(Debug)]
pub struct BuildArtifact {
    pub wasm_bytes: Vec<u8>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug)]
pub enum BuildFailure {
    Setup(anyhow::Error),
    Cargo {
        dur: Duration,
        stdout: String,
        stderr: String,
    },
}

impl From<anyhow::Error> for BuildFailure {
    fn from(e: anyhow::Error) -> Self {
        BuildFailure::Setup(e)
    }
}

/// Sanitize an arbitrary string for use as a single path component or
/// websocket room id. Lowercase letters, digits, `-`, `_` survive;
/// everything else collapses to `_`.
pub fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

/// Run one `cargo build` invocation.
///
/// Layout (per-job scratch dir, per-plugin target dir):
/// ```text
/// <scratch_root>/<plugin_id>/<job_id>/Cargo.toml
/// <scratch_root>/<plugin_id>/<job_id>/src/lib.rs
/// CARGO_TARGET_DIR=<target_cache_root>/<plugin_id>
/// ```
/// The target dir is shared across jobs for the same plugin so
/// incremental compilation wins on iterative builds.
pub async fn compile_one(
    cfg: &Config,
    job_id: &str,
    plugin_id: &str,
    cargo_toml: &str,
    lib_rs: &str,
    target: &str,
    profile: &str,
) -> Result<BuildArtifact, BuildFailure> {
    let safe_plugin = sanitize(plugin_id);
    let job_dir = cfg.scratch_root.join(&safe_plugin).join(job_id);
    let src_dir = job_dir.join("src");
    let cargo_target = cfg.target_cache_root.join(&safe_plugin);

    tokio::fs::create_dir_all(&src_dir)
        .await
        .with_context(|| format!("create {}", src_dir.display()))?;
    tokio::fs::create_dir_all(&cargo_target)
        .await
        .with_context(|| format!("create {}", cargo_target.display()))?;
    tokio::fs::write(job_dir.join("Cargo.toml"), cargo_toml)
        .await
        .context("write Cargo.toml")?;
    tokio::fs::write(src_dir.join("lib.rs"), lib_rs)
        .await
        .context("write lib.rs")?;

    let mut cmd = tokio::process::Command::new("cargo");
    cmd.arg("build")
        .arg("--target")
        .arg(target)
        .arg("--message-format=json");
    if profile == "release" {
        cmd.arg("--release");
    } else if !profile.is_empty() && profile != "dev" {
        cmd.arg("--profile").arg(profile);
    }
    cmd.current_dir(&job_dir).env("CARGO_TARGET_DIR", &cargo_target);

    let start = Instant::now();
    let output = cmd
        .output()
        .await
        .context("spawn cargo")
        .map_err(BuildFailure::Setup)?;
    let dur = start.elapsed();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    if !output.status.success() {
        log::warn!("[{job_id}] cargo build failed in {:?}", dur);
        return Err(BuildFailure::Cargo { dur, stdout, stderr });
    }

    let wasm_path = find_wasm_artifact(&cargo_target, target, profile, &safe_plugin)
        .await
        .map_err(BuildFailure::Setup)?;
    let bytes = tokio::fs::read(&wasm_path)
        .await
        .with_context(|| format!("read {}", wasm_path.display()))
        .map_err(BuildFailure::Setup)?;

    log::info!(
        "[{job_id}] built {} bytes in {:?} ({})",
        bytes.len(),
        dur,
        wasm_path.display()
    );
    Ok(BuildArtifact {
        wasm_bytes: bytes,
        stdout,
        stderr,
    })
}

async fn find_wasm_artifact(
    cargo_target: &Path,
    target: &str,
    profile: &str,
    sanitized_plugin: &str,
) -> Result<PathBuf> {
    let profile_dir = if profile == "release" || profile.is_empty() {
        "release"
    } else if profile == "dev" {
        "debug"
    } else {
        profile
    };
    let release_dir = cargo_target.join(target).join(profile_dir);
    let underscored = sanitized_plugin.replace('-', "_");
    let primary = release_dir.join(format!("{underscored}.wasm"));
    if tokio::fs::try_exists(&primary).await.unwrap_or(false) {
        return Ok(primary);
    }
    let fallback = release_dir.join(format!("{sanitized_plugin}.wasm"));
    if tokio::fs::try_exists(&fallback).await.unwrap_or(false) {
        return Ok(fallback);
    }
    anyhow::bail!(
        "no .wasm artifact under {}; tried {} and {}",
        release_dir.display(),
        primary.display(),
        fallback.display(),
    );
}
