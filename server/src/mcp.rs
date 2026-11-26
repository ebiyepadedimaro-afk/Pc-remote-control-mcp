//! MCP (Model Context Protocol) core: a stateless, JSON-only Streamable HTTP
//! server, the same shape the Android Remote Control MCP uses. Every request
//! is self-contained; no session id is required, no SSE stream is offered.
//!
//! Connections are always approved: no interactive approval step exists. An
//! optional bearer token can be enabled in config (`token_enabled`); it is
//! verified automatically and rejected with 401 when it does not match.

use crate::state::AppState;
use crate::tools;
use serde_json::{json, Value};
use std::sync::Arc;

pub const SUPPORTED_PROTOCOLS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
pub const LATEST_PROTOCOL: &str = "2025-06-18";

pub const SERVER_NAME: &str = "pc-remote-control-mcp";

fn ok(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: &Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}})
}

fn instructions() -> String {
    "Remote control of this PC's desktop (Linux, macOS or Windows).\n\
     - Screenshots come back as images. Coordinates in the image match the input coordinate space \
       (captures are scaled to session coordinates unless disabled).\n\
     - Use pc_get_screen_state first: it returns display geometry, cursor position and a screenshot.\n\
     - Mouse tools take absolute session coordinates; keyboard tools accept chords like \"CTRL+SHIFT+T\".\n\
     - Wayland sessions need the uinput input backend (/dev/uinput permissions, see the desktop app).\n\
     - File tools are confined to the configured root when one is set; shell tools can be disabled or blocklisted.\n\
     - Tool names carry the configured prefix (default \"pc_\")."
        .to_string()
}

/// Handles one JSON-RPC message. Returns None for notifications.
pub async fn dispatch(state: &Arc<AppState>, msg: Value) -> Option<Value> {
    if !msg.is_object() {
        return Some(error(&Value::Null, -32600, "Invalid Request: expected a JSON object"));
    }
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(json!({}));

    // Notifications (and responses) carry no id; never answer them.
    if id.is_null() {
        return None;
    }
    if method.is_empty() {
        return Some(error(&id, -32600, "Invalid Request: missing method"));
    }

    match method {
        "initialize" => {
            let requested = params.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or("");
            let negotiated = if SUPPORTED_PROTOCOLS.contains(&requested) {
                requested.to_string()
            } else {
                LATEST_PROTOCOL.to_string()
            };
            Some(ok(
                &id,
                json!({
                    "protocolVersion": negotiated,
                    "capabilities": {
                        "tools": {"listChanged": false},
                        "logging": {}
                    },
                    "serverInfo": {
                        "name": SERVER_NAME,
                        "title": "PC Remote Control MCP",
                        "version": state.version
                    },
                    "instructions": instructions()
                }),
            ))
        }
        "ping" => Some(ok(&id, json!({}))),
        "tools/list" => Some(ok(&id, json!({ "tools": tools::list(state) }))),
        "tools/call" => {
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match tools::call(state, name, args).await {
                Ok(result) => Some(ok(&id, result)),
                Err(e) => Some(error(&id, e.code, e.message)),
            }
        }
        "resources/list" => Some(ok(&id, json!({"resources": []}))),
        "resources/templates/list" => Some(ok(&id, json!({"resourceTemplates": []}))),
        "prompts/list" => Some(ok(&id, json!({"prompts": []}))),
        "logging/setLevel" => Some(ok(&id, json!({}))),
        "notifications/initialized" | "notifications/cancelled" | "notifications/progress" => None,
        _ => Some(error(&id, -32601, format!("Method not found: {method}"))),
    }
}
