//! Tiny logger: every line goes to the console (stdout, or stderr in stdio
//! mode where stdout carries the MCP protocol) and into a ring buffer the
//! desktop UI reads through `/admin/logs`.

use serde::Serialize;
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub seq: u64,
    /// Unix epoch milliseconds.
    pub ts: u64,
    pub level: String,
    pub msg: String,
}

struct Ring {
    entries: VecDeque<LogEntry>,
    cap: usize,
    next_seq: u64,
    to_stderr: bool,
}

static RING: OnceLock<Mutex<Ring>> = OnceLock::new();

pub fn init(cap: usize, to_stderr: bool) {
    let _ = RING.set(Mutex::new(Ring {
        entries: VecDeque::with_capacity(cap.min(1024)),
        cap: cap.max(50),
        next_seq: 1,
        to_stderr,
    }));
}

fn ring() -> &'static Mutex<Ring> {
    RING.get_or_init(|| {
        Mutex::new(Ring { entries: VecDeque::new(), cap: 500, next_seq: 1, to_stderr: false })
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn hhmmss(ms: u64) -> String {
    // UTC clock time; the UI formats dates itself from `ts`.
    let secs = (ms / 1000) % 86_400;
    format!("{:02}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
}

pub fn log(level: &str, msg: impl AsRef<str>) {
    let msg = msg.as_ref();
    let ts = now_ms();
    let line = format!("[{}] {:5} {}", hhmmss(ts), level.to_ascii_uppercase(), msg);
    let mut r = ring().lock().unwrap();
    if r.to_stderr {
        eprintln!("{line}");
    } else {
        println!("{line}");
    }
    let seq = r.next_seq;
    r.next_seq += 1;
    r.entries.push_back(LogEntry {
        seq,
        ts,
        level: level.to_ascii_uppercase(),
        msg: msg.to_string(),
    });
    while r.entries.len() > r.cap {
        r.entries.pop_front();
    }
}

pub fn info(msg: impl AsRef<str>) {
    log("info", msg);
}

pub fn warn(msg: impl AsRef<str>) {
    log("warn", msg);
}

pub fn error(msg: impl AsRef<str>) {
    log("error", msg);
}

/// Entries after `since` (exclusive), at most `limit`, plus the newest seq.
pub fn snapshot(since: u64, limit: usize) -> (Vec<LogEntry>, u64) {
    let r = ring().lock().unwrap();
    let newest = r.next_seq.saturating_sub(1);
    let mut out: Vec<LogEntry> = r
        .entries
        .iter()
        .filter(|e| e.seq > since)
        .take(limit.max(1))
        .cloned()
        .collect();
    if out.len() > limit.max(1) {
        out.truncate(limit.max(1));
    }
    (out, newest)
}
