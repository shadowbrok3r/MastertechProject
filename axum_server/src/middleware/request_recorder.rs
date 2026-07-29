//! Records every request into the management console's ring buffer.
//!
//! Runs on the request path (unlike [`crate::middleware::middleware_log`],
//! which only sees the response) so it can attribute a request to its socket
//! peer, capture the header set, and buffer the raw body for opted-in paths.
//! It also installs a per-request [`Ctx`], replacing the single process-wide
//! one from the extension layer so `req_id` actually distinguishes requests.

use std::net::SocketAddr;
use std::time::Instant;

use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

use crate::AppState;
use crate::error::Error;
use crate::middleware::context::Ctx;
use crate::routes::api::admin::{
    MAX_BUFFERED_BODY, RequestRecord, collect_headers, now_rfc3339, path_matches,
};

pub async fn mw_record_request(
    State(app): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let cfg = app.admin.config().await;
    let path = req.uri().path().to_string();
    let is_admin_path = path.starts_with("/api/v1/admin");
    let record = cfg.enabled && (cfg.record_admin || !is_admin_path);

    let req_id = Uuid::new_v4();
    // Logged before the handler runs: a request that never returns has no
    // response-side line, so this is the only record that it was in flight.
    tracing::debug!(%req_id, method = %req.method(), %path, "->> REQUEST      - begin");
    req.extensions_mut().insert(Ctx::new(Ok("Shadowbroker".to_string()), req_id));

    if !record {
        return next.run(req).await;
    }

    let started = Instant::now();
    let at = now_rfc3339();
    let seq = app.admin.next_seq();
    let method = req.method().to_string();
    let query = req.uri().query().map(str::to_string);
    let version = format!("{:?}", req.version());
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(p)| p.to_string());
    // Scoped so nothing borrowing the request is held across the awaits below —
    // `Request<Body>` is not `Sync`, and a live borrow makes the future non-Send.
    let (forwarded_for, real_ip, user_agent, content_type, content_length, headers) = {
        let h = req.headers();
        let get = |name: &str| h.get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
        (
            get("x-forwarded-for"),
            get("x-real-ip"),
            get(header::USER_AGENT.as_str()),
            get(header::CONTENT_TYPE.as_str()),
            get(header::CONTENT_LENGTH.as_str()).and_then(|v| v.parse().ok()),
            collect_headers(h),
        )
    };

    let want_body = cfg.max_body > 0 && path_matches(&cfg.body_paths, &path);
    let (req, body, body_bytes, body_truncated) = if want_body {
        let (parts, raw) = req.into_parts();
        match axum::body::to_bytes(raw, MAX_BUFFERED_BODY).await {
            Ok(bytes) => {
                let total = bytes.len();
                let text = String::from_utf8_lossy(&bytes[..total.min(cfg.max_body)]).to_string();
                (
                    Request::from_parts(parts, Body::from(bytes)),
                    Some(text),
                    total,
                    total > cfg.max_body,
                )
            }
            Err(e) => {
                tracing::warn!(
                    %path,
                    error = %e,
                    "request body exceeds the recorder's buffer; drop the path from capture.body_paths to pass it through"
                );
                return (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "body too large to record; narrow capture.body_paths",
                )
                    .into_response();
            }
        }
    } else {
        (req, None, 0, false)
    };

    let res = next.run(req).await;
    let latency_ms = started.elapsed().as_secs_f64() * 1000.0;
    let error = res.extensions().get::<Error>().map(|e| format!("{e:?}"));

    app.admin
        .record(RequestRecord {
            seq,
            req_id: req_id.to_string(),
            at,
            method,
            path,
            query,
            version,
            peer,
            forwarded_for,
            real_ip,
            user_agent,
            content_type,
            content_length,
            headers,
            body,
            body_bytes,
            body_truncated,
            status: res.status().as_u16(),
            latency_ms,
            error,
        })
        .await;

    res
}
