//! File tools, confined to the configured files root when one is set.

use super::{arg_bool, arg_i64, arg_str, arg_u32, expand_path, ToolOutput};
use crate::state::AppState;
use base64::Engine;
use futures_util::StreamExt;
use serde_json::json;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

pub(crate) fn resolve(state: &AppState, raw: &str, for_write: bool) -> Result<PathBuf, String> {
    if raw.trim().is_empty() {
        return Err("path is empty".into());
    }
    let expanded = expand_path(raw);
    let p = PathBuf::from(&expanded);
    if !p.is_absolute() {
        return Err(format!("path must be absolute (got {raw})"));
    }
    let cfg = state.config.get();
    if cfg.files.root.trim().is_empty() {
        return Ok(p);
    }
    let root = std::fs::canonicalize(expand_path(cfg.files.root.trim()))
        .map_err(|e| format!("configured files root is invalid: {e}"))?;
    if for_write {
        let mut probe = p.clone();
        let mut suffix: Vec<std::ffi::OsString> = Vec::new();
        while !probe.exists() {
            match probe.file_name().map(|n| n.to_os_string()) {
                Some(name) => {
                    suffix.push(name);
                    if !probe.pop() {
                        break;
                    }
                }
                None => break,
            }
        }
        let mut full = std::fs::canonicalize(&probe)
            .map_err(|e| format!("cannot resolve {raw}: {e}"))?;
        for part in suffix.iter().rev() {
            full.push(part);
        }
        if full.starts_with(&root) {
            Ok(full)
        } else {
            Err(format!("path is outside the configured files root {}", root.display()))
        }
    } else {
        let canon = std::fs::canonicalize(&p).map_err(|e| format!("cannot access {raw}: {e}"))?;
        if canon.starts_with(&root) {
            Ok(canon)
        } else {
            Err(format!("path is outside the configured files root {}", root.display()))
        }
    }
}

fn entry_type(ft: &std::fs::FileType) -> &'static str {
    if ft.is_symlink() {
        "symlink"
    } else if ft.is_dir() {
        "directory"
    } else if ft.is_file() {
        "file"
    } else {
        "other"
    }
}

fn modified_ms(meta: &std::fs::Metadata) -> Option<u64> {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
}

pub async fn list_directory(state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let raw = arg_str(&args, "path").ok_or("path is required")?;
    let path = resolve(&state, raw, false)?;
    if !path.is_dir() {
        return Err(format!("{} is not a directory", path.display()));
    }
    let depth = arg_i64(&args, "depth").unwrap_or(1).clamp(1, 4) as usize;
    let hidden = arg_bool(&args, "include_hidden", false);
    let mut queue: Vec<(PathBuf, usize)> = vec![(path.clone(), 1)];
    let mut entries = Vec::new();
    while let Some((dir, level)) = queue.pop() {
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) => {
                entries.push(json!({"path": dir.to_string_lossy(), "error": e.to_string()}));
                continue;
            }
        };
        let mut here: Vec<serde_json::Value> = Vec::new();
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !hidden && name.starts_with('.') {
                continue;
            }
            let p = entry.path();
            let meta = match std::fs::symlink_metadata(&p) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let ft = meta.file_type();
            let ty = entry_type(&ft);
            here.push(json!({
                "name": name,
                "path": p.to_string_lossy(),
                "type": ty,
                "size": if ft.is_file() { meta.len() } else { 0 },
                "modified_ms": modified_ms(&meta),
            }));
            if ft.is_dir() && level < depth {
                queue.push((p, level + 1));
            }
            if entries.len() + here.len() >= 2000 {
                break;
            }
        }
        here.sort_by(|a, b| {
            let ad = a.get("type").and_then(|t| t.as_str()) == Some("directory");
            let bd = b.get("type").and_then(|t| t.as_str()) == Some("directory");
            bd.cmp(&ad).then_with(|| {
                a.get("name").and_then(|n| n.as_str()).unwrap_or("").cmp(b.get("name").and_then(|n| n.as_str()).unwrap_or(""))
            })
        });
        entries.extend(here);
        if entries.len() >= 2000 {
            break;
        }
    }
    Ok(ToolOutput::json(json!({
        "path": path.to_string_lossy(),
        "count": entries.len(),
        "entries": entries,
    })))
}

pub async fn read_file(state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let raw = arg_str(&args, "path").ok_or("path is required")?;
    let path = resolve(&state, raw, false)?;
    let cfg = state.config.get();
    let max = arg_u32(&args, "max_bytes")
        .map(|m| m as u64)
        .unwrap_or(cfg.files.max_read_bytes)
        .min(cfg.files.max_read_bytes);
    let meta = std::fs::metadata(&path).map_err(|e| format!("cannot stat {}: {e}", path.display()))?;
    if meta.is_dir() {
        return Err(format!("{} is a directory", path.display()));
    }
    if meta.len() > max {
        return Err(format!(
            "file is {} bytes, above the {max}-byte limit (raise max_bytes or the config limit)",
            meta.len()
        ));
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let encoding = arg_str(&args, "encoding").unwrap_or("utf8");
    let content = if encoding == "base64" {
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    } else {
        String::from_utf8(bytes.clone())
            .map_err(|_| "file is not valid UTF-8; use encoding=\"base64\"".to_string())?
    };
    Ok(ToolOutput::json(json!({
        "path": path.to_string_lossy(),
        "size": bytes.len(),
        "encoding": if encoding == "base64" { "base64" } else { "utf8" },
        "content": content,
    })))
}

pub async fn write_file(state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let cfg = state.config.get();
    if !cfg.files.allow_write {
        return Err("file writes are disabled in the server configuration".into());
    }
    let raw = arg_str(&args, "path").ok_or("path is required")?;
    let content = arg_str(&args, "content").ok_or("content is required")?;
    let encoding = arg_str(&args, "encoding").unwrap_or("utf8");
    let bytes = if encoding == "base64" {
        base64::engine::general_purpose::STANDARD
            .decode(content.as_bytes())
            .map_err(|e| format!("invalid base64 content: {e}"))?
    } else {
        content.as_bytes().to_vec()
    };
    if bytes.len() as u64 > cfg.files.max_write_bytes {
        return Err(format!(
            "content is {} bytes, above the {}-byte write limit",
            bytes.len(),
            cfg.files.max_write_bytes
        ));
    }
    let path = resolve(&state, raw, true)?;
    if arg_bool(&args, "create_parents", true) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
    }
    std::fs::write(&path, &bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(ToolOutput::json(json!({
        "ok": true,
        "path": path.to_string_lossy(),
        "bytes_written": bytes.len(),
    })))
}

pub async fn delete_path(state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let cfg = state.config.get();
    if !cfg.files.allow_delete {
        return Err("file deletion is disabled in the server configuration".into());
    }
    let raw = arg_str(&args, "path").ok_or("path is required")?;
    let path = resolve(&state, raw, true)?;
    let home = dirs::home_dir();
    if path == Path::new("/") || Some(&path) == home.as_ref() || path.as_os_str().is_empty() {
        return Err("refusing to delete a filesystem root or the home directory".into());
    }
    let meta = std::fs::symlink_metadata(&path).map_err(|e| format!("cannot stat {}: {e}", path.display()))?;
    let recursive = arg_bool(&args, "recursive", false);
    if meta.is_dir() {
        if recursive {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_dir(&path)
        }
        .map_err(|e| format!("cannot delete {}: {e}", path.display()))?;
    } else {
        std::fs::remove_file(&path).map_err(|e| format!("cannot delete {}: {e}", path.display()))?;
    }
    Ok(ToolOutput::json(json!({"ok": true, "deleted": path.to_string_lossy(), "recursive": recursive})))
}

pub async fn move_path(state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let cfg = state.config.get();
    if !cfg.files.allow_write {
        return Err("file writes are disabled in the server configuration".into());
    }
    let from = resolve(&state, arg_str(&args, "from").ok_or("from is required")?, true)?;
    let to = resolve(&state, arg_str(&args, "to").ok_or("to is required")?, true)?;
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    match std::fs::rename(&from, &to) {
        Ok(()) => {}
        Err(_) => {
            let bytes = copy_tree(&from, &to, 0)?;
            if from.is_dir() {
                std::fs::remove_dir_all(&from)
            } else {
                std::fs::remove_file(&from)
            }
            .map_err(|e| format!("moved copy but cannot remove {}: {e}", from.display()))?;
            let _ = bytes;
        }
    }
    Ok(ToolOutput::json(json!({"ok": true, "from": from.to_string_lossy(), "to": to.to_string_lossy()})))
}

fn copy_tree(src: &Path, dst: &Path, depth: usize) -> Result<u64, String> {
    if depth > 32 {
        return Err("directory tree too deep (symlink loop?)".into());
    }
    let meta = std::fs::symlink_metadata(src).map_err(|e| format!("cannot stat {}: {e}", src.display()))?;
    if meta.is_dir() {
        std::fs::create_dir_all(dst).map_err(|e| format!("cannot create {}: {e}", dst.display()))?;
        let mut total = 0;
        for entry in std::fs::read_dir(src).map_err(|e| format!("cannot list {}: {e}", src.display()))?.flatten() {
            let name = entry.file_name();
            total += copy_tree(&entry.path(), &dst.join(&name), depth + 1)?;
        }
        Ok(total)
    } else {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        std::fs::copy(src, dst).map_err(|e| format!("cannot copy {} to {}: {e}", src.display(), dst.display()))
    }
}

pub async fn copy_path(state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let cfg = state.config.get();
    if !cfg.files.allow_write {
        return Err("file writes are disabled in the server configuration".into());
    }
    let from = resolve(&state, arg_str(&args, "from").ok_or("from is required")?, false)?;
    let to = resolve(&state, arg_str(&args, "to").ok_or("to is required")?, true)?;
    let bytes = copy_tree(&from, &to, 0)?;
    Ok(ToolOutput::json(json!({
        "ok": true,
        "from": from.to_string_lossy(),
        "to": to.to_string_lossy(),
        "bytes_copied": bytes,
    })))
}

pub async fn make_directory(state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let cfg = state.config.get();
    if !cfg.files.allow_write {
        return Err("file writes are disabled in the server configuration".into());
    }
    let path = resolve(&state, arg_str(&args, "path").ok_or("path is required")?, true)?;
    std::fs::create_dir_all(&path).map_err(|e| format!("cannot create {}: {e}", path.display()))?;
    Ok(ToolOutput::json(json!({"ok": true, "path": path.to_string_lossy()})))
}

pub async fn file_info(state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let path = resolve(&state, arg_str(&args, "path").ok_or("path is required")?, false)?;
    let meta = std::fs::symlink_metadata(&path).map_err(|e| format!("cannot stat {}: {e}", path.display()))?;
    let ft = meta.file_type();
    let mut info = json!({
        "path": path.to_string_lossy(),
        "type": entry_type(&ft),
        "size": meta.len(),
        "modified_ms": modified_ms(&meta),
        "readonly": meta.permissions().readonly(),
    });
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        info["mode"] = json!(format!("{:o}", meta.permissions().mode() & 0o7777));
    }
    Ok(ToolOutput::json(info))
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || (v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1]))
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                || v6.to_ipv4_mapped().map(|v4| is_private_ip(IpAddr::V4(v4))).unwrap_or(false)
        }
    }
}

pub async fn download_from_url(state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let cfg = state.config.get();
    if !cfg.downloads.enabled {
        return Err("downloads are disabled in the server configuration".into());
    }
    if !cfg.files.allow_write {
        return Err("file writes are disabled in the server configuration".into());
    }
    let url_str = arg_str(&args, "url").ok_or("url is required")?;
    let dest_raw = arg_str(&args, "path").ok_or("path is required")?;
    let dest = resolve(&state, dest_raw, true)?;
    let max = arg_u32(&args, "max_bytes")
        .map(|m| m as u64)
        .unwrap_or(cfg.downloads.max_bytes)
        .min(cfg.downloads.max_bytes);

    let url = reqwest::Url::parse(url_str).map_err(|e| format!("invalid URL: {e}"))?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err("only http and https URLs are supported".into());
    }
    let host = url.host_str().ok_or("URL has no host")?.to_string();
    let port = url.port_or_known_default().unwrap_or(443);
    if !cfg.downloads.allow_private_hosts {
        let addrs = tokio::net::lookup_host((host.as_str(), port))
            .await
            .map_err(|e| format!("cannot resolve {host}: {e}"))?;
        for addr in addrs {
            if is_private_ip(addr.ip()) {
                return Err(format!(
                    "refusing to download from private address {} (allow_private_hosts is false)",
                    addr.ip()
                ));
            }
        }
    }

    let timeout = Duration::from_millis(cfg.downloads.timeout_ms);
    let result = tokio::time::timeout(timeout, async {
        let resp = state
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| format!("download failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("download failed: HTTP {}", resp.status()));
        }
        if let Some(len) = resp.content_length() {
            if len > max {
                return Err(format!("remote file is {len} bytes, above the {max}-byte limit"));
            }
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        let mut file = tokio::fs::File::create(&dest)
            .await
            .map_err(|e| format!("cannot create {}: {e}", dest.display()))?;
        let mut written: u64 = 0;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("download failed: {e}"))?;
            written += chunk.len() as u64;
            if written > max {
                drop(file);
                let _ = std::fs::remove_file(&dest);
                return Err(format!("download exceeded the {max}-byte limit"));
            }
            use tokio::io::AsyncWriteExt;
            file.write_all(&chunk)
                .await
                .map_err(|e| format!("cannot write {}: {e}", dest.display()))?;
        }
        use tokio::io::AsyncWriteExt;
        file.flush().await.ok();
        Ok(written)
    })
    .await;
    match result {
        Ok(Ok(written)) => Ok(ToolOutput::json(json!({
            "ok": true,
            "path": dest.to_string_lossy(),
            "bytes": written,
        }))),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(format!("download timed out after {} ms", cfg.downloads.timeout_ms)),
    }
}
