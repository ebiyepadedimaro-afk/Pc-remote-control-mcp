//! Agentic coding CLI orchestration: runs Claude Code, Codex, Gemini CLI,
//! Aider, opencode, Cursor CLI (or a custom command) as managed jobs and
//! exposes their output. Jobs run detached from HTTP requests, so long
//! development tasks can be polled.

use super::{arg_i64, arg_str, arg_u32, truncate_chars, which, ToolOutput};
use crate::state::AppState;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, BufReader};

struct AgentDef {
    name: &'static str,
    command: &'static str,
    /// Non-interactive invocation; `{prompt}` is replaced.
    prompt_args: &'static [&'static str],
    hint: &'static str,
}

const REGISTRY: &[AgentDef] = &[
    AgentDef {
        name: "claude",
        command: "claude",
        prompt_args: &["-p", "{prompt}", "--output-format", "text"],
        hint: "npm install -g @anthropic-ai/claude-code",
    },
    AgentDef {
        name: "codex",
        command: "codex",
        prompt_args: &["exec", "{prompt}"],
        hint: "npm install -g @openai/codex",
    },
    AgentDef {
        name: "gemini",
        command: "gemini",
        prompt_args: &["-p", "{prompt}"],
        hint: "npm install -g @google/gemini-cli",
    },
    AgentDef {
        name: "aider",
        command: "aider",
        prompt_args: &["--message", "{prompt}", "--yes", "--no-auto-commits"],
        hint: "pip install aider-chat",
    },
    AgentDef {
        name: "opencode",
        command: "opencode",
        prompt_args: &["run", "{prompt}"],
        hint: "https://opencode.ai (install script)",
    },
    AgentDef {
        name: "cursor-agent",
        command: "cursor-agent",
        prompt_args: &["-p", "{prompt}"],
        hint: "Cursor CLI (cursor.com/cli)",
    },
    AgentDef {
        name: "copilot",
        command: "copilot",
        prompt_args: &["-p", "{prompt}"],
        hint: "npm install -g @github/copilot",
    },
];

fn canonical(name: &str) -> String {
    match name.trim().to_ascii_lowercase().as_str() {
        "claude-code" | "claudecode" | "claude_code" => "claude".into(),
        "open-code" | "open_code" => "opencode".into(),
        "cursor" => "cursor-agent".into(),
        "google-gemini" => "gemini".into(),
        "github-copilot" => "copilot".into(),
        other => other.to_string(),
    }
}

fn def(name: &str) -> Option<&'static AgentDef> {
    let c = canonical(name);
    REGISTRY.iter().find(|d| d.name == c)
}

pub fn detect_summary(state: &AppState) -> Value {
    let cfg = state.config.get();
    let agents: Vec<Value> = REGISTRY
        .iter()
        .map(|d| {
            let path = which(d.command);
            json!({
                "name": d.name,
                "command": d.command,
                "installed": path.is_some(),
                "path": path.map(|p| p.to_string_lossy().to_string()),
                "prompt_args": d.prompt_args,
                "install": d.hint,
            })
        })
        .collect();
    json!({
        "enabled": cfg.agents.enabled,
        "max_jobs": cfg.agents.max_jobs,
        "max_runtime_ms": cfg.agents.max_runtime_ms,
        "agents": agents,
    })
}

struct Job {
    id: String,
    agent: String,
    command: String,
    cwd: String,
    prompt: String,
    started_ms: u64,
    status: String,
    exit_code: Option<i32>,
    error: Option<String>,
    pid: Option<u32>,
    stdout: String,
    stderr: String,
}

type JobHandle = Arc<Mutex<Job>>;

static JOBS: OnceLock<Mutex<HashMap<String, JobHandle>>> = OnceLock::new();

fn jobs() -> &'static Mutex<HashMap<String, JobHandle>> {
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn push_capped(dst: &mut String, chunk: &str, cap: usize) {
    dst.push_str(chunk);
    if dst.len() > cap {
        let mut cut = dst.len() - cap;
        while !dst.is_char_boundary(cut) {
            cut += 1;
        }
        dst.drain(..cut);
    }
}

fn kill_tree(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
        libc::kill(pid as i32, libc::SIGKILL);
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

fn get_job(id: &str) -> Result<JobHandle, String> {
    jobs()
        .lock()
        .unwrap()
        .get(id)
        .cloned()
        .ok_or_else(|| format!("unknown job id: {id}"))
}

fn snapshot(job: &JobHandle, max_chars: usize) -> Value {
    let g = job.lock().unwrap();
    let (out, out_trunc) = truncate_chars(&g.stdout, max_chars);
    let (err, err_trunc) = truncate_chars(&g.stderr, max_chars);
    json!({
        "job_id": g.id,
        "agent": g.agent,
        "status": g.status,
        "exit_code": g.exit_code,
        "error": g.error,
        "pid": g.pid,
        "command": g.command,
        "cwd": g.cwd,
        "prompt": g.prompt.chars().take(500).collect::<String>(),
        "started_ms": g.started_ms,
        "duration_ms": now_ms().saturating_sub(g.started_ms),
        "stdout": out,
        "stderr": err,
        "stdout_truncated": out_trunc,
        "stderr_truncated": err_trunc,
    })
}

async fn spawn_job(
    state: &Arc<AppState>,
    agent_name: &str,
    prompt: &str,
    cwd_raw: Option<&str>,
    args_override: &[String],
    command_override: Option<&str>,
    timeout_ms: Option<u64>,
) -> Result<String, String> {
    let cfg = state.config.get();
    if !cfg.agents.enabled {
        return Err("agent CLIs are disabled in the server configuration (agents.enabled)".into());
    }
    {
        let map = jobs().lock().unwrap();
        let running = map
            .values()
            .filter(|j| {
                let g = j.lock().unwrap();
                g.status == "running" || g.status == "stopping"
            })
            .count();
        if running >= cfg.agents.max_jobs {
            return Err(format!(
                "too many running agent jobs ({} of {}); stop one or raise agents.max_jobs",
                running, cfg.agents.max_jobs
            ));
        }
    }

    let agent = canonical(agent_name);
    let d = def(&agent);
    if d.is_none() && command_override.is_none() {
        let installed = REGISTRY
            .iter()
            .filter_map(|a| which(a.command).map(|_| a.name))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "unknown agent `{agent_name}`. Supported: {}. Installed here: {}. For anything else pass agent=\"custom\" with command and args.",
            REGISTRY.iter().map(|a| a.name).collect::<Vec<_>>().join(", "),
            if installed.is_empty() { "none".to_string() } else { installed }
        ));
    }
    let command = command_override
        .map(str::to_string)
        .or_else(|| d.map(|x| x.command.to_string()))
        .ok_or("no command resolved for the agent")?;
    let program = which(&command)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| command.clone());

    let mut args: Vec<String> = if !args_override.is_empty() {
        let mut v: Vec<String> = args_override.iter().map(|a| a.replace("{prompt}", prompt)).collect();
        if !args_override.iter().any(|a| a.contains("{prompt}")) {
            v.push(prompt.to_string());
        }
        v
    } else if let Some(def) = d {
        def.prompt_args.iter().map(|a| a.replace("{prompt}", prompt)).collect()
    } else {
        vec![prompt.to_string()]
    };
    if args.is_empty() {
        args.push(prompt.to_string());
    }

    let cwd = cwd_raw
        .map(super::expand_path)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| dirs::home_dir().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| ".".into()));
    if !std::path::Path::new(&cwd).is_dir() {
        return Err(format!("working directory does not exist: {cwd}"));
    }

    let mut cmd = tokio::process::Command::new(&program);
    cmd.args(&args)
        .current_dir(&cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    cmd.process_group(0);

    let mut child = cmd.spawn().map_err(|e| {
        format!(
            "cannot start {program}: {e}{}",
            d.map(|x| format!(" (install: {})", x.hint)).unwrap_or_default()
        )
    })?;
    let pid = child.id();
    let stdout = child.stdout.take().ok_or("cannot capture agent stdout")?;
    let stderr = child.stderr.take().ok_or("cannot capture agent stderr")?;

    let id = uuid::Uuid::new_v4().to_string();
    let job: JobHandle = Arc::new(Mutex::new(Job {
        id: id.clone(),
        agent: d.map(|x| x.name.to_string()).unwrap_or_else(|| agent.clone()),
        command: format!("{program} {}", args.join(" ")),
        cwd,
        prompt: prompt.to_string(),
        started_ms: now_ms(),
        status: "running".into(),
        exit_code: None,
        error: None,
        pid,
        stdout: String::new(),
        stderr: String::new(),
    }));
    jobs().lock().unwrap().insert(id.clone(), job.clone());

    let j = job.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let mut g = j.lock().unwrap();
            push_capped(&mut g.stdout, &format!("{line}\n"), 512 * 1024);
        }
    });
    let j = job.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let mut g = j.lock().unwrap();
            push_capped(&mut g.stderr, &format!("{line}\n"), 512 * 1024);
        }
    });

    let max_runtime = timeout_ms.unwrap_or(cfg.agents.max_runtime_ms).clamp(1000, cfg.agents.max_runtime_ms);
    let j = job.clone();
    tokio::spawn(async move {
        let mut child = child;
        match tokio::time::timeout(Duration::from_millis(max_runtime), child.wait()).await {
            Ok(Ok(status)) => {
                let mut g = j.lock().unwrap();
                if g.status == "stopping" {
                    g.status = "stopped".into();
                } else {
                    g.exit_code = status.code();
                    g.status = if status.success() { "done".into() } else { "failed".into() };
                }
            }
            Ok(Err(e)) => {
                let mut g = j.lock().unwrap();
                g.status = "failed".into();
                g.error = Some(e.to_string());
            }
            Err(_) => {
                kill_tree(pid);
                let _ = child.kill().await;
                let _ = child.wait().await;
                let mut g = j.lock().unwrap();
                g.status = "timeout".into();
                g.error = Some(format!("runtime exceeded {max_runtime} ms"));
            }
        }
    });

    Ok(id)
}

pub async fn detect(state: Arc<AppState>, _args: Value) -> Result<ToolOutput, String> {
    Ok(ToolOutput::json(detect_summary(&state)))
}

pub async fn list(state: Arc<AppState>, _args: Value) -> Result<ToolOutput, String> {
    let _ = state;
    let map = jobs().lock().unwrap();
    let mut rows: Vec<Value> = map
        .values()
        .map(|j| {
            let g = j.lock().unwrap();
            json!({
                "job_id": g.id,
                "agent": g.agent,
                "status": g.status,
                "exit_code": g.exit_code,
                "cwd": g.cwd,
                "started_ms": g.started_ms,
                "duration_ms": now_ms().saturating_sub(g.started_ms),
            })
        })
        .collect();
    rows.sort_by_key(|r| r.get("started_ms").and_then(|v| v.as_u64()).unwrap_or(0));
    rows.reverse();
    Ok(ToolOutput::json(json!({"count": rows.len(), "jobs": rows})))
}

pub async fn start(state: Arc<AppState>, args: Value) -> Result<ToolOutput, String> {
    let agent = arg_str(&args, "agent").ok_or("agent is required (or agent=\"custom\" with command)")?;
    let prompt = arg_str(&args, "prompt").ok_or("prompt is required")?;
    let cwd = arg_str(&args, "cwd");
    let args_override = super::arg_strings(&args, "args");
    let command = arg_str(&args, "command");
    let timeout = arg_i64(&args, "timeout_ms").map(|t| t.max(1000) as u64);
    let id = spawn_job(&state, agent, prompt, cwd, &args_override, command, timeout).await?;
    let job = get_job(&id)?;
    let mut out = snapshot(&job, 4000);
    out["running"] = json!(true);
    Ok(ToolOutput::json(out))
}

pub async fn status(_state: Arc<AppState>, args: Value) -> Result<ToolOutput, String> {
    let id = arg_str(&args, "job_id").ok_or("job_id is required")?;
    let max = arg_u32(&args, "max_chars").unwrap_or(20_000).clamp(100, 2_000_000) as usize;
    let job = get_job(id)?;
    Ok(ToolOutput::json(snapshot(&job, max)))
}

pub async fn stop(_state: Arc<AppState>, args: Value) -> Result<ToolOutput, String> {
    let id = arg_str(&args, "job_id").ok_or("job_id is required")?;
    let job = get_job(id)?;
    {
        let mut g = job.lock().unwrap();
        if g.status != "running" && g.status != "stopping" {
            return Ok(ToolOutput::json(json!({"ok": true, "job_id": id, "status": g.status, "note": "already finished"})));
        }
        g.status = "stopping".into();
    }
    let pid = job.lock().unwrap().pid;
    kill_tree(pid);
    tokio::time::sleep(Duration::from_millis(400)).await;
    Ok(ToolOutput::json(snapshot(&job, 4000)))
}

pub async fn run(state: Arc<AppState>, args: Value) -> Result<ToolOutput, String> {
    let agent = arg_str(&args, "agent").ok_or("agent is required")?;
    let prompt = arg_str(&args, "prompt").ok_or("prompt is required")?;
    let cwd = arg_str(&args, "cwd");
    let args_override = super::arg_strings(&args, "args");
    let command = arg_str(&args, "command");
    let timeout = arg_i64(&args, "timeout_ms").map(|t| t.max(1000) as u64);
    let id = spawn_job(&state, agent, prompt, cwd, &args_override, command, timeout).await?;
    let job = get_job(&id)?;

    let wait_ms = arg_i64(&args, "wait_ms").unwrap_or(300_000).clamp(1000, 3_600_000) as u64;
    let deadline = tokio::time::Instant::now() + Duration::from_millis(wait_ms);
    loop {
        let status = job.lock().unwrap().status.clone();
        if status != "running" && status != "stopping" {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let max = arg_u32(&args, "max_chars").unwrap_or(20_000).clamp(100, 2_000_000) as usize;
    let mut out = snapshot(&job, max);
    out["waited_ms"] = json!(wait_ms);
    out["still_running"] = json!(out.get("status").and_then(|s| s.as_str()) == Some("running"));
    Ok(ToolOutput::json(out))
}
