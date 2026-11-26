//! Shell tool with timeout, output caps and a blocklist.

use super::{arg_i64, arg_str, ToolOutput};
use crate::state::AppState;
use serde_json::json;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt};

async fn read_capped<R: AsyncRead + Unpin>(mut r: R, cap: usize) -> (Vec<u8>, bool) {
    let mut kept = Vec::new();
    let mut total = 0usize;
    let mut buf = [0u8; 8192];
    loop {
        match r.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                total += n;
                if kept.len() < cap {
                    let take = (cap - kept.len()).min(n);
                    kept.extend_from_slice(&buf[..take]);
                }
            }
            Err(_) => break,
        }
    }
    (kept, total > cap)
}

pub async fn exec(state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let cfg = state.config.get();
    if !cfg.shell.enabled {
        return Err("shell execution is disabled in the server configuration".into());
    }
    let command = arg_str(&args, "command").ok_or("command is required")?.trim().to_string();
    if command.is_empty() {
        return Err("command is empty".into());
    }
    let arg_list = super::arg_strings(&args, "args");
    let use_shell = arg_list.is_empty();

    let full_text = if use_shell { command.clone() } else { format!("{command} {}", arg_list.join(" ")) };
    let lower = full_text.to_ascii_lowercase();
    for blocked in &cfg.shell.blocklist {
        if !blocked.trim().is_empty() && lower.contains(&blocked.to_ascii_lowercase()) {
            return Err(format!("command refused by the blocklist (`{blocked}`)"));
        }
    }

    let timeout_ms = arg_i64(&args, "timeout_ms")
        .unwrap_or(cfg.shell.default_timeout_ms as i64)
        .clamp(100, cfg.shell.max_timeout_ms as i64) as u64;
    let cap = cfg.shell.max_output_bytes;

    #[cfg(windows)]
    let mut cmd = if use_shell {
        let mut c = tokio::process::Command::new("cmd.exe");
        c.args(["/C", &command]);
        c
    } else {
        let mut c = tokio::process::Command::new(&command);
        c.args(&arg_list);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = if use_shell {
        let mut c = tokio::process::Command::new("sh");
        c.args(["-c", &command]);
        c
    } else {
        let mut c = tokio::process::Command::new(&command);
        c.args(&arg_list);
        c
    };

    if let Some(dir) = arg_str(&args, "cwd") {
        cmd.current_dir(super::expand_path(dir));
    }
    if let Some(env) = args.get("env").and_then(|e| e.as_object()) {
        for (k, v) in env {
            if let Some(v) = v.as_str() {
                cmd.env(k, v);
            }
        }
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    cmd.process_group(0);

    let started = Instant::now();
    let mut child = cmd.spawn().map_err(|e| format!("failed to start `{command}`: {e}"))?;
    let pid = child.id();
    let stdout = child.stdout.take().ok_or("cannot capture stdout")?;
    let stderr = child.stderr.take().ok_or("cannot capture stderr")?;
    let out_task = tokio::spawn(read_capped(stdout, cap));
    let err_task = tokio::spawn(read_capped(stderr, cap));

    let deadline = Duration::from_millis(timeout_ms);
    let wait = tokio::time::timeout(deadline, child.wait()).await;
    let (status, timed_out) = match wait {
        Ok(res) => (Some(res.map_err(|e| format!("wait failed: {e}"))?), false),
        Err(_) => {
            kill_tree(pid).await;
            let _ = child.kill().await;
            let _ = child.wait().await;
            (None, true)
        }
    };
    let duration_ms = started.elapsed().as_millis();

    let (out_bytes, out_trunc) = match tokio::time::timeout(Duration::from_secs(2), out_task).await {
        Ok(Ok(v)) => v,
        _ => (Vec::new(), false),
    };
    let (err_bytes, err_trunc) = match tokio::time::timeout(Duration::from_secs(2), err_task).await {
        Ok(Ok(v)) => v,
        _ => (Vec::new(), false),
    };

    let mut result = json!({
        "exit_code": status.and_then(|s| s.code()),
        "timed_out": timed_out,
        "duration_ms": duration_ms,
        "stdout": String::from_utf8_lossy(&out_bytes),
        "stderr": String::from_utf8_lossy(&err_bytes),
        "stdout_truncated": out_trunc,
        "stderr_truncated": err_trunc,
    });
    #[cfg(unix)]
    if let Some(st) = status {
        use std::os::unix::process::ExitStatusExt;
        if st.signal().is_some() {
            result["signal"] = json!(st.signal());
        }
    }
    let _ = cfg;
    Ok(ToolOutput::json(result))
}

async fn kill_tree(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    #[cfg(unix)]
    unsafe {
        // The child was spawned as its own process group leader.
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
    #[cfg(windows)]
    {
        let _ = tokio::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }
    let _ = pid;
}
