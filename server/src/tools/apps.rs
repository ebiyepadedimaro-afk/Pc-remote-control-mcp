//! App and session tools: open, launch, notify, lock.

use super::{arg_str, arg_strings, expand_path, ToolOutput};
use crate::platform;
use crate::state::AppState;
use serde_json::json;
use std::sync::Arc;

pub async fn open_url(_state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let url = arg_str(&args, "url").ok_or("url is required")?.trim();
    if url.is_empty() {
        return Err("url is empty".into());
    }
    let pid = platform::open_target(url)?;
    Ok(ToolOutput::json(json!({"ok": true, "url": url, "pid": pid})))
}

pub async fn open_path(_state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let raw = arg_str(&args, "path").ok_or("path is required")?;
    let path = expand_path(raw);
    if !std::path::Path::new(&path).exists() {
        return Err(format!("path does not exist: {path}"));
    }
    let pid = platform::open_target(&path)?;
    Ok(ToolOutput::json(json!({"ok": true, "path": path, "pid": pid})))
}

pub async fn launch(_state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let program = arg_str(&args, "program").ok_or("program is required")?;
    let list = arg_strings(&args, "args");
    let cwd = arg_str(&args, "cwd").map(expand_path);
    let pid = platform::launch_detached(program, &list, cwd.as_deref())?;
    Ok(ToolOutput::json(json!({"ok": true, "program": program, "pid": pid})))
}

pub async fn notify(_state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let title = arg_str(&args, "title").ok_or("title is required")?;
    let body = arg_str(&args, "body").ok_or("body is required")?;
    platform::notify(title, body)?;
    Ok(ToolOutput::json(json!({"ok": true})))
}

pub async fn lock(_state: Arc<AppState>, _args: serde_json::Value) -> Result<ToolOutput, String> {
    platform::lock_screen()?;
    Ok(ToolOutput::json(json!({"ok": true})))
}
