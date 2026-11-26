//! Power and system-administration tools.

use super::{arg_i64, arg_str, arg_u32, ToolOutput};
use crate::platform;
use crate::state::AppState;
use serde_json::json;
use std::sync::Arc;

pub async fn power(state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let cfg = state.config.get();
    if !cfg.system.power_enabled {
        return Err("power actions are disabled in the server configuration (system.power_enabled)".into());
    }
    let raw = arg_str(&args, "action").ok_or("action is required")?;
    let action = match raw.to_ascii_lowercase().as_str() {
        "shutdown" | "poweroff" | "power-off" => "shutdown",
        "restart" | "reboot" => "restart",
        "hibernate" => "hibernate",
        "suspend" | "sleep" => "suspend",
        "logout" | "log-out" | "signout" => "logout",
        "lock" => "lock",
        other => return Err(format!("unknown power action: {other} (shutdown, restart, hibernate, suspend, logout, lock)")),
    };
    let delay = arg_u32(&args, "delay_secs").unwrap_or(0).min(cfg.system.max_delay_secs);
    platform::power_action(action, delay).await?;
    crate::logging::warn(format!("power action requested: {action} (delay {delay}s)"));
    let note = if action == "lock" {
        "session locked"
    } else if delay > 0 {
        "action scheduled; the session may end before this response is delivered"
    } else {
        "the session may end before this response is delivered"
    };
    Ok(ToolOutput::json(json!({"ok": true, "action": action, "delay_secs": delay, "note": note})))
}

#[cfg(windows)]
fn shell_line(command: &str) -> (String, Vec<String>) {
    ("cmd.exe".into(), vec!["/C".into(), command.to_string()])
}
#[cfg(not(windows))]
fn shell_line(command: &str) -> (String, Vec<String>) {
    ("sh".into(), vec!["-c".into(), command.to_string()])
}

pub async fn admin_exec(state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let cfg = state.config.get();
    if !cfg.system.admin_enabled {
        return Err("administrator execution is disabled in the server configuration (system.admin_enabled)".into());
    }
    let command = arg_str(&args, "command").ok_or("command is required")?;
    let list = super::arg_strings(&args, "args");
    let timeout = arg_i64(&args, "timeout_ms").unwrap_or(60_000).clamp(1_000, 600_000) as u64;
    let (program, prog_args) = if list.is_empty() {
        shell_line(command)
    } else {
        (command.to_string(), list)
    };
    let result = platform::admin_exec(&program, &prog_args, timeout).await?;
    crate::logging::warn(format!("admin command requested: {}", result.get("command").and_then(|c| c.as_str()).unwrap_or(command)));
    Ok(ToolOutput::json(result))
}

pub async fn service(state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let cfg = state.config.get();
    if !cfg.system.admin_enabled {
        return Err("service management is disabled in the server configuration (system.admin_enabled)".into());
    }
    let action = arg_str(&args, "action")
        .ok_or("action is required (list, status, start, stop, restart, enable, disable)")?
        .to_ascii_lowercase();
    let name = arg_str(&args, "name");
    let result = platform::service_control(&action, name).await?;
    Ok(ToolOutput::json(result))
}

pub async fn terminal_run(state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let cfg = state.config.get();
    if !cfg.system.terminal_enabled {
        return Err("terminal execution is disabled in the server configuration (system.terminal_enabled)".into());
    }
    let command = arg_str(&args, "command").ok_or("command is required")?;
    let cwd = arg_str(&args, "cwd");
    let pid = platform::run_in_terminal(command, cwd)?;
    Ok(ToolOutput::json(json!({
        "ok": true,
        "pid": pid,
        "command": command,
        "cwd": cwd,
        "note": "opened in a visible terminal window",
    })))
}
