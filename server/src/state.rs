//! Shared server state handed to every MCP tool.

use crate::config::ConfigHandle;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Default)]
pub struct Stats {
    pub mcp_requests: AtomicU64,
    pub tool_calls: AtomicU64,
    pub tool_errors: AtomicU64,
    pub last_client: Mutex<String>,
    pub last_tool: Mutex<String>,
    pub last_error: Mutex<String>,
}

impl Stats {
    pub fn bump_requests(&self, client: &str) {
        self.mcp_requests.fetch_add(1, Ordering::Relaxed);
        if !client.is_empty() {
            *self.last_client.lock().unwrap() = client.to_string();
        }
    }

    pub fn bump_tool(&self, name: &str) {
        self.tool_calls.fetch_add(1, Ordering::Relaxed);
        *self.last_tool.lock().unwrap() = name.to_string();
    }

    pub fn bump_error(&self, msg: &str) {
        self.tool_errors.fetch_add(1, Ordering::Relaxed);
        *self.last_error.lock().unwrap() = msg.chars().take(300).collect();
    }

    pub fn counts(&self) -> (u64, u64, u64) {
        (
            self.mcp_requests.load(Ordering::Relaxed),
            self.tool_calls.load(Ordering::Relaxed),
            self.tool_errors.load(Ordering::Relaxed),
        )
    }
}

pub struct AppState {
    pub config: Arc<ConfigHandle>,
    pub started: Instant,
    pub stats: Stats,
    pub http: reqwest::Client,
    pub version: &'static str,
    pub stdio_mode: bool,
    /// Effective bind address (CLI overrides included).
    pub bind: String,
    /// Effective port (CLI overrides included).
    pub port: u16,
    /// Asked to shut down (admin API / desktop app).
    pub shutdown: tokio::sync::Notify,
}

impl AppState {
    pub fn new(config: Arc<ConfigHandle>, stdio_mode: bool, bind: String, port: u16) -> Arc<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("pcrc-server/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .unwrap_or_default();
        Arc::new(Self {
            config,
            started: Instant::now(),
            stats: Stats::default(),
            http,
            version: env!("CARGO_PKG_VERSION"),
            stdio_mode,
            bind,
            port,
            shutdown: tokio::sync::Notify::new(),
        })
    }

    pub fn uptime_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }
}
