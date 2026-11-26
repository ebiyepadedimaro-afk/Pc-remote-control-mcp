//! Mouse tools.

use super::{arg_i64, arg_str, arg_u32, input_backend, input_note, ToolOutput};
use crate::platform::{self, MouseButton};
use crate::state::AppState;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

fn button(args: &serde_json::Value) -> Result<MouseButton, String> {
    match arg_str(args, "button").unwrap_or("left") {
        s => MouseButton::parse(s).ok_or_else(|| format!("unknown mouse button: {s}")),
    }
}

fn note(state: &AppState) -> Option<String> {
    input_note(state)
}

pub async fn move_mouse(state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let pref = input_backend(&state);
    if let (Some(x), Some(y)) = (arg_i64(&args, "x"), arg_i64(&args, "y")) {
        platform::move_abs(pref, x as i32, y as i32)?;
        let mut out = json!({"ok": true, "mode": "absolute", "x": x, "y": y});
        if let Some(w) = note(&state) {
            out["warning"] = json!(w);
        }
        return Ok(ToolOutput::json(out));
    }
    if let (Some(dx), Some(dy)) = (arg_i64(&args, "dx"), arg_i64(&args, "dy")) {
        platform::move_rel(pref, dx as i32, dy as i32)?;
        let mut out = json!({"ok": true, "mode": "relative", "dx": dx, "dy": dy});
        if let Some(w) = note(&state) {
            out["warning"] = json!(w);
        }
        return Ok(ToolOutput::json(out));
    }
    Err("provide absolute x and y, or relative dx and dy".into())
}

pub async fn click(state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let pref = input_backend(&state);
    let btn = button(&args)?;
    let count = arg_u32(&args, "clicks").unwrap_or(1).clamp(1, 10) as u8;
    let interval = arg_i64(&args, "interval_ms").unwrap_or(40).clamp(0, 5000) as u64;
    if let (Some(x), Some(y)) = (arg_i64(&args, "x"), arg_i64(&args, "y")) {
        platform::move_abs(pref, x as i32, y as i32)?;
    }
    platform::click(pref, btn, count, interval)?;
    let mut out = json!({"ok": true, "button": format!("{btn:?}").to_lowercase(), "clicks": count});
    if let Some(w) = note(&state) {
        out["warning"] = json!(w);
    }
    Ok(ToolOutput::json(out))
}

pub async fn scroll(state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let pref = input_backend(&state);
    let dy = arg_i64(&args, "dy").ok_or("dy is required")? as i32;
    let dx = arg_i64(&args, "dx").unwrap_or(0) as i32;
    if let (Some(x), Some(y)) = (arg_i64(&args, "x"), arg_i64(&args, "y")) {
        platform::move_abs(pref, x as i32, y as i32)?;
    }
    platform::scroll(pref, dx, dy)?;
    let mut out = json!({"ok": true, "dx": dx, "dy": dy});
    if let Some(w) = note(&state) {
        out["warning"] = json!(w);
    }
    Ok(ToolOutput::json(out))
}

pub async fn drag(state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let pref = input_backend(&state);
    let fx = arg_i64(&args, "from_x").ok_or("from_x is required")? as i32;
    let fy = arg_i64(&args, "from_y").ok_or("from_y is required")? as i32;
    let tx = arg_i64(&args, "to_x").ok_or("to_x is required")? as i32;
    let ty = arg_i64(&args, "to_y").ok_or("to_y is required")? as i32;
    let btn = match arg_str(&args, "button").unwrap_or("left") {
        "right" => MouseButton::Right,
        "middle" => MouseButton::Middle,
        _ => MouseButton::Left,
    };
    let duration = arg_i64(&args, "duration_ms").unwrap_or(300).clamp(50, 5000) as u64;
    let steps = (duration / 16).clamp(4, 120);

    platform::move_abs(pref, fx, fy)?;
    platform::button_down(pref, btn)?;
    let mut result = Ok(());
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let x = fx as f64 + (tx - fx) as f64 * t;
        let y = fy as f64 + (ty - fy) as f64 * t;
        if let Err(e) = platform::move_abs(pref, x.round() as i32, y.round() as i32) {
            result = Err(e);
            break;
        }
        tokio::time::sleep(Duration::from_millis(duration / steps)).await;
    }
    let up = platform::button_up(pref, btn);
    result?;
    up?;
    let mut out = json!({"ok": true, "from": [fx, fy], "to": [tx, ty], "duration_ms": duration});
    if let Some(w) = note(&state) {
        out["warning"] = json!(w);
    }
    Ok(ToolOutput::json(out))
}
