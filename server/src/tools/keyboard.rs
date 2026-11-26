//! Keyboard tools.

use super::{arg_i64, arg_str, arg_strings, input_backend, input_note, ToolOutput};
use crate::platform::{self, Chord};
use crate::state::AppState;
use serde_json::json;
use std::sync::Arc;

pub async fn key_press(state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let pref = input_backend(&state);
    let specs = arg_strings(&args, "keys");
    if specs.is_empty() {
        return Err("keys is required (a chord string or an array of them)".into());
    }
    let repeat = arg_i64(&args, "repeat").unwrap_or(1).clamp(1, 50) as u32;
    let interval = arg_i64(&args, "interval_ms").unwrap_or(50).clamp(0, 5000) as u64;

    let chords: Vec<Chord> = specs
        .iter()
        .map(|s| Chord::parse(s))
        .collect::<Result<_, _>>()?;

    for r in 0..repeat {
        for chord in &chords {
            platform::key_chord(pref, &chord.modifiers, &chord.key)?;
        }
        if r + 1 < repeat && interval > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(interval)).await;
        }
    }
    let mut out = json!({"ok": true, "keys": specs, "repeat": repeat});
    if let Some(w) = input_note(&state) {
        out["warning"] = json!(w);
    }
    Ok(ToolOutput::json(out))
}

pub async fn type_text(state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let pref = input_backend(&state);
    let text = arg_str(&args, "text").ok_or("text is required")?;
    if text.is_empty() {
        return Ok(ToolOutput::text(r#"{"ok":true,"typed":0}"#));
    }
    let interval = arg_i64(&args, "interval_ms").unwrap_or(0).clamp(0, 1000) as u64;
    platform::type_text(pref, text, interval)?;
    let mut out = json!({"ok": true, "typed": text.chars().count()});
    if let Some(w) = input_note(&state) {
        out["warning"] = json!(w);
    }
    Ok(ToolOutput::json(out))
}
