//! Raw uinput backend for Wayland sessions, where XTEST cannot reach native
//! Wayland windows. Creates virtual devices through /dev/uinput:
//! - a keyboard (US layout + modifier chords),
//! - a relative mouse (buttons + wheel),
//! - an absolute "tablet" pointer so clicks land on exact coordinates without
//!   pointer acceleration.
//!
//! Requires write access to /dev/uinput; see `setup_instructions()`.

use crate::platform::{Key, Modifier, MouseButton};
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::sync::Mutex;
use std::time::Duration;

const UI_DEV_CREATE: libc::c_ulong = 0x5501;
const UI_DEV_SETUP: libc::c_ulong = 0x405c_5503;
const UI_SET_EVBIT: libc::c_ulong = 0x4004_5564;
const UI_SET_KEYBIT: libc::c_ulong = 0x4004_5565;
const UI_SET_RELBIT: libc::c_ulong = 0x4004_5566;
const UI_SET_ABSBIT: libc::c_ulong = 0x4004_5567;
const UI_SET_PROPBIT: libc::c_ulong = 0x4004_556e;

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_REL: u16 = 0x02;
const EV_ABS: u16 = 0x03;

const SYN_REPORT: u16 = 0;

const REL_X: u16 = 0x00;
const REL_Y: u16 = 0x01;
const REL_HWHEEL: u16 = 0x06;
const REL_WHEEL: u16 = 0x08;

const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const ABS_PRESSURE: u16 = 0x18;
const ABS_DISTANCE: u16 = 0x19;

const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;
const BTN_TOOL_PEN: u16 = 0x140;
const BTN_TOUCH: u16 = 0x14a;
const BTN_STYLUS: u16 = 0x14b;
const BTN_STYLUS2: u16 = 0x14c;

const KEY_ESC: u16 = 1;
const KEY_1: u16 = 2;
const KEY_0: u16 = 11;
const KEY_MINUS: u16 = 12;
const KEY_EQUAL: u16 = 13;
const KEY_BACKSPACE: u16 = 14;
const KEY_TAB: u16 = 15;
const KEY_ENTER: u16 = 28;
const KEY_LEFTCTRL: u16 = 29;
const KEY_LEFTSHIFT: u16 = 42;
const KEY_LEFTALT: u16 = 56;
const KEY_SPACE: u16 = 57;
const KEY_CAPSLOCK: u16 = 58;
const KEY_F1: u16 = 59;
const KEY_LEFTMETA: u16 = 125;
const KEY_RIGHTMETA: u16 = 126;
const KEY_HOME: u16 = 102;
const KEY_UP: u16 = 103;
const KEY_PAGEUP: u16 = 104;
const KEY_LEFT: u16 = 105;
const KEY_RIGHT: u16 = 106;
const KEY_END: u16 = 107;
const KEY_DOWN: u16 = 108;
const KEY_PAGEDOWN: u16 = 109;
const KEY_INSERT: u16 = 110;
const KEY_DELETE: u16 = 111;
const KEY_PAUSE: u16 = 119;
const KEY_MENU: u16 = 139;
const KEY_PRINT: u16 = 210;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct InputId {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}

#[repr(C)]
struct UinputSetup {
    id: InputId,
    name: [u8; 80],
    ff_effects_max: u32,
}

#[repr(C)]
struct TimeVal {
    tv_sec: libc::time_t,
    tv_usec: libc::suseconds_t,
}

#[repr(C)]
struct InputEvent {
    time: TimeVal,
    type_: u16,
    code: u16,
    value: i32,
}

fn ioctl_int(fd: i32, req: libc::c_ulong, val: i32) -> std::io::Result<()> {
    let r = unsafe { libc::ioctl(fd, req, val) };
    if r < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn setup_instructions() -> String {
    "Input on Wayland needs write access to /dev/uinput. Run:\n  \
     sudo tee /etc/udev/rules.d/60-pcrc-uinput.rules >/dev/null <<'EOF'\n  \
     KERNEL==\"uinput\", GROUP=\"input\", MODE=\"0660\"\n  \
     EOF\n  \
     sudo modprobe uinput\n  \
     echo uinput | sudo tee /etc/modules-load.d/uinput.conf\n  \
     sudo usermod -aG input $USER\n\
     Then log out and back in (or run `newgrp input`)."
        .to_string()
}

pub fn available() -> bool {
    let Ok(c) = CString::new("/dev/uinput") else { return false };
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

struct Devices {
    keyboard: File,
    mouse: File,
    tablet: Option<File>,
}

static DEVICES: Mutex<Option<Devices>> = Mutex::new(None);

fn open_uinput() -> Result<File, String> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/uinput")
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                format!("/dev/uinput is not writable. {}", setup_instructions())
            } else {
                format!("cannot open /dev/uinput: {e}")
            }
        })
}

fn setup_keyboard() -> Result<File, String> {
    let f = open_uinput()?;
    let fd = f.as_raw_fd();
    let _ = ioctl_int(fd, UI_SET_EVBIT, EV_KEY as i32);
    let _ = ioctl_int(fd, UI_SET_EVBIT, EV_SYN as i32);
    for code in 1..=248 {
        let _ = ioctl_int(fd, UI_SET_KEYBIT, code);
    }
    let _ = ioctl_int(fd, UI_SET_KEYBIT, KEY_PRINT as i32);
    let _ = ioctl_int(fd, UI_SET_KEYBIT, KEY_MENU as i32);
    let _ = ioctl_int(fd, UI_SET_KEYBIT, KEY_PAUSE as i32);
    let _ = ioctl_int(fd, UI_SET_KEYBIT, KEY_LEFTMETA as i32);
    let _ = ioctl_int(fd, UI_SET_KEYBIT, KEY_RIGHTMETA as i32);
    let mut setup = UinputSetup {
        id: InputId { bustype: 0x0003, vendor: 0x1234, product: 0x5678, version: 1 },
        name: [0; 80],
        ff_effects_max: 0,
    };
    let name = b"pcrc-virtual-keyboard";
    setup.name[..name.len()].copy_from_slice(name);
    let r = unsafe { libc::ioctl(fd, UI_DEV_SETUP, &setup as *const UinputSetup) };
    if r < 0 {
        return Err(format!("UI_DEV_SETUP failed: {}", std::io::Error::last_os_error()));
    }
    let r = unsafe { libc::ioctl(fd, UI_DEV_CREATE) };
    if r < 0 {
        return Err(format!("UI_DEV_CREATE failed: {}", std::io::Error::last_os_error()));
    }
    Ok(f)
}

fn setup_mouse() -> Result<File, String> {
    let f = open_uinput()?;
    let fd = f.as_raw_fd();
    let _ = ioctl_int(fd, UI_SET_EVBIT, EV_KEY as i32);
    let _ = ioctl_int(fd, UI_SET_EVBIT, EV_REL as i32);
    for b in [BTN_LEFT, BTN_RIGHT, BTN_MIDDLE] {
        let _ = ioctl_int(fd, UI_SET_KEYBIT, b as i32);
    }
    for r in [REL_X, REL_Y, REL_WHEEL, REL_HWHEEL] {
        let _ = ioctl_int(fd, UI_SET_RELBIT, r as i32);
    }
    let mut setup = UinputSetup {
        id: InputId { bustype: 0x0003, vendor: 0x1234, product: 0x5679, version: 1 },
        name: [0; 80],
        ff_effects_max: 0,
    };
    let name = b"pcrc-virtual-mouse";
    setup.name[..name.len()].copy_from_slice(name);
    let r = unsafe { libc::ioctl(fd, UI_DEV_SETUP, &setup as *const UinputSetup) };
    if r < 0 {
        return Err(format!("UI_DEV_SETUP failed: {}", std::io::Error::last_os_error()));
    }
    let r = unsafe { libc::ioctl(fd, UI_DEV_CREATE) };
    if r < 0 {
        return Err(format!("UI_DEV_CREATE failed: {}", std::io::Error::last_os_error()));
    }
    Ok(f)
}

/// Absolute pointer as a pen tablet: the compositor maps ABS_X/ABS_Y across
/// the desktop with no pointer acceleration. Experimental on some compositors.
fn setup_tablet() -> Result<File, String> {
    let f = open_uinput()?;
    let fd = f.as_raw_fd();
    let _ = ioctl_int(fd, UI_SET_EVBIT, EV_KEY as i32);
    let _ = ioctl_int(fd, UI_SET_EVBIT, EV_ABS as i32);
    let _ = ioctl_int(fd, UI_SET_EVBIT, EV_SYN as i32);
    for b in [BTN_LEFT, BTN_RIGHT, BTN_MIDDLE, BTN_TOOL_PEN, BTN_TOUCH, BTN_STYLUS, BTN_STYLUS2] {
        let _ = ioctl_int(fd, UI_SET_KEYBIT, b as i32);
    }
    for a in [ABS_X, ABS_Y, ABS_PRESSURE, ABS_DISTANCE] {
        let _ = ioctl_int(fd, UI_SET_ABSBIT, a as i32);
    }
    let _ = ioctl_int(fd, UI_SET_PROPBIT, 0x00); // INPUT_PROP_POINTER

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct AbsInfo {
        value: i32,
        minimum: i32,
        maximum: i32,
        fuzz: i32,
        flat: i32,
        resolution: i32,
    }
    #[repr(C)]
    struct UinputAbsSetup {
        absinfo: AbsInfo,
        code: u16,
        _pad: [u8; 2],
    }

    const UI_ABS_SETUP: libc::c_ulong = 0x401c_5504; // _IOW('U', 4, struct uinput_abs_setup)
    unsafe {
        for (code, info) in [
            (ABS_X, AbsInfo { value: 0, minimum: 0, maximum: 32767, fuzz: 0, flat: 0, resolution: 0 }),
            (ABS_Y, AbsInfo { value: 0, minimum: 0, maximum: 32767, fuzz: 0, flat: 0, resolution: 0 }),
            (ABS_PRESSURE, AbsInfo { value: 0, minimum: 0, maximum: 1023, fuzz: 0, flat: 0, resolution: 0 }),
            (ABS_DISTANCE, AbsInfo { value: 0, minimum: 0, maximum: 63, fuzz: 0, flat: 0, resolution: 0 }),
        ] {
            let setup_abs = UinputAbsSetup { absinfo: info, code, _pad: [0; 2] };
            libc::ioctl(fd, UI_ABS_SETUP, &setup_abs as *const UinputAbsSetup);
        }
    }

    let mut setup = UinputSetup {
        id: InputId { bustype: 0x0003, vendor: 0x1234, product: 0x567a, version: 1 },
        name: [0; 80],
        ff_effects_max: 0,
    };
    let name = b"pcrc-virtual-tablet";
    setup.name[..name.len()].copy_from_slice(name);
    let r = unsafe { libc::ioctl(fd, UI_DEV_SETUP, &setup as *const UinputSetup) };
    if r < 0 {
        return Err(format!("UI_DEV_SETUP failed: {}", std::io::Error::last_os_error()));
    }
    let r = unsafe { libc::ioctl(fd, UI_DEV_CREATE) };
    if r < 0 {
        return Err(format!("UI_DEV_CREATE failed: {}", std::io::Error::last_os_error()));
    }
    Ok(f)
}

fn with_devices<T>(f: impl FnOnce(&Devices) -> Result<T, String>) -> Result<T, String> {
    let mut guard = DEVICES.lock().unwrap();
    if guard.is_none() {
        let keyboard = setup_keyboard()?;
        let mouse = setup_mouse()?;
        let tablet = setup_tablet().ok();
        // Give udev/libinput a moment to pick the devices up.
        std::thread::sleep(Duration::from_millis(150));
        *guard = Some(Devices { keyboard, mouse, tablet });
    }
    f(guard.as_ref().unwrap())
}

fn emit(f: &File, type_: u16, code: u16, value: i32) -> Result<(), String> {
    let ev = InputEvent {
        time: TimeVal { tv_sec: 0, tv_usec: 0 },
        type_,
        code,
        value,
    };
    let buf = unsafe {
        std::slice::from_raw_parts(&ev as *const InputEvent as *const u8, std::mem::size_of::<InputEvent>())
    };
    let mut file = f;
    file.write_all(buf).map_err(|e| format!("uinput write failed: {e}"))
}

fn syn(f: &File) -> Result<(), String> {
    emit(f, EV_SYN, SYN_REPORT, 0)
}

fn key_down(f: &File, code: u16) -> Result<(), String> {
    emit(f, EV_KEY, code, 1)?;
    syn(f)
}

fn key_up(f: &File, code: u16) -> Result<(), String> {
    emit(f, EV_KEY, code, 0)?;
    syn(f)
}

fn press_key(f: &File, code: u16, shift: bool) -> Result<(), String> {
    if shift {
        key_down(f, KEY_LEFTSHIFT)?;
    }
    key_down(f, code)?;
    key_up(f, code)?;
    if shift {
        key_up(f, KEY_LEFTSHIFT)?;
    }
    Ok(())
}

/// (evdev keycode, shift needed) for a US layout character.
fn char_key(c: char) -> Option<(u16, bool)> {
    let (code, shift) = match c {
        'a' => (30, false),
        'b' => (48, false),
        'c' => (46, false),
        'd' => (32, false),
        'e' => (18, false),
        'f' => (33, false),
        'g' => (34, false),
        'h' => (35, false),
        'i' => (23, false),
        'j' => (36, false),
        'k' => (37, false),
        'l' => (38, false),
        'm' => (50, false),
        'n' => (49, false),
        'o' => (24, false),
        'p' => (25, false),
        'q' => (16, false),
        'r' => (19, false),
        's' => (31, false),
        't' => (20, false),
        'u' => (22, false),
        'v' => (47, false),
        'w' => (17, false),
        'x' => (45, false),
        'y' => (21, false),
        'z' => (44, false),
        'A'..='Z' => return char_key(c.to_ascii_lowercase()).map(|(k, _)| (k, true)),
        '1' => (KEY_1, false),
        '2' => (KEY_1 + 1, false),
        '3' => (KEY_1 + 2, false),
        '4' => (KEY_1 + 3, false),
        '5' => (KEY_1 + 4, false),
        '6' => (KEY_1 + 5, false),
        '7' => (KEY_1 + 6, false),
        '8' => (KEY_1 + 7, false),
        '9' => (KEY_1 + 8, false),
        '0' => (KEY_0, false),
        '!' => (KEY_1, true),
        '@' => (KEY_1 + 1, true),
        '#' => (KEY_1 + 2, true),
        '$' => (KEY_1 + 3, true),
        '%' => (KEY_1 + 4, true),
        '^' => (KEY_1 + 5, true),
        '&' => (KEY_1 + 6, true),
        '*' => (KEY_1 + 7, true),
        '(' => (KEY_1 + 8, true),
        ')' => (KEY_0, true),
        '-' => (KEY_MINUS, false),
        '_' => (KEY_MINUS, true),
        '=' => (KEY_EQUAL, false),
        '+' => (KEY_EQUAL, true),
        '[' => (26, false),
        '{' => (26, true),
        ']' => (27, false),
        '}' => (27, true),
        ';' => (39, false),
        ':' => (39, true),
        '\'' => (40, false),
        '"' => (40, true),
        '`' => (41, false),
        '~' => (41, true),
        '\\' => (43, false),
        '|' => (43, true),
        ',' => (51, false),
        '<' => (51, true),
        '.' => (52, false),
        '>' => (52, true),
        '/' => (53, false),
        '?' => (53, true),
        ' ' => (KEY_SPACE, false),
        '\n' | '\r' => (KEY_ENTER, false),
        '\t' => (KEY_TAB, false),
        _ => return None,
    };
    Some((code, shift))
}

fn named_key(key: &Key) -> Option<(u16, bool)> {
    Some(match key {
        Key::Enter => (KEY_ENTER, false),
        Key::Tab => (KEY_TAB, false),
        Key::Escape => (KEY_ESC, false),
        Key::Backspace => (KEY_BACKSPACE, false),
        Key::Delete => (KEY_DELETE, false),
        Key::Insert => (KEY_INSERT, false),
        Key::Home => (KEY_HOME, false),
        Key::End => (KEY_END, false),
        Key::PageUp => (KEY_PAGEUP, false),
        Key::PageDown => (KEY_PAGEDOWN, false),
        Key::Up => (KEY_UP, false),
        Key::Down => (KEY_DOWN, false),
        Key::Left => (KEY_LEFT, false),
        Key::Right => (KEY_RIGHT, false),
        Key::Space => (KEY_SPACE, false),
        Key::CapsLock => (KEY_CAPSLOCK, false),
        Key::PrintScreen => (KEY_PRINT, false),
        Key::Menu => (KEY_MENU, false),
        Key::F(n) if (1..=12).contains(n) => (KEY_F1 + (*n as u16 - 1), false),
        Key::Char(c) => char_key(*c)?,
        _ => return None,
    })
}

fn modifier_code(m: Modifier) -> u16 {
    match m {
        Modifier::Ctrl => KEY_LEFTCTRL,
        Modifier::Shift => KEY_LEFTSHIFT,
        Modifier::Alt => KEY_LEFTALT,
        Modifier::Meta => KEY_LEFTMETA,
    }
}

pub fn key_chord(modifiers: &[Modifier], key: &Key) -> Result<(), String> {
    let (code, shift) = named_key(key).ok_or_else(|| {
        format!("key {key:?} is not supported by the uinput backend (US layout); try the x11 backend")
    })?;
    with_devices(|d| {
        let mut held = Vec::new();
        for m in modifiers {
            let c = modifier_code(*m);
            key_down(&d.keyboard, c)?;
            held.push(c);
        }
        if shift && !held.contains(&KEY_LEFTSHIFT) {
            key_down(&d.keyboard, KEY_LEFTSHIFT)?;
            held.push(KEY_LEFTSHIFT);
        }
        key_down(&d.keyboard, code)?;
        key_up(&d.keyboard, code)?;
        for c in held.iter().rev() {
            key_up(&d.keyboard, *c)?;
        }
        Ok(())
    })
}

pub fn type_text(text: &str, interval_ms: u64) -> Result<(), String> {
    with_devices(|d| {
        for ch in text.chars() {
            let (code, shift) = char_key(ch).ok_or_else(|| {
                format!("character {ch:?} is not typable through uinput (US layout); use pc_set_clipboard + paste")
            })?;
            press_key(&d.keyboard, code, shift)?;
            if interval_ms > 0 {
                std::thread::sleep(Duration::from_millis(interval_ms));
            }
        }
        Ok(())
    })
}

pub fn move_abs(x: i32, y: i32) -> Result<(), String> {
    let (sw, sh) = crate::platform::linux::sys::screen_size().unwrap_or((1920, 1080));
    let ax = (x.clamp(0, sw.saturating_sub(1) as i32) as i64 * 32767 / sw.saturating_sub(1).max(1) as i64) as i32;
    let ay = (y.clamp(0, sh.saturating_sub(1) as i32) as i64 * 32767 / sh.saturating_sub(1).max(1) as i64) as i32;
    with_devices(|d| {
        let tablet = d.tablet.as_ref().ok_or_else(|| {
            "absolute pointing is unavailable: the uinput tablet device could not be created; use dx/dy or an X11 session"
                .to_string()
        })?;
        // Pen in proximity, hovering.
        emit(tablet, EV_KEY, BTN_TOOL_PEN, 1)?;
        emit(tablet, EV_ABS, ABS_X, ax)?;
        emit(tablet, EV_ABS, ABS_Y, ay)?;
        emit(tablet, EV_ABS, ABS_PRESSURE, 0)?;
        syn(tablet)
    })
}

pub fn move_rel(dx: i32, dy: i32) -> Result<(), String> {
    with_devices(|d| {
        emit(&d.mouse, EV_REL, REL_X, dx)?;
        emit(&d.mouse, EV_REL, REL_Y, dy)?;
        syn(&d.mouse)
    })
}

pub fn click(button: MouseButton, count: u8, interval_ms: u64) -> Result<(), String> {
    let (code, tablet_code) = match button {
        MouseButton::Left => (BTN_LEFT, Some(BTN_TOUCH)),
        MouseButton::Right => (BTN_RIGHT, Some(BTN_STYLUS)),
        MouseButton::Middle => (BTN_MIDDLE, Some(BTN_STYLUS2)),
        MouseButton::Back | MouseButton::Forward => (BTN_LEFT, None),
    };
    with_devices(|d| {
        for i in 0..count.max(1) {
            if let (Some(tablet), Some(tcode)) = (d.tablet.as_ref(), tablet_code) {
                emit(tablet, EV_KEY, BTN_TOOL_PEN, 1)?;
                emit(tablet, EV_ABS, ABS_PRESSURE, 600)?;
                emit(tablet, EV_KEY, tcode, 1)?;
                syn(tablet)?;
                std::thread::sleep(Duration::from_millis(20));
                emit(tablet, EV_KEY, tcode, 0)?;
                emit(tablet, EV_ABS, ABS_PRESSURE, 0)?;
                syn(tablet)?;
            } else {
                key_down(&d.mouse, code)?;
                std::thread::sleep(Duration::from_millis(20));
                key_up(&d.mouse, code)?;
            }
            if i + 1 < count.max(1) && interval_ms > 0 {
                std::thread::sleep(Duration::from_millis(interval_ms));
            }
        }
        Ok(())
    })
}

pub fn button_down(button: MouseButton) -> Result<(), String> {
    let code = match button {
        MouseButton::Left | MouseButton::Back | MouseButton::Forward => BTN_LEFT,
        MouseButton::Right => BTN_RIGHT,
        MouseButton::Middle => BTN_MIDDLE,
    };
    with_devices(|d| key_down(&d.mouse, code))
}

pub fn button_up(button: MouseButton) -> Result<(), String> {
    let code = match button {
        MouseButton::Left | MouseButton::Back | MouseButton::Forward => BTN_LEFT,
        MouseButton::Right => BTN_RIGHT,
        MouseButton::Middle => BTN_MIDDLE,
    };
    with_devices(|d| key_up(&d.mouse, code))
}

pub fn scroll(dx: i32, dy: i32) -> Result<(), String> {
    with_devices(|d| {
        if dy != 0 {
            emit(&d.mouse, EV_REL, REL_WHEEL, -dy.signum() * 120)?;
        }
        if dx != 0 {
            emit(&d.mouse, EV_REL, REL_HWHEEL, dx.signum() * 120)?;
        }
        syn(&d.mouse)
    })
}
