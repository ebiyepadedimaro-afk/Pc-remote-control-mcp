//! Window tools.

use super::{arg_str, ToolOutput};
use crate::platform;
use crate::state::AppState;
use serde_json::json;
use std::sync::Arc;

pub async fn list(_state: Arc<AppState>, _args: serde_json::Value) -> Result<ToolOutput, String> {
    let mut windows = platform::list_windows()?;
    windows.truncate(200);
    let wayland = platform::session_info().get("wayland").and_then(|w| w.as_bool()).unwrap_or(false);
    let mut out = json!({
        "count": windows.len(),
        "windows": windows,
    });
    if wayland {
        out["note"] = json!("On Wayland only XWayland windows are visible to the X11 window list.");
    }
    Ok(ToolOutput::json(out))
}

pub async fn focus(_state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let id = arg_str(&args, "id").ok_or("id is required")?;
    platform::focus_window(id)?;
    Ok(ToolOutput::json(json!({"ok": true, "id": id})))
}
