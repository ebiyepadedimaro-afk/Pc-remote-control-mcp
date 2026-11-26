//! Clipboard tools (arboard). The Clipboard object is kept alive for the
//! server's lifetime so the X11 selection owner (and therefore the copied
//! text) survives between MCP calls.

use super::{arg_str, ToolOutput};
use crate::state::AppState;
use serde_json::json;
use std::sync::{Arc, Mutex, OnceLock};

static CLIPBOARD: OnceLock<Mutex<Option<arboard::Clipboard>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<arboard::Clipboard>> {
    CLIPBOARD.get_or_init(|| Mutex::new(None))
}

fn ensure<'a>(
    guard: &'a mut Option<arboard::Clipboard>,
) -> Result<&'a mut arboard::Clipboard, String> {
    if guard.is_none() {
        *guard = Some(arboard::Clipboard::new().map_err(|e| {
            format!("cannot access the clipboard: {e}. A display server (X11 or XWayland) is needed for clipboard access.")
        })?);
    }
    Ok(guard.as_mut().unwrap())
}

pub async fn get(_state: Arc<AppState>, _args: serde_json::Value) -> Result<ToolOutput, String> {
    let mut guard = slot().lock().unwrap();
    let cb = ensure(&mut guard)?;
    match cb.get_text() {
        Ok(text) => Ok(ToolOutput::json(json!({"text": text, "length": text.chars().count()}))),
        Err(arboard::Error::ContentNotAvailable) => Ok(ToolOutput::json(json!({"text": "", "length": 0}))),
        Err(e) => {
            *guard = None;
            Err(format!("cannot read the clipboard: {e}"))
        }
    }
}

pub async fn set(_state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let text = arg_str(&args, "text").ok_or("text is required")?;
    let mut guard = slot().lock().unwrap();
    let cb = ensure(&mut guard)?;
    match cb.set_text(text.to_string()) {
        Ok(()) => Ok(ToolOutput::json(json!({"ok": true, "length": text.chars().count()}))),
        Err(e) => {
            *guard = None;
            Err(format!("cannot write the clipboard: {e}"))
        }
    }
}
