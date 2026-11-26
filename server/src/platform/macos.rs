//! macOS backend: `screencapture` for screenshots, enigo for input,
//! AppleScript for windows and notifications.
//!
//! Permissions: Screen Recording for screenshots (System Settings > Privacy &
//! Security) and Accessibility for input.

use super::{Backend, Captured, DisplayInfo, Key, Modifier, MouseButton, WindowInfo};
use enigo::{Axis, Button, Coordinate, Direction, Enigo, Key as EKey, Keyboard, Mouse, Settings};
use serde_json::{json, Value};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

fn enigo() -> Result<Enigo, String> {
    Enigo::new(&Settings::default()).map_err(|e| {
        format!("cannot create the input controller (grant Accessibility permission to this app): {e}")
    })
}

fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run {cmd}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{cmd} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn osa(script: &str) -> Result<String, String> {
    run("osascript", &["-e", script])
}

fn apple_quote(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn detached(cmd: &mut Command) -> Result<u32, String> {
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    cmd.spawn().map(|c| c.id()).map_err(|e| format!("failed to start process: {e}"))
}

pub async fn screenshot() -> Result<Captured, String> {
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!("pcrc-shot-{stamp}.png"));
    let status = Command::new("screencapture")
        .args(["-x", "-t", "png"])
        .arg(&path)
        .status()
        .map_err(|e| format!("failed to run screencapture: {e}"))?;
    if !status.success() {
        return Err("screencapture failed (grant Screen Recording permission to this app)".into());
    }
    let img = image::open(&path).map_err(|e| format!("cannot read the screenshot: {e}"))?;
    let _ = std::fs::remove_file(&path);
    let (w, h) = (img.width(), img.height());
    let scale = match enigo().and_then(|mut e| e.main_display().map_err(|err| err.to_string())) {
        Ok((lw, _)) if lw > 0 => {
            let s = (w as f32 / lw as f32 * 100.0).round() / 100.0;
            if s >= 1.0 && s <= 4.0 {
                s
            } else {
                1.0
            }
        }
        _ => 1.0,
    };
    Ok(Captured { image: img, width: w, height: h, scale, backend: "screencapture" })
}

pub fn displays() -> Vec<DisplayInfo> {
    let mut out = Vec::new();
    if let Ok(json_text) = run(
        "system_profiler",
        &["SPDisplaysDataType", "-json", "-detailLevel", "mini"],
    ) {
        if let Ok(v) = serde_json::from_str::<Value>(&json_text) {
            if let Some(gpus) = v.get("SPDisplaysDataType").and_then(|g| g.as_array()) {
                for gpu in gpus {
                    if let Some(displays) = gpu.get("spdisplays_ndrvs").and_then(|d| d.as_array()) {
                        for d in displays {
                            let name = d
                                .get("_name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("display")
                                .to_string();
                            let res = d
                                .get("_spdisplays_resolution")
                                .and_then(|r| r.as_str())
                                .unwrap_or("")
                                .to_string();
                            if let Some((w, h)) = res.split_once('x') {
                                let w = w.split_whitespace().next().unwrap_or("0");
                                if let (Ok(w), Ok(h)) = (
                                    w.parse::<u32>(),
                                    h.split_whitespace().next().unwrap_or("0").parse::<u32>(),
                                ) {
                                    out.push(DisplayInfo {
                                        id: out.len().to_string(),
                                        name,
                                        x: 0,
                                        y: 0,
                                        width: w,
                                        height: h,
                                        primary: out.is_empty(),
                                        scale: 1.0,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if out.is_empty() {
        if let Ok(mut e) = enigo() {
            if let Ok((w, h)) = e.main_display() {
                out.push(DisplayInfo {
                    id: "0".into(),
                    name: "main".into(),
                    x: 0,
                    y: 0,
                    width: w.max(0) as u32,
                    height: h.max(0) as u32,
                    primary: true,
                    scale: 1.0,
                });
            }
        }
    }
    out
}

pub fn cursor_position() -> Option<(i32, i32)> {
    enigo().ok().and_then(|mut e| e.location().ok())
}

pub fn move_abs(_pref: Backend, x: i32, y: i32) -> Result<(), String> {
    enigo()?.move_mouse(x, y, Coordinate::Abs).map_err(|e| e.to_string())
}

pub fn move_rel(_pref: Backend, dx: i32, dy: i32) -> Result<(), String> {
    enigo()?.move_mouse(dx, dy, Coordinate::Rel).map_err(|e| e.to_string())
}

fn button(b: MouseButton) -> Button {
    match b {
        MouseButton::Left => Button::Left,
        MouseButton::Right => Button::Right,
        MouseButton::Middle => Button::Middle,
        MouseButton::Back => Button::Back,
        MouseButton::Forward => Button::Forward,
    }
}

pub fn button_down(_pref: Backend, b: MouseButton) -> Result<(), String> {
    enigo()?.button(button(b), Direction::Press).map_err(|e| e.to_string())
}

pub fn button_up(_pref: Backend, b: MouseButton) -> Result<(), String> {
    enigo()?.button(button(b), Direction::Release).map_err(|e| e.to_string())
}

pub fn click(_pref: Backend, b: MouseButton, count: u8, interval_ms: u64) -> Result<(), String> {
    let mut e = enigo()?;
    for i in 0..count.max(1) {
        e.button(button(b), Direction::Click).map_err(|err| err.to_string())?;
        if i + 1 < count.max(1) && interval_ms > 0 {
            std::thread::sleep(Duration::from_millis(interval_ms));
        }
    }
    Ok(())
}

pub fn scroll(_pref: Backend, dx: i32, dy: i32) -> Result<(), String> {
    let mut e = enigo()?;
    if dy != 0 {
        e.scroll(-dy, Axis::Vertical).map_err(|err| err.to_string())?;
    }
    if dx != 0 {
        e.scroll(dx, Axis::Horizontal).map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn enigo_key(key: &Key) -> Option<EKey> {
    Some(match key {
        Key::Char(c) => EKey::Unicode(*c),
        Key::Enter => EKey::Return,
        Key::Tab => EKey::Tab,
        Key::Escape => EKey::Escape,
        Key::Backspace => EKey::Backspace,
        Key::Delete => EKey::Delete,
        Key::Home => EKey::Home,
        Key::End => EKey::End,
        Key::PageUp => EKey::PageUp,
        Key::PageDown => EKey::PageDown,
        Key::Up => EKey::UpArrow,
        Key::Down => EKey::DownArrow,
        Key::Left => EKey::LeftArrow,
        Key::Right => EKey::RightArrow,
        Key::Space => EKey::Space,
        Key::CapsLock => EKey::CapsLock,
        Key::Menu | Key::Insert | Key::PrintScreen => return None,
        Key::F(n) => match n {
            1 => EKey::F1,
            2 => EKey::F2,
            3 => EKey::F3,
            4 => EKey::F4,
            5 => EKey::F5,
            6 => EKey::F6,
            7 => EKey::F7,
            8 => EKey::F8,
            9 => EKey::F9,
            10 => EKey::F10,
            11 => EKey::F11,
            12 => EKey::F12,
            _ => return None,
        },
        Key::Other(_) => return None,
    })
}

pub fn key_chord(_pref: Backend, modifiers: &[Modifier], key: &Key) -> Result<(), String> {
    let mut e = enigo()?;
    let ek = enigo_key(key).ok_or_else(|| format!("key {key:?} is not supported on macOS"))?;
    let mods: Vec<EKey> = modifiers
        .iter()
        .map(|m| match m {
            Modifier::Ctrl => EKey::Control,
            Modifier::Shift => EKey::Shift,
            Modifier::Alt => EKey::Alt,
            Modifier::Meta => EKey::Meta,
        })
        .collect();
    for m in &mods {
        e.key(*m, Direction::Press).map_err(|err| err.to_string())?;
    }
    let result = e.key(ek, Direction::Click).map_err(|err| err.to_string());
    for m in mods.iter().rev() {
        let _ = e.key(*m, Direction::Release);
    }
    result
}

pub fn type_text(_pref: Backend, text: &str, interval_ms: u64) -> Result<(), String> {
    let mut e = enigo()?;
    if interval_ms == 0 {
        e.text(text).map_err(|err| err.to_string())
    } else {
        for ch in text.chars() {
            e.key(EKey::Unicode(ch), Direction::Click).map_err(|err| err.to_string())?;
            std::thread::sleep(Duration::from_millis(interval_ms));
        }
        Ok(())
    }
}

pub fn input_note(_pref: Backend) -> Option<String> {
    None
}

pub fn list_windows() -> Result<Vec<WindowInfo>, String> {
    let script = r#"tell application "System Events"
set out to ""
repeat with p in (application processes whose background only is false)
set out to out & (name of p as text) & tab & (unix id of p as text) & linefeed
end repeat
return out
end tell"#;
    let text = osa(script)?;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (name, pid) = line.split_once('\t').unwrap_or((line, "0"));
        let pid = pid.trim().parse::<u32>().ok();
        out.push(WindowInfo {
            id: pid.map(|p| p.to_string()).unwrap_or_default(),
            title: name.trim().to_string(),
            pid,
            app: name.trim().to_string(),
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            focused: false,
        });
    }
    Ok(out)
}

pub fn focus_window(id: &str) -> Result<(), String> {
    let pid: u32 = id.parse().map_err(|_| format!("invalid macOS window id: {id}"))?;
    let script = format!(
        "tell application \"System Events\" to set frontmost of (first application process whose unix id is {pid}) to true"
    );
    osa(&script).map(|_| ())
}

pub fn open_target(target: &str) -> Result<u32, String> {
    detached(Command::new("open").arg(target))
}

pub fn launch_detached(program: &str, args: &[String], cwd: Option<&str>) -> Result<u32, String> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    detached(&mut cmd)
}

pub fn notify(title: &str, body: &str) -> Result<(), String> {
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        apple_quote(body),
        apple_quote(title)
    );
    osa(&script).map(|_| ())
}

pub fn lock_screen() -> Result<(), String> {
    let path = "/System/Library/CoreServices/Menu Extras/User.menu/Contents/Resources/CGSession";
    let status = Command::new(path).arg("-suspend").status().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("CGSession exited with {status}"))
    }
}

pub fn backends_report() -> Value {
    json!({
        "screenshot": "screencapture",
        "input": "enigo",
        "wayland": false,
        "permissions": "screenshots need Screen Recording; input needs Accessibility",
        "portal": Value::Null,
        "uinput": Value::Null,
    })
}

// ------------------------------------------------------ power / admin / term

fn clip(s: String, max: usize) -> String {
    if s.chars().count() > max {
        s.chars().take(max).collect()
    } else {
        s
    }
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub async fn power_action(action: &str, delay_secs: u32) -> Result<(), String> {
    match action {
        "lock" => return lock_screen(),
        "suspend" => return run("pmset", &["sleepnow"]).map(|_| ()),
        "hibernate" => {
            run("sudo", &["-n", "pmset", "hibernatemode", "25"]).map_err(|e| {
                format!("hibernate needs passwordless sudo for pmset ({e}); run `sudo pmset hibernatemode 25 && sudo pmset sleepnow` in a visible terminal")
            })?;
            return run("sudo", &["-n", "pmset", "sleepnow"]).map(|_| ());
        }
        "shutdown" | "restart" | "logout" => {}
        other => return Err(format!("unknown power action: {other}")),
    }
    let script = match action {
        "shutdown" => "tell application \"System Events\" to shut down",
        "restart" => "tell application \"System Events\" to restart",
        _ => "tell application \"System Events\" to log out",
    };
    if delay_secs > 0 {
        let line = format!("sleep {}; osascript -e {}", delay_secs, sh_quote(script));
        std::process::Command::new("sh")
            .arg("-c")
            .arg(line)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("cannot schedule the power action: {e}"))?;
        return Ok(());
    }
    osa(script).map(|_| ())
}

pub async fn admin_exec(program: &str, args: &[String], timeout_ms: u64) -> Result<Value, String> {
    let command_line = format!(
        "{} {}",
        program,
        args.iter().map(|a| sh_quote(a)).collect::<Vec<_>>().join(" ")
    );
    let script = format!(
        "do shell script \"{}\" with administrator privileges",
        apple_quote(&command_line)
    );
    let out = tokio::time::timeout(
        Duration::from_millis(timeout_ms.max(120_000)),
        tokio::process::Command::new("osascript")
            .args(["-e", &script])
            .stdin(Stdio::null())
            .output(),
    )
    .await
    .map_err(|_| "the macOS administrator prompt timed out or was dismissed".to_string())?
    .map_err(|e| format!("osascript failed: {e}"))?;
    Ok(json!({
        "ok": out.status.success(),
        "via": "osascript administrator privileges",
        "command": command_line,
        "exit_code": out.status.code(),
        "stdout": clip(String::from_utf8_lossy(&out.stdout).to_string(), 64 * 1024),
        "stderr": clip(String::from_utf8_lossy(&out.stderr).to_string(), 64 * 1024),
    }))
}

pub async fn service_control(action: &str, name: Option<&str>) -> Result<Value, String> {
    let uid = run("id", &["-u"]).unwrap_or_else(|_| "501".into()).trim().to_string();
    let args: Vec<String> = match action {
        "list" => vec!["list".into()],
        "status" => {
            let n = name.ok_or("name is required for status")?;
            vec!["print".into(), format!("gui/{uid}/{n}")]
        }
        "start" | "restart" => {
            let n = name.ok_or("name is required")?;
            vec!["kickstart".into(), "-k".into(), format!("gui/{uid}/{n}")]
        }
        "stop" => {
            let n = name.ok_or("name is required")?;
            vec!["bootout".into(), format!("gui/{uid}/{n}")]
        }
        "enable" | "disable" => {
            let n = name.ok_or("name is required")?;
            vec![action.into(), format!("gui/{uid}/{n}")]
        }
        other => return Err(format!("unknown service action: {other}")),
    };
    let out = tokio::time::timeout(
        Duration::from_secs(30),
        tokio::process::Command::new("launchctl").args(&args).output(),
    )
    .await
    .map_err(|_| "launchctl timed out".to_string())?
    .map_err(|e| format!("cannot run launchctl: {e}"))?;
    Ok(json!({
        "action": action,
        "name": name,
        "via": "launchctl",
        "exit_code": out.status.code(),
        "stdout": clip(String::from_utf8_lossy(&out.stdout).to_string(), 32 * 1024),
        "stderr": clip(String::from_utf8_lossy(&out.stderr).to_string(), 32 * 1024),
    }))
}

pub fn run_in_terminal(command: &str, cwd: Option<&str>) -> Result<u32, String> {
    let mut line = String::new();
    if let Some(dir) = cwd {
        line.push_str(&format!("cd {} && ", sh_quote(dir)));
    }
    line.push_str(command);
    let script = format!("tell application \"Terminal\" to do script \"{}\"", apple_quote(&line));
    let pid = std::process::Command::new("osascript")
        .args(["-e", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot open Terminal: {e}"))?
        .id();
    let _ = std::process::Command::new("osascript")
        .args(["-e", "tell application \"Terminal\" to activate"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    Ok(pid)
}
