//! MCP tool registry and shared result types.

pub mod agents;
pub mod apps;
pub mod browser;
pub mod clipboard;
pub mod docs;
pub mod files;
pub mod keyboard;
pub mod mouse;
pub mod power;
pub mod processes;
pub mod screen;
pub mod shell;
pub mod system;
pub mod windows;

use crate::state::AppState;
use serde::Serialize;
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Content {
    Text { text: String },
    Image { data: String, #[serde(rename = "mimeType")] mime_type: String },
}

pub struct ToolOutput {
    pub content: Vec<Content>,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn text(text: impl Into<String>) -> Self {
        Self { content: vec![Content::Text { text: text.into() }], is_error: false }
    }
    pub fn json(v: Value) -> Self {
        Self::text(v.to_string())
    }
    pub fn image_b64(mime: &str, data: String) -> Self {
        Self {
            content: vec![Content::Image { data, mime_type: mime.to_string() }],
            is_error: false,
        }
    }
    pub fn image(mime: &str, bytes: &[u8]) -> Self {
        Self {
            content: vec![Content::Image {
                data: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes),
                mime_type: mime.to_string(),
            }],
            is_error: false,
        }
    }
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.content.push(Content::Text { text: text.into() });
        self
    }
    pub fn error(msg: impl Into<String>) -> Self {
        Self { content: vec![Content::Text { text: msg.into() }], is_error: true }
    }
    pub fn into_json(self) -> Value {
        json!({
            "content": self.content,
            "isError": self.is_error,
        })
    }
}

pub type BoxFut = Pin<Box<dyn Future<Output = Result<ToolOutput, String>> + Send>>;
pub type Handler = fn(Arc<AppState>, Value) -> BoxFut;

pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
    pub handler: Handler,
}

macro_rules! spec {
    ($name:literal, $desc:literal, $schema:expr, $handler:path) => {
        ToolSpec {
            name: $name,
            description: $desc,
            schema: $schema,
            handler: |s, a| Box::pin($handler(s, a)),
        }
    };
}

fn props(p: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": p,
        "required": required,
        "additionalProperties": false
    })
}

fn tool_list() -> Vec<ToolSpec> {
    vec![
        spec!(
            "screenshot",
            "Capture the screen as a PNG or JPEG image. Coordinates in the image match input coordinates unless native_scale is true.",
            props(
                json!({
                    "format": {"type": "string", "enum": ["png", "jpeg"], "description": "Image format (default from config, usually png)"},
                    "quality": {"type": "integer", "minimum": 1, "maximum": 100, "description": "JPEG quality"},
                    "max_width": {"type": "integer", "description": "Downscale so the width fits (0 = native)"},
                    "max_height": {"type": "integer", "description": "Downscale so the height fits (0 = native)"},
                    "native_scale": {"type": "boolean", "description": "Keep physical pixels on HiDPI screens instead of matching input coordinates"},
                    "save_to": {"type": "string", "description": "Optional absolute path to also write the image file"}
                }),
                &[]
            ),
            screen::screenshot
        ),
        spec!(
            "get_screen_state",
            "Screen snapshot with display geometry, cursor position, session type and (optionally) the window list. Start here.",
            props(
                json!({
                    "include_screenshot": {"type": "boolean", "description": "Attach an image (default true)"},
                    "include_windows": {"type": "boolean", "description": "Include the window list (default false)"},
                    "format": {"type": "string", "enum": ["png", "jpeg"]},
                    "quality": {"type": "integer", "minimum": 1, "maximum": 100},
                    "max_width": {"type": "integer"}
                }),
                &[]
            ),
            screen::get_screen_state
        ),
        spec!(
            "mouse_move",
            "Move the pointer. Pass absolute x/y, or relative dx/dy.",
            props(
                json!({
                    "x": {"type": "integer", "description": "Absolute X in session coordinates"},
                    "y": {"type": "integer", "description": "Absolute Y in session coordinates"},
                    "dx": {"type": "integer", "description": "Relative horizontal movement"},
                    "dy": {"type": "integer", "description": "Relative vertical movement"}
                }),
                &[]
            ),
            mouse::move_mouse
        ),
        spec!(
            "mouse_click",
            "Click a mouse button, optionally moving first.",
            props(
                json!({
                    "button": {"type": "string", "enum": ["left", "right", "middle", "back", "forward"], "description": "Default left"},
                    "clicks": {"type": "integer", "minimum": 1, "maximum": 10, "description": "Click count (2 = double click)"},
                    "x": {"type": "integer"},
                    "y": {"type": "integer"},
                    "interval_ms": {"type": "integer", "description": "Delay between clicks (default 40)"}
                }),
                &[]
            ),
            mouse::click
        ),
        spec!(
            "mouse_scroll",
            "Scroll the wheel. Positive dy scrolls down, positive dx scrolls right.",
            props(
                json!({
                    "dy": {"type": "integer", "description": "Vertical wheel notches (0 = none)"},
                    "dx": {"type": "integer", "description": "Horizontal wheel notches (0 = none)"},
                    "x": {"type": "integer", "description": "Optional absolute X to move to first"},
                    "y": {"type": "integer", "description": "Optional absolute Y to move to first"}
                }),
                &["dy"]
            ),
            mouse::scroll
        ),
        spec!(
            "mouse_drag",
            "Press a button at from_x/from_y, drag to to_x/to_y and release.",
            props(
                json!({
                    "from_x": {"type": "integer"},
                    "from_y": {"type": "integer"},
                    "to_x": {"type": "integer"},
                    "to_y": {"type": "integer"},
                    "button": {"type": "string", "enum": ["left", "right", "middle"], "description": "Default left"},
                    "duration_ms": {"type": "integer", "description": "Drag duration (default 300)"}
                }),
                &["from_x", "from_y", "to_x", "to_y"]
            ),
            mouse::drag
        ),
        spec!(
            "key_press",
            "Press a key chord such as \"CTRL+SHIFT+T\", \"ENTER\" or \"F5\". Accepts one string or a list.",
            props(
                json!({
                    "keys": {"description": "A chord string or an array of chord strings", "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]},
                    "repeat": {"type": "integer", "minimum": 1, "maximum": 50, "description": "Repeat count (default 1)"},
                    "interval_ms": {"type": "integer", "description": "Delay between repeats (default 50)"}
                }),
                &["keys"]
            ),
            keyboard::key_press
        ),
        spec!(
            "type_text",
            "Type text at the focused window (US layout; non-ASCII may need the X11 backend or clipboard paste).",
            props(
                json!({
                    "text": {"type": "string"},
                    "interval_ms": {"type": "integer", "description": "Delay between characters (default 0)"}
                }),
                &["text"]
            ),
            keyboard::type_text
        ),
        spec!(
            "get_clipboard",
            "Read the clipboard's text.",
            props(json!({}), &[]),
            clipboard::get
        ),
        spec!(
            "set_clipboard",
            "Write text to the clipboard.",
            props(json!({"text": {"type": "string"}}), &["text"]),
            clipboard::set
        ),
        spec!(
            "list_directory",
            "List a directory (depth 1 by default).",
            props(
                json!({
                    "path": {"type": "string", "description": "Absolute path (or ~/...)"},
                    "depth": {"type": "integer", "minimum": 1, "maximum": 4, "description": "Recursion depth (default 1)"},
                    "include_hidden": {"type": "boolean", "description": "Include dotfiles (default false)"}
                }),
                &["path"]
            ),
            files::list_directory
        ),
        spec!(
            "read_file",
            "Read a text or binary file (binary as base64).",
            props(
                json!({
                    "path": {"type": "string"},
                    "encoding": {"type": "string", "enum": ["utf8", "base64"], "description": "Default utf8"},
                    "max_bytes": {"type": "integer", "description": "Optional cap for this read"}
                }),
                &["path"]
            ),
            files::read_file
        ),
        spec!(
            "write_file",
            "Write or overwrite a file (text or base64).",
            props(
                json!({
                    "path": {"type": "string"},
                    "content": {"type": "string"},
                    "encoding": {"type": "string", "enum": ["utf8", "base64"], "description": "Default utf8"},
                    "create_parents": {"type": "boolean", "description": "Create missing parents (default true)"}
                }),
                &["path", "content"]
            ),
            files::write_file
        ),
        spec!(
            "delete_path",
            "Delete a file/directory (recursive optional).",
            props(
                json!({
                    "path": {"type": "string"},
                    "recursive": {"type": "boolean", "description": "Delete directories recursively"}
                }),
                &["path"]
            ),
            files::delete_path
        ),
        spec!(
            "move_path",
            "Move or rename a file/directory.",
            props(json!({"from": {"type": "string"}, "to": {"type": "string"}}), &["from", "to"]),
            files::move_path
        ),
        spec!(
            "copy_path",
            "Copy a file or directory (recursive for directories).",
            props(json!({"from": {"type": "string"}, "to": {"type": "string"}}), &["from", "to"]),
            files::copy_path
        ),
        spec!(
            "make_directory",
            "Create a directory (and parents).",
            props(json!({"path": {"type": "string"}}), &["path"]),
            files::make_directory
        ),
        spec!(
            "file_info",
            "Stat a path: type, size, timestamps, permissions.",
            props(json!({"path": {"type": "string"}}), &["path"]),
            files::file_info
        ),
        spec!(
            "download_from_url",
            "Download an http(s) URL to a local file.",
            props(
                json!({
                    "url": {"type": "string"},
                    "path": {"type": "string", "description": "Absolute destination path"},
                    "max_bytes": {"type": "integer", "description": "Optional cap"}
                }),
                &["url", "path"]
            ),
            files::download_from_url
        ),
        spec!(
            "shell_exec",
            "Run a command and return stdout/stderr/exit code. Shell syntax when `args` is omitted.",
            props(
                json!({
                    "command": {"type": "string", "description": "Executable, or a shell command line when args is omitted"},
                    "args": {"type": "array", "items": {"type": "string"}, "description": "Arguments (no shell involved)"},
                    "cwd": {"type": "string"},
                    "env": {"type": "object", "additionalProperties": {"type": "string"}},
                    "timeout_ms": {"type": "integer", "description": "Default from config (30 s)"}
                }),
                &["command"]
            ),
            shell::exec
        ),
        spec!(
            "list_processes",
            "List running processes sorted by CPU, memory, pid or name.",
            props(
                json!({
                    "sort_by": {"type": "string", "enum": ["cpu", "memory", "pid", "name"], "description": "Default cpu"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 500, "description": "Default 30"},
                    "filter": {"type": "string", "description": "Case-insensitive name filter"}
                }),
                &[]
            ),
            processes::list
        ),
        spec!(
            "kill_process",
            "Terminate a process by pid.",
            props(
                json!({
                    "pid": {"type": "integer"},
                    "force": {"type": "boolean", "description": "SIGKILL / TerminateProcess instead of a polite termination"}
                }),
                &["pid"]
            ),
            processes::kill
        ),
        spec!(
            "system_info",
            "OS, CPU, memory, disks, network interfaces, uptime and session details.",
            props(json!({}), &[]),
            system::info
        ),
        spec!(
            "list_windows",
            "List visible windows (X11/EWMH on Linux, process windows on macOS/Windows).",
            props(json!({}), &[]),
            windows::list
        ),
        spec!(
            "focus_window",
            "Raise and focus a window by id from list_windows.",
            props(json!({"id": {"type": "string"}}), &["id"]),
            windows::focus
        ),
        spec!(
            "open_url",
            "Open a URL in the default browser.",
            props(json!({"url": {"type": "string"}}), &["url"]),
            apps::open_url
        ),
        spec!(
            "open_path",
            "Open a file or directory with the default application.",
            props(json!({"path": {"type": "string"}}), &["path"]),
            apps::open_path
        ),
        spec!(
            "launch_app",
            "Launch a program detached from the server.",
            props(
                json!({
                    "program": {"type": "string", "description": "Executable path or name"},
                    "args": {"type": "array", "items": {"type": "string"}},
                    "cwd": {"type": "string"}
                }),
                &["program"]
            ),
            apps::launch
        ),
        spec!(
            "notify",
            "Show a desktop notification.",
            props(json!({"title": {"type": "string"}, "body": {"type": "string"}}), &["title", "body"]),
            apps::notify
        ),
        spec!(
            "lock_screen",
            "Lock the user session.",
            props(json!({}), &[]),
            apps::lock
        ),
        spec!(
            "browser_open",
            "Launch (or reuse) a Chromium-based browser and open a URL in a tab. Returns the page title/URL and tab id.",
            props(
                json!({
                    "url": {"type": "string", "description": "Optional URL to open in a new tab"},
                    "headless": {"type": "boolean", "description": "Run the automation browser headless"},
                    "new_tab": {"type": "boolean", "description": "Open a new tab (default true when url is given)"},
                    "wait_ms": {"type": "integer", "description": "Max wait for page load (default 10000)"}
                }),
                &[]
            ),
            browser::open
        ),
        spec!(
            "browser_navigate",
            "Navigate the current browser tab to a URL.",
            props(json!({"url": {"type": "string"}, "timeout_ms": {"type": "integer"}}), &["url"]),
            browser::navigate
        ),
        spec!(
            "browser_get_content",
            "Read the current page as text, HTML or markdown (optionally an element by CSS selector).",
            props(
                json!({
                    "format": {"type": "string", "enum": ["text", "html", "markdown"], "description": "Default text"},
                    "selector": {"type": "string", "description": "CSS selector (default: whole body)"},
                    "max_chars": {"type": "integer", "description": "Default 20000"}
                }),
                &[]
            ),
            browser::get_content
        ),
        spec!(
            "browser_click",
            "Click by CSS selector, or by viewport x/y coordinates.",
            props(
                json!({
                    "selector": {"type": "string"},
                    "x": {"type": "integer"},
                    "y": {"type": "integer"},
                    "button": {"type": "string", "enum": ["left", "right", "middle"]},
                    "double": {"type": "boolean", "description": "Double click"}
                }),
                &[]
            ),
            browser::click
        ),
        spec!(
            "browser_type",
            "Type text into the page (optionally focusing a selector first); optionally press Enter.",
            props(
                json!({
                    "text": {"type": "string"},
                    "selector": {"type": "string"},
                    "submit": {"type": "boolean", "description": "Press Enter after typing"}
                }),
                &["text"]
            ),
            browser::type_text
        ),
        spec!(
            "browser_screenshot",
            "Screenshot the current browser tab (viewport or full page) as PNG.",
            props(json!({"full_page": {"type": "boolean"}}), &[]),
            browser::screenshot
        ),
        spec!(
            "browser_eval",
            "Execute JavaScript in the current page and return the result.",
            props(
                json!({
                    "expression": {"type": "string"},
                    "await_promise": {"type": "boolean", "description": "Await a returned Promise"},
                    "max_chars": {"type": "integer"}
                }),
                &["expression"]
            ),
            browser::eval_js
        ),
        spec!(
            "browser_tabs",
            "List, open, select or close browser tabs.",
            props(
                json!({
                    "action": {"type": "string", "enum": ["list", "new", "select", "close"], "description": "Default list"},
                    "id": {"type": "string", "description": "Tab id for select/close"},
                    "url": {"type": "string", "description": "URL for new"}
                }),
                &[]
            ),
            browser::tabs
        ),
        spec!(
            "browser_close",
            "Close the automation browser and its DevTools session.",
            props(json!({}), &[]),
            browser::close
        ),
        spec!(
            "fetch_page",
            "Fetch an http(s) URL without a browser and return readable text or raw HTML (no JavaScript execution).",
            props(
                json!({
                    "url": {"type": "string"},
                    "format": {"type": "string", "enum": ["text", "html"], "description": "Default text"},
                    "max_chars": {"type": "integer", "description": "Default 20000"}
                }),
                &["url"]
            ),
            browser::fetch_page
        ),
        spec!(
            "doc_read",
            "Extract text from a document: PDF, DOCX, PPTX, XLSX/XLS/ODS, HTML, CSV, markdown, JSON, XML or text.",
            props(
                json!({
                    "path": {"type": "string"},
                    "format": {"type": "string", "enum": ["text", "html"], "description": "For HTML files: text (default) or raw html"},
                    "max_chars": {"type": "integer", "description": "Default from docs.max_chars"}
                }),
                &["path"]
            ),
            docs::read
        ),
        spec!(
            "doc_info",
            "Document metadata: type, size, modification time, PDF page count.",
            props(json!({"path": {"type": "string"}}), &["path"]),
            docs::info
        ),
        spec!(
            "doc_convert",
            "Convert documents with LibreOffice (e.g. docx->pdf, xlsx->csv, pptx->pdf, pdf->docx).",
            props(
                json!({
                    "input": {"type": "string"},
                    "output": {"type": "string", "description": "Absolute destination path"},
                    "format": {"type": "string", "description": "Target format (default: output extension)"}
                }),
                &["input", "output"]
            ),
            docs::convert
        ),
        spec!(
            "doc_ocr",
            "OCR an image (png/jpg/tiff/bmp/webp) with tesseract and return the text.",
            props(
                json!({
                    "path": {"type": "string"},
                    "lang": {"type": "string", "description": "Tesseract language (default eng)"},
                    "max_chars": {"type": "integer"}
                }),
                &["path"]
            ),
            docs::ocr
        ),
        spec!(
            "agent_list",
            "List agentic coding CLIs (Claude Code, Codex, Gemini, Aider, opencode, Cursor) with install status.",
            props(json!({}), &[]),
            agents::detect
        ),
        spec!(
            "agent_start",
            "Start a coding agent CLI non-interactively in a project directory; returns a job id to poll.",
            props(
                json!({
                    "agent": {"type": "string", "description": "claude, codex, gemini, aider, opencode, cursor-agent, copilot, or custom"},
                    "prompt": {"type": "string"},
                    "cwd": {"type": "string", "description": "Project directory (default: home)"},
                    "args": {"type": "array", "items": {"type": "string"}, "description": "Override CLI args; {prompt} is substituted, otherwise the prompt is appended"},
                    "command": {"type": "string", "description": "Executable override (required for agent=custom)"},
                    "timeout_ms": {"type": "integer", "description": "Kill after this many ms (max agents.max_runtime_ms)"}
                }),
                &["agent", "prompt"]
            ),
            agents::start
        ),
        spec!(
            "agent_status",
            "Read an agent job's status and recent stdout/stderr.",
            props(json!({"job_id": {"type": "string"}, "max_chars": {"type": "integer"}}), &["job_id"]),
            agents::status
        ),
        spec!(
            "agent_run",
            "Run a coding agent CLI and wait for it (bounded by wait_ms); returns output or the running job id.",
            props(
                json!({
                    "agent": {"type": "string"},
                    "prompt": {"type": "string"},
                    "cwd": {"type": "string"},
                    "args": {"type": "array", "items": {"type": "string"}},
                    "command": {"type": "string"},
                    "timeout_ms": {"type": "integer"},
                    "wait_ms": {"type": "integer", "description": "How long to wait before returning (default 300000)"},
                    "max_chars": {"type": "integer"}
                }),
                &["agent", "prompt"]
            ),
            agents::run
        ),
        spec!(
            "agent_stop",
            "Stop a running agent job and its child processes.",
            props(json!({"job_id": {"type": "string"}}), &["job_id"]),
            agents::stop
        ),
        spec!(
            "agent_jobs",
            "List agent jobs started by this server.",
            props(json!({}), &[]),
            agents::list
        ),
        spec!(
            "power",
            "System power: shutdown, restart, hibernate, suspend, logout or lock. Optionally delayed.",
            props(
                json!({
                    "action": {"type": "string", "enum": ["shutdown", "restart", "hibernate", "suspend", "logout", "lock"]},
                    "delay_secs": {"type": "integer", "description": "Delay before acting (0 = now)"}
                }),
                &["action"]
            ),
            power::power
        ),
        spec!(
            "admin_exec",
            "Run a command with administrator privileges (sudo -n / root on Linux, osascript admin on macOS, UAC on Windows).",
            props(
                json!({
                    "command": {"type": "string", "description": "Executable, or a shell line when args is omitted"},
                    "args": {"type": "array", "items": {"type": "string"}},
                    "timeout_ms": {"type": "integer", "description": "Default 60000"}
                }),
                &["command"]
            ),
            power::admin_exec
        ),
        spec!(
            "service",
            "Manage system services (systemd on Linux, launchctl on macOS, Windows services).",
            props(
                json!({
                    "action": {"type": "string", "enum": ["list", "status", "start", "stop", "restart", "enable", "disable"]},
                    "name": {"type": "string", "description": "Service name (required except for list)"}
                }),
                &["action"]
            ),
            power::service
        ),
        spec!(
            "terminal_run",
            "Open a visible terminal window and run a command in it (for interactive/sudo commands).",
            props(
                json!({
                    "command": {"type": "string", "description": "Shell command line"},
                    "cwd": {"type": "string", "description": "Working directory"}
                }),
                &["command"]
            ),
            power::terminal_run
        ),
    ]
}

static SPECS: std::sync::OnceLock<Vec<ToolSpec>> = std::sync::OnceLock::new();

pub fn specs() -> &'static [ToolSpec] {
    SPECS.get_or_init(tool_list)
}


pub fn list(state: &AppState) -> Vec<Value> {
    let cfg = state.config.get();
    let prefix = cfg.prefix();
    specs()
        .iter()
        .filter(|s| cfg.tool_enabled(s.name))
        .map(|s| {
            json!({
                "name": format!("{prefix}{}", s.name),
                "description": s.description,
                "inputSchema": s.schema,
            })
        })
        .collect()
}

pub struct CallError {
    pub code: i64,
    pub message: String,
}

pub async fn call(state: &Arc<AppState>, name: &str, args: Value) -> Result<Value, CallError> {
    let cfg = state.config.get();
    let prefix = cfg.prefix();
    let base = if !prefix.is_empty() && name.starts_with(&prefix) {
        &name[prefix.len()..]
    } else {
        name
    };
    let spec = specs()
        .iter()
        .find(|s| s.name == base)
        .ok_or_else(|| CallError { code: -32602, message: format!("Unknown tool: {name}") })?;
    if !cfg.tool_enabled(spec.name) {
        return Err(CallError {
            code: -32602,
            message: format!("Tool {name} is disabled in the server configuration"),
        });
    }
    state.stats.bump_tool(spec.name);
    let started = std::time::Instant::now();
    let result = (spec.handler)(state.clone(), args).await;
    let ms = started.elapsed().as_millis();
    match result {
        Ok(out) => {
            if out.is_error {
                let msg = match out.content.first() {
                    Some(Content::Text { text }) => text.clone(),
                    _ => "tool error".into(),
                };
                crate::logging::warn(format!("tool {} failed ({ms} ms): {msg}", spec.name));
                state.stats.bump_error(&msg);
            } else {
                crate::logging::info(format!("tool {} ok ({ms} ms)", spec.name));
            }
            Ok(out.into_json())
        }
        Err(e) => {
            crate::logging::warn(format!("tool {} failed ({ms} ms): {e}", spec.name));
            state.stats.bump_error(&e);
            Ok(ToolOutput::error(e).into_json())
        }
    }
}

// ---------------------------------------------------------------- arg helpers

pub fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str())
}

pub fn arg_i64(args: &Value, key: &str) -> Option<i64> {
    args.get(key).and_then(|v| {
        v.as_i64()
            .or_else(|| v.as_f64().map(|f| f as i64))
            .or_else(|| v.as_str().and_then(|s| s.trim().parse::<i64>().ok()))
    })
}

pub fn arg_u32(args: &Value, key: &str) -> Option<u32> {
    args.get(key).and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_f64().map(|f| f.max(0.0) as u64))
            .or_else(|| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
            .map(|n| n as u32)
    })
}

pub fn arg_bool(args: &Value, key: &str, default: bool) -> bool {
    args.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

pub fn arg_strings(args: &Value, key: &str) -> Vec<String> {
    match args.get(key) {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => vec![],
    }
}

pub fn input_backend(state: &AppState) -> platform::Backend {
    platform::Backend::parse(&state.config.get().input.backend)
}

pub fn input_note(state: &AppState) -> Option<String> {
    platform::input_note(input_backend(state))
}

pub fn expand_path(path: &str) -> String {
    if path == "~" {
        return dirs::home_dir().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string());
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().to_string();
        }
    }
    path.to_string()
}

use crate::platform;

/// Finds an executable on PATH (cross-platform).
pub fn which(name: &str) -> Option<std::path::PathBuf> {
    if name.contains('/') || name.contains('\\') {
        let p = std::path::PathBuf::from(name);
        return if p.is_file() { Some(p) } else { None };
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let cand = dir.join(name);
            if cand.is_file() {
                return Some(cand);
            }
            #[cfg(windows)]
            {
                let cand = dir.join(format!("{name}.exe"));
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let direct = dir.join(name);
        if direct.is_file() {
            return Some(direct);
        }
        #[cfg(windows)]
        {
            for ext in ["exe", "cmd", "bat", "com"] {
                let cand = dir.join(format!("{name}.{ext}"));
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
    }
    None
}

/// Config override (absolute path) or the first candidate found on PATH.
pub fn find_binary(override_path: &str, candidates: &[&str]) -> Option<std::path::PathBuf> {
    if !override_path.trim().is_empty() {
        let p = std::path::PathBuf::from(expand_path(override_path.trim()));
        return if p.is_file() { Some(p) } else { None };
    }
    candidates.iter().find_map(|c| which(c))
}

/// Char-safe truncation (max characters).
pub fn truncate_chars(s: &str, max: usize) -> (String, bool) {
    if s.chars().count() <= max {
        return (s.to_string(), false);
    }
    (s.chars().take(max).collect(), true)
}

fn decode_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last = 0;
    let mut i = 0;
    let bytes = input.as_bytes();
    while i < bytes.len() {
        if bytes[i] != b'&' {
            i += 1;
            continue;
        }
        let rest = &input[i..];
        let Some(end) = rest.find(';') else { break };
        if end > 10 {
            i += 1;
            continue;
        }
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            "hellip" => Some('…'),
            _ => {
                if let Some(hex) = entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X")) {
                    u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
                } else if let Some(dec) = entity.strip_prefix('#') {
                    dec.parse::<u32>().ok().and_then(char::from_u32)
                } else {
                    None
                }
            }
        };
        if let Some(ch) = decoded {
            out.push_str(&input[last..i]);
            out.push(ch);
            last = i + end + 1;
            i = last;
        } else {
            i += 1;
        }
    }
    out.push_str(&input[last..]);
    out
}

/// Very small HTML-to-text renderer, good enough for reading pages and docs
/// (not a browser engine).
pub fn html_to_text(html: &str) -> String {
    let mut cleaned = html.to_string();
    for (open, close) in [
        ("<script", "</script>"),
        ("<style", "</style>"),
        ("<noscript", "</noscript>"),
        ("<svg", "</svg>"),
        ("<template", "</template>"),
    ] {
        let mut out = String::with_capacity(cleaned.len());
        let mut rest = cleaned.as_str();
        loop {
            let lower = rest.to_ascii_lowercase();
            match lower.find(open) {
                Some(i) => {
                    out.push_str(&rest[..i]);
                    let after = &rest[i..];
                    match after.to_ascii_lowercase().find(close) {
                        Some(j) => rest = &after[j + close.len()..],
                        None => {
                            break;
                        }
                    }
                }
                None => {
                    out.push_str(rest);
                    break;
                }
            }
        }
        cleaned = out;
    }

    let mut out = String::with_capacity(cleaned.len());
    let mut rest = cleaned.as_str();
    while let Some(lt) = rest.find('<') {
        let text = &rest[..lt];
        if !text.trim().is_empty() {
            out.push_str(text);
            out.push(' ');
        }
        let Some(gt) = rest[lt..].find('>') else {
            rest = "";
            break;
        };
        let tag = &rest[lt + 1..lt + gt];
        let name = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/' || c == '>')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let closing = tag.trim_start().starts_with('/');
        let block = [
            "p", "div", "br", "li", "tr", "h1", "h2", "h3", "h4", "h5", "h6", "section", "article",
            "header", "footer", "blockquote", "pre", "table", "ul", "ol", "hr",
        ]
        .contains(&name.as_str());
        if block {
            out.push('\n');
            if name == "li" && !closing {
                out.push_str("- ");
            }
        } else if !closing && (name == "td" || name == "th") {
            out.push(' ');
        }
        rest = &rest[lt + gt + 1..];
    }
    out.push_str(rest);
    let decoded = decode_entities(&out);
    let lines: Vec<String> = decoded
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .take(5000)
        .collect();
    lines.join("\n").trim().to_string()
}

/// Browser/document/agent capability summary for the admin status endpoint.
pub fn capabilities_report(state: &AppState) -> Value {
    json!({
        "browser": browser::detect_summary(state),
        "docs": docs::capabilities(state),
        "agents": agents::detect_summary(state),
    })
}
