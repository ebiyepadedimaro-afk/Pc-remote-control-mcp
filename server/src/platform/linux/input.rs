//! Linux input dispatch: XTEST on X11, uinput on Wayland (or by config).

use super::uinput;
use super::x11::X11;
use crate::platform::{Key, Modifier, MouseButton};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Backend {
    X11,
    Uinput,
}

pub fn select(pref: &str) -> Result<Backend, String> {
    let x11_ok = X11::connect().is_ok();
    let wayland = super::sys::session_is_wayland();
    match pref {
        "x11" => Ok(Backend::X11),
        "uinput" => Ok(Backend::Uinput),
        _ => {
            if wayland {
                if uinput::available() {
                    Ok(Backend::Uinput)
                } else if x11_ok {
                    Ok(Backend::X11)
                } else {
                    Err(format!(
                        "no usable input backend: /dev/uinput is not writable and no X11 display is reachable. {}",
                        uinput::setup_instructions()
                    ))
                }
            } else if x11_ok {
                Ok(Backend::X11)
            } else if uinput::available() {
                Ok(Backend::Uinput)
            } else {
                Err(format!(
                    "no usable input backend: no X11 display and /dev/uinput is not writable. {}",
                    uinput::setup_instructions()
                ))
            }
        }
    }
}

fn x11() -> Result<X11, String> {
    X11::connect()
}

pub fn backend_name(pref: &str) -> &'static str {
    match select(pref) {
        Ok(Backend::X11) => "x11",
        Ok(Backend::Uinput) => "uinput",
        Err(_) => "none",
    }
}

pub fn warning(pref: &str) -> Option<String> {
    if super::sys::session_is_wayland() && backend_name(pref) == "x11" {
        Some(
            "This is a Wayland session using the X11 input backend: input only reaches XWayland windows. \
             Enable /dev/uinput access (see the desktop app) for full control."
                .to_string(),
        )
    } else {
        None
    }
}

pub fn move_abs(pref: &str, x: i32, y: i32) -> Result<(), String> {
    match select(pref)? {
        Backend::X11 => x11()?.move_abs(x, y),
        Backend::Uinput => uinput::move_abs(x, y),
    }
}

pub fn move_rel(pref: &str, dx: i32, dy: i32) -> Result<(), String> {
    match select(pref)? {
        Backend::X11 => x11()?.move_rel(dx, dy),
        Backend::Uinput => uinput::move_rel(dx, dy),
    }
}

pub fn button_down(pref: &str, button: MouseButton) -> Result<(), String> {
    match select(pref)? {
        Backend::X11 => x11()?.button_down(button),
        Backend::Uinput => uinput::button_down(button),
    }
}

pub fn button_up(pref: &str, button: MouseButton) -> Result<(), String> {
    match select(pref)? {
        Backend::X11 => x11()?.button_up(button),
        Backend::Uinput => uinput::button_up(button),
    }
}

pub fn click(pref: &str, button: MouseButton, count: u8, interval_ms: u64) -> Result<(), String> {
    match select(pref)? {
        Backend::X11 => x11()?.click(button, count, interval_ms),
        Backend::Uinput => uinput::click(button, count, interval_ms),
    }
}

pub fn scroll(pref: &str, dx: i32, dy: i32) -> Result<(), String> {
    match select(pref)? {
        Backend::X11 => x11()?.scroll(dx, dy),
        Backend::Uinput => uinput::scroll(dx, dy),
    }
}

pub fn key_chord(pref: &str, modifiers: &[Modifier], key: &Key) -> Result<(), String> {
    match select(pref)? {
        Backend::X11 => x11()?.key_chord(modifiers, key),
        Backend::Uinput => uinput::key_chord(modifiers, key),
    }
}

pub fn type_text(pref: &str, text: &str, interval_ms: u64) -> Result<(), String> {
    match select(pref)? {
        Backend::X11 => x11()?.type_text(text, interval_ms),
        Backend::Uinput => uinput::type_text(text, interval_ms),
    }
}
