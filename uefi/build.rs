//! Injects the default upload endpoint at compile time.
//!
//! Reads `ORCHESTRATOR_URL` from the process env first, then the workspace-root
//! `.env`, falling back to the production URL. Exposed to the app as
//! `env!("ORCHESTRATOR_URL")`.

use std::fs;

fn main() {
    println!("cargo:rerun-if-changed=../.env");
    println!("cargo:rerun-if-env-changed=ORCHESTRATOR_URL");

    let mut url = String::from("https://axum.master-tech.app");

    if let Ok(v) = std::env::var("ORCHESTRATOR_URL") {
        if !v.trim().is_empty() {
            url = v.trim().to_string();
        }
    } else if let Ok(text) = fs::read_to_string("../.env") {
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("ORCHESTRATOR_URL=") {
                let v = rest.trim().trim_matches('"').trim_matches('\'');
                if !v.is_empty() {
                    url = v.to_string();
                }
            }
        }
    }

    println!("cargo:rustc-env=ORCHESTRATOR_URL={url}");
}
