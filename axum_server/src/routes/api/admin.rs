//! Root-only management console API.
//!
//! Surfaces what the process actually received rather than what the tracing
//! lines happened to print: a ring buffer of recorded requests (peer address,
//! forwarding headers, full header list, raw body for opted-in paths),
//! per-path counters, and the whole pre-boot registry including console
//! advertisements and the bodies that were rejected as malformed.
//!
//! Every route requires an `Authorization: Bearer <surrealdb record token>`
//! whose user row has `authorization = 'Root'`. The token is verified against
//! SurrealDB on a throwaway connection so the server's own session is never
//! re-authenticated as the caller; verdicts are cached briefly per token.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::AppState;

/// Recorded requests kept in memory.
pub const DEFAULT_RING_CAPACITY: usize = 1000;
/// Bytes of a captured body retained per record.
pub const DEFAULT_MAX_BODY: usize = 8 * 1024;
/// Hard ceiling on how much of a request body the recorder will buffer. A body
/// above this on a capture path is refused rather than silently unrecorded.
pub const MAX_BUFFERED_BODY: usize = 4 * 1024 * 1024;
/// Distinct path keys tracked in the per-path counters.
const MAX_PATH_KEYS: usize = 512;
/// How long a Root verdict is trusted before re-checking against SurrealDB.
const AUTH_CACHE_TTL: Duration = Duration::from_secs(60);
/// How long a rejection is remembered (bounds connection churn under a flood).
const AUTH_NEGATIVE_TTL: Duration = Duration::from_secs(30);
const AUTH_CACHE_MAX: usize = 256;
/// Header values never stored in a record.
const REDACTED_HEADERS: &[&str] = &["authorization", "cookie", "set-cookie", "proxy-authorization"];

/// One recorded request/response pair.
#[derive(Clone, Debug, Serialize)]
pub struct RequestRecord {
    pub seq: u64,
    pub req_id: String,
    pub at: String,
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub version: String,
    /// Socket peer as seen by this process — the ingress proxy when fronted.
    pub peer: Option<String>,
    pub forwarded_for: Option<String>,
    pub real_ip: Option<String>,
    pub user_agent: Option<String>,
    pub content_type: Option<String>,
    pub content_length: Option<u64>,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub body_bytes: usize,
    pub body_truncated: bool,
    pub status: u16,
    pub latency_ms: f64,
    pub error: Option<String>,
}

/// Rolling counters for one `METHOD path` key.
#[derive(Clone, Debug, Default, Serialize)]
pub struct PathStat {
    pub key: String,
    pub count: u64,
    pub last_at: String,
    pub statuses: BTreeMap<u16, u64>,
    pub bytes_in: u64,
    pub latency_ms_total: f64,
    pub latency_ms_max: f64,
}

/// Runtime knobs for the recorder, editable through `POST /api/v1/admin/capture`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CaptureConfig {
    /// Master switch for recording into the ring.
    pub enabled: bool,
    pub capacity: usize,
    pub max_body: usize,
    /// Path prefixes whose request body is buffered and stored. `*` matches all.
    pub body_paths: Vec<String>,
    /// Record the management API's own traffic (off so polling can't evict the ring).
    pub record_admin: bool,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            capacity: DEFAULT_RING_CAPACITY,
            max_body: DEFAULT_MAX_BODY,
            body_paths: vec!["/api/v1/qc/preboot/console".to_string()],
            record_admin: false,
        }
    }
}

/// Partial update; absent fields keep their current value.
#[derive(Debug, Default, Deserialize)]
pub struct CaptureUpdate {
    pub enabled: Option<bool>,
    pub capacity: Option<usize>,
    pub max_body: Option<usize>,
    pub body_paths: Option<Vec<String>>,
    pub record_admin: Option<bool>,
}

/// A Root user that passed verification.
#[derive(Clone, Debug, Serialize)]
pub struct RootIdentity {
    pub email: String,
    pub name: String,
    pub user_id: String,
}

#[derive(Clone)]
struct CachedVerdict {
    at: Instant,
    identity: Option<RootIdentity>,
}

#[derive(Default)]
struct Recorder {
    ring: VecDeque<RequestRecord>,
    paths: BTreeMap<String, PathStat>,
    /// Requests dropped from the per-path map because it hit [`MAX_PATH_KEYS`].
    paths_overflow: u64,
    config: CaptureConfig,
}

#[derive(Clone)]
pub struct AdminState {
    recorder: Arc<Mutex<Recorder>>,
    auth: Arc<Mutex<HashMap<u64, CachedVerdict>>>,
    seq: Arc<AtomicU64>,
    started: Instant,
    started_at: String,
}

impl Default for AdminState {
    fn default() -> Self {
        Self::new()
    }
}

impl AdminState {
    pub fn new() -> Self {
        Self {
            recorder: Arc::new(Mutex::new(Recorder {
                config: CaptureConfig::default(),
                ..Recorder::default()
            })),
            auth: Arc::new(Mutex::new(HashMap::new())),
            seq: Arc::new(AtomicU64::new(1)),
            started: Instant::now(),
            started_at: now_rfc3339(),
        }
    }

    pub fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed)
    }

    pub async fn config(&self) -> CaptureConfig {
        self.recorder.lock().await.config.clone()
    }

    /// Store a record and fold it into the per-path counters.
    pub async fn record(&self, rec: RequestRecord) {
        let mut r = self.recorder.lock().await;
        let cap = r.config.capacity.max(1);
        let key = format!("{} {}", rec.method, rec.path);
        if r.paths.contains_key(&key) || r.paths.len() < MAX_PATH_KEYS {
            let stat = r.paths.entry(key.clone()).or_insert_with(|| PathStat {
                key,
                ..PathStat::default()
            });
            stat.count += 1;
            stat.last_at = rec.at.clone();
            *stat.statuses.entry(rec.status).or_insert(0) += 1;
            stat.bytes_in += rec.content_length.unwrap_or(rec.body_bytes as u64);
            stat.latency_ms_total += rec.latency_ms;
            stat.latency_ms_max = stat.latency_ms_max.max(rec.latency_ms);
        } else {
            r.paths_overflow += 1;
        }
        r.ring.push_back(rec);
        while r.ring.len() > cap {
            r.ring.pop_front();
        }
    }

    async fn snapshot(&self, q: &RequestQuery) -> (Vec<RequestRecord>, usize) {
        let r = self.recorder.lock().await;
        let total = r.ring.len();
        let limit = q.limit.unwrap_or(200).clamp(1, DEFAULT_RING_CAPACITY * 4);
        let contains = q.contains.as_ref().map(|s| s.to_lowercase());
        let mut out: Vec<RequestRecord> = r
            .ring
            .iter()
            .rev()
            .filter(|rec| q.since_seq.is_none_or(|s| rec.seq > s))
            .filter(|rec| q.path.as_ref().is_none_or(|p| rec.path.contains(p.as_str())))
            .filter(|rec| {
                q.method
                    .as_ref()
                    .is_none_or(|m| rec.method.eq_ignore_ascii_case(m))
            })
            .filter(|rec| q.status.is_none_or(|s| rec.status == s))
            .filter(|rec| {
                contains.as_ref().is_none_or(|needle| {
                    rec.body
                        .as_ref()
                        .is_some_and(|b| b.to_lowercase().contains(needle))
                        || rec.path.to_lowercase().contains(needle)
                        || rec
                            .user_agent
                            .as_ref()
                            .is_some_and(|u| u.to_lowercase().contains(needle))
                        || rec.peer.as_ref().is_some_and(|p| p.contains(needle))
                })
            })
            .take(limit)
            .cloned()
            .collect();
        out.reverse();
        (out, total)
    }

    async fn get_record(&self, seq: u64) -> Option<RequestRecord> {
        let r = self.recorder.lock().await;
        r.ring.iter().find(|rec| rec.seq == seq).cloned()
    }

    async fn clear(&self) -> usize {
        let mut r = self.recorder.lock().await;
        let n = r.ring.len();
        r.ring.clear();
        r.paths.clear();
        r.paths_overflow = 0;
        n
    }

    pub fn uptime_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// Verify a bearer token belongs to a `Root` user, or produce the error
    /// response to return. Verdicts are cached per token for [`AUTH_CACHE_TTL`].
    pub async fn require_root(&self, headers: &HeaderMap) -> Result<RootIdentity, Response> {
        let Some(token) = bearer_token(headers) else {
            return Err(deny(
                StatusCode::UNAUTHORIZED,
                "missing Authorization: Bearer <surrealdb token>",
            ));
        };
        let key = hash_token(&token);
        if let Some(v) = self.auth.lock().await.get(&key).cloned() {
            let ttl = if v.identity.is_some() {
                AUTH_CACHE_TTL
            } else {
                AUTH_NEGATIVE_TTL
            };
            if v.at.elapsed() < ttl {
                return match v.identity {
                    Some(id) => Ok(id),
                    None => Err(deny(StatusCode::FORBIDDEN, "Root authorization required")),
                };
            }
        }
        let verdict = verify_root_token(&token).await;
        {
            let mut cache = self.auth.lock().await;
            if cache.len() >= AUTH_CACHE_MAX {
                cache.clear();
            }
            cache.insert(
                key,
                CachedVerdict {
                    at: Instant::now(),
                    identity: verdict.as_ref().ok().cloned().flatten(),
                },
            );
        }
        match verdict {
            Ok(Some(id)) => Ok(id),
            Ok(None) => Err(deny(StatusCode::FORBIDDEN, "Root authorization required")),
            Err(e) => {
                tracing::warn!(error = %e, "admin: token verification failed");
                Err(deny(StatusCode::UNAUTHORIZED, "token rejected by SurrealDB"))
            }
        }
    }
}

/// `Ok(None)` means the token authenticated but the user is not Root.
async fn verify_root_token(token: &str) -> anyhow::Result<Option<RootIdentity>> {
    use surrealdb::Surreal;
    use surrealdb::engine::remote::ws::{Client, Ws, Wss};

    let db: Surreal<Client> = Surreal::init();
    let url = database::active_db_url();
    if database::active_db_secure() {
        db.connect::<Wss>(url).await?;
    } else {
        db.connect::<Ws>(url).await?;
    }
    db.use_ns(database::NS).use_db(database::DB).await?;
    db.authenticate(token.to_string()).await?;
    let row: Option<serde_json::Value> = db
        .query("SELECT id, email, name, authorization FROM user WHERE id = $auth.id LIMIT 1")
        .await?
        .take(0)?;
    let Some(row) = row else {
        return Ok(None);
    };
    // `id` comes back as a record-id object on some engines, so fall back to the
    // JSON rendering rather than dropping the value.
    let field = |k: &str| {
        row.get(k)
            .map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))
            .unwrap_or_default()
    };
    if field("authorization").trim_matches('"') != "Root" {
        return Ok(None);
    }
    Ok(Some(RootIdentity {
        email: field("email"),
        name: field("name"),
        user_id: field("id"),
    }))
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer ").or_else(|| s.strip_prefix("bearer ")))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn hash_token(token: &str) -> u64 {
    let mut h = DefaultHasher::new();
    token.hash(&mut h);
    h.finish()
}

fn deny(code: StatusCode, msg: &str) -> Response {
    (code, Json(serde_json::json!({ "error": msg }))).into_response()
}

pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// `true` when `path` starts with any prefix in `prefixes` (`*` matches all).
pub fn path_matches(prefixes: &[String], path: &str) -> bool {
    prefixes
        .iter()
        .any(|p| p == "*" || (!p.is_empty() && path.starts_with(p.as_str())))
}

/// Socket peer, or `None` when the server was not built with connect-info.
/// Never rejects, so adding it to a handler cannot turn a request into a 500.
pub struct PeerAddr(pub Option<std::net::SocketAddr>);

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for PeerAddr {
    type Rejection = std::convert::Infallible;

    fn from_request_parts(
        parts: &mut http::request::Parts,
        _state: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        let peer = parts
            .extensions
            .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
            .map(|axum::extract::ConnectInfo(p)| *p);
        std::future::ready(Ok(Self(peer)))
    }
}

/// Header list with sensitive values replaced by their byte length.
pub fn collect_headers(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(k, v)| {
            let name = k.as_str().to_string();
            let value = if REDACTED_HEADERS.contains(&name.as_str()) {
                format!("<redacted {} bytes>", v.len())
            } else {
                v.to_str().unwrap_or("<non-utf8>").to_string()
            };
            (name, value)
        })
        .collect()
}

#[derive(Debug, Default, Deserialize)]
pub struct RequestQuery {
    pub limit: Option<usize>,
    pub path: Option<String>,
    pub method: Option<String>,
    pub status: Option<u16>,
    pub contains: Option<String>,
    pub since_seq: Option<u64>,
}

#[derive(Serialize)]
struct ServerInfo {
    version: String,
    pid: u32,
    started_at: String,
    uptime_secs: u64,
    now: String,
    rust_log: String,
    db_url: String,
    db_connected: bool,
    capture: CaptureConfig,
    recorded: usize,
    next_seq: u64,
    preboot_sessions: usize,
    preboot_consoles: usize,
    preboot_rejects: usize,
    fleet_agents: usize,
    viewer: RootIdentity,
}

async fn info(State(app): State<AppState>, headers: HeaderMap) -> Response {
    let viewer = match app.admin.require_root(&headers).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let counts = super::preboot::admin_counts(&app.preboot).await;
    let fleet_agents = app.fleet.lock().await.agents.len();
    let recorded = app.admin.recorder.lock().await.ring.len();
    Json(ServerInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        pid: std::process::id(),
        started_at: app.admin.started_at.clone(),
        uptime_secs: app.admin.uptime_secs(),
        now: now_rfc3339(),
        rust_log: std::env::var("RUST_LOG").unwrap_or_default(),
        db_url: database::active_db_url().to_string(),
        db_connected: database::is_db_connected().await,
        capture: app.admin.config().await,
        recorded,
        next_seq: app.admin.seq.load(Ordering::Relaxed),
        preboot_sessions: counts.0,
        preboot_consoles: counts.1,
        preboot_rejects: counts.2,
        fleet_agents,
        viewer,
    })
    .into_response()
}

async fn list_requests(
    State(app): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<RequestQuery>,
) -> Response {
    if let Err(r) = app.admin.require_root(&headers).await {
        return r;
    }
    let (records, total) = app.admin.snapshot(&q).await;
    Json(serde_json::json!({ "total": total, "returned": records.len(), "records": records }))
        .into_response()
}

async fn get_request(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path(seq): Path<u64>,
) -> Response {
    if let Err(r) = app.admin.require_root(&headers).await {
        return r;
    }
    match app.admin.get_record(seq).await {
        Some(rec) => Json(rec).into_response(),
        None => deny(StatusCode::NOT_FOUND, "no record with that seq"),
    }
}

async fn clear_requests(State(app): State<AppState>, headers: HeaderMap) -> Response {
    let viewer = match app.admin.require_root(&headers).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let n = app.admin.clear().await;
    tracing::info!(by = %viewer.email, cleared = n, "admin: request ring cleared");
    Json(serde_json::json!({ "cleared": n })).into_response()
}

async fn list_paths(State(app): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(r) = app.admin.require_root(&headers).await {
        return r;
    }
    let r = app.admin.recorder.lock().await;
    let mut stats: Vec<PathStat> = r.paths.values().cloned().collect();
    stats.sort_by(|a, b| b.count.cmp(&a.count));
    Json(serde_json::json!({ "overflow": r.paths_overflow, "paths": stats })).into_response()
}

async fn get_capture(State(app): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(r) = app.admin.require_root(&headers).await {
        return r;
    }
    Json(app.admin.config().await).into_response()
}

async fn set_capture(
    State(app): State<AppState>,
    headers: HeaderMap,
    Json(update): Json<CaptureUpdate>,
) -> Response {
    let viewer = match app.admin.require_root(&headers).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let cfg = {
        let mut r = app.admin.recorder.lock().await;
        if let Some(v) = update.enabled {
            r.config.enabled = v;
        }
        if let Some(v) = update.capacity {
            r.config.capacity = v.clamp(1, DEFAULT_RING_CAPACITY * 10);
        }
        if let Some(v) = update.max_body {
            r.config.max_body = v.min(MAX_BUFFERED_BODY);
        }
        if let Some(v) = update.body_paths {
            r.config.body_paths = v.into_iter().filter(|p| !p.trim().is_empty()).collect();
        }
        if let Some(v) = update.record_admin {
            r.config.record_admin = v;
        }
        let cap = r.config.capacity;
        while r.ring.len() > cap {
            r.ring.pop_front();
        }
        r.config.clone()
    };
    tracing::info!(by = %viewer.email, ?cfg, "admin: capture config updated");
    Json(cfg).into_response()
}

async fn preboot_detail(State(app): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(r) = app.admin.require_root(&headers).await {
        return r;
    }
    Json(super::preboot::admin_snapshot(&app.preboot).await).into_response()
}

async fn evict_console(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path(addr): Path<String>,
) -> Response {
    let viewer = match app.admin.require_root(&headers).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let removed = super::preboot::admin_evict_console(&app.preboot, &addr).await;
    tracing::info!(by = %viewer.email, %addr, removed, "admin: console eviction");
    Json(serde_json::json!({ "addr": addr, "removed": removed })).into_response()
}

pub fn admin_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/admin/info", axum::routing::get(info))
        .route(
            "/api/v1/admin/requests",
            axum::routing::get(list_requests).delete(clear_requests),
        )
        .route("/api/v1/admin/requests/{seq}", axum::routing::get(get_request))
        .route("/api/v1/admin/paths", axum::routing::get(list_paths))
        .route(
            "/api/v1/admin/capture",
            axum::routing::get(get_capture).post(set_capture),
        )
        .route("/api/v1/admin/preboot", axum::routing::get(preboot_detail))
        .route(
            "/api/v1/admin/preboot/console/{addr}",
            axum::routing::delete(evict_console),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(seq: u64, method: &str, path: &str, status: u16, body: Option<&str>) -> RequestRecord {
        RequestRecord {
            seq,
            req_id: format!("id-{seq}"),
            at: now_rfc3339(),
            method: method.to_string(),
            path: path.to_string(),
            query: None,
            version: "HTTP/1.1".to_string(),
            peer: Some("192.168.22.243:51000".to_string()),
            forwarded_for: None,
            real_ip: None,
            user_agent: Some("MasterTech/4.8.0".to_string()),
            content_type: None,
            content_length: None,
            headers: Vec::new(),
            body: body.map(str::to_string),
            body_bytes: body.map(str::len).unwrap_or(0),
            body_truncated: false,
            status,
            latency_ms: 1.0,
            error: None,
        }
    }

    #[tokio::test]
    async fn ring_evicts_oldest_past_capacity() {
        let admin = AdminState::new();
        admin
            .recorder
            .lock()
            .await
            .config
            .capacity = 3;
        for seq in 1..=5 {
            admin.record(rec(seq, "GET", "/x", 200, None)).await;
        }
        let (records, total) = admin.snapshot(&RequestQuery::default()).await;
        assert_eq!(total, 3);
        assert_eq!(records.iter().map(|r| r.seq).collect::<Vec<_>>(), vec![3, 4, 5]);
    }

    #[tokio::test]
    async fn snapshot_filters_narrow_the_ring() {
        let admin = AdminState::new();
        admin.record(rec(1, "POST", "/api/v1/qc/preboot/console", 200, Some(r#"{"addr":"a:1"}"#))).await;
        admin.record(rec(2, "POST", "/api/v1/qc/preboot/console", 400, Some("garbage"))).await;
        admin.record(rec(3, "GET", "/api/v1/qc/agents", 200, None)).await;

        let by_status = RequestQuery { status: Some(400), ..Default::default() };
        assert_eq!(admin.snapshot(&by_status).await.0.len(), 1);

        let by_path = RequestQuery { path: Some("preboot".into()), ..Default::default() };
        assert_eq!(admin.snapshot(&by_path).await.0.len(), 2);

        let by_method = RequestQuery { method: Some("get".into()), ..Default::default() };
        assert_eq!(admin.snapshot(&by_method).await.0.len(), 1);

        let by_body = RequestQuery { contains: Some("garbage".into()), ..Default::default() };
        assert_eq!(admin.snapshot(&by_body).await.0[0].seq, 2);

        let tail = RequestQuery { since_seq: Some(2), ..Default::default() };
        assert_eq!(admin.snapshot(&tail).await.0.len(), 1);
    }

    #[tokio::test]
    async fn path_counters_fold_per_method_and_path() {
        let admin = AdminState::new();
        admin.record(rec(1, "POST", "/c", 200, None)).await;
        admin.record(rec(2, "POST", "/c", 400, None)).await;
        let paths = &admin.recorder.lock().await.paths;
        let stat = paths.get("POST /c").expect("counter");
        assert_eq!(stat.count, 2);
        assert_eq!(stat.statuses.get(&200), Some(&1));
        assert_eq!(stat.statuses.get(&400), Some(&1));
    }

    #[tokio::test]
    async fn missing_bearer_is_401() {
        let admin = AdminState::new();
        let err = admin.require_root(&HeaderMap::new()).await.unwrap_err();
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn body_capture_matches_prefixes_and_wildcard() {
        let prefixes = vec!["/api/v1/qc/preboot/console".to_string()];
        assert!(path_matches(&prefixes, "/api/v1/qc/preboot/console"));
        assert!(!path_matches(&prefixes, "/api/v1/qc/preboot/SN1/frame"));
        assert!(path_matches(&["*".to_string()], "/anything"));
        assert!(!path_matches(&[], "/anything"));
        assert!(!path_matches(&["".to_string()], "/anything"));
    }

    #[test]
    fn sensitive_headers_are_redacted() {
        let mut h = HeaderMap::new();
        h.insert(header::AUTHORIZATION, "Bearer secret-token".parse().unwrap());
        h.insert(header::USER_AGENT, "MasterTech/4.8.0".parse().unwrap());
        let out = collect_headers(&h);
        let auth = out.iter().find(|(k, _)| k == "authorization").unwrap();
        assert!(!auth.1.contains("secret-token"));
        assert!(out.iter().any(|(k, v)| k == "user-agent" && v == "MasterTech/4.8.0"));
    }
}
