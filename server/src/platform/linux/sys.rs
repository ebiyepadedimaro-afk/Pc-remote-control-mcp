//! Linux system helpers: screenshot selection, displays, cursor, apps.

use super::x11::X11;
use crate::platform::{Captured, DisplayInfo};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub fn session_is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").map(|v| !v.is_empty()).unwrap_or(false)
        || std::env::var("XDG_SESSION_TYPE").map(|v| v.eq_ignore_ascii_case("wayland")).unwrap_or(false)
}

/// Combined screen size in session coordinates (XWayland RandR first, then
/// DRM modes as a fallback).
pub fn screen_size() -> Option<(u32, u32)> {
    if let Ok(x) = X11::connect() {
        if let Some(size) = x.root_size() {
            return Some(size);
        }
    }
    let mut best: Option<(u32, u32)> = None;
    if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            let dir = entry.path();
            let status = std::fs::read_to_string(dir.join("status")).unwrap_or_default();
            if !status.trim().eq_ignore_ascii_case("connected") {
                continue;
            }
            if let Ok(modes) = std::fs::read_to_string(dir.join("modes")) {
                for line in modes.lines() {
                    if let Some((w, h)) = line.trim().split_once('x') {
                        if let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>()) {
                            if w > 0 && h > 0 && best.map(|(bw, bh)| (w as u64 * h as u64) > (bw * bh) as u64).unwrap_or(true) {
                                best = Some((w, h));
                            }
                            break;
                        }
                    }
                }
            }
        }
    }
    best
}

fn scale_for(width: u32, height: u32) -> f32 {
    if let Some((lw, lh)) = screen_size() {
        if lw > 0 && lh > 0 {
            let sx = width as f32 / lw as f32;
            let sy = height as f32 / lh as f32;
            let s = ((sx + sy) / 2.0 * 100.0).round() / 100.0;
            if s >= 1.0 && s <= 4.0 {
                return s;
            }
        }
    }
    1.0
}

pub async fn screenshot() -> Result<Captured, String> {
    let mut portal_error: Option<String> = None;
    if session_is_wayland() && super::portal::portal_installed() {
        match super::portal::screenshot().await {
            Ok(img) => {
                let (w, h) = (img.width(), img.height());
                return Ok(Captured {
                    image: img,
                    width: w,
                    height: h,
                    scale: scale_for(w, h),
                    backend: "wayland-portal",
                });
            }
            Err(e) => portal_error = Some(e),
        }
    }
    let x11_result = X11::connect().and_then(|x| x.screenshot());
    match x11_result {
        Ok(c) => Ok(c),
        Err(e) => Err(match portal_error {
            Some(pe) => format!("screenshot failed. Portal: {pe}. X11: {e}"),
            None => format!("screenshot failed: {e}"),
        }),
    }
}

pub fn displays() -> Vec<DisplayInfo> {
    if let Ok(x) = X11::connect() {
        let d = x.displays();
        if !d.is_empty() {
            return d;
        }
    }
    // Best effort without XWayland: list connected DRM outputs at the origin.
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            let dir = entry.path();
            let status = std::fs::read_to_string(dir.join("status")).unwrap_or_default();
            if !status.trim().eq_ignore_ascii_case("connected") {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if let Ok(modes) = std::fs::read_to_string(dir.join("modes")) {
                if let Some((w, h)) = modes.lines().next().and_then(|l| l.split_once('x')) {
                    if let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>()) {
                        out.push(DisplayInfo {
                            id: name.clone(),
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
    out
}

pub fn cursor_position() -> Option<(i32, i32)> {
    if let Ok(x) = X11::connect() {
        return x.cursor();
    }
    None
}

fn detached(cmd: &mut Command) -> Result<u32, String> {
    use std::os::unix::process::CommandExt;
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    cmd.process_group(0);
    cmd.spawn()
        .map(|c| c.id())
        .map_err(|e| format!("failed to start process: {e}"))
}

pub fn open_target(target: &str) -> Result<u32, String> {
    let mut cmd = Command::new("xdg-open");
    cmd.arg(target);
    detached(&mut cmd)
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
    let has_notify_send = Command::new("sh")
        .args(["-c", "command -v notify-send"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !has_notify_send {
        return Err("notify-send is not installed (package `libnotify-bin`)".into());
    }
    let status = Command::new("notify-send")
        .arg(title)
        .arg(body)
        .status()
        .map_err(|e| format!("notify-send failed: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("notify-send exited with {status}"))
    }
}

pub fn lock_screen() -> Result<(), String> {
    for (cmd, args) in [("loginctl", vec!["lock-session"]), ("xdg-screensaver", vec!["lock"])] {
        if let Ok(status) = Command::new(cmd).args(&args).status() {
            if status.success() {
                return Ok(());
            }
        }
    }
    Err("could not lock the screen: loginctl and xdg-screensaver both failed".into())
}

pub fn backends_report() -> Value {
    let x11_ok = X11::connect().is_ok();
    let uinput = super::uinput::available();
    let wayland = session_is_wayland();
    let preferred = if wayland {
        if uinput { "uinput" } else if x11_ok { "x11" } else { "none" }
    } else if x11_ok {
        "x11"
    } else if uinput {
        "uinput"
    } else {
        "none"
    };
    json!({
        "screenshot": if wayland && super::portal::portal_installed() { "wayland-portal" } else if x11_ok { "x11" } else { "none" },
        "input": preferred,
        "input_preferred": preferred,
        "wayland": wayland,
        "x11": {"display": std::env::var("DISPLAY").ok(), "reachable": x11_ok},
        "portal": {"installed": super::portal::portal_installed(), "available": wayland && super::portal::portal_installed()},
        "uinput": {
            "path": "/dev/uinput",
            "exists": Path::new("/dev/uinput").exists(),
            "writable": uinput,
            "instructions": if uinput { Value::Null } else { Value::String(super::uinput::setup_instructions()) },
        },
        "session": std::env::var("XDG_SESSION_TYPE").ok(),
        "desktop": std::env::var("XDG_CURRENT_DESKTOP").ok(),
    })
}

// ------------------------------------------------------ power / admin / term

use std::time::Duration;

fn clip(s: String, max: usize) -> String {
    if s.chars().count() > max {
        s.chars().take(max).collect()
    } else {
        s
    }
}

fn path_of(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let p = PathBuf::from(name);
        return if p.is_file() { Some(p) } else { None };
    }
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

fn expand_home(p: &str) -> String {
    if p == "~" {
        return dirs::home_dir().map(|h| h.to_string_lossy().to_string()).unwrap_or_else(|| p.to_string());
    }
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().to_string();
        }
    }
    p.to_string()
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub async fn power_action(action: &str, delay_secs: u32) -> Result<(), String> {
    let mut candidates: Vec<(String, Vec<String>)> = Vec::new();
    match action {
        "shutdown" => {
            candidates.push(("systemctl".into(), vec!["poweroff".into()]));
            candidates.push(("loginctl".into(), vec!["poweroff".into()]));
            candidates.push(("shutdown".into(), vec!["-h".into(), "now".into()]));
        }
        "restart" => {
            candidates.push(("systemctl".into(), vec!["reboot".into()]));
            candidates.push(("loginctl".into(), vec!["reboot".into()]));
            candidates.push(("shutdown".into(), vec!["-r".into(), "now".into()]));
        }
        "hibernate" => {
            candidates.push(("systemctl".into(), vec!["hibernate".into()]));
            candidates.push(("loginctl".into(), vec!["hibernate".into()]));
        }
        "suspend" => {
            candidates.push(("systemctl".into(), vec!["suspend".into()]));
            candidates.push(("loginctl".into(), vec!["suspend".into()]));
        }
        "logout" => {
            candidates.push(("gnome-session-quit".into(), vec!["--logout".into(), "--no-prompt".into()]));
            if let Ok(sid) = std::env::var("XDG_SESSION_ID") {
                candidates.push(("loginctl".into(), vec!["terminate-session".into(), sid]));
            }
        }
        "lock" => return lock_screen(),
        other => return Err(format!("unknown power action: {other}")),
    }

    if delay_secs > 0 {
        let (prog, args) = &candidates[0];
        let line = format!(
            "sleep {}; exec {} {}",
            delay_secs,
            prog,
            args.iter().map(|a| sh_quote(a)).collect::<Vec<_>>().join(" ")
        );
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

    let mut last = "no command available".to_string();
    for (prog, args) in candidates {
        let mut cmd = tokio::process::Command::new(&prog);
        cmd.args(&args).stdin(Stdio::null());
        match tokio::time::timeout(Duration::from_secs(20), cmd.output()).await {
            Ok(Ok(out)) if out.status.success() => return Ok(()),
            Ok(Ok(out)) => {
                last = format!(
                    "{prog} exited with {:?}: {}",
                    out.status.code(),
                    String::from_utf8_lossy(&out.stderr).trim()
                )
            }
            Ok(Err(e)) => last = format!("{prog}: {e}"),
            // Taking the machine down can block until the session ends.
            Err(_) => return Ok(()),
        }
    }
    Err(format!("power action failed ({last})"))
}

pub async fn admin_exec(program: &str, args: &[String], timeout_ms: u64) -> Result<Value, String> {
    let is_root = unsafe { libc::geteuid() } == 0;
    let (bin, full_args, via) = if is_root {
        (program.to_string(), args.to_vec(), "root")
    } else if path_of("sudo").is_some() {
        let mut v: Vec<String> = vec!["-n".into(), "--".into(), program.to_string()];
        v.extend(args.iter().cloned());
        ("sudo".to_string(), v, "sudo -n")
    } else {
        return Err(
            "administrator execution needs root or passwordless sudo; run the command visibly with pc_terminal_run(\"sudo ...\")"
                .into(),
        );
    };
    let mut cmd = tokio::process::Command::new(&bin);
    cmd.args(&full_args)
        .stdin(Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let out = tokio::time::timeout(Duration::from_millis(timeout_ms), cmd.output())
        .await
        .map_err(|_| format!("admin command timed out after {timeout_ms} ms"))?
        .map_err(|e| format!("cannot run {bin}: {e}"))?;
    let stdout = clip(String::from_utf8_lossy(&out.stdout).to_string(), 64 * 1024);
    let stderr = clip(String::from_utf8_lossy(&out.stderr).to_string(), 64 * 1024);
    if !out.status.success() && stderr.to_ascii_lowercase().contains("password is required") {
        return Err(
            "sudo requires a password and no NOPASSWD rule is configured; run it visibly with pc_terminal_run(\"sudo ...\") or add a sudoers NOPASSWD entry"
                .into(),
        );
    }
    Ok(json!({
        "ok": out.status.success(),
        "via": via,
        "command": format!("{bin} {}", full_args.join(" ")),
        "exit_code": out.status.code(),
        "stdout": stdout,
        "stderr": stderr,
    }))
}

pub async fn service_control(action: &str, name: Option<&str>) -> Result<Value, String> {
    let is_root = unsafe { libc::geteuid() } == 0;
    let mut args: Vec<String> = match action {
        "list" => vec![
            "list-units".into(),
            "--type=service".into(),
            "--all".into(),
            "--no-pager".into(),
            "--plain".into(),
            "--no-legend".into(),
        ],
        "status" => vec![
            "status".into(),
            name.ok_or("name is required for status")?.into(),
            "--no-pager".into(),
            "-n".into(),
            "20".into(),
        ],
        "start" | "stop" | "restart" | "enable" | "disable" => {
            vec![action.into(), name.ok_or("name is required")?.into()]
        }
        other => return Err(format!("unknown service action: {other}")),
    };
    let modifies = matches!(action, "start" | "stop" | "restart" | "enable" | "disable");
    let mut program = "systemctl".to_string();
    let mut via = "systemctl".to_string();
    if modifies && !is_root {
        if path_of("sudo").is_some() {
            let mut v: Vec<String> = vec!["-n".into(), "--".into(), "systemctl".into()];
            v.append(&mut args);
            args = v;
            program = "sudo".into();
            via = "sudo -n systemctl".into();
        } else {
            return Err("changing services needs administrator privileges; use pc_terminal_run(\"sudo systemctl ...\")".into());
        }
    }
    let mut cmd = tokio::process::Command::new(&program);
    cmd.args(&args).stdin(Stdio::null());
    let out = tokio::time::timeout(Duration::from_secs(60), cmd.output())
        .await
        .map_err(|_| "service command timed out".to_string())?
        .map_err(|e| format!("cannot run systemctl: {e}"))?;
    let stdout = clip(String::from_utf8_lossy(&out.stdout).to_string(), 32 * 1024);
    let stderr = clip(String::from_utf8_lossy(&out.stderr).to_string(), 32 * 1024);
    if modifies && !out.status.success() && stderr.to_ascii_lowercase().contains("password is required") {
        return Err("sudo requires a password; use pc_terminal_run(\"sudo systemctl ...\") or configure NOPASSWD".into());
    }
    Ok(json!({
        "action": action,
        "name": name,
        "via": via,
        "exit_code": out.status.code(),
        "stdout": stdout,
        "stderr": stderr,
    }))
}

pub fn run_in_terminal(command: &str, cwd: Option<&str>) -> Result<u32, String> {
    let mut script = String::new();
    if let Some(dir) = cwd {
        let dir = expand_home(dir);
        script.push_str(&format!("cd {} && ", sh_quote(&dir)));
    }
    script.push_str(command);
    script.push_str("; printf '\\n[pcrc] command finished - press Enter to close\\n'; read -r _");

    let candidates: Vec<(&str, Vec<String>)> = vec![
        ("gnome-terminal", vec!["--".into(), "bash".into(), "-c".into(), script.clone()]),
        ("konsole", vec!["-e".into(), "bash".into(), "-c".into(), script.clone()]),
        ("xfce4-terminal", vec!["-x".into(), "bash".into(), "-c".into(), script.clone()]),
        ("kitty", vec!["bash".into(), "-c".into(), script.clone()]),
        ("alacritty", vec!["-e".into(), "bash".into(), "-c".into(), script.clone()]),
        ("xterm", vec!["-e".into(), "bash".into(), "-c".into(), script.clone()]),
        ("x-terminal-emulator", vec!["-e".into(), "bash".into(), "-c".into(), script]),
    ];
    let mut errors: Vec<String> = Vec::new();
    for (term, args) in candidates {
        if path_of(term).is_none() {
            continue;
        }
        match std::process::Command::new(term)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => return Ok(child.id()),
            Err(e) => errors.push(format!("{term}: {e}")),
        }
    }
    Err(format!(
        "no terminal emulator could be started ({}); install gnome-terminal, konsole, xterm or similar",
        if errors.is_empty() { "none found".to_string() } else { errors.join("; ") }
    ))
}
