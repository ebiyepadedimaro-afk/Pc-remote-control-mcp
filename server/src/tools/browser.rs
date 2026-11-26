//! Browser automation through the Chrome DevTools Protocol.
//!
//! Launches a Chromium-based browser (Chrome, Chromium, Edge, Brave) with a
//! dedicated profile and a loopback debugging port, then drives the page with
//! Runtime/Page/Input CDP commands. Works headed or headless.

use super::{arg_bool, arg_str, arg_u32, expand_path, which, ToolOutput};
use crate::state::AppState;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

struct BrowserProc {
    child: tokio::process::Child,
    port: u16,
    label: String,
    headless: bool,
    target: Option<String>,
}

static SLOT: OnceLock<Mutex<Option<BrowserProc>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<BrowserProc>> {
    SLOT.get_or_init(|| Mutex::new(None))
}

fn candidates() -> Vec<&'static str> {
    #[cfg(target_os = "linux")]
    {
        vec![
            "google-chrome",
            "google-chrome-stable",
            "chromium",
            "chromium-browser",
            "brave-browser",
            "microsoft-edge",
            "microsoft-edge-stable",
            "chrome",
        ]
    }
    #[cfg(target_os = "macos")]
    {
        vec!["google-chrome", "chromium", "brave", "microsoft-edge", "chrome"]
    }
    #[cfg(target_os = "windows")]
    {
        vec!["chrome", "chromium", "msedge", "brave", "chrome.exe", "msedge.exe"]
    }
}

fn app_paths() -> Vec<PathBuf> {
    #[allow(unused_mut)]
    let mut out = Vec::new();
    #[cfg(target_os = "macos")]
    {
        for p in [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        ] {
            out.push(PathBuf::from(p));
        }
    }
    #[cfg(target_os = "windows")]
    {
        for env in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Some(base) = std::env::var_os(env) {
                let base = PathBuf::from(base);
                for rel in [
                    "Google\\Chrome\\Application\\chrome.exe",
                    "Microsoft\\Edge\\Application\\msedge.exe",
                    "BraveSoftware\\Brave-Browser\\Application\\brave.exe",
                ] {
                    out.push(base.join(rel));
                }
            }
        }
    }
    out
}

/// Returns (executable, label).
pub fn detect(cfg: &crate::config::Config) -> Option<(String, String)> {
    if !cfg.browser.path.trim().is_empty() {
        let p = PathBuf::from(expand_path(cfg.browser.path.trim()));
        if p.is_file() {
            let label = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "browser".into());
            return Some((p.to_string_lossy().to_string(), label));
        }
        return None;
    }
    for name in candidates() {
        if let Some(p) = which(name) {
            return Some((p.to_string_lossy().to_string(), name.to_string()));
        }
    }
    for p in app_paths() {
        if p.is_file() {
            let label = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "browser".into());
            return Some((p.to_string_lossy().to_string(), label));
        }
    }
    None
}

pub fn detect_summary(state: &AppState) -> Value {
    let cfg = state.config.get();
    match detect(&cfg) {
        Some((path, label)) => json!({"installed": true, "path": path, "name": label, "headless": cfg.browser.headless}),
        None => json!({"installed": false, "path": Value::Null, "hint": "install Chrome/Chromium/Edge/Brave or set browser.path"}),
    }
}

fn free_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| format!("cannot reserve a port: {e}"))?;
    Ok(listener.local_addr().map_err(|e| e.to_string())?.port())
}

async fn api(state: &AppState, port: u16, method: reqwest::Method, path: &str) -> Result<Value, String> {
    let url = format!("http://127.0.0.1:{port}{path}");
    let resp = state
        .http
        .request(method, &url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("browser DevTools HTTP error: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "DevTools endpoint {path} returned {status}: {}",
            text.chars().take(300).collect::<String>()
        ));
    }
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text).map_err(|e| format!("invalid DevTools response: {e}"))
}

async fn pages(state: &AppState, port: u16) -> Result<Vec<Value>, String> {
    let v = api(state, port, reqwest::Method::GET, "/json/list").await?;
    Ok(v.as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|t| {
            t.get("type").and_then(|x| x.as_str()) == Some("page")
                && !t.get("url").and_then(|u| u.as_str()).unwrap_or("").starts_with("devtools://")
        })
        .collect())
}

async fn new_tab(state: &AppState, port: u16) -> Result<Value, String> {
    if let Ok(v) = api(state, port, reqwest::Method::PUT, "/json/new").await {
        if v.get("webSocketDebuggerUrl").is_some() {
            return Ok(v);
        }
    }
    let v = api(state, port, reqwest::Method::GET, "/json/new").await?;
    if v.get("webSocketDebuggerUrl").is_some() {
        Ok(v)
    } else {
        Err("the browser did not create a new tab".into())
    }
}

struct BrowserInfo {
    port: u16,
    label: String,
    headless: bool,
}

async fn ensure_browser(state: &AppState, headless_override: Option<bool>) -> Result<BrowserInfo, String> {
    let mut guard = slot().lock().await;
    if let Some(b) = guard.as_mut() {
        match b.child.try_wait() {
            Ok(None) => {
                return Ok(BrowserInfo { port: b.port, label: b.label.clone(), headless: b.headless });
            }
            _ => *guard = None,
        }
    }
    let cfg = state.config.get();
    let (path, label) = detect(&cfg).ok_or_else(|| {
        "no Chromium-based browser found (Chrome, Chromium, Edge, Brave). Install one or set browser.path in the config."
            .to_string()
    })?;
    let headless = headless_override.unwrap_or(cfg.browser.headless);
    let port = free_port()?;
    let profile = if cfg.browser.profile_dir.trim().is_empty() {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(crate::config::APP_DIR)
            .join("browser-profile")
    } else {
        PathBuf::from(expand_path(cfg.browser.profile_dir.trim()))
    };
    std::fs::create_dir_all(&profile).map_err(|e| format!("cannot create browser profile {}: {e}", profile.display()))?;

    let mut cmd = tokio::process::Command::new(&path);
    cmd.arg(format!("--remote-debugging-port={port}"))
        .arg(format!("--user-data-dir={}", profile.display()))
        .args([
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "--disable-default-apps",
            "--disable-sync",
            "--disable-extensions",
            "--disable-component-update",
            "--disable-features=Translate,ChromeWhatsNewUI,PrivacySandboxSettings4",
        ])
        .arg(format!("--window-size={},{}", cfg.browser.window_width, cfg.browser.window_height))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if headless {
        cmd.arg("--headless=new").arg("--disable-gpu").arg("--hide-scrollbars");
    }

    let mut child = cmd.spawn().map_err(|e| format!("cannot start the browser ({path}): {e}"))?;
    for _ in 0..200 {
        if api(state, port, reqwest::Method::GET, "/json/version").await.is_ok() {
            *guard = Some(BrowserProc { child, port, label: label.clone(), headless, target: None });
            return Ok(BrowserInfo { port, label, headless });
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let _ = child.kill().await;
    Err("the browser started but its DevTools endpoint did not answer in 20 s. Try launching it manually once (profile creation dialogs), or set browser.path.".into())
}

struct Session {
    port: u16,
    ws: String,
    id: String,
    label: String,
    headless: bool,
}

async fn session(state: &AppState) -> Result<Session, String> {
    let info = ensure_browser(state, None).await?;
    let mut guard = slot().lock().await;
    let b = guard.as_mut().ok_or("browser state lost")?;
    let list = pages(state, info.port).await?;
    let chosen = b
        .target
        .as_ref()
        .and_then(|t| list.iter().find(|p| p.get("id").and_then(|i| i.as_str()) == Some(t.as_str())).cloned())
        .or_else(|| list.first().cloned());
    let target = match chosen {
        Some(t) => t,
        None => new_tab(state, info.port).await?,
    };
    b.target = target.get("id").and_then(|i| i.as_str()).map(str::to_string);
    let ws = target
        .get("webSocketDebuggerUrl")
        .and_then(|w| w.as_str())
        .ok_or("the browser tab has no DevTools websocket URL")?
        .to_string();
    Ok(Session {
        port: info.port,
        ws,
        id: target.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string(),
        label: info.label,
        headless: info.headless,
    })
}

async fn set_target(id: &str) {
    let mut guard = slot().lock().await;
    if let Some(b) = guard.as_mut() {
        b.target = Some(id.to_string());
    }
}

async fn cdp(ws_url: &str, method: &str, params: Value) -> Result<Value, String> {
    let (mut ws, _) = connect_async(ws_url)
        .await
        .map_err(|e| format!("cannot connect to the browser DevTools: {e}"))?;
    let msg = json!({"id": 1, "method": method, "params": params});
    ws.send(Message::Text(msg.to_string().into()))
        .await
        .map_err(|e| format!("DevTools send failed: {e}"))?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(format!("CDP {method} timed out"));
        }
        match tokio::time::timeout(remaining, ws.next()).await {
            Err(_) => return Err(format!("CDP {method} timed out")),
            Ok(None) => return Err("the browser closed the DevTools connection".into()),
            Ok(Some(Err(e))) => return Err(format!("DevTools error: {e}")),
            Ok(Some(Ok(Message::Text(t)))) => {
                let v: Value = match serde_json::from_str(t.as_str()) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if v.get("id").and_then(|i| i.as_u64()) == Some(1) {
                    if let Some(err) = v.get("error") {
                        return Err(format!(
                            "CDP {method}: {}",
                            err.get("message").and_then(|m| m.as_str()).unwrap_or("unknown error")
                        ));
                    }
                    return Ok(v.get("result").cloned().unwrap_or_else(|| json!({})));
                }
            }
            Ok(Some(Ok(_))) => continue,
        }
    }
}

async fn eval(ws: &str, expression: &str, await_promise: bool) -> Result<Value, String> {
    let res = cdp(
        ws,
        "Runtime.evaluate",
        json!({
            "expression": expression,
            "returnByValue": true,
            "awaitPromise": await_promise,
            "userGesture": true,
        }),
    )
    .await?;
    if let Some(exc) = res.get("exceptionDetails") {
        let text = exc
            .get("exception")
            .and_then(|e| e.get("description"))
            .and_then(|d| d.as_str())
            .or_else(|| exc.get("text").and_then(|t| t.as_str()))
            .unwrap_or("JavaScript exception");
        return Err(format!("JavaScript error: {text}"));
    }
    let result = &res["result"];
    if let Some(v) = result.get("value") {
        return Ok(v.clone());
    }
    if let Some(d) = result.get("description") {
        return Ok(Value::String(d.as_str().unwrap_or("").to_string()));
    }
    Ok(Value::Null)
}

async fn wait_ready(ws: &str, timeout_ms: u64) -> Result<String, String> {
    let start = tokio::time::Instant::now();
    let deadline = Duration::from_millis(timeout_ms.clamp(200, 60_000));
    loop {
        let state = match eval(ws, "document.readyState", false).await {
            Ok(Value::String(s)) => s,
            _ => "unknown".to_string(),
        };
        if state == "complete" {
            return Ok(state);
        }
        if start.elapsed() >= deadline {
            return Ok(state);
        }
        tokio::time::sleep(Duration::from_millis(120)).await;
    }
}

async fn page_info(ws: &str) -> Result<Value, String> {
    let value = eval(ws, "JSON.stringify({url: location.href, title: document.title})", false).await?;
    match value {
        Value::String(s) => Ok(serde_json::from_str(&s).unwrap_or_else(|_| json!({"url": "", "title": ""}))),
        _ => Ok(json!({"url": "", "title": ""})),
    }
}

fn js_selector(selector: Option<&str>) -> String {
    selector
        .map(|s| serde_json::to_string(s).unwrap_or_else(|_| "null".into()))
        .unwrap_or_else(|| "null".into())
}

fn content_js(format: &str, selector: Option<&str>) -> String {
    let sel = js_selector(selector);
    let template = match format {
        "html" => r#"(() => { const root = __SEL__ ? document.querySelector(__SEL__) : document.body; return root ? (root.outerHTML || '') : ''; })()"#,
        "markdown" => r#"(() => {
            const root = __SEL__ ? document.querySelector(__SEL__) : document.body;
            if (!root) return '';
            const out = [];
            const walk = (el) => {
                for (const n of el.childNodes) {
                    if (n.nodeType === 3) { const t = n.textContent.replace(/\s+/g, ' '); if (t.trim()) out.push(t); continue; }
                    if (n.nodeType !== 1) continue;
                    const tag = n.tagName.toLowerCase();
                    if (['script','style','noscript','svg','template','iframe'].includes(tag)) continue;
                    if (/^h[1-6]$/.test(tag)) { out.push('\n\n' + '#'.repeat(+tag[1]) + ' ' + (n.innerText || '').trim() + '\n'); continue; }
                    if (tag === 'br') { out.push('\n'); continue; }
                    if (tag === 'li') { out.push('\n- '); }
                    if (tag === 'a') { out.push('[' + (n.innerText || '').trim() + '](' + (n.href || '') + ')'); continue; }
                    if (tag === 'img') { out.push('![' + (n.alt || '') + '](' + (n.src || '') + ')'); continue; }
                    if (tag === 'td' || tag === 'th') { out.push(' ' + (n.innerText || '').trim() + ' | '); continue; }
                    if (tag === 'pre') { out.push('\n```\n'); walk(n); out.push('\n```\n'); continue; }
                    if (['p','div','section','article','header','footer','tr','table','ul','ol','blockquote','li','h1','h2','h3','h4','h5','h6'].includes(tag)) out.push('\n');
                    walk(n);
                    if (['p','div','section','article','header','footer','tr','table','ul','ol','blockquote','li','h1','h2','h3','h4','h5','h6'].includes(tag)) out.push('\n');
                }
            };
            walk(root);
            return out.join('').replace(/\n{3,}/g, '\n\n').trim();
        })()"#,
        _ => r#"(() => { const root = __SEL__ ? document.querySelector(__SEL__) : document.body; return root ? (root.innerText || '') : ''; })()"#,
    };
    template.replace("__SEL__", &sel)
}

fn click_js(selector: &str) -> String {
    let sel = serde_json::to_string(selector).unwrap_or_else(|_| "null".into());
    r#"(() => {
        const el = document.querySelector(__SEL__);
        if (!el) return {found: false};
        el.scrollIntoView({block: 'center', inline: 'center'});
        el.click();
        return {found: true, tag: el.tagName.toLowerCase(), text: (el.innerText || el.value || '').slice(0, 120)};
    })()"#
        .replace("__SEL__", &sel)
}

// -------------------------------------------------------------------- tools

pub async fn open(state: Arc<AppState>, args: Value) -> Result<ToolOutput, String> {
    let headless = args.get("headless").and_then(|v| v.as_bool());
    let info = ensure_browser(&state, headless).await?;
    let url = arg_str(&args, "url");
    let wait_ms = arg_u32(&args, "wait_ms").unwrap_or(10_000) as u64;

    let session = if url.is_some() || arg_bool(&args, "new_tab", false) {
        let target = new_tab(&state, info.port).await?;
        let id = target.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
        let ws = target
            .get("webSocketDebuggerUrl")
            .and_then(|w| w.as_str())
            .ok_or("new tab has no websocket URL")?
            .to_string();
        set_target(&id).await;
        Session { port: info.port, ws, id, label: info.label.clone(), headless: info.headless }
    } else {
        session(&state).await?
    };

    let mut out = json!({
        "ok": true,
        "browser": session.label,
        "headless": session.headless,
        "port": session.port,
        "tab_id": session.id,
    });
    if let Some(u) = url {
        cdp(&session.ws, "Page.navigate", json!({"url": u})).await?;
        let ready = wait_ready(&session.ws, wait_ms).await?;
        let info = page_info(&session.ws).await.unwrap_or_else(|_| json!({}));
        out["url"] = info.get("url").cloned().unwrap_or(json!(u));
        out["title"] = info.get("title").cloned().unwrap_or(Value::Null);
        out["ready_state"] = json!(ready);
    } else {
        let info = page_info(&session.ws).await.unwrap_or_else(|_| json!({}));
        out["url"] = info.get("url").cloned().unwrap_or(Value::Null);
        out["title"] = info.get("title").cloned().unwrap_or(Value::Null);
    }
    Ok(ToolOutput::json(out))
}

pub async fn navigate(state: Arc<AppState>, args: Value) -> Result<ToolOutput, String> {
    let url = arg_str(&args, "url").ok_or("url is required")?;
    let session = session(&state).await?;
    cdp(&session.ws, "Page.navigate", json!({"url": url})).await?;
    let ready = wait_ready(&session.ws, arg_u32(&args, "timeout_ms").unwrap_or(15_000) as u64).await?;
    let info = page_info(&session.ws).await?;
    Ok(ToolOutput::json(json!({
        "ok": true,
        "tab_id": session.id,
        "url": info.get("url").cloned().unwrap_or(json!(url)),
        "title": info.get("title").cloned().unwrap_or(Value::Null),
        "ready_state": ready,
    })))
}

pub async fn get_content(state: Arc<AppState>, args: Value) -> Result<ToolOutput, String> {
    let format = arg_str(&args, "format").unwrap_or("text").to_ascii_lowercase();
    let selector = arg_str(&args, "selector");
    let max_chars = arg_u32(&args, "max_chars").unwrap_or(20_000).clamp(100, 5_000_000) as usize;
    let session = session(&state).await?;
    let raw = eval(&session.ws, &content_js(&format, selector), false).await?;
    let text = match raw {
        Value::String(s) => s,
        other => serde_json::to_string(&other).unwrap_or_default(),
    };
    let (content, truncated) = super::truncate_chars(&text, max_chars);
    let info = page_info(&session.ws).await.unwrap_or_else(|_| json!({}));
    Ok(ToolOutput::json(json!({
        "tab_id": session.id,
        "url": info.get("url").cloned().unwrap_or(Value::Null),
        "title": info.get("title").cloned().unwrap_or(Value::Null),
        "format": format,
        "chars": text.chars().count(),
        "truncated": truncated,
        "content": content,
    })))
}

pub async fn click(state: Arc<AppState>, args: Value) -> Result<ToolOutput, String> {
    let session = session(&state).await?;
    if let Some(selector) = arg_str(&args, "selector") {
        let clicked = eval(&session.ws, &click_js(selector), false).await?;
        if clicked.get("found").and_then(|f| f.as_bool()) != Some(true) {
            return Err(format!("no element matches selector `{selector}`"));
        }
        return Ok(ToolOutput::json(json!({
            "ok": true,
            "mode": "selector",
            "selector": selector,
            "clicked": clicked,
        })));
    }
    let x = super::arg_i64(&args, "x").ok_or("pass selector, or x and y")?;
    let y = super::arg_i64(&args, "y").ok_or("pass selector, or x and y")?;
    let button = match arg_str(&args, "button").unwrap_or("left") {
        "right" => "right",
        "middle" => "middle",
        _ => "left",
    };
    let count = if arg_bool(&args, "double", false) { 2 } else { 1 };
    cdp(&session.ws, "Input.dispatchMouseEvent", json!({"type": "mouseMoved", "x": x, "y": y})).await?;
    cdp(
        &session.ws,
        "Input.dispatchMouseEvent",
        json!({"type": "mousePressed", "x": x, "y": y, "button": button, "clickCount": count}),
    )
    .await?;
    cdp(
        &session.ws,
        "Input.dispatchMouseEvent",
        json!({"type": "mouseReleased", "x": x, "y": y, "button": button, "clickCount": count}),
    )
    .await?;
    Ok(ToolOutput::json(json!({"ok": true, "mode": "coordinates", "x": x, "y": y, "button": button, "clicks": count})))
}

pub async fn type_text(state: Arc<AppState>, args: Value) -> Result<ToolOutput, String> {
    let text = arg_str(&args, "text").ok_or("text is required")?;
    let session = session(&state).await?;
    if let Some(selector) = arg_str(&args, "selector") {
        let sel = js_selector(Some(selector));
        let ok = eval(
            &session.ws,
            &format!("(() => {{ const el = document.querySelector({sel}); if (!el) return false; el.focus(); return true; }})()"),
            false,
        )
        .await?;
        if ok.as_bool() != Some(true) {
            return Err(format!("no element matches selector `{selector}`"));
        }
    }
    cdp(&session.ws, "Input.insertText", json!({"text": text})).await?;
    if arg_bool(&args, "submit", false) {
        cdp(
            &session.ws,
            "Input.dispatchKeyEvent",
            json!({"type": "keyDown", "key": "Enter", "code": "Enter", "windowsVirtualKeyCode": 13, "nativeVirtualKeyCode": 13}),
        )
        .await?;
        cdp(
            &session.ws,
            "Input.dispatchKeyEvent",
            json!({"type": "keyUp", "key": "Enter", "code": "Enter", "windowsVirtualKeyCode": 13, "nativeVirtualKeyCode": 13}),
        )
        .await?;
    }
    Ok(ToolOutput::json(json!({
        "ok": true,
        "typed": text.chars().count(),
        "submitted": arg_bool(&args, "submit", false),
    })))
}

pub async fn screenshot(state: Arc<AppState>, args: Value) -> Result<ToolOutput, String> {
    let session = session(&state).await?;
    let full = arg_bool(&args, "full_page", false);
    let res = cdp(
        &session.ws,
        "Page.captureScreenshot",
        json!({"format": "png", "captureBeyondViewport": full, "fromSurface": true}),
    )
    .await?;
    let data = res
        .get("data")
        .and_then(|d| d.as_str())
        .ok_or("the browser returned no screenshot data")?;
    let info = page_info(&session.ws).await.unwrap_or_else(|_| json!({}));
    let mut out = ToolOutput::image_b64("image/png", data.to_string());
    out.content.push(super::Content::Text {
        text: json!({
            "ok": true,
            "tab_id": session.id,
            "url": info.get("url").cloned().unwrap_or(Value::Null),
            "title": info.get("title").cloned().unwrap_or(Value::Null),
            "full_page": full,
        })
        .to_string(),
    });
    Ok(out)
}

pub async fn eval_js(state: Arc<AppState>, args: Value) -> Result<ToolOutput, String> {
    let expression = arg_str(&args, "expression").ok_or("expression is required")?;
    let await_promise = arg_bool(&args, "await_promise", false);
    let max_chars = arg_u32(&args, "max_chars").unwrap_or(20_000).clamp(100, 5_000_000) as usize;
    let session = session(&state).await?;
    let value = eval(&session.ws, expression, await_promise).await?;
    let mut text = match &value {
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    };
    let truncated;
    if text.chars().count() > max_chars {
        let (t, _) = super::truncate_chars(&text, max_chars);
        text = t;
        truncated = true;
    } else {
        truncated = false;
    }
    Ok(ToolOutput::json(json!({
        "tab_id": session.id,
        "result": text,
        "truncated": truncated,
    })))
}

pub async fn tabs(state: Arc<AppState>, args: Value) -> Result<ToolOutput, String> {
    let action = arg_str(&args, "action").unwrap_or("list").to_ascii_lowercase();
    let info = ensure_browser(&state, None).await?;
    match action.as_str() {
        "list" => {
            let list = pages(&state, info.port).await?;
            let current = {
                let guard = slot().lock().await;
                guard.as_ref().and_then(|b| b.target.clone())
            };
            let rows: Vec<Value> = list
                .iter()
                .map(|t| {
                    let id = t.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                    json!({
                        "id": id,
                        "title": t.get("title").cloned().unwrap_or(Value::Null),
                        "url": t.get("url").cloned().unwrap_or(Value::Null),
                        "active": current.as_deref() == Some(id.as_str()),
                    })
                })
                .collect();
            Ok(ToolOutput::json(json!({"tabs": rows, "count": rows.len()})))
        }
        "new" => {
            let target = new_tab(&state, info.port).await?;
            let id = target.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
            set_target(&id).await;
            let ws = target
                .get("webSocketDebuggerUrl")
                .and_then(|w| w.as_str())
                .ok_or("new tab has no websocket URL")?
                .to_string();
            if let Some(url) = arg_str(&args, "url") {
                cdp(&ws, "Page.navigate", json!({"url": url})).await?;
                wait_ready(&ws, 10_000).await?;
            }
            let page = page_info(&ws).await.unwrap_or_else(|_| json!({}));
            Ok(ToolOutput::json(json!({"ok": true, "action": "new", "tab_id": id, "url": page.get("url").cloned().unwrap_or(Value::Null), "title": page.get("title").cloned().unwrap_or(Value::Null)})))
        }
        "select" => {
            let id = arg_str(&args, "id").ok_or("id is required for select")?;
            api(&state, info.port, reqwest::Method::GET, &format!("/json/activate/{id}")).await?;
            set_target(id).await;
            Ok(ToolOutput::json(json!({"ok": true, "action": "select", "tab_id": id})))
        }
        "close" => {
            let id = arg_str(&args, "id").ok_or("id is required for close")?;
            api(&state, info.port, reqwest::Method::GET, &format!("/json/close/{id}")).await?;
            let mut guard = slot().lock().await;
            if let Some(b) = guard.as_mut() {
                if b.target.as_deref() == Some(id) {
                    b.target = None;
                }
            }
            Ok(ToolOutput::json(json!({"ok": true, "action": "close", "tab_id": id})))
        }
        other => Err(format!("unknown tabs action: {other} (list, new, select, close)")),
    }
}

pub async fn close(state: Arc<AppState>, _args: Value) -> Result<ToolOutput, String> {
    let mut guard = slot().lock().await;
    if let Some(mut b) = guard.take() {
        let _ = b.child.kill().await;
        let _ = b.child.wait().await;
        Ok(ToolOutput::json(json!({"ok": true, "closed": b.label})))
    } else {
        let _ = state;
        Ok(ToolOutput::json(json!({"ok": true, "closed": Value::Null, "note": "the automation browser was not running"})))
    }
}

/// Plain HTTP fetch (no browser) with HTML-to-text extraction.
pub async fn fetch_page(state: Arc<AppState>, args: Value) -> Result<ToolOutput, String> {
    let url_str = arg_str(&args, "url").ok_or("url is required")?;
    let url = reqwest::Url::parse(url_str).map_err(|e| format!("invalid URL: {e}"))?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err("only http and https URLs are supported".into());
    }
    let max_chars = arg_u32(&args, "max_chars").unwrap_or(20_000).clamp(100, 5_000_000) as usize;
    let format = arg_str(&args, "format").unwrap_or("text").to_ascii_lowercase();
    let resp = tokio::time::timeout(Duration::from_secs(30), state.http.get(url.clone()).send())
        .await
        .map_err(|_| "fetch timed out after 30 s".to_string())?
        .map_err(|e| format!("fetch failed: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let is_html = content_type.contains("html") || content_type.is_empty();
    let bytes = tokio::time::timeout(Duration::from_secs(60), resp.bytes())
        .await
        .map_err(|_| "fetch body timed out".to_string())?
        .map_err(|e| format!("fetch failed: {e}"))?;
    if bytes.len() > 10 * 1024 * 1024 {
        return Err("page is larger than 10 MB".into());
    }
    let raw = String::from_utf8_lossy(&bytes).to_string();
    let text = match format.as_str() {
        "html" | "raw" => raw,
        _ if is_html => super::html_to_text(&raw),
        _ => raw,
    };
    let (content, truncated) = super::truncate_chars(&text, max_chars);
    Ok(ToolOutput::json(json!({
        "url": url.as_str(),
        "status": status.as_u16(),
        "content_type": content_type,
        "format": if format == "html" || format == "raw" { "html" } else { "text" },
        "truncated": truncated,
        "content": content,
    })))
}
