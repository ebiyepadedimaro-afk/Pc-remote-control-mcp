//! Windows backend: PowerShell + .NET for screenshots/windows/notifications,
//! enigo for input.

use super::{Backend, Captured, DisplayInfo, Key, Modifier, MouseButton, WindowInfo};
use enigo::{Axis, Button, Coordinate, Direction, Enigo, Key as EKey, Keyboard, Mouse, Settings};
use serde_json::{json, Value};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

const DETACHED_PROCESS: u32 = 0x0000_0008;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

fn enigo() -> Result<Enigo, String> {
    Enigo::new(&Settings::default()).map_err(|e| format!("cannot create the input controller: {e}"))
}

fn ps(script: &str) -> Result<String, String> {
    let out = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .output()
        .map_err(|e| format!("failed to run PowerShell: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "PowerShell failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn ps_quote(s: &str) -> String {
    s.replace('\'', "''")
}

fn detached(cmd: &mut Command) -> Result<u32, String> {
    use std::os::windows::process::CommandExt;
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    cmd.spawn().map(|c| c.id()).map_err(|e| format!("failed to start process: {e}"))
}

pub async fn screenshot() -> Result<Captured, String> {
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!("pcrc-shot-{stamp}.png"));
    let p = ps_quote(&path.to_string_lossy());
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms,System.Drawing; \
         $b=[System.Windows.Forms.SystemInformation]::VirtualScreen; \
         $bmp=New-Object System.Drawing.Bitmap $b.Width,$b.Height; \
         $g=[System.Drawing.Graphics]::FromImage($bmp); \
         $g.CopyFromScreen($b.Left,$b.Top,0,0,$bmp.Size); \
         $bmp.Save('{p}',[System.Drawing.Imaging.ImageFormat]::Png); \
         $g.Dispose(); $bmp.Dispose()"
    );
    ps(&script)?;
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
    Ok(Captured { image: img, width: w, height: h, scale, backend: "powershell-gdi" })
}

pub fn displays() -> Vec<DisplayInfo> {
    let script = "Add-Type -AssemblyName System.Windows.Forms; \
        [System.Windows.Forms.Screen]::AllScreens | ForEach-Object { \
        [pscustomobject]@{ name=$_.DeviceName; primary=$_.Primary; \
        x=$_.Bounds.X; y=$_.Bounds.Y; width=$_.Bounds.Width; height=$_.Bounds.Height } } \
        | ConvertTo-Json -Compress";
    let mut out = Vec::new();
    if let Ok(text) = ps(script) {
        if let Ok(v) = serde_json::from_str::<Value>(text.trim()) {
            let arr = v.as_array().cloned().unwrap_or_else(|| vec![v]);
            for (i, d) in arr.iter().enumerate() {
                out.push(DisplayInfo {
                    id: i.to_string(),
                    name: d.get("name").and_then(|n| n.as_str()).unwrap_or("display").to_string(),
                    x: d.get("x").and_then(|n| n.as_i64()).unwrap_or(0) as i32,
                    y: d.get("y").and_then(|n| n.as_i64()).unwrap_or(0) as i32,
                    width: d.get("width").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
                    height: d.get("height").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
                    primary: d.get("primary").and_then(|p| p.as_bool()).unwrap_or(i == 0),
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
        Key::Insert => EKey::Insert,
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
        Key::PrintScreen => EKey::PrintScr,
        Key::Menu => return None,
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
    let ek = enigo_key(key).ok_or_else(|| format!("key {key:?} is not supported on Windows"))?;
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
    let script = r#"Add-Type @'
using System;
using System.Runtime.InteropServices;
public struct PcrcRect { public int Left, Top, Right, Bottom; }
public class PcrcWin {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out PcrcRect r);
}
'@
Get-Process | Where-Object { $_.MainWindowTitle -ne '' } | ForEach-Object {
  $r = New-Object PcrcRect
  [PcrcWin]::GetWindowRect($_.MainWindowHandle, [ref]$r) | Out-Null
  [pscustomobject]@{ id=$_.Id.ToString(); pid=$_.Id; title=$_.MainWindowTitle;
    app=$_.ProcessName; x=$r.Left; y=$r.Top;
    width=($r.Right-$r.Left); height=($r.Bottom-$r.Top) }
} | ConvertTo-Json -Compress"#;
    let text = ps(script)?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let v: Value = serde_json::from_str(trimmed).map_err(|e| format!("cannot parse window list: {e}"))?;
    let arr = v.as_array().cloned().unwrap_or_else(|| vec![v]);
    let mut out = Vec::new();
    for d in arr {
        out.push(WindowInfo {
            id: d.get("id").and_then(|n| n.as_str()).unwrap_or("").to_string(),
            title: d.get("title").and_then(|n| n.as_str()).unwrap_or("").to_string(),
            pid: d.get("pid").and_then(|n| n.as_u64()).map(|p| p as u32),
            app: d.get("app").and_then(|n| n.as_str()).unwrap_or("").to_string(),
            x: d.get("x").and_then(|n| n.as_i64()).unwrap_or(0) as i32,
            y: d.get("y").and_then(|n| n.as_i64()).unwrap_or(0) as i32,
            width: d.get("width").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
            height: d.get("height").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
            focused: false,
        });
    }
    Ok(out)
}

pub fn focus_window(id: &str) -> Result<(), String> {
    let pid: u32 = id.parse().map_err(|_| format!("invalid Windows window id: {id}"))?;
    let script = format!(
        "$ws = New-Object -ComObject WScript.Shell; \
         [void]$ws.AppActivate({pid})"
    );
    ps(&script).map(|_| ())
}

pub fn open_target(target: &str) -> Result<u32, String> {
    let mut cmd = Command::new("cmd.exe");
    cmd.args(["/C", "start", "", target]);
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
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms,System.Drawing; \
         $n = New-Object System.Windows.Forms.NotifyIcon; \
         $n.Icon = [System.Drawing.SystemIcons]::Information; $n.Visible = $true; \
         $n.ShowBalloonTip(5000, '{}', '{}', 'Info'); Start-Sleep -Milliseconds 500; $n.Dispose()",
        ps_quote(title),
        ps_quote(body)
    );
    ps(&script).map(|_| ())
}

pub fn lock_screen() -> Result<(), String> {
    let status = Command::new("rundll32.exe")
        .args(["user32.dll,LockWorkStation"])
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("rundll32 exited with {status}"))
    }
}

pub fn backends_report() -> Value {
    json!({
        "screenshot": "powershell-gdi",
        "input": "enigo",
        "wayland": false,
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

pub async fn power_action(action: &str, delay_secs: u32) -> Result<(), String> {
    if action == "lock" {
        return lock_screen();
    }
    let t = delay_secs.to_string();
    let (program, args): (&str, Vec<&str>) = match action {
        "shutdown" => ("shutdown.exe", vec!["/s", "/f", "/t", t.as_str()]),
        "restart" => ("shutdown.exe", vec!["/r", "/f", "/t", t.as_str()]),
        "hibernate" => ("shutdown.exe", vec!["/h"]),
        "suspend" => ("rundll32.exe", vec!["powrprof.dll,SetSuspendState", "0,1,0"]),
        "logout" => ("shutdown.exe", vec!["/l"]),
        other => return Err(format!("unknown power action: {other}")),
    };
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(&args).stdin(Stdio::null());
    let status = tokio::time::timeout(Duration::from_secs(20), cmd.status())
        .await
        .map_err(|_| "power command timed out".to_string())?
        .map_err(|e| format!("cannot run {program}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} exited with {status}"))
    }
}

pub async fn admin_exec(program: &str, args: &[String], timeout_ms: u64) -> Result<Value, String> {
    let is_admin = std::process::Command::new("net")
        .arg("session")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if is_admin {
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args);
        let out = tokio::time::timeout(Duration::from_millis(timeout_ms), cmd.output())
            .await
            .map_err(|_| format!("admin command timed out after {timeout_ms} ms"))?
            .map_err(|e| format!("cannot run {program}: {e}"))?;
        return Ok(json!({
            "ok": out.status.success(),
            "via": "already elevated",
            "command": format!("{program} {}", args.join(" ")),
            "exit_code": out.status.code(),
            "stdout": clip(String::from_utf8_lossy(&out.stdout).to_string(), 64 * 1024),
            "stderr": clip(String::from_utf8_lossy(&out.stderr).to_string(), 64 * 1024),
        }));
    }
    let argline = args
        .iter()
        .map(|a| format!("\"{}\"", a.replace('"', "\\\"")))
        .collect::<Vec<_>>()
        .join(" ");
    let script = format!(
        "$p = Start-Process -FilePath '{}' -ArgumentList '{}' -Verb RunAs -Wait -PassThru; exit $p.ExitCode",
        ps_quote(program),
        ps_quote(&argline)
    );
    let out = tokio::time::timeout(
        Duration::from_millis(timeout_ms.max(120_000)),
        tokio::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script])
            .output(),
    )
    .await
    .map_err(|_| "the UAC prompt timed out or was dismissed".to_string())?
    .map_err(|e| format!("cannot start the elevated process: {e}"))?;
    Ok(json!({
        "ok": out.status.success(),
        "via": "UAC (RunAs)",
        "command": format!("{program} {}", args.join(" ")),
        "exit_code": out.status.code(),
        "stdout": "",
        "stderr": clip(String::from_utf8_lossy(&out.stderr).to_string(), 16 * 1024),
        "note": "elevated output is not captured; use terminal_run when output matters",
    }))
}

pub async fn service_control(action: &str, name: Option<&str>) -> Result<Value, String> {
    let script = match action {
        "list" => "Get-Service | Sort-Object Status,DisplayName | Select-Object Status,Name,DisplayName | Format-Table -AutoSize | Out-String -Width 200".to_string(),
        "status" => {
            let n = name.ok_or("name is required for status")?;
            format!("Get-Service -Name '{}' | Format-List | Out-String -Width 200", ps_quote(n))
        }
        "start" => {
            let n = name.ok_or("name is required")?;
            format!("Start-Service -Name '{}'; Get-Service -Name '{}' | Format-List | Out-String", ps_quote(n), ps_quote(n))
        }
        "stop" => {
            let n = name.ok_or("name is required")?;
            format!("Stop-Service -Name '{}' -Force; Get-Service -Name '{}' | Format-List | Out-String", ps_quote(n), ps_quote(n))
        }
        "restart" => {
            let n = name.ok_or("name is required")?;
            format!("Restart-Service -Name '{}' -Force; Get-Service -Name '{}' | Format-List | Out-String", ps_quote(n), ps_quote(n))
        }
        "enable" => {
            let n = name.ok_or("name is required")?;
            format!("Set-Service -Name '{}' -StartupType Automatic; Get-Service -Name '{}' | Format-List | Out-String", ps_quote(n), ps_quote(n))
        }
        "disable" => {
            let n = name.ok_or("name is required")?;
            format!("Set-Service -Name '{}' -StartupType Disabled; Get-Service -Name '{}' | Format-List | Out-String", ps_quote(n), ps_quote(n))
        }
        other => return Err(format!("unknown service action: {other}")),
    };
    let out = tokio::time::timeout(
        Duration::from_secs(60),
        tokio::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script])
            .output(),
    )
    .await
    .map_err(|_| "service command timed out".to_string())?
    .map_err(|e| format!("PowerShell failed: {e}"))?;
    Ok(json!({
        "action": action,
        "name": name,
        "via": "powershell",
        "exit_code": out.status.code(),
        "stdout": clip(String::from_utf8_lossy(&out.stdout).to_string(), 32 * 1024),
        "stderr": clip(String::from_utf8_lossy(&out.stderr).to_string(), 32 * 1024),
    }))
}

pub fn run_in_terminal(command: &str, cwd: Option<&str>) -> Result<u32, String> {
    let line = if let Some(dir) = cwd {
        format!("cd /d \"{}\" && {}", dir, command)
    } else {
        command.to_string()
    };
    let mut cmd = Command::new("cmd.exe");
    cmd.args(["/C", "start", "pcrc", "cmd.exe", "/K", &line]);
    detached(&mut cmd)
}
