//! Remote-access tunnels: expose the local MCP server through a public or
//! tailnet URL so clients on another network can reach this PC.
//!
//! Managed kinds:
//! - cloudflared quick tunnel (no account): https://<random>.trycloudflare.com
//! - Tailscale Serve: https://<machine>.<tailnet>.ts.net (tailnet clients)
//! - ngrok: https://<random>.ngrok-free.app (account/authtoken required)
//!
//! Starting a tunnel is refused unless bearer-token auth is enabled: a public
//! remote-control endpoint must not be open.

use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, BufReader};

struct TunnelInfo {
    kind: String,
    state: String,
    url: Option<String>,
    error: Option<String>,
    pid: Option<u32>,
    started_ms: u64,
    log_tail: String,
}

impl Default for TunnelInfo {
    fn default() -> Self {
        Self {
            kind: String::new(),
            state: "stopped".into(),
            url: None,
            error: None,
            pid: None,
            started_ms: 0,
            log_tail: String::new(),
        }
    }
}

static TUNNEL: OnceLock<Mutex<TunnelInfo>> = OnceLock::new();

fn info() -> &'static Mutex<TunnelInfo> {
    TUNNEL.get_or_init(|| Mutex::new(TunnelInfo::default()))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn installed() -> Value {
    let find = |name: &str| crate::tools::which(name).map(|p| p.to_string_lossy().to_string());
    json!({
        "cloudflared": find("cloudflared"),
        "tailscale": find("tailscale"),
        "ngrok": find("ngrok"),
    })
}

fn install_hint(kind: &str) -> String {
    match kind {
        "cloudflared" => "cloudflared is not installed. Install it from https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/ (a quick tunnel needs no Cloudflare account).".into(),
        "tailscale" => "tailscale is not installed. Install it from https://tailscale.com/download, run `tailscale up`, and enable HTTPS certificates in the tailnet settings.".into(),
        "ngrok" => "ngrok is not installed. Install it from https://ngrok.com/download and run `ngrok config add-authtoken <token>`.".into(),
        _ => "no tunnel client is installed".into(),
    }
}

fn resolve_kind(requested: &str) -> Result<String, String> {
    let req = requested.trim().to_ascii_lowercase();
    let has = |k: &str| crate::tools::which(k).is_some();
    match req.as_str() {
        "auto" | "" => {
            for k in ["cloudflared", "tailscale", "ngrok"] {
                if has(k) {
                    return Ok(k.to_string());
                }
            }
            Err(format!(
                "no tunnel client found. Install one of: cloudflared (free quick tunnel), tailscale, ngrok. {}",
                install_hint("cloudflared")
            ))
        }
        "cloudflared" | "tailscale" | "ngrok" => {
            if has(&req) {
                Ok(req)
            } else {
                Err(install_hint(&req))
            }
        }
        other => Err(format!("unknown tunnel kind `{other}` (cloudflared, tailscale, ngrok, auto)")),
    }
}

fn push_log(line: &str) {
    let mut g = info().lock().unwrap();
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    g.log_tail.push_str(line);
    g.log_tail.push('\n');
    if g.log_tail.len() > 8192 {
        let mut cut = g.log_tail.len() - 8192;
        while !g.log_tail.is_char_boundary(cut) {
            cut += 1;
        }
        g.log_tail.drain(..cut);
    }
}

fn is_ngrok_url(url: &str) -> bool {
    let host = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or("");
    host.ends_with(".ngrok-free.app")
        || host.ends_with(".ngrok-free.dev")
        || host.ends_with(".ngrok.app")
        || host.ends_with(".ngrok.dev")
        || host.ends_with(".ngrok.io")
}

fn extract_https(line: &str, needle: &str) -> Option<String> {
    let mut rest = line;
    while let Some(i) = rest.find("https://") {
        let s = &rest[i..];
        let end = s
            .find(|c: char| c.is_whitespace() || c == '"' || c == '<' || c == '>' || c == '\\' || c == ')')
            .unwrap_or(s.len());
        let cand = &s[..end];
        if cand.len() > 12 && (needle.is_empty() || cand.contains(needle)) {
            return Some(cand.to_string());
        }
        rest = &s[3..];
    }
    None
}

fn set_running(url: Option<String>) {
    let mut g = info().lock().unwrap();
    g.state = "running".into();
    if url.is_some() {
        g.url = url;
    }
    g.error = None;
}

fn kill_tree(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as i32), libc::SIGTERM);
        libc::kill(pid as i32, libc::SIGTERM);
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = pid;
}

fn ts_cli_args(socket: &str, args: Vec<String>) -> Vec<String> {
    let socket = socket.trim();
    if socket.is_empty() {
        return args;
    }
    let mut out = vec!["--socket".to_string(), socket.to_string()];
    out.extend(args);
    out
}

async fn tailscale_dns(bin: &PathBuf, socket: &str) -> Result<String, String> {
    let out = tokio::process::Command::new(bin)
        .args(ts_cli_args(socket, vec!["status".into(), "--json".into()]))
        .output()
        .await
        .map_err(|e| format!("cannot run tailscale: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "`tailscale status` failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("cannot parse tailscale status: {e}"))?;
    let dns = v
        .get("Self")
        .and_then(|s| s.get("DNSName"))
        .and_then(|d| d.as_str())
        .map(|d| d.trim_end_matches('.').to_string())
        .filter(|d| !d.is_empty())
        .ok_or_else(|| "tailscale is not logged in (run `tailscale up`)".to_string())?;
    Ok(dns)
}

pub async fn start(requested_kind: &str, port: u16, ngrok_authtoken: &str, tailscale_socket: &str) -> Result<Value, String> {
    let busy = {
        let g = info().lock().unwrap();
        g.state == "running" || g.state == "starting" || g.state == "stopping"
    };
    if busy {
        return Ok(status());
    }
    let kind = resolve_kind(requested_kind)?;

    match kind.as_str() {
        "tailscale" => {
            let bin = crate::tools::which("tailscale").ok_or_else(|| install_hint("tailscale"))?;
            let dns = tailscale_dns(&bin, tailscale_socket).await?;
            let target = format!("http://127.0.0.1:{port}");
            let https = tokio::process::Command::new(&bin)
                .args(ts_cli_args(tailscale_socket, vec![
                    "serve".into(),
                    "--bg".into(),
                    "--https=443".into(),
                    target.clone(),
                ]))
                .output()
                .await
                .map_err(|e| format!("cannot run `tailscale serve`: {e}"))?;
            let (scheme, out) = if https.status.success() {
                ("https", https)
            } else {
                let http = tokio::process::Command::new(&bin)
                    .args(ts_cli_args(tailscale_socket, vec![
                        "serve".into(),
                        "--bg".into(),
                        "--http=80".into(),
                        target,
                    ]))
                    .output()
                    .await
                    .map_err(|e| format!("cannot run `tailscale serve`: {e}"))?;
                if !http.status.success() {
                    return Err(format!(
                        "`tailscale serve` failed. HTTPS: {}. HTTP: {}. Enable HTTPS certificates for the tailnet, or allow port 80.",
                        String::from_utf8_lossy(&https.stderr).trim(),
                        String::from_utf8_lossy(&http.stderr).trim()
                    ));
                }
                ("http", http)
            };
            {
                let mut g = info().lock().unwrap();
                *g = TunnelInfo {
                    kind,
                    state: "running".into(),
                    url: Some(format!("{scheme}://{dns}")),
                    started_ms: now_ms(),
                    log_tail: String::from_utf8_lossy(&out.stdout).to_string(),
                    ..Default::default()
                };
            }
            Ok(status())
        }
        "cloudflared" | "ngrok" => {
            let (program, args): (PathBuf, Vec<String>) = if kind == "cloudflared" {
                (
                    crate::tools::which("cloudflared").ok_or_else(|| install_hint("cloudflared"))?,
                    vec![
                        "tunnel".into(),
                        "--no-autoupdate".into(),
                        "--url".into(),
                        format!("http://127.0.0.1:{port}"),
                    ],
                )
            } else {
                (
                    crate::tools::which("ngrok").ok_or_else(|| install_hint("ngrok"))?,
                    {
                        let mut args = vec![
                            "http".into(),
                            port.to_string(),
                            "--log=stdout".into(),
                            "--log-format=json".into(),
                        ];
                        if !ngrok_authtoken.trim().is_empty() {
                            args.push("--authtoken".into());
                            args.push(ngrok_authtoken.trim().to_string());
                        }
                        args
                    },
                )
            };
            let mut cmd = tokio::process::Command::new(&program);
            cmd.args(&args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            #[cfg(unix)]
            cmd.process_group(0);
            #[cfg(target_os = "linux")]
            unsafe {
                // Take the tunnel client down with the server, even on SIGKILL.
                cmd.pre_exec(|| {
                    libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                    Ok(())
                });
            }
            let mut child = cmd
                .spawn()
                .map_err(|e| format!("cannot start {}: {e}", program.display()))?;
            let pid = child.id();
            {
                let mut g = info().lock().unwrap();
                *g = TunnelInfo {
                    kind: kind.clone(),
                    state: "starting".into(),
                    pid,
                    started_ms: now_ms(),
                    ..Default::default()
                };
            }

            let kind_for_logs = kind.clone();
            if let Some(stdout) = child.stdout.take() {
                tokio::spawn(async move {
                    let mut lines = BufReader::new(stdout).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        push_log(&line);
                        let found = if kind_for_logs == "ngrok" {
                            serde_json::from_str::<Value>(&line)
                                .ok()
                                .and_then(|v| v.get("url").and_then(|u| u.as_str()).map(str::to_string))
                                .filter(|u| is_ngrok_url(u))
                                .or_else(|| extract_https(&line, ".ngrok-free."))
                                .or_else(|| extract_https(&line, ".ngrok.app"))
                                .or_else(|| extract_https(&line, ".ngrok.io"))
                                .filter(|u| is_ngrok_url(u))
                        } else {
                            extract_https(&line, "trycloudflare.com")
                        };
                        if let Some(url) = found {
                            set_running(Some(url));
                        }
                    }
                });
            }
            if let Some(stderr) = child.stderr.take() {
                let kind_for_logs = kind.clone();
                tokio::spawn(async move {
                    let mut lines = BufReader::new(stderr).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        push_log(&line);
                        let found = if kind_for_logs == "ngrok" {
                            extract_https(&line, ".ngrok-free.")
                                .or_else(|| extract_https(&line, ".ngrok.app"))
                                .or_else(|| extract_https(&line, ".ngrok.io"))
                                .filter(|u| is_ngrok_url(u))
                        } else {
                            extract_https(&line, "trycloudflare.com")
                        };
                        if let Some(url) = found {
                            set_running(Some(url));
                        }
                    }
                });
            }
            tokio::spawn(async move {
                let result = child.wait().await;
                let mut g = info().lock().unwrap();
                g.pid = None;
                if g.state == "stopping" {
                    g.state = "stopped".into();
                } else {
                    g.state = "exited".into();
                    if let Err(e) = result {
                        g.error = Some(e.to_string());
                    }
                    // Keep the last tunnel log lines as the error hint.
                    if g.error.is_none() {
                        let tail = g.log_tail.trim();
                        if !tail.is_empty() {
                            g.error = Some(tail.chars().take(400).collect());
                        }
                    }
                }
            });

            // Give the client a moment to publish its URL. For ngrok, the local
            // agent API (127.0.0.1:4040) is authoritative.
            let client = reqwest::Client::new();
            for _ in 0..65 {
                let state = info().lock().unwrap().state.clone();
                if state != "starting" {
                    break;
                }
                if kind == "ngrok" {
                    if let Ok(resp) = client
                        .get("http://127.0.0.1:4040/api/tunnels")
                        .timeout(Duration::from_millis(800))
                        .send()
                        .await
                    {
                        if let Ok(bytes) = resp.bytes().await {
                            if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
                                if let Some(url) = v
                                    .get("tunnels")
                                    .and_then(|t| t.as_array())
                                    .and_then(|a| a.first())
                                    .and_then(|t| t.get("public_url"))
                                    .and_then(|u| u.as_str())
                                    .filter(|u| u.starts_with("https://"))
                                {
                                    set_running(Some(url.to_string()));
                                }
                            }
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Ok(status())
        }
        other => Err(format!("unsupported tunnel kind: {other}")),
    }
}

pub async fn stop(tailscale_socket: &str) -> Result<Value, String> {
    let (kind, pid, state) = {
        let g = info().lock().unwrap();
        (g.kind.clone(), g.pid, g.state.clone())
    };
    if state == "stopped" || state == "exited" || state.is_empty() {
        {
            let mut g = info().lock().unwrap();
            g.state = "stopped".into();
            g.url = None;
        }
        return Ok(status());
    }
    {
        let mut g = info().lock().unwrap();
        g.state = "stopping".into();
    }
    if kind == "tailscale" {
        if let Some(bin) = crate::tools::which("tailscale") {
            let _ = tokio::process::Command::new(bin)
                .args(ts_cli_args(tailscale_socket, vec!["serve".into(), "reset".into()]))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }
        {
            let mut g = info().lock().unwrap();
            g.state = "stopped".into();
            g.url = None;
            g.pid = None;
        }
        return Ok(status());
    }
    kill_tree(pid);
    for _ in 0..20 {
        let state = info().lock().unwrap().state.clone();
        if state == "stopped" || state == "exited" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    {
        let mut g = info().lock().unwrap();
        if g.state != "stopped" {
            g.state = "stopped".into();
        }
        g.url = None;
        g.pid = None;
    }
    Ok(status())
}

pub fn status() -> Value {
    let g = info().lock().unwrap();
    let uptime = if g.started_ms > 0 && g.state != "stopped" {
        now_ms().saturating_sub(g.started_ms) / 1000
    } else {
        0
    };
    json!({
        "state": g.state,
        "kind": g.kind,
        "url": g.url,
        "error": g.error,
        "pid": g.pid,
        "uptime_secs": uptime,
        "log_tail": g.log_tail,
        "installed": installed(),
    })
}
