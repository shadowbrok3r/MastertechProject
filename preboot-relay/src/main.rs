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

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri, header::CONTENT_TYPE};
use axum::response::{IntoResponse, Response};

#[derive(Clone)]
struct Relay {
    client: reqwest::Client,
    upstream: String,
}

#[tokio::main]
async fn main() {
    let upstream = std::env::var("UPSTREAM_URL")
        .unwrap_or_else(|_| "https://axum.master-tech.app".to_string())
        .trim_end_matches('/')
        .to_string();
    let listen =
        std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:8082".to_string());
    let relay = Relay {
        client: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("reqwest client"),
        upstream,
    };
    println!("preboot-relay: listening on {listen} -> {}", relay.upstream);
    let app = Router::new().fallback(proxy).with_state(relay);
    let listener = tokio::net::TcpListener::bind(&listen).await.expect("bind listen addr");
    axum::serve(listener, app).await.expect("serve");
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
