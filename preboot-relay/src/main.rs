//! Plain-HTTP → HTTPS relay for pre-boot UEFI boxes.
//!
//! Firmware has no DNS/TLS, so it can only speak HTTP/1.1 to a LAN IPv4. This
//! relay listens on the LAN, re-originates each request over HTTPS to the real
//! axum server (correct Host/SNI, unlike a raw TCP tunnel), and passes the
//! response back. Every request is logged to stdout — a live console of all
//! firmware traffic.
//!
//! Run on the relay host (defaults suit 192.168.22.139):
//!   cargo run --release
//! Overrides: LISTEN_ADDR=0.0.0.0:8082  UPSTREAM_URL=https://axum.master-tech.app

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path as AxPath, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri, header::CONTENT_TYPE};
use axum::response::{IntoResponse, Response};

/// Route firmware fetches BIOSLove payloads from, content-addressed by digest.
const PAYLOAD_ROUTE: &str = "/api/v1/qc/bioslove/payload/{sha256}";

/// Route firmware fetches the model index from when no volume carries one.
const INDEX_ROUTE: &str = "/api/v1/qc/bioslove/index";

/// Route firmware fetches the EFI Shell from. Vendor flashers are EDK2 StdLib
/// applications and hang when launched without one, so it is served like the
/// index rather than being a per-model payload.
const SHELL_ROUTE: &str = "/api/v1/qc/bioslove/shell";

/// BIOSLove ships the shell under this name at the share root.
const SHELL_FILE: &str = "_BiosLove.efi";

/// Ceiling on one share read. A 16 MiB ROM takes well under a second on the LAN;
/// anything near this means the share is gone.
const SHARE_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

#[derive(Clone)]
struct Relay {
    client: reqwest::Client,
    upstream: String,
    /// sha256 -> file on the firmware share. Empty when no index was loaded.
    payloads: Arc<HashMap<String, PathBuf>>,
    /// The index document itself, served to firmware booted off a bare stick.
    index_bytes: Arc<Vec<u8>>,
    /// Share root, for artifacts served by name rather than digest.
    share: Arc<String>,
}

#[tokio::main]
async fn main() {
    let upstream = std::env::var("UPSTREAM_URL")
        .unwrap_or_else(|_| "https://axum.master-tech.app".to_string())
        .trim_end_matches('/')
        .to_string();
    let listen =
        std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:8082".to_string());
    // Payloads are served from the LAN share, not proxied: the upstream cannot
    // reach it, the files are large, and a firmware flash must not depend on the
    // cloud being up.
    let share = std::env::var("BIOSLOVE_SHARE").unwrap_or_else(|_| {
        r"\\opk-riv\winbits\Drivers\Thumb\multiboot\BiosLove".to_string()
    });
    let index = resolve_index_path();
    let index_bytes = std::fs::read(&index).unwrap_or_default();
    let payloads = load_payload_map(&index, &share);

    let relay = Relay {
        client: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("reqwest client"),
        upstream,
        payloads: Arc::new(payloads),
        index_bytes: Arc::new(index_bytes),
        share: Arc::new(share.clone()),
    };
    println!("preboot-relay: listening on {listen} -> {}", relay.upstream);
    println!(
        "preboot-relay: {} payload digest(s) from {index} over {share}",
        relay.payloads.len()
    );
    println!(
        "preboot-relay: index {} ({} bytes) on {INDEX_ROUTE}",
        if relay.index_bytes.is_empty() { "MISSING" } else { "ready" },
        relay.index_bytes.len()
    );
    if !relay.payloads.is_empty() && !std::path::Path::new(&share).is_dir() {
        eprintln!(
            "preboot-relay: WARNING {share} is not reachable - payload fetches will fail until it is"
        );
    }
    let app = Router::new()
        .route(PAYLOAD_ROUTE, axum::routing::get(payload))
        .route(INDEX_ROUTE, axum::routing::get(index_doc))
        .route(SHELL_ROUTE, axum::routing::get(shell))
        .fallback(proxy)
        .with_state(relay);
    let listener = tokio::net::TcpListener::bind(&listen).await.expect("bind listen addr");
    axum::serve(listener, app).await.expect("serve");
}

/// Locate `index.json` without depending on the working directory.
///
/// `cargo run` from this crate, from the repo root, and the built binary run
/// from anywhere all have different working directories, so a bare relative
/// default silently finds nothing. `BIOSLOVE_INDEX` overrides the search.
fn resolve_index_path() -> String {
    if let Ok(v) = std::env::var("BIOSLOVE_INDEX") {
        if !v.trim().is_empty() {
            return v;
        }
    }
    let mut candidates: Vec<PathBuf> = vec![
        PathBuf::from("index.json"),
        // Running from this crate's directory.
        PathBuf::from("../bioslove-index/index.json"),
        // Running from the repo root.
        PathBuf::from("bioslove-index/index.json"),
    ];
    // Installed layout: <repo>/preboot-relay/target/<profile>/preboot-relay.exe
    if let Ok(exe) = std::env::current_exe() {
        for up in [3usize, 4] {
            let mut p = exe.clone();
            for _ in 0..=up {
                p.pop();
            }
            candidates.push(p.join("bioslove-index").join("index.json"));
        }
    }
    for c in &candidates {
        if c.is_file() {
            return c.display().to_string();
        }
    }
    // Nothing found: report the bare name so the error names something familiar.
    "index.json".to_string()
}

/// Map every digest in a `bioslove-index` document to its file on the share.
///
/// Serving by digest rather than by path means a request cannot name a file
/// outside the share, and identical payloads shared between models resolve once.
fn load_payload_map(index_path: &str, share: &str) -> HashMap<String, PathBuf> {
    let mut map = HashMap::new();
    let bytes = match std::fs::read(index_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("preboot-relay: no payload index ({index_path}: {e}); serving none");
            return map;
        }
    };
    let doc: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("preboot-relay: {index_path} is not an index ({e}); serving none");
            return map;
        }
    };
    let root = PathBuf::from(share);
    let Some(entries) = doc.get("entries").and_then(|v| v.as_array()) else {
        return map;
    };
    for e in entries {
        let folder = e.get("folder").and_then(|v| v.as_str()).unwrap_or("");
        // The share spells the two trees differently.
        let side = match e.get("side").and_then(|v| v.as_str()) {
            Some("desktop") => "Desktop",
            _ => "laptop",
        };
        if folder.is_empty() {
            continue;
        }
        let dir = root.join(side).join(folder);
        let Some(steps) = e.get("steps").and_then(|v| v.as_array()) else {
            continue;
        };
        for s in steps {
            let mut add = |name: Option<&str>, sha: Option<&str>| {
                if let (Some(name), Some(sha)) = (name, sha) {
                    if sha.len() == 64 && !name.is_empty() {
                        let rel: PathBuf = name.split('\\').collect();
                        map.insert(sha.to_ascii_lowercase(), dir.join(rel));
                    }
                }
            };
            add(
                s.get("exec").and_then(|v| v.as_str()),
                s.get("exec_sha256").and_then(|v| v.as_str()),
            );
            if let Some(files) = s.get("files").and_then(|v| v.as_array()) {
                for f in files {
                    add(
                        f.get("name").and_then(|v| v.as_str()),
                        f.get("sha256").and_then(|v| v.as_str()),
                    );
                }
            }
        }
    }
    map
}

/// Serve the model index, so a box booted off a bare stick can populate its
/// Flash tab without carrying a copy.
async fn index_doc(State(relay): State<Relay>) -> Response {
    if relay.index_bytes.is_empty() {
        println!("GET bioslove/index -> 503 (no index loaded)");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "relay has no index; generate it with bioslove-index",
        )
            .into_response();
    }
    println!("GET bioslove/index -> 200 ({}B)", relay.index_bytes.len());
    (
        StatusCode::OK,
        [(CONTENT_TYPE, "application/json")],
        relay.index_bytes.to_vec(),
    )
        .into_response()
}

/// Serve the EFI Shell vendor tools are launched under.
async fn shell(State(relay): State<Relay>) -> Response {
    let path = std::path::Path::new(relay.share.as_str()).join(SHELL_FILE);
    match tokio::time::timeout(SHARE_READ_TIMEOUT, tokio::fs::read(&path)).await {
        Ok(Ok(bytes)) => {
            println!("GET bioslove/shell -> 200 ({}B) {}", bytes.len(), path.display());
            (
                StatusCode::OK,
                [(CONTENT_TYPE, "application/octet-stream")],
                bytes,
            )
                .into_response()
        }
        Ok(Err(e)) => {
            eprintln!("GET bioslove/shell -> 502 {}: {e}", path.display());
            (StatusCode::BAD_GATEWAY, format!("shell read failed: {e}")).into_response()
        }
        Err(_) => (
            StatusCode::GATEWAY_TIMEOUT,
            "firmware share did not respond; check it is reachable",
        )
            .into_response(),
    }
}

/// Serve one payload by digest, straight off the share.
async fn payload(State(relay): State<Relay>, AxPath(sha): AxPath<String>) -> Response {
    let sha = sha.to_ascii_lowercase();
    let Some(path) = relay.payloads.get(&sha) else {
        println!("GET payload/{sha} -> 404 (not in index)");
        return (StatusCode::NOT_FOUND, "unknown payload digest").into_response();
    };
    // An unreachable SMB host blocks for tens of seconds; firmware waiting on a
    // flash deserves a fast, legible refusal instead of a stall.
    match tokio::time::timeout(SHARE_READ_TIMEOUT, tokio::fs::read(path)).await {
        Ok(Ok(bytes)) => {
            println!("GET payload/{sha} -> 200 ({}B) {}", bytes.len(), path.display());
            (
                StatusCode::OK,
                [(CONTENT_TYPE, "application/octet-stream")],
                bytes,
            )
                .into_response()
        }
        Ok(Err(e)) => {
            eprintln!("GET payload/{sha} -> 502 {}: {e}", path.display());
            (StatusCode::BAD_GATEWAY, format!("share read failed: {e}")).into_response()
        }
        Err(_) => {
            eprintln!(
                "GET payload/{sha} -> 504 share did not respond in {}s: {}",
                SHARE_READ_TIMEOUT.as_secs(),
                path.display()
            );
            (
                StatusCode::GATEWAY_TIMEOUT,
                "firmware share did not respond; check it is reachable",
            )
                .into_response()
        }
    }
}

/// Forward any request to the upstream verbatim (method, path+query, body,
/// content-type) and mirror back status, content-type, and body.
async fn proxy(
    State(relay): State<Relay>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let path = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/").to_string();
    let url = format!("{}{}", relay.upstream, path);
    let mut req = relay.client.request(method.clone(), &url).body(body.to_vec());
    if let Some(ct) = headers.get(CONTENT_TYPE) {
        req = req.header(reqwest::header::CONTENT_TYPE, ct.as_bytes());
    }
    match req.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let ct = resp.headers().get(reqwest::header::CONTENT_TYPE).cloned();
            let bytes = resp.bytes().await.unwrap_or_default();
            println!("{method} {path} -> {status} ({}B)", bytes.len());
            let mut out = Response::builder().status(status);
            if let Some(ct) = ct {
                out = out.header(CONTENT_TYPE, ct.as_bytes());
            }
            out.body(axum::body::Body::from(bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(e) => {
            eprintln!("{method} {path} -> upstream error: {e}");
            (StatusCode::BAD_GATEWAY, format!("relay: upstream error: {e}")).into_response()
        }
    }
}
