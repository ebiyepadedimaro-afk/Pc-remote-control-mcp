//! Platform abstraction. Linux has three input/screenshot paths (X11 via
//! XTEST, Wayland via the screenshot portal + uinput), macOS and Windows use
//! enigo and native shell-outs.

use serde::Serialize;
use serde_json::{json, Value};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

impl MouseButton {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "left" | "primary" => Some(Self::Left),
            "right" | "secondary" => Some(Self::Right),
            "middle" | "middle_click" => Some(Self::Middle),
            "back" => Some(Self::Back),
            "forward" => Some(Self::Forward),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Modifier {
    Ctrl,
    Shift,
    Alt,
    Meta,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Tab,
    Escape,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    Space,
    CapsLock,
    PrintScreen,
    Menu,
    F(u8),
    Other(String),
}

impl Key {
    /// Parses one key name (`"enter"`, `"f5"`, `"a"`) into a Key.
    pub fn parse(name: &str) -> Option<Self> {
        let n = name.trim();
        if n.len() == 1 {
            return Some(Key::Char(n.chars().next().unwrap()));
        }
        let upper = n.to_ascii_uppercase().replace([' ', '-'], "_");
        Some(match upper.as_str() {
            "ENTER" | "RETURN" => Key::Enter,
            "TAB" => Key::Tab,
            "ESC" | "ESCAPE" => Key::Escape,
            "BACKSPACE" | "BACK_SPACE" => Key::Backspace,
            "DEL" | "DELETE" => Key::Delete,
            "INS" | "INSERT" => Key::Insert,
            "HOME" => Key::Home,
            "END" => Key::End,
            "PAGEUP" | "PAGE_UP" | "PRIOR" => Key::PageUp,
            "PAGEDOWN" | "PAGE_DOWN" | "NEXT" => Key::PageDown,
            "UP" | "UP_ARROW" | "ARROWUP" => Key::Up,
            "DOWN" | "DOWN_ARROW" | "ARROWDOWN" => Key::Down,
            "LEFT" | "LEFT_ARROW" | "ARROWLEFT" => Key::Left,
            "RIGHT" | "RIGHT_ARROW" | "ARROWRIGHT" => Key::Right,
            "SPACE" | "SPACEBAR" => Key::Space,
            "CAPSLOCK" | "CAPS_LOCK" => Key::CapsLock,
            "PRINTSCREEN" | "PRINT_SCREEN" | "PRTSC" => Key::PrintScreen,
            "MENU" | "CONTEXT_MENU" | "APPS" => Key::Menu,
            _ => {
                if let Some(num) = upper.strip_prefix('F').and_then(|d| d.parse::<u8>().ok()) {
                    if (1..=24).contains(&num) {
                        return Some(Key::F(num));
                    }
                }
                return Some(Key::Other(n.to_string()));
            }
        })
    }
}

/// A parsed chord such as `"CTRL+SHIFT+T"`.
#[derive(Clone, Debug)]
pub struct Chord {
    pub modifiers: Vec<Modifier>,
    pub key: Key,
}

impl Chord {
    pub fn parse(spec: &str) -> Result<Self, String> {
        let spec = spec.trim();
        if spec.is_empty() {
            return Err("empty key chord".into());
        }
        if spec.len() == 1 {
            return Ok(Chord { modifiers: vec![], key: Key::Char(spec.chars().next().unwrap()) });
        }
        let parts: Vec<&str> = spec.split('+').map(str::trim).filter(|p| !p.is_empty()).collect();
        if parts.is_empty() {
            return Err(format!("invalid key chord: {spec}"));
        }
        let mut modifiers = Vec::new();
        for part in &parts[..parts.len() - 1] {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "ctl" => modifiers.push(Modifier::Ctrl),
                "shift" => modifiers.push(Modifier::Shift),
                "alt" | "option" | "opt" => modifiers.push(Modifier::Alt),
                "meta" | "win" | "super" | "cmd" | "command" => modifiers.push(Modifier::Meta),
                _ => return Err(format!("unknown modifier `{part}` in `{spec}`")),
            }
        }
        let key_part = parts[parts.len() - 1];
        let key = Key::parse(key_part).ok_or_else(|| format!("unknown key `{key_part}`"))?;
        Ok(Chord { modifiers, key })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DisplayInfo {
    pub id: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
    pub scale: f32,
}

pub struct Captured {
    pub image: image::DynamicImage,
    /// Width in the screenshot's own pixels.
    pub width: u32,
    /// Height in the screenshot's own pixels.
    pub height: u32,
    /// physical pixel / session coordinate ratio (1.0 when unknown).
    pub scale: f32,
    pub backend: &'static str,
}

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "windows")]
pub mod windows;

pub fn session_info() -> Value {
    #[cfg(target_os = "linux")]
    {
        let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
        let wayland = std::env::var("WAYLAND_DISPLAY").ok();
        let display = std::env::var("DISPLAY").ok();
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        json!({
            "session_type": if session_type.is_empty() { if wayland.is_some() { "wayland" } else if display.is_some() { "x11" } else { "headless" } } else { session_type.as_str() },
            "wayland": wayland.is_some(),
            "wayland_display": wayland,
            "display": display,
            "desktop": desktop,
        })
    }
    #[cfg(target_os = "macos")]
    {
        let display = std::env::var("DISPLAY").ok();
        json!({"session_type": "aqua", "wayland": false, "display": display, "desktop": "macOS"})
    }
    #[cfg(target_os = "windows")]
    {
        json!({"session_type": "windows", "wayland": false, "display": Value::Null, "desktop": "Windows"})
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        json!({"session_type": "unknown"})
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WindowInfo {
    pub id: String,
    pub title: String,
    pub pid: Option<u32>,
    pub app: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub focused: bool,
}

// ============================================================ top-level API

/// Preferred input backend: "auto", "x11" or "uinput" (Linux only).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Backend {
    Auto,
    X11,
    Uinput,
}

impl Backend {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "x11" => Backend::X11,
            "uinput" => Backend::Uinput,
            _ => Backend::Auto,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Backend::Auto => "auto",
            Backend::X11 => "x11",
            Backend::Uinput => "uinput",
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;

    pub async fn screenshot() -> Result<Captured, String> {
        super::linux::sys::screenshot().await
    }
    pub fn displays() -> Vec<DisplayInfo> {
        super::linux::sys::displays()
    }
    pub fn cursor_position() -> Option<(i32, i32)> {
        super::linux::sys::cursor_position()
    }
    pub fn move_abs(pref: Backend, x: i32, y: i32) -> Result<(), String> {
        super::linux::input::move_abs(pref.as_str(), x, y)
    }
    pub fn move_rel(pref: Backend, dx: i32, dy: i32) -> Result<(), String> {
        super::linux::input::move_rel(pref.as_str(), dx, dy)
    }
    pub fn button_down(pref: Backend, button: MouseButton) -> Result<(), String> {
        super::linux::input::button_down(pref.as_str(), button)
    }
    pub fn button_up(pref: Backend, button: MouseButton) -> Result<(), String> {
        super::linux::input::button_up(pref.as_str(), button)
    }
    pub fn click(pref: Backend, button: MouseButton, count: u8, interval_ms: u64) -> Result<(), String> {
        super::linux::input::click(pref.as_str(), button, count, interval_ms)
    }
    pub fn scroll(pref: Backend, dx: i32, dy: i32) -> Result<(), String> {
        super::linux::input::scroll(pref.as_str(), dx, dy)
    }
    pub fn key_chord(pref: Backend, modifiers: &[Modifier], key: &Key) -> Result<(), String> {
        super::linux::input::key_chord(pref.as_str(), modifiers, key)
    }
    pub fn type_text(pref: Backend, text: &str, interval_ms: u64) -> Result<(), String> {
        super::linux::input::type_text(pref.as_str(), text, interval_ms)
    }
    pub fn input_note(pref: Backend) -> Option<String> {
        super::linux::input::warning(pref.as_str())
    }
    pub fn list_windows() -> Result<Vec<WindowInfo>, String> {
        super::linux::x11::X11::connect()?.list_windows()
    }
    pub fn focus_window(id: &str) -> Result<(), String> {
        super::linux::x11::X11::connect()?.focus_window(id)
    }
    pub fn open_target(target: &str) -> Result<u32, String> {
        super::linux::sys::open_target(target)
    }
    pub fn launch_detached(program: &str, args: &[String], cwd: Option<&str>) -> Result<u32, String> {
        super::linux::sys::launch_detached(program, args, cwd)
    }
    pub fn notify(title: &str, body: &str) -> Result<(), String> {
        super::linux::sys::notify(title, body)
    }
    pub fn lock_screen() -> Result<(), String> {
        super::linux::sys::lock_screen()
    }
    pub async fn power_action(action: &str, delay_secs: u32) -> Result<(), String> {
        super::linux::sys::power_action(action, delay_secs).await
    }
    pub async fn admin_exec(program: &str, args: &[String], timeout_ms: u64) -> Result<Value, String> {
        super::linux::sys::admin_exec(program, args, timeout_ms).await
    }
    pub async fn service_control(action: &str, name: Option<&str>) -> Result<Value, String> {
        super::linux::sys::service_control(action, name).await
    }
    pub fn run_in_terminal(command: &str, cwd: Option<&str>) -> Result<u32, String> {
        super::linux::sys::run_in_terminal(command, cwd)
    }
    pub fn backends_report() -> Value {
        super::linux::sys::backends_report()
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    pub use super::macos::{
        admin_exec, backends_report, button_down, button_up, click, cursor_position, displays, focus_window,
        input_note, key_chord, launch_detached, list_windows, lock_screen, move_abs, move_rel, notify, open_target,
        power_action, run_in_terminal, screenshot, scroll, service_control, type_text,
    };
}

#[cfg(target_os = "windows")]
mod imp {
    use super::*;
    pub use super::windows::{
        admin_exec, backends_report, button_down, button_up, click, cursor_position, displays, focus_window,
        input_note, key_chord, launch_detached, list_windows, lock_screen, move_abs, move_rel, notify, open_target,
        power_action, run_in_terminal, screenshot, scroll, service_control, type_text,
    };
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod imp {
    use super::*;
    pub async fn screenshot() -> Result<Captured, String> {
        Err("unsupported platform".into())
    }
    pub fn displays() -> Vec<DisplayInfo> {
        vec![]
    }
    pub fn cursor_position() -> Option<(i32, i32)> {
        None
    }
    pub fn move_abs(_: Backend, _: i32, _: i32) -> Result<(), String> {
        Err("unsupported platform".into())
    }
    pub fn move_rel(_: Backend, _: i32, _: i32) -> Result<(), String> {
        Err("unsupported platform".into())
    }
    pub fn button_down(_: Backend, _: MouseButton) -> Result<(), String> {
        Err("unsupported platform".into())
    }
    pub fn button_up(_: Backend, _: MouseButton) -> Result<(), String> {
        Err("unsupported platform".into())
    }
    pub fn click(_: Backend, _: MouseButton, _: u8, _: u64) -> Result<(), String> {
        Err("unsupported platform".into())
    }
    pub fn scroll(_: Backend, _: i32, _: i32) -> Result<(), String> {
        Err("unsupported platform".into())
    }
    pub fn key_chord(_: Backend, _: &[Modifier], _: &Key) -> Result<(), String> {
        Err("unsupported platform".into())
    }
    pub fn type_text(_: Backend, _: &str, _: u64) -> Result<(), String> {
        Err("unsupported platform".into())
    }
    pub fn input_note(_: Backend) -> Option<String> {
        None
    }
    pub fn list_windows() -> Result<Vec<WindowInfo>, String> {
        Err("unsupported platform".into())
    }
    pub fn focus_window(_: &str) -> Result<(), String> {
        Err("unsupported platform".into())
    }
    pub fn open_target(_: &str) -> Result<u32, String> {
        Err("unsupported platform".into())
    }
    pub fn launch_detached(_: &str, _: &[String], _: Option<&str>) -> Result<u32, String> {
        Err("unsupported platform".into())
    }
    pub fn notify(_: &str, _: &str) -> Result<(), String> {
        Err("unsupported platform".into())
    }
    pub fn lock_screen() -> Result<(), String> {
        Err("unsupported platform".into())
    }
    pub fn backends_report() -> Value {
        json!({})
    }
    pub async fn power_action(_: &str, _: u32) -> Result<(), String> {
        Err("unsupported platform".into())
    }
    pub async fn admin_exec(_: &str, _: &[String], _: u64) -> Result<Value, String> {
        Err("unsupported platform".into())
    }
    pub async fn service_control(_: &str, _: Option<&str>) -> Result<Value, String> {
        Err("unsupported platform".into())
    }
    pub fn run_in_terminal(_: &str, _: Option<&str>) -> Result<u32, String> {
        Err("unsupported platform".into())
    }
}

pub async fn screenshot() -> Result<Captured, String> {
    imp::screenshot().await
}

pub fn displays() -> Vec<DisplayInfo> {
    imp::displays()
}

pub fn cursor_position() -> Option<(i32, i32)> {
    imp::cursor_position()
}

pub fn move_abs(pref: Backend, x: i32, y: i32) -> Result<(), String> {
    imp::move_abs(pref, x, y)
}

pub fn move_rel(pref: Backend, dx: i32, dy: i32) -> Result<(), String> {
    imp::move_rel(pref, dx, dy)
}

pub fn button_down(pref: Backend, button: MouseButton) -> Result<(), String> {
    imp::button_down(pref, button)
}

pub fn button_up(pref: Backend, button: MouseButton) -> Result<(), String> {
    imp::button_up(pref, button)
}

pub fn click(pref: Backend, button: MouseButton, count: u8, interval_ms: u64) -> Result<(), String> {
    imp::click(pref, button, count, interval_ms)
}

pub fn scroll(pref: Backend, dx: i32, dy: i32) -> Result<(), String> {
    imp::scroll(pref, dx, dy)
}

pub fn key_chord(pref: Backend, modifiers: &[Modifier], key: &Key) -> Result<(), String> {
    imp::key_chord(pref, modifiers, key)
}

pub fn type_text(pref: Backend, text: &str, interval_ms: u64) -> Result<(), String> {
    imp::type_text(pref, text, interval_ms)
}

pub fn input_note(pref: Backend) -> Option<String> {
    imp::input_note(pref)
}

pub fn list_windows() -> Result<Vec<WindowInfo>, String> {
    imp::list_windows()
}

pub fn focus_window(id: &str) -> Result<(), String> {
    imp::focus_window(id)
}

pub fn open_target(target: &str) -> Result<u32, String> {
    imp::open_target(target)
}

pub fn launch_detached(program: &str, args: &[String], cwd: Option<&str>) -> Result<u32, String> {
    imp::launch_detached(program, args, cwd)
}

pub fn notify(title: &str, body: &str) -> Result<(), String> {
    imp::notify(title, body)
}

pub fn lock_screen() -> Result<(), String> {
    imp::lock_screen()
}

pub fn backends_report() -> Value {
    imp::backends_report()
}


pub async fn power_action(action: &str, delay_secs: u32) -> Result<(), String> {
    imp::power_action(action, delay_secs).await
}

pub async fn admin_exec(program: &str, args: &[String], timeout_ms: u64) -> Result<Value, String> {
    imp::admin_exec(program, args, timeout_ms).await
}

pub async fn service_control(action: &str, name: Option<&str>) -> Result<Value, String> {
    imp::service_control(action, name).await
}

pub fn run_in_terminal(command: &str, cwd: Option<&str>) -> Result<u32, String> {
    imp::run_in_terminal(command, cwd)
}
