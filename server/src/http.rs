//! HTTP surface:
//! - `POST /mcp`   stateless JSON-RPC (MCP). Always approves connections; an
//!                 optional bearer token is checked automatically when enabled.
//! - `/admin/*`    loopback-only API the desktop UI uses (status, logs, config).
//! - `GET /healthz`

use crate::config::Config;
use crate::logging;
use crate::mcp;
use crate::platform;
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/mcp", post(mcp_post).get(mcp_get).delete(mcp_get).options(preflight))
        .route("/healthz", get(healthz))
        .route("/", get(root))
        .route("/admin/status", get(admin_status))
        .route("/admin/logs", get(admin_logs))
        .route("/admin/config", get(admin_get_config).put(admin_put_config))
        .route("/admin/token", post(admin_token))
        .route("/admin/quit", post(admin_quit))
        .route("/admin/tunnel", get(admin_tunnel_status).post(admin_tunnel_action))
        .route("/admin/update", get(admin_update))
        .route("/admin/update/open", post(admin_update_open))
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
        .with_state(state)
}

fn cors(mut resp: Response) -> Response {
    let h = resp.headers_mut();
    h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".parse().unwrap());
    h.insert(header::ACCESS_CONTROL_ALLOW_METHODS, "POST, GET, DELETE, OPTIONS".parse().unwrap());
    h.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        "Content-Type, Authorization, X-API-Key, Mcp-Session-Id, MCP-Protocol-Version".parse().unwrap(),
    );
    h.insert(header::ACCESS_CONTROL_MAX_AGE, "86400".parse().unwrap());
    h.insert(header::ACCESS_CONTROL_EXPOSE_HEADERS, "Mcp-Session-Id".parse().unwrap());
    resp
}

fn json_response(status: StatusCode, body: Value) -> Response {
    cors((status, [(header::CONTENT_TYPE, "application/json")], body.to_string()).into_response())
}

async fn preflight() -> Response {
    cors(StatusCode::NO_CONTENT.into_response())
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn authorized(state: &AppState, headers: &HeaderMap) -> bool {
    let cfg = state.config.get();
    if !cfg.token_enabled {
        return true;
    }
    let provided = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            let v = v.trim();
            v.strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("bearer "))
                .or_else(|| v.strip_prefix("Token "))
                .map(str::trim)
        })
        .or_else(|| headers.get("x-api-key").and_then(|v| v.to_str().ok()))
        .or_else(|| headers.get("x-goog-api-key").and_then(|v| v.to_str().ok()));
    match provided {
        Some(p) => constant_time_eq(p.trim(), cfg.token.trim()),
        None => false,
    }
}

fn client_label(addr: &SocketAddr, headers: &HeaderMap) -> String {
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown client");
    format!("{} ({ua})", addr.ip())
}

async fn mcp_post(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let client = client_label(&addr, &headers);
    if !authorized(&state, &headers) {
        logging::warn(format!("MCP connection refused (bad credentials) from {client}"));
        let mut resp = json_response(
            StatusCode::UNAUTHORIZED,
            json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32001, "message": "Unauthorized: bad or missing bearer token"}}),
        );
        resp.headers_mut().insert(header::WWW_AUTHENTICATE, "Bearer realm=\"pcrc\"".parse().unwrap());
        return resp;
    }

    let msg: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            logging::warn(format!("MCP parse error from {client}: {e}"));
            return json_response(
                StatusCode::BAD_REQUEST,
                json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("Parse error: {e}")}}),
            );
        }
    };
    if msg.is_array() {
        return json_response(
            StatusCode::BAD_REQUEST,
            json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32600, "message": "JSON-RPC batches are not supported; POST one request per call"}}),
        );
    }

    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("").to_string();
    state.stats.bump_requests(&client);
    let started = std::time::Instant::now();
    let reply = mcp::dispatch(&state, msg).await;
    let ms = started.elapsed().as_millis();
    logging::info(format!("mcp {method} <- {client} ({ms} ms)"));

    match reply {
        Some(resp) => json_response(StatusCode::OK, resp),
        None => cors(StatusCode::ACCEPTED.into_response()),
    }
}

async fn mcp_get() -> Response {
    // Stateless JSON-only server: no SSE stream to offer on GET.
    let mut resp = json_response(
        StatusCode::METHOD_NOT_ALLOWED,
        json!({"error": "This MCP server is stateless and JSON-only. POST JSON-RPC requests to /mcp."}),
    );
    resp.headers_mut().insert(header::ALLOW, "POST, OPTIONS".parse().unwrap());
    resp
}

async fn healthz() -> Response {
    cors((StatusCode::OK, "ok").into_response())
}

async fn root() -> Response {
    cors(
        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "PC Remote Control MCP\n\nPOST JSON-RPC to /mcp\nAdmin API (loopback only): /admin/status, /admin/logs, /admin/config\n",
        )
            .into_response(),
    )
}

// ---------------------------------------------------------------- admin API

fn loopback_only(addr: &SocketAddr) -> Option<Response> {
    if addr.ip().is_loopback() {
        None
    } else {
        Some(json_response(
            StatusCode::FORBIDDEN,
            json!({"error": "The admin API is available on loopback only"}),
        ))
    }
}

async fn admin_status(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    if let Some(r) = loopback_only(&addr) {
        return r;
    }
    let cfg = state.config.get();
    let (requests, tool_calls, tool_errors) = state.stats.counts();
    let local_mcp_url = format!("http://127.0.0.1:{}/mcp", state.port);
    let tunnel_status = crate::tunnel::status();
    let tunnel_live = tunnel_status.get("state").and_then(|s| s.as_str()) == Some("running");
    let mcp_url = match tunnel_status.get("url").and_then(|u| u.as_str()).filter(|_| tunnel_live) {
        Some(url) => format!("{}/mcp", url.trim_end_matches('/')),
        None => local_mcp_url.clone(),
    };
    let lan_urls = lan_urls(state.port);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let body = json!({
        "ok": true,
        "name": mcp::SERVER_NAME,
        "version": state.version,
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "hostname": sysinfo::System::host_name(),
        "uptime_secs": state.uptime_secs(),
        "now_ms": now,
        "transport": if state.stdio_mode { "stdio" } else { "http" },
        "mcp_url": mcp_url,
        "mcp_url_local": local_mcp_url,
        "admin_url": format!("http://127.0.0.1:{}/admin/status", state.port),
        "lan_urls": lan_urls,
        "bind": state.bind,
        "port": state.port,
        "token_enabled": cfg.token_enabled,
        "token": if cfg.token_enabled { Value::String(cfg.token.clone()) } else { Value::Null },
        "approval": if cfg.token_enabled { "token" } else { "always" },
        "tool_prefix": cfg.tool_prefix,
        "tools": {
            "total": crate::tools::specs().len(),
            "enabled": crate::tools::specs().iter().filter(|t| cfg.tool_enabled(t.name)).count(),
            "disabled": cfg.tools.disabled,
        },
        "stats": {
            "mcp_requests": requests,
            "tool_calls": tool_calls,
            "tool_errors": tool_errors,
            "last_client": state.stats.last_client.lock().unwrap().clone(),
            "last_tool": state.stats.last_tool.lock().unwrap().clone(),
            "last_error": state.stats.last_error.lock().unwrap().clone(),
        },
        "session": platform::session_info(),
        "backends": platform::backends_report(),
        "capabilities": crate::tools::capabilities_report(&state),
        "tunnel": tunnel_status,
        "config_path": state.config.path().to_string_lossy(),
        "config": cfg,
    });
    json_response(StatusCode::OK, body)
}

#[derive(Deserialize)]
struct LogsQuery {
    #[serde(default)]
    since: u64,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    300
}

async fn admin_logs(
    State(_state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(q): Query<LogsQuery>,
) -> Response {
    if let Some(r) = loopback_only(&addr) {
        return r;
    }
    let (entries, newest) = logging::snapshot(q.since, q.limit.min(5000));
    json_response(StatusCode::OK, json!({"entries": entries, "newest": newest}))
}

async fn admin_get_config(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    if let Some(r) = loopback_only(&addr) {
        return r;
    }
    let cfg = state.config.get();
    json_response(StatusCode::OK, json!({ "config": cfg, "path": state.config.path().to_string_lossy() }))
}

async fn admin_put_config(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(mut cfg): Json<Config>,
) -> Response {
    if let Some(r) = loopback_only(&addr) {
        return r;
    }
    cfg.sanitize();
    let restart_required = cfg.bind != state.bind || cfg.port != state.port;
    match state.config.set(cfg) {
        Ok(saved) => {
            logging::info("configuration updated via admin API");
            json_response(
                StatusCode::OK,
                json!({"saved": true, "restart_required": restart_required, "config": saved}),
            )
        }
        Err(e) => json_response(StatusCode::INTERNAL_SERVER_ERROR, json!({"error": format!("could not save config: {e}")})),
    }
}

async fn admin_token(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    if let Some(r) = loopback_only(&addr) {
        return r;
    }
    let mut cfg = state.config.get();
    cfg.token = uuid::Uuid::new_v4().to_string();
    match state.config.set(cfg.clone()) {
        Ok(_) => {
            logging::warn("MCP bearer token regenerated");
            json_response(StatusCode::OK, json!({"token": cfg.token, "token_enabled": cfg.token_enabled}))
        }
        Err(e) => json_response(StatusCode::INTERNAL_SERVER_ERROR, json!({"error": format!("could not save config: {e}")})),
    }
}

fn default_route_iface() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let text = std::fs::read_to_string("/proc/net/route").ok()?;
        for line in text.lines().skip(1) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() > 1 && fields[1] == "00000000" {
                return Some(fields[0].to_string());
            }
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

fn lan_urls(port: u16) -> Vec<String> {
    use sysinfo::Networks;
    let nets = Networks::new_with_refreshed_list();
    let iface = default_route_iface();
    let virtual_prefixes = ["lo", "docker", "veth", "br-", "virbr", "lxc", "vnet", "tun", "tap", "zt"];
    let mut out: Vec<String> = Vec::new();
    for (name, data) in nets.list() {
        if let Some(wanted) = &iface {
            if name != wanted {
                continue;
            }
        } else if virtual_prefixes.iter().any(|p| name.starts_with(p)) {
            continue;
        }
        for net in data.ip_networks() {
            if let IpAddr::V4(v4) = net.addr {
                if !v4.is_loopback() && !v4.is_link_local() && !v4.is_unspecified() {
                    out.push(format!("http://{v4}:{port}/mcp"));
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

async fn admin_quit(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    if let Some(r) = loopback_only(&addr) {
        return r;
    }
    logging::warn("shutdown requested through the admin API");
    let st = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        st.shutdown.notify_waiters();
    });
    json_response(StatusCode::OK, json!({"ok": true, "shutting_down": true}))
}

async fn admin_tunnel_status(
    State(_state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    if let Some(r) = loopback_only(&addr) {
        return r;
    }
    json_response(StatusCode::OK, crate::tunnel::status())
}

#[derive(Deserialize)]
struct TunnelRequest {
    action: String,
    kind: Option<String>,
}

async fn admin_tunnel_action(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<TunnelRequest>,
) -> Response {
    if let Some(r) = loopback_only(&addr) {
        return r;
    }
    let cfg = state.config.get();
    if !cfg.system.tunnel_enabled {
        return json_response(
            StatusCode::FORBIDDEN,
            json!({"error": "remote tunnels are disabled in the server configuration (system.tunnel_enabled)"}),
        );
    }
    match req.action.as_str() {
        "status" => json_response(StatusCode::OK, crate::tunnel::status()),
        "start" => {
            if !cfg.token_enabled {
                return json_response(
                    StatusCode::CONFLICT,
                    json!({
                        "error": "bearer-token auth must be enabled before exposing the server through a tunnel",
                        "code": "token_required",
                        "installed": crate::tunnel::installed(),
                    }),
                );
            }
            match crate::tunnel::start(req.kind.as_deref().unwrap_or("auto"), state.port, &cfg.tunnel.ngrok_authtoken, &cfg.tunnel.tailscale_socket).await {
                Ok(s) => {
                    logging::warn(format!("remote-access tunnel started: {}", s.get("kind").and_then(|k| k.as_str()).unwrap_or("?")));
                    json_response(StatusCode::OK, s)
                }
                Err(e) => json_response(StatusCode::BAD_REQUEST, json!({"error": e, "installed": crate::tunnel::installed()})),
            }
        }
        "stop" => match crate::tunnel::stop(&cfg.tunnel.tailscale_socket).await {
            Ok(s) => {
                logging::info("remote-access tunnel stopped");
                json_response(StatusCode::OK, s)
            }
            Err(e) => json_response(StatusCode::BAD_REQUEST, json!({"error": e})),
        },
        other => json_response(StatusCode::BAD_REQUEST, json!({"error": format!("unknown tunnel action: {other}")})),
    }
}

async fn admin_update(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    if let Some(r) = loopback_only(&addr) {
        return r;
    }
    json_response(StatusCode::OK, crate::updates::check(&state.http).await)
}

async fn admin_update_open(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    if let Some(r) = loopback_only(&addr) {
        return r;
    }
    let info = crate::updates::check(&state.http).await;
    let url = info
        .get("download_url")
        .and_then(|u| u.as_str())
        .or_else(|| info.get("url").and_then(|u| u.as_str()))
        .map(str::to_string);
    match url {
        Some(url) => match platform::open_target(&url) {
            Ok(_) => json_response(StatusCode::OK, json!({"ok": true, "opened": url})),
            Err(e) => json_response(StatusCode::INTERNAL_SERVER_ERROR, json!({"error": e})),
        },
        None => json_response(StatusCode::NOT_FOUND, json!({"error": "no update is available"})),
    }
}
