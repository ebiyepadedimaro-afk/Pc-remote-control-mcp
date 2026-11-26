//! Update checking against GitHub Releases. The desktop app asks the local
//! server (loopback admin API), which queries GitHub with a 10-minute cache.

use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const REPO: &str = "ebiyepadedimaro-afk/Pc-remote-control-mcp";

static CACHE: OnceLock<Mutex<Option<(u64, Value)>>> = OnceLock::new();

fn cache() -> &'static Mutex<Option<(u64, Value)>> {
    CACHE.get_or_init(|| Mutex::new(None))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn version_parts(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches(['v', 'V'])
        .split(['.', '-', '+'])
        .filter_map(|p| p.parse::<u64>().ok())
        .collect()
}

fn is_newer(latest: &str, current: &str) -> bool {
    let a = version_parts(latest);
    let b = version_parts(current);
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

fn pick_asset(assets: &[Value]) -> Option<(String, String)> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let os_keys: &[&str] = match os {
        "linux" => &["linux"],
        "macos" => &["macos", "darwin", "apple"],
        "windows" => &["windows", "win"],
        _ => &[],
    };
    let arch_keys: &[&str] = match arch {
        "x86_64" | "amd64" => &["x86_64", "amd64"],
        "aarch64" | "arm64" => &["arm64", "aarch64"],
        _ => &[],
    };
    for asset in assets {
        let Some(name) = asset.get("name").and_then(|n| n.as_str()) else { continue };
        let lower = name.to_ascii_lowercase();
        if os_keys.iter().any(|k| lower.contains(k)) && arch_keys.iter().any(|k| lower.contains(k)) {
            if let Some(url) = asset.get("browser_download_url").and_then(|u| u.as_str()) {
                return Some((name.to_string(), url.to_string()));
            }
        }
    }
    None
}

async fn fetch(http: &reqwest::Client) -> Value {
    let current = env!("CARGO_PKG_VERSION");
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let resp = http
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(15))
        .send()
        .await;
    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            return json!({"available": false, "current": current, "repo": REPO, "error": format!("cannot reach GitHub: {e}")})
        }
    };
    if !resp.status().is_success() {
        return json!({
            "available": false,
            "current": current,
            "repo": REPO,
            "error": format!("GitHub returned HTTP {}", resp.status()),
        });
    }
    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return json!({"available": false, "current": current, "repo": REPO, "error": e.to_string()}),
    };
    let release: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => return json!({"available": false, "current": current, "repo": REPO, "error": format!("invalid GitHub response: {e}")}),
    };
    let latest = release.get("tag_name").and_then(|t| t.as_str()).unwrap_or("").to_string();
    if latest.is_empty() {
        return json!({"available": false, "current": current, "repo": REPO, "error": "release has no tag"});
    }
    let assets = release
        .get("assets")
        .and_then(|a| a.as_array())
        .cloned()
        .unwrap_or_default();
    let picked = pick_asset(&assets);
    let notes: String = release
        .get("body")
        .and_then(|b| b.as_str())
        .unwrap_or("")
        .chars()
        .take(4000)
        .collect();
    json!({
        "available": is_newer(&latest, current),
        "current": current,
        "latest": latest.trim_start_matches(['v', 'V']),
        "tag": latest,
        "url": release.get("html_url").cloned().unwrap_or(Value::Null),
        "notes": notes,
        "published_at": release.get("published_at").cloned().unwrap_or(Value::Null),
        "asset": picked.as_ref().map(|(n, _)| n.clone()),
        "download_url": picked.map(|(_, u)| u),
        "repo": REPO,
        "checked_ms": now_ms(),
    })
}

fn asset_name() -> Option<String> {
    let name = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "pcrc-server-linux-x86_64.tar.gz",
        ("macos", "aarch64") => "pcrc-server-macos-arm64.tar.gz",
        ("macos", "x86_64") => "pcrc-server-macos-x86_64.tar.gz",
        ("windows", "x86_64") => "pcrc-server-windows-x86_64.zip",
        _ => return None,
    };
    Some(name.to_string())
}

/// api.github.com is blocked on some networks; github.com still redirects
/// `/releases/latest` to `/releases/tag/<tag>`, which is all we need.
async fn fallback_redirect(http: &reqwest::Client) -> Option<Value> {
    let url = format!("https://github.com/{REPO}/releases/latest");
    let resp = http.get(&url).timeout(Duration::from_secs(15)).send().await.ok()?;
    let final_url = resp.url().as_str().to_string();
    let tag = final_url
        .split("/releases/tag/")
        .nth(1)?
        .split(['?', '#'])
        .next()?
        .trim_end_matches('/')
        .to_string();
    if tag.is_empty() {
        return None;
    }
    let current = env!("CARGO_PKG_VERSION");
    let latest = tag.trim_start_matches(['v', 'V']).to_string();
    let asset = asset_name();
    let download_url = asset
        .as_ref()
        .map(|a| format!("https://github.com/{REPO}/releases/download/{tag}/{a}"));
    Some(json!({
        "available": is_newer(&latest, current),
        "current": current,
        "latest": latest,
        "tag": tag,
        "url": format!("https://github.com/{REPO}/releases/tag/{tag}"),
        "notes": "",
        "published_at": Value::Null,
        "asset": asset,
        "download_url": download_url,
        "repo": REPO,
        "source": "github-redirect",
        "checked_ms": now_ms(),
    }))
}

pub async fn check(http: &reqwest::Client) -> Value {
    let now = now_ms();
    if let Some((ts, value)) = cache().lock().unwrap().as_ref() {
        if now.saturating_sub(*ts) < 600_000 {
            return value.clone();
        }
    }
    let mut value = fetch(http).await;
    if value.get("error").is_some() {
        if let Some(fallback) = fallback_redirect(http).await {
            value = fallback;
        }
    }
    *cache().lock().unwrap() = Some((now, value.clone()));
    value
}
