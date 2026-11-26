//! X11 backend: screenshots via GetImage, input via XTEST, windows via EWMH.
//! Works on an X11 session and reaches XWayland windows otherwise.

use crate::platform::{Captured, DisplayInfo, Key, Modifier, MouseButton, WindowInfo};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as RandrExt;
use x11rb::protocol::xproto::*;
use x11rb::protocol::xtest::ConnectionExt as XtestExt;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as WrapperConnectionExt;

const XK_BACKSPACE: u32 = 0xff08;
const XK_TAB: u32 = 0xff09;
const XK_RETURN: u32 = 0xff0d;
const XK_ESCAPE: u32 = 0xff1b;
const XK_HOME: u32 = 0xff50;
const XK_LEFT: u32 = 0xff51;
const XK_UP: u32 = 0xff52;
const XK_RIGHT: u32 = 0xff53;
const XK_DOWN: u32 = 0xff54;
const XK_PAGE_UP: u32 = 0xff55;
const XK_PAGE_DOWN: u32 = 0xff56;
const XK_END: u32 = 0xff57;
const XK_PRINT: u32 = 0xff61;
const XK_INSERT: u32 = 0xff63;
const XK_MENU: u32 = 0xff67;
const XK_DELETE: u32 = 0xffff;
const XK_F1: u32 = 0xffbe;
const XK_SHIFT_L: u32 = 0xffe1;
const XK_CONTROL_L: u32 = 0xffe3;
const XK_CAPS_LOCK: u32 = 0xffe5;
const XK_ALT_L: u32 = 0xffe9;
const XK_SUPER_L: u32 = 0xffeb;

pub struct X11 {
    pub conn: RustConnection,
    pub root: Window,
}

struct Keymap {
    map: HashMap<u32, (u8, bool)>,
    per: u8,
    min: u8,
    max: u8,
    used: HashSet<u8>,
}

impl Keymap {
    fn load(conn: &RustConnection) -> Result<Self, String> {
        let setup = conn.setup();
        let min = setup.min_keycode;
        let max = setup.max_keycode;
        let count = max.saturating_sub(min).saturating_add(1);
        let reply = conn
            .get_keyboard_mapping(min, count)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        let per = reply.keysyms_per_keycode;
        let mut map = HashMap::new();
        let mut used = HashSet::new();
        for kc in min..=max {
            let base = (kc - min) as usize * per as usize;
            let mut any = false;
            for level in 0..per.min(2) as usize {
                if let Some(&ks) = reply.keysyms.get(base + level) {
                    if ks != 0 {
                        any = true;
                        map.entry(ks).or_insert((kc, level == 1));
                    }
                }
            }
            if any {
                used.insert(kc);
            }
        }
        Ok(Self { map, per, min, max, used })
    }

    fn lookup(&self, keysym: u32) -> Option<(u8, bool)> {
        self.map.get(&keysym).copied()
    }

    fn lookup_char(&self, c: char) -> Option<(u8, bool)> {
        let cp = c as u32;
        if let Some(hit) = self.lookup(cp) {
            return Some(hit);
        }
        // Uppercase letters usually live on the shifted level: try the
        // lowercase keysym and ask for Shift.
        if c.is_ascii_uppercase() {
            if let Some((kc, _)) = self.lookup(c.to_ascii_lowercase() as u32) {
                return Some((kc, true));
            }
        }
        None
    }

    fn spare(&self) -> Option<u8> {
        (self.min..=self.max).rev().find(|kc| !self.used.contains(kc))
    }
}

fn named_keysym(key: &Key) -> Option<u32> {
    Some(match key {
        Key::Enter => XK_RETURN,
        Key::Tab => XK_TAB,
        Key::Escape => XK_ESCAPE,
        Key::Backspace => XK_BACKSPACE,
        Key::Delete => XK_DELETE,
        Key::Insert => XK_INSERT,
        Key::Home => XK_HOME,
        Key::End => XK_END,
        Key::PageUp => XK_PAGE_UP,
        Key::PageDown => XK_PAGE_DOWN,
        Key::Up => XK_UP,
        Key::Down => XK_DOWN,
        Key::Left => XK_LEFT,
        Key::Right => XK_RIGHT,
        Key::Space => 0x20,
        Key::CapsLock => XK_CAPS_LOCK,
        Key::PrintScreen => XK_PRINT,
        Key::Menu => XK_MENU,
        Key::F(n) => XK_F1 + (*n as u32).saturating_sub(1).min(23),
        Key::Char(c) => return char_keysym(*c),
        Key::Other(_) => return None,
    })
}

fn char_keysym(c: char) -> Option<u32> {
    let cp = c as u32;
    if cp >= 0x20 && cp <= 0x7e {
        Some(cp)
    } else if cp > 0x7e {
        Some(0x0100_0000 | cp)
    } else {
        None
    }
}

impl X11 {
    pub fn connect() -> Result<Self, String> {
        let (conn, screen_num) = x11rb::connect(None).map_err(|e| {
            format!(
                "cannot connect to X11 (DISPLAY={:?}): {e}. On a Wayland session X11 tools only reach XWayland windows.",
                std::env::var("DISPLAY").ok()
            )
        })?;
        let root = conn.setup().roots[screen_num].root;
        Ok(Self { conn, root })
    }

    // ------------------------------------------------------------ screenshot

    pub fn screenshot(&self) -> Result<Captured, String> {
        let geom = self
            .conn
            .get_geometry(self.root)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        let (w, h) = (geom.width as u16, geom.height as u16);
        let reply = self
            .conn
            .get_image(ImageFormat::Z_PIXMAP, self.root, 0, 0, w, h, !0)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;

        let setup = self.conn.setup();
        let fmt = setup
            .pixmap_formats
            .iter()
            .find(|f| f.depth == reply.depth)
            .ok_or_else(|| format!("unsupported X11 pixmap depth {}", reply.depth))?;
        let bpp = fmt.bits_per_pixel as usize;
        let pad = fmt.scanline_pad as usize;
        let row_bits = w as usize * bpp;
        let stride = (row_bits + pad - 1) / pad * pad / 8;
        let lsb = setup.image_byte_order == ImageOrder::LSB_FIRST;
        let needed = stride * h as usize;
        if reply.data.len() < needed {
            return Err(format!(
                "short X11 image: {} bytes, expected {needed}",
                reply.data.len()
            ));
        }
        let mut img = image::RgbaImage::new(w as u32, h as u32);
        for y in 0..h as usize {
            let row = &reply.data[y * stride..y * stride + stride];
            for x in 0..w as usize {
                let px = match bpp {
                    32 => {
                        let b = &row[x * 4..x * 4 + 4];
                        if lsb {
                            [b[2], b[1], b[0], 255]
                        } else {
                            [b[1], b[2], b[3], 255]
                        }
                    }
                    24 => {
                        let b = &row[x * 3..x * 3 + 3];
                        if lsb {
                            [b[2], b[1], b[0], 255]
                        } else {
                            [b[0], b[1], b[2], 255]
                        }
                    }
                    16 => {
                        let v = u16::from_le_bytes([row[x * 2], row[x * 2 + 1]]);
                        let r = ((v >> 11) & 0x1f) as u8 * 255 / 31;
                        let g = ((v >> 5) & 0x3f) as u8 * 255 / 63;
                        let b = (v & 0x1f) as u8 * 255 / 31;
                        [r, g, b, 255]
                    }
                    _ => return Err(format!("unsupported bits-per-pixel {bpp}")),
                };
                img.put_pixel(x as u32, y as u32, image::Rgba(px));
            }
        }
        Ok(Captured {
            image: image::DynamicImage::ImageRgba8(img),
            width: w as u32,
            height: h as u32,
            scale: 1.0,
            backend: "x11",
        })
    }

    pub fn root_size(&self) -> Option<(u32, u32)> {
        self.conn
            .get_geometry(self.root)
            .ok()?
            .reply()
            .ok()
            .map(|g| (g.width as u32, g.height as u32))
    }

    // -------------------------------------------------------------- displays

    pub fn displays(&self) -> Vec<DisplayInfo> {
        let mut out = Vec::new();
        if let Ok(cookie) = self.conn.randr_get_monitors(self.root, true) {
            if let Ok(reply) = cookie.reply() {
                for (i, m) in reply.monitors.iter().enumerate() {
                    let name = self
                        .conn
                        .get_atom_name(m.name)
                        .ok()
                        .and_then(|c| c.reply().ok())
                        .map(|r| String::from_utf8_lossy(&r.name).to_string())
                        .unwrap_or_else(|| format!("monitor{i}"));
                    out.push(DisplayInfo {
                        id: i.to_string(),
                        name,
                        x: m.x as i32,
                        y: m.y as i32,
                        width: m.width as u32,
                        height: m.height as u32,
                        primary: m.primary,
                        scale: 1.0,
                    });
                }
            }
        }
        if out.is_empty() {
            if let Some((w, h)) = self.root_size() {
                out.push(DisplayInfo {
                    id: "0".into(),
                    name: "screen".into(),
                    x: 0,
                    y: 0,
                    width: w,
                    height: h,
                    primary: true,
                    scale: 1.0,
                });
            }
        }
        out
    }

    pub fn cursor(&self) -> Option<(i32, i32)> {
        let r = self.conn.query_pointer(self.root).ok()?.reply().ok()?;
        Some((r.root_x as i32, r.root_y as i32))
    }

    // ---------------------------------------------------------------- input

    fn fake(&self, type_: u8, detail: u8, x: i16, y: i16) -> Result<(), String> {
        self.conn
            .xtest_fake_input(type_, detail, 0, self.root, x, y, 0)
            .map_err(|e| format!("XTEST failed: {e}"))?;
        self.conn.flush().map_err(|e| e.to_string())
    }

    fn sync(&self) -> Result<(), String> {
        self.conn.sync().map_err(|e| e.to_string())
    }

    pub fn move_abs(&self, x: i32, y: i32) -> Result<(), String> {
        self.fake(
            MOTION_NOTIFY_EVENT,
            0,
            x.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            y.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
        )?;
        self.sync()
    }

    pub fn move_rel(&self, dx: i32, dy: i32) -> Result<(), String> {
        let (x, y) = self.cursor().unwrap_or((0, 0));
        self.move_abs(x.saturating_add(dx), y.saturating_add(dy))
    }

    fn button_code(button: MouseButton) -> u8 {
        match button {
            MouseButton::Left => 1,
            MouseButton::Middle => 2,
            MouseButton::Right => 3,
            MouseButton::Back => 8,
            MouseButton::Forward => 9,
        }
    }

    pub fn button_down(&self, button: MouseButton) -> Result<(), String> {
        self.fake(BUTTON_PRESS_EVENT, Self::button_code(button), 0, 0)
    }

    pub fn button_up(&self, button: MouseButton) -> Result<(), String> {
        self.fake(BUTTON_RELEASE_EVENT, Self::button_code(button), 0, 0)
    }

    pub fn click(&self, button: MouseButton, count: u8, interval_ms: u64) -> Result<(), String> {
        for i in 0..count.max(1) {
            self.button_down(button)?;
            self.button_up(button)?;
            if i + 1 < count.max(1) && interval_ms > 0 {
                std::thread::sleep(Duration::from_millis(interval_ms));
            }
        }
        self.sync()
    }

    pub fn scroll(&self, dx: i32, dy: i32) -> Result<(), String> {
        let vbtn = if dy >= 0 { 5 } else { 4 };
        for _ in 0..dy.abs() {
            self.fake(BUTTON_PRESS_EVENT, vbtn, 0, 0)?;
            self.fake(BUTTON_RELEASE_EVENT, vbtn, 0, 0)?;
        }
        let hbtn = if dx >= 0 { 7 } else { 6 };
        for _ in 0..dx.abs() {
            self.fake(BUTTON_PRESS_EVENT, hbtn, 0, 0)?;
            self.fake(BUTTON_RELEASE_EVENT, hbtn, 0, 0)?;
        }
        self.sync()
    }

    fn press_keycode(&self, keymap: &Keymap, kc: u8, shift: bool) -> Result<(), String> {
        let shift_kc = keymap.lookup(XK_SHIFT_L).map(|(kc, _)| kc);
        if shift {
            if let Some(sk) = shift_kc {
                self.fake(KEY_PRESS_EVENT, sk, 0, 0)?;
            }
        }
        self.fake(KEY_PRESS_EVENT, kc, 0, 0)?;
        self.fake(KEY_RELEASE_EVENT, kc, 0, 0)?;
        if shift {
            if let Some(sk) = shift_kc {
                self.fake(KEY_RELEASE_EVENT, sk, 0, 0)?;
            }
        }
        Ok(())
    }

    fn remap_press(&self, keymap: &Keymap, keysym: u32) -> Result<(), String> {
        let spare = keymap
            .spare()
            .ok_or_else(|| "no spare X11 keycode available for Unicode input".to_string())?;
        let syms = vec![keysym; keymap.per.max(2) as usize];
        self.conn
            .change_keyboard_mapping(1, spare, keymap.per.max(2), &syms)
            .map_err(|e| e.to_string())?;
        self.sync()?;
        std::thread::sleep(Duration::from_millis(12));
        self.fake(KEY_PRESS_EVENT, spare, 0, 0)?;
        self.fake(KEY_RELEASE_EVENT, spare, 0, 0)?;
        let zeros = vec![0u32; keymap.per.max(2) as usize];
        self.conn
            .change_keyboard_mapping(1, spare, keymap.per.max(2), &zeros)
            .map_err(|e| e.to_string())?;
        self.sync()
    }

    fn press_keysym(&self, keymap: &Keymap, keysym: u32) -> Result<(), String> {
        match keymap.lookup(keysym) {
            Some((kc, shift)) => self.press_keycode(keymap, kc, shift),
            None => self.remap_press(keymap, keysym),
        }
    }

    pub fn key_chord(&self, modifiers: &[Modifier], key: &Key) -> Result<(), String> {
        let keymap = Keymap::load(&self.conn)?;
        let modsyms: Vec<u32> = modifiers
            .iter()
            .map(|m| match m {
                Modifier::Ctrl => XK_CONTROL_L,
                Modifier::Shift => XK_SHIFT_L,
                Modifier::Alt => XK_ALT_L,
                Modifier::Meta => XK_SUPER_L,
            })
            .collect();
        let mut pressed = Vec::new();
        for sym in &modsyms {
            if let Some((kc, _)) = keymap.lookup(*sym) {
                self.fake(KEY_PRESS_EVENT, kc, 0, 0)?;
                pressed.push(kc);
            }
        }
        let result = match named_keysym(key) {
            Some(sym) => self.press_keysym(&keymap, sym),
            None => Err(format!("key not supported on X11: {key:?}")),
        };
        for kc in pressed.iter().rev() {
            let _ = self.fake(KEY_RELEASE_EVENT, *kc, 0, 0);
        }
        self.sync()?;
        result
    }

    pub fn type_text(&self, text: &str, interval_ms: u64) -> Result<(), String> {
        let keymap = Keymap::load(&self.conn)?;
        for ch in text.chars() {
            match ch {
                '\n' | '\r' => self.press_keysym(&keymap, XK_RETURN)?,
                '\t' => self.press_keysym(&keymap, XK_TAB)?,
                c => match keymap.lookup_char(c) {
                    Some((kc, shift)) => self.press_keycode(&keymap, kc, shift)?,
                    None => {
                        let sym = char_keysym(c)
                            .ok_or_else(|| format!("cannot type character {c:?} on X11"))?;
                        self.remap_press(&keymap, sym)?;
                    }
                },
            }
            if interval_ms > 0 {
                std::thread::sleep(Duration::from_millis(interval_ms));
            }
        }
        Ok(())
    }

    // -------------------------------------------------------------- windows

    fn atom(&self, name: &str) -> Result<Atom, String> {
        Ok(self
            .conn
            .intern_atom(false, name.as_bytes())
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?
            .atom)
    }

    fn prop_bytes(&self, win: Window, prop: Atom, type_: Atom) -> Option<Vec<u8>> {
        let r = self
            .conn
            .get_property(false, win, prop, type_, 0, 8192)
            .ok()?
            .reply()
            .ok()?;
        Some(r.value)
    }

    pub fn list_windows(&self) -> Result<Vec<WindowInfo>, String> {
        let client_list = self.atom("_NET_CLIENT_LIST")?;
        let wm_name = self.atom("_NET_WM_NAME")?;
        let utf8 = self.atom("UTF8_STRING")?;
        let wm_pid = self.atom("_NET_WM_PID")?;
        let active = self.atom("_NET_ACTIVE_WINDOW")?;

        let focused = self
            .prop_bytes(self.root, active, AtomEnum::WINDOW.into())
            .and_then(|v| v.get(0..4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])));

        let list = self
            .conn
            .get_property(false, self.root, client_list, AtomEnum::WINDOW, 0, 8192)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        let windows: Vec<Window> = list.value32().map(|it| it.collect()).unwrap_or_default();

        let mut out = Vec::new();
        for win in windows {
            let title = self
                .prop_bytes(win, wm_name, utf8)
                .map(|b| String::from_utf8_lossy(&b).to_string())
                .or_else(|| {
                    self.prop_bytes(win, AtomEnum::WM_NAME.into(), AtomEnum::STRING.into())
                        .map(|b| String::from_utf8_lossy(&b).to_string())
                })
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            let pid = self
                .prop_bytes(win, wm_pid, AtomEnum::CARDINAL.into())
                .and_then(|v| v.get(0..4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])));
            let geom = match self.conn.get_geometry(win).ok().and_then(|c| c.reply().ok()) {
                Some(g) => g,
                None => continue,
            };
            let (tx, ty) = self
                .conn
                .translate_coordinates(win, self.root, 0, 0)
                .ok()
                .and_then(|c| c.reply().ok())
                .map(|r| (r.dst_x as i32, r.dst_y as i32))
                .unwrap_or((0, 0));
            out.push(WindowInfo {
                id: format!("{win}"),
                title,
                pid,
                app: String::new(),
                x: tx,
                y: ty,
                width: geom.width as u32,
                height: geom.height as u32,
                focused: focused == Some(win),
            });
        }
        Ok(out)
    }

    pub fn focus_window(&self, id: &str) -> Result<(), String> {
        let win: Window = id.parse().map_err(|_| format!("invalid X11 window id: {id}"))?;
        let active = self.atom("_NET_ACTIVE_WINDOW")?;
        let data = ClientMessageData::from([1u32, 0, 0, 0, 0]);
        let event = ClientMessageEvent {
            response_type: CLIENT_MESSAGE_EVENT,
            format: 32,
            sequence: 0,
            window: win,
            type_: active,
            data,
        };
        self.conn
            .send_event(
                false,
                self.root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )
            .map_err(|e| e.to_string())?;
        self.sync()
    }
}
