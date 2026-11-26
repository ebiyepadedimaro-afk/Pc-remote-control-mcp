//! Process tools (sysinfo).

use super::{arg_bool, arg_i64, arg_str, ToolOutput};
use crate::state::AppState;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use sysinfo::{Pid, ProcessesToUpdate, Signal, System};

pub async fn list(_state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let mut sys = System::new_all();
    tokio::time::sleep(Duration::from_millis(150)).await;
    sys.refresh_processes(ProcessesToUpdate::All, true);

    let filter = arg_str(&args, "filter").map(|f| f.to_ascii_lowercase());
    let sort_by = arg_str(&args, "sort_by").unwrap_or("cpu").to_ascii_lowercase();
    let limit = arg_i64(&args, "limit").unwrap_or(30).clamp(1, 500) as usize;

    let mut rows: Vec<Value> = sys
        .processes()
        .values()
        .filter(|p| {
            filter
                .as_ref()
                .map(|f| p.name().to_string_lossy().to_ascii_lowercase().contains(f))
                .unwrap_or(true)
        })
        .map(|p| {
            json!({
                "pid": p.pid().as_u32(),
                "name": p.name().to_string_lossy(),
                "cpu_percent": (p.cpu_usage() * 10.0).round() / 10.0,
                "memory_bytes": p.memory(),
                "memory_mb": (p.memory() as f64 / 1_048_576.0 * 10.0).round() / 10.0,
                "status": format!("{:?}", p.status()),
                "exe": p.exe().map(|e| e.to_string_lossy().to_string()),
            })
        })
        .collect();

    match sort_by.as_str() {
        "memory" => rows.sort_by(|a, b| {
            b.get("memory_bytes").and_then(|v| v.as_u64()).cmp(&a.get("memory_bytes").and_then(|v| v.as_u64()))
        }),
        "pid" => rows.sort_by_key(|r| r.get("pid").and_then(|v| v.as_u64())),
        "name" => rows.sort_by(|a, b| {
            a.get("name").and_then(|v| v.as_str()).unwrap_or("").cmp(b.get("name").and_then(|v| v.as_str()).unwrap_or(""))
        }),
        _ => rows.sort_by(|a, b| {
            b.get("cpu_percent")
                .and_then(|v| v.as_f64())
                .partial_cmp(&a.get("cpu_percent").and_then(|v| v.as_f64()))
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
    }
    let total = rows.len();
    rows.truncate(limit);
    Ok(ToolOutput::json(json!({
        "total": total,
        "returned": rows.len(),
        "sort_by": sort_by,
        "processes": rows,
    })))
}

pub async fn kill(_state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let pid = arg_i64(&args, "pid").ok_or("pid is required")?;
    let force = arg_bool(&args, "force", false);
    let mut sys = System::new_all();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let pid = Pid::from_u32(pid as u32);
    let process = sys.process(pid).ok_or_else(|| format!("process {pid} not found"))?;
    let name = process.name().to_string_lossy().to_string();
    let ok = if force {
        process.kill()
    } else {
        process.kill_with(Signal::Term).unwrap_or_else(|| process.kill())
    };
    if ok {
        Ok(ToolOutput::json(json!({"ok": true, "pid": pid.as_u32(), "name": name, "force": force})))
    } else {
        Err(format!("could not signal process {pid} ({name}); permission denied?"))
    }
}
