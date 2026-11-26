//! Screenshots through the freedesktop portal, the path that works on
//! Wayland (GNOME, KDE, wlroots). Non-interactive: no consent dialog is
//! expected from the portal for `interactive: false`.

use futures_util::StreamExt;
use std::collections::HashMap;
use std::time::Duration;
use zbus::zvariant::{OwnedValue, Value};

const PORTAL_DEST: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SCREENSHOT_IFACE: &str = "org.freedesktop.portal.Screenshot";
const REQUEST_IFACE: &str = "org.freedesktop.portal.Request";

pub fn portal_installed() -> bool {
    std::path::Path::new("/usr/share/xdg-desktop-portal/portals").exists()
        || std::path::Path::new("/usr/lib/xdg-desktop-portal/portals").exists()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub async fn screenshot() -> Result<image::DynamicImage, String> {
    let conn = zbus::Connection::session()
        .await
        .map_err(|e| format!("cannot connect to the session bus: {e}"))?;

    let token = format!("pcrc{}", std::process::id());
    let sender = conn
        .unique_name()
        .map(|n| n.as_str().trim_start_matches(':').replace('.', "_"))
        .unwrap_or_default();
    let predicted = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");

    // Subscribe to the Response signal before calling the portal.
    let request_proxy = zbus::Proxy::new_owned(
        conn.clone(),
        PORTAL_DEST,
        predicted.clone(),
        REQUEST_IFACE,
    )
    .await
    .map_err(|e| format!("cannot prepare the portal request: {e}"))?;
    let mut responses = request_proxy
        .receive_signal("Response")
        .await
        .map_err(|e| format!("cannot subscribe to the portal response: {e}"))?;

    let screenshot_proxy = zbus::Proxy::new_owned(
        conn.clone(),
        PORTAL_DEST,
        PORTAL_PATH,
        SCREENSHOT_IFACE,
    )
    .await
    .map_err(|e| format!("the screenshot portal is unavailable: {e}"))?;

    let mut options: HashMap<&str, Value> = HashMap::new();
    options.insert("interactive", Value::from(false));
    options.insert("handle_token", Value::from(token.as_str()));
    let handle: zbus::zvariant::OwnedObjectPath = screenshot_proxy
        .call("Screenshot", &("", options))
        .await
        .map_err(|e| format!("the portal refused the screenshot request: {e}"))?;

    let signal_msg = if handle.as_str() == predicted {
        tokio::time::timeout(Duration::from_secs(20), responses.next())
            .await
            .map_err(|_| "the screenshot portal did not answer in 20 s".to_string())?
            .ok_or_else(|| "the screenshot portal response stream closed".to_string())?
    } else {
        // Fallback: the portal ignored handle_token and answered on another path.
        let proxy = zbus::Proxy::new_owned(
            conn.clone(),
            PORTAL_DEST,
            handle.as_str().to_string(),
            REQUEST_IFACE,
        )
        .await
        .map_err(|e| format!("cannot read the portal response: {e}"))?;
        let mut stream = proxy
            .receive_signal("Response")
            .await
            .map_err(|e| format!("cannot read the portal response: {e}"))?;
        tokio::time::timeout(Duration::from_secs(20), stream.next())
            .await
            .map_err(|_| "the screenshot portal did not answer in 20 s".to_string())?
            .ok_or_else(|| "the screenshot portal response stream closed".to_string())?
    };

    let (code, results): (u32, HashMap<String, OwnedValue>) = signal_msg
        .body()
        .deserialize()
        .map_err(|e| format!("cannot parse the portal response: {e}"))?;
    if code != 0 {
        return Err(match code {
            1 => "the screenshot was cancelled".to_string(),
            2 => "the screenshot portal ended the request".to_string(),
            other => format!("the screenshot portal returned response code {other}"),
        });
    }
    let uri: String = results
        .get("uri")
        .and_then(|v| String::try_from(v.clone()).ok())
        .ok_or_else(|| "the portal response has no screenshot file".to_string())?;
    let path = percent_decode(uri.strip_prefix("file://").unwrap_or(&uri));
    let bytes = std::fs::read(&path).map_err(|e| format!("cannot read the portal screenshot {path}: {e}"))?;
    let img = image::load_from_memory(&bytes)
        .map_err(|e| format!("the portal screenshot is not a readable image: {e}"))?;
    let _ = std::fs::remove_file(&path);
    Ok(img)
}
