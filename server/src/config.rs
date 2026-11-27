//! Persistent server configuration. Kept in one JSON file the desktop UI edits
//! (directly or through the loopback admin API) and the server owns on disk.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

pub const APP_DIR: &str = "pc-remote-control-mcp";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Address to bind. `127.0.0.1` keeps the server on the machine.
    pub bind: String,
    pub port: u16,
    /// When false (default) every MCP connection is approved: no token is asked.
    pub token_enabled: bool,
    /// Bearer token used when `token_enabled` is true.
    pub token: String,
    /// Tool name prefix, without trailing underscore. Default "pc" -> `pc_screenshot`.
    pub tool_prefix: String,
    pub screenshot: ScreenshotConfig,
    pub shell: ShellConfig,
    pub files: FilesConfig,
    pub downloads: DownloadConfig,
    pub input: InputConfig,
    pub browser: BrowserConfig,
    pub docs: DocsConfig,
    pub agents: AgentsConfig,
    pub system: SystemConfig,
    pub tunnel: TunnelConfig,
    pub tools: ToolsConfig,
    pub log_lines: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScreenshotConfig {
    /// "png" or "jpeg".
    pub format: String,
    pub jpeg_quality: u8,
    /// Downscale so the width fits (0 = no limit).
    pub max_width: u32,
    /// Downscale so the height fits (0 = no limit).
    pub max_height: u32,
    /// Resize captures to the session's coordinate space so a pixel in the
    /// screenshot is a pixel the input tools understand (HiDPI friendly).
    pub match_input_coordinates: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ShellConfig {
    pub enabled: bool,
    pub default_timeout_ms: u64,
    pub max_timeout_ms: u64,
    pub max_output_bytes: usize,
    /// Case-insensitive substrings refused before execution.
    pub blocklist: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FilesConfig {
    /// Restrict file tools to this directory (empty = the whole filesystem).
    pub root: String,
    /// Folders file tools may never touch, even when root is empty
    /// (e.g. ~/.ssh, ~/.aws). Paths starting with ~ are expanded.
    pub protected: Vec<String>,
    pub allow_write: bool,
    pub allow_delete: bool,
    pub max_read_bytes: u64,
    pub max_write_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadConfig {
    pub enabled: bool,
    pub max_bytes: u64,
    pub timeout_ms: u64,
    /// Allow URLs that resolve to loopback/private/link-local addresses.
    pub allow_private_hosts: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolsConfig {
    /// Base tool names (without prefix) that are hidden and refused.
    pub disabled: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InputConfig {
    /// Linux: "auto", "x11" or "uinput". Ignored on macOS/Windows.
    pub backend: String,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self { backend: "auto".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BrowserConfig {
    /// Chromium-based browser executable (empty = auto-detect Chrome/Chromium/Edge/Brave).
    pub path: String,
    /// Start the automation browser headless by default.
    pub headless: bool,
    pub window_width: u32,
    pub window_height: u32,
    /// User data directory (empty = <data dir>/pc-remote-control-mcp/browser-profile).
    pub profile_dir: String,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            path: String::new(),
            headless: false,
            window_width: 1280,
            window_height: 900,
            profile_dir: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DocsConfig {
    pub max_chars: usize,
    /// Empty = auto-detect on PATH.
    pub soffice_path: String,
    pub pdftotext_path: String,
    pub tesseract_path: String,
}

impl Default for DocsConfig {
    fn default() -> Self {
        Self { max_chars: 200_000, soffice_path: String::new(), pdftotext_path: String::new(), tesseract_path: String::new() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentsConfig {
    /// Allow the coding-agent tools to spawn CLIs.
    pub enabled: bool,
    pub max_jobs: usize,
    pub max_runtime_ms: u64,
}

impl Default for AgentsConfig {
    fn default() -> Self {
        Self { enabled: true, max_jobs: 4, max_runtime_ms: 1_800_000 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SystemConfig {
    /// Allow shutdown/restart/hibernate/suspend/logout.
    pub power_enabled: bool,
    /// Allow elevated command execution (sudo -n / osascript admin / UAC).
    pub admin_enabled: bool,
    /// Allow opening a visible terminal window with a command.
    pub terminal_enabled: bool,
    /// Allow starting a remote-access tunnel (requires bearer-token auth).
    pub tunnel_enabled: bool,
    pub max_delay_secs: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TunnelConfig {
    /// auto | cloudflared | tailscale | ngrok
    pub kind: String,
    /// Start a tunnel automatically when the server starts (needs the bearer token).
    pub auto_start: bool,
    /// ngrok agent authtoken (dashboard.ngrok.com). Empty = use ngrok's own config.
    pub ngrok_authtoken: String,
    /// Optional `tailscaled` socket for a bundled/userspace daemon.
    pub tailscale_socket: String,
    /// Cloudflare named-tunnel token (Zero Trust dashboard) for "Your Domain".
    pub cloudflare_token: String,
    /// The public hostname mapped to that tunnel (e.g. mcp.example.com).
    pub cloudflare_hostname: String,
}

impl Default for TunnelConfig {
    fn default() -> Self {
        Self {
            kind: "auto".into(),
            auto_start: false,
            ngrok_authtoken: String::new(),
            tailscale_socket: String::new(),
            cloudflare_token: String::new(),
            cloudflare_hostname: String::new(),
        }
    }
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self { power_enabled: true, admin_enabled: true, terminal_enabled: true, tunnel_enabled: true, max_delay_secs: 3600 }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1".into(),
            port: 8080,
            token_enabled: false,
            token: uuid::Uuid::new_v4().to_string(),
            tool_prefix: "pc".into(),
            screenshot: ScreenshotConfig::default(),
            shell: ShellConfig::default(),
            files: FilesConfig::default(),
            downloads: DownloadConfig::default(),
            input: InputConfig::default(),
            browser: BrowserConfig::default(),
            docs: DocsConfig::default(),
            agents: AgentsConfig::default(),
            system: SystemConfig::default(),
            tunnel: TunnelConfig::default(),
            tools: ToolsConfig::default(),
            log_lines: 500,
        }
    }
}

impl Default for ScreenshotConfig {
    fn default() -> Self {
        Self {
            format: "png".into(),
            jpeg_quality: 80,
            max_width: 0,
            max_height: 0,
            match_input_coordinates: true,
        }
    }
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_timeout_ms: 30_000,
            max_timeout_ms: 300_000,
            max_output_bytes: 256 * 1024,
            blocklist: vec![
                ":(){:|:&};:".into(),
                "rm -rf /".into(),
                "mkfs.".into(),
                "dd if=/dev/zero of=/dev/sd".into(),
            ],
        }
    }
}

impl Default for FilesConfig {
    fn default() -> Self {
        Self {
            root: String::new(),
            protected: vec![
                "~/.ssh".into(),
                "~/.aws".into(),
                "~/.gnupg".into(),
                "~/.kube".into(),
                "~/.config/gcloud".into(),
                "~/.azure".into(),
                "~/.docker".into(),
                "~/.netrc".into(),
                "~/.vault-token".into(),
                "~/Library/Keychains".into(),
            ],
            allow_write: true,
            allow_delete: true,
            max_read_bytes: 10 * 1024 * 1024,
            max_write_bytes: 10 * 1024 * 1024,
        }
    }
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_bytes: 100 * 1024 * 1024,
            timeout_ms: 120_000,
            allow_private_hosts: false,
        }
    }
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self { disabled: Vec::new() }
    }
}

impl Config {
    /// Clamp values so a hand-edited or hostile config can't break the server.
    pub fn sanitize(&mut self) {
        self.bind = self.bind.trim().to_string();
        if self.bind.parse::<std::net::IpAddr>().is_err() {
            self.bind = "127.0.0.1".into();
        }
        if self.port == 0 {
            self.port = 8080;
        }
        self.tool_prefix = self
            .tool_prefix
            .trim()
            .trim_end_matches('_')
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if self.tool_prefix.len() > 24 {
            self.tool_prefix.truncate(24);
        }
        let f = self.screenshot.format.to_ascii_lowercase();
        self.screenshot.format = if f == "jpeg" || f == "jpg" { "jpeg".into() } else { "png".into() };
        self.screenshot.jpeg_quality = self.screenshot.jpeg_quality.clamp(1, 100);
        self.shell.default_timeout_ms = self.shell.default_timeout_ms.clamp(100, 600_000);
        self.shell.max_timeout_ms = self.shell.max_timeout_ms.clamp(self.shell.default_timeout_ms, 600_000);
        self.shell.max_output_bytes = self.shell.max_output_bytes.clamp(1024, 16 * 1024 * 1024);
        self.files.max_read_bytes = self.files.max_read_bytes.clamp(1024, 512 * 1024 * 1024);
        self.files.max_write_bytes = self.files.max_write_bytes.clamp(1024, 512 * 1024 * 1024);
        self.downloads.max_bytes = self.downloads.max_bytes.clamp(1024, 4 * 1024 * 1024 * 1024);
        self.downloads.timeout_ms = self.downloads.timeout_ms.clamp(1000, 3_600_000);
        self.log_lines = self.log_lines.clamp(50, 10_000);
        self.input.backend = match self.input.backend.to_ascii_lowercase().as_str() {
            "x11" => "x11".into(),
            "uinput" => "uinput".into(),
            _ => "auto".into(),
        };
        self.browser.path = self.browser.path.trim().to_string();
        self.browser.profile_dir = self.browser.profile_dir.trim().to_string();
        self.browser.window_width = self.browser.window_width.clamp(320, 7680);
        self.browser.window_height = self.browser.window_height.clamp(240, 4320);
        self.docs.max_chars = self.docs.max_chars.clamp(1_000, 5_000_000);
        self.agents.max_jobs = self.agents.max_jobs.clamp(1, 16);
        self.agents.max_runtime_ms = self.agents.max_runtime_ms.clamp(10_000, 86_400_000);
        self.system.max_delay_secs = self.system.max_delay_secs.clamp(0, 86_400);
        self.tunnel.ngrok_authtoken = self.tunnel.ngrok_authtoken.trim().to_string();
        self.tunnel.tailscale_socket = self.tunnel.tailscale_socket.trim().to_string();
        self.tunnel.cloudflare_token = self.tunnel.cloudflare_token.trim().to_string();
        self.tunnel.cloudflare_hostname = self
            .tunnel
            .cloudflare_hostname
            .trim()
            .trim_start_matches("https://")
            .trim_end_matches('/')
            .to_string();
        self.tunnel.kind = match self.tunnel.kind.trim().to_ascii_lowercase().as_str() {
            "cloudflared" => "cloudflared".into(),
            "tailscale" => "tailscale".into(),
            "ngrok" => "ngrok".into(),
            _ => "auto".into(),
        };
        if self.token.trim().is_empty() {
            self.token = uuid::Uuid::new_v4().to_string();
        }
        self.files.root = self.files.root.trim().to_string();
    }

    /// Prefix including the separator, e.g. "pc_".
    pub fn prefix(&self) -> String {
        if self.tool_prefix.is_empty() {
            String::new()
        } else {
            format!("{}_", self.tool_prefix)
        }
    }

    pub fn tool_enabled(&self, base_name: &str) -> bool {
        !self.tools.disabled.iter().any(|d| d == base_name)
    }
}

/// Default config path: ~/.config/pc-remote-control-mcp/config.json
pub fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR)
        .join("config.json")
}

pub struct ConfigHandle {
    path: PathBuf,
    inner: RwLock<Config>,
}

impl ConfigHandle {
    /// Loads the config, creating it with defaults when missing.
    pub fn load(path: PathBuf) -> anyhow::Result<Arc<Self>> {
        let mut cfg = if path.exists() {
            let text = std::fs::read_to_string(&path)?;
            serde_json::from_str::<Config>(&text)?
        } else {
            Config::default()
        };
        cfg.sanitize();
        let created = !path.exists();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if created {
            std::fs::write(&path, serde_json::to_string_pretty(&cfg)?)?;
            secure_permissions(&path);
        }
        Ok(Arc::new(Self { path, inner: RwLock::new(cfg) }))
    }

    pub fn get(&self) -> Config {
        self.inner.read().unwrap().clone()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Replaces the config in memory and persists it atomically.
    pub fn set(&self, mut cfg: Config) -> anyhow::Result<Config> {
        cfg.sanitize();
        let text = serde_json::to_string_pretty(&cfg)?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &text)?;
        secure_permissions(&tmp);
        std::fs::rename(&tmp, &self.path)?;
        *self.inner.write().unwrap() = cfg.clone();
        Ok(cfg)
    }
}

/// The config holds the bearer token and the ngrok authtoken: owner-only.
fn secure_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}
