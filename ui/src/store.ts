//! App state and actions: server process control, admin API polling, settings.

import { Clipboard, signal } from "nsh";
import * as fsp from "fs/promises";
import type { Config, LogEntry, Status, ToolDef, UiSettings } from "./types";

export type Phase = "unknown" | "stopped" | "starting" | "running" | "external";

const DATA_DIR: string = process.dataDir ?? ".";
const SETTINGS_PATH = `${DATA_DIR}/ui-settings.json`;

export const settings = signal<UiSettings>({ serverPath: "", port: 8080, autostart: true });
export const phase = signal<Phase>("unknown");
export const status = signal<Status | null>(null);
export const cfg = signal<Config | null>(null);
export const dirty = signal(false);
export const busy = signal(false);
export const managed = signal(false);
export const port = signal<number>(8080);
export const logs = signal<LogEntry[]>([]);
export const tools = signal<ToolDef[]>([]);
export const notice = signal<{ kind: string; text: string } | null>(null);
export const updateInfo = signal<any>(null);
export const updateDismissed = signal<string>("");

let child: any = null;
let lastLogSeq = 0;
let timer: number | undefined;
let noticeTimer: number | undefined;
let inited = false;
let lastUpdateCheck = 0;
let uiSeq = -1;

export function baseUrl(): string {
  return `http://127.0.0.1:${port()}`;
}

export function mcpUrl(): string {
  const s = status();
  if (s && s.mcp_url) return s.mcp_url;
  return `${baseUrl()}/mcp`;
}

export function errText(e: any): string {
  return e && e.message ? String(e.message) : String(e);
}

export function notify(kind: string, text: string, ms = 5000): void {
  notice.set({ kind, text });
  if (noticeTimer !== undefined) clearTimeout(noticeTimer);
  noticeTimer = setTimeout(() => notice.set(null), ms) as unknown as number;
}

export function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export function addLog(level: string, msg: string): void {
  uiSeq -= 1;
  const entry: LogEntry = { seq: uiSeq, ts: Date.now(), level, msg };
  logs.update((list) => {
    const next = list.concat(entry);
    return next.length > 3000 ? next.slice(next.length - 3000) : next;
  });
}

async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(baseUrl() + path, init);
  const text = await res.text();
  if (!res.ok) throw new Error(`HTTP ${res.status}: ${text.slice(0, 300)}`);
  return (text ? JSON.parse(text) : null) as T;
}

export async function loadSettings(): Promise<void> {
  try {
    const raw = await fsp.readFile(SETTINGS_PATH, "utf8");
    const s = JSON.parse(raw) as Partial<UiSettings>;
    settings.set({ serverPath: s.serverPath ?? "", port: s.port ?? 8080, autostart: s.autostart ?? true });
  } catch {
    // first run
  }
  port.set(settings().port);
}

export async function saveSettings(patch: Partial<UiSettings>): Promise<void> {
  settings.set({ ...settings(), ...patch });
  if (patch.port) port.set(patch.port);
  try {
    await fsp.writeFile(SETTINGS_PATH, JSON.stringify(settings(), null, 2));
  } catch (e) {
    addLog("WARN", `cannot save UI settings: ${errText(e)}`);
  }
}

export async function detectBinary(): Promise<string> {
  const explicit = settings().serverPath.trim();
  if (explicit !== "") return explicit;
  const home = (await import("os")).homedir();
  // Standard install locations are on PATH and granted by executable name.
  const onPath = [`${home}/.local/bin/pcrc-server`, "/usr/local/bin/pcrc-server", "/usr/bin/pcrc-server"];
  for (const candidate of onPath) {
    try {
      if (await fsp.exists(candidate)) return "pcrc-server";
    } catch {
      // not readable/allowed; try the next one
    }
  }
  // App-bundle resources are not on PATH and are granted as absolute paths.
  const bundled = [
    "/Applications/PC Remote Control.app/Contents/Resources/pcrc-server",
    `${home}/Applications/PC Remote Control.app/Contents/Resources/pcrc-server`,
  ];
  for (const candidate of bundled) {
    try {
      if (await fsp.exists(candidate)) return candidate;
    } catch {
      // try the next one
    }
  }
  return "pcrc-server";
}

export async function refresh(): Promise<boolean> {
  try {
    const s = await api<Status>("/admin/status");
    status.set(s);
    if (!dirty()) cfg.set(s.config);
    if (s.port !== port()) port.set(s.port);
    phase.set(managed() ? "running" : "external");
    return true;
  } catch {
    status.set(null);
    if (child === null && phase() !== "starting") phase.set("stopped");
    return false;
  }
}

async function pullLogs(): Promise<void> {
  try {
    const res = await api<{ entries: LogEntry[]; newest: number }>(`/admin/logs?since=${lastLogSeq}&limit=300`);
    if (res.entries && res.entries.length > 0) {
      for (const e of res.entries) {
        if (e.seq > lastLogSeq) lastLogSeq = e.seq;
      }
      logs.update((list) => {
        const next = list.concat(res.entries);
        return next.length > 3000 ? next.slice(next.length - 3000) : next;
      });
    }
  } catch {
    // server down
  }
}

async function tick(): Promise<void> {
  if (await refresh()) {
    await pullLogs();
    if (tools().length === 0) await loadTools();
    await checkUpdates();
  }
}

export async function checkUpdates(force = false): Promise<void> {
  if (!status()) return;
  const now = Date.now();
  if (!force && now - lastUpdateCheck < 30 * 60_000) return;
  lastUpdateCheck = now;
  try {
    const info = await api<any>("/admin/update");
    updateInfo.set(info);
  } catch {
    // offline or GitHub unreachable
  }
}

export async function applyUpdate(): Promise<void> {
  busy.set(true);
  try {
    notify("info", "Downloading the update...", 20000);
    const res = await api<any>("/admin/update/apply", { method: "POST" });
    if (res && res.staged) {
      notify("info", "Update downloaded; restarting the server...", 20000);
      await stop();
      await sleep(800);
      await start();
      await checkUpdates(true);
      const u = updateInfo();
      notify("success", u && u.latest ? `Updated to v${u.latest}.` : "Update complete.", 9000);
    } else {
      notify("error", "The update could not be applied.", 9000);
    }
  } catch (e) {
    notify("error", `Update failed: ${errText(e)}`, 14000);
    addLog("ERROR", `update apply failed: ${errText(e)}`);
  } finally {
    busy.set(false);
  }
}

export async function openUpdate(): Promise<void> {
  try {
    await api("/admin/update/open", { method: "POST" });
  } catch (e) {
    notify("error", `Cannot open the update page: ${errText(e)}`, 9000);
  }
}

export async function init(): Promise<void> {
  if (inited) return;
  inited = true;
  await loadSettings();
  addLog("UI", "desktop app ready");
  const up = await refresh();
  if (up) {
    await pullLogs();
    await loadTools();
    await checkUpdates(true);
  } else if (settings().autostart) {
    await start();
  }
  if (timer === undefined) {
    timer = setInterval(() => {
      void tick();
    }, 1500) as unknown as number;
  }
}

export async function start(): Promise<void> {
  if (phase() === "running" || phase() === "external" || phase() === "starting") {
    notify("info", "The server is already running.");
    return;
  }
  busy.set(true);
  phase.set("starting");
  notify("info", "Starting the server...");
  try {
    const bin = await detectBinary();
    addLog("UI", `spawning ${bin}`);
    const proc = process.spawn(bin, [], { lines: true });
    child = proc;
    managed.set(true);
    proc.on("stdout", (line: string) => addLog("PROC", line));
    proc.on("stderr", (line: string) => addLog("PROC", line));
    proc.on("exit", (r: any) => {
      if (child === proc) child = null;
      managed.set(false);
      addLog("UI", `server process exited (code ${r && r.code !== null ? r.code : "?"})`);
      if (phase() === "running" || phase() === "external") phase.set("stopped");
      status.set(null);
    });
    let up = false;
    for (let i = 0; i < 60; i++) {
      await sleep(250);
      if (child === null) break;
      if (await refresh()) {
        up = true;
        break;
      }
    }
    if (up) {
      await pullLogs();
      await loadTools();
      notify("success", "Server is running.");
    } else if (child !== null) {
      phase.set("stopped");
      notify("error", "The server did not become ready. Check the Logs page.", 9000);
    }
  } catch (e: any) {
    child = null;
    managed.set(false);
    phase.set("stopped");
    const msg = errText(e);
    addLog("ERROR", msg);
    if (msg.includes("ERR_ACCESS_DENIED")) {
      notify(
        "error",
        "Cannot launch the binary: not granted. Install it on PATH as `pcrc-server` (e.g. ~/.local/bin) or add its absolute path to ui/nsh.json capabilities.process.execute.",
        12000,
      );
    } else {
      notify("error", `Cannot start the server: ${msg}`, 10000);
    }
  } finally {
    busy.set(false);
  }
}

export async function stop(): Promise<void> {
  if (!status() && child === null) {
    notify("info", "The server is not running.");
    return;
  }
  busy.set(true);
  try {
    if (status()) {
      await api("/admin/quit", { method: "POST" });
      addLog("UI", "shutdown requested");
    }
  } catch (e) {
    addLog("WARN", `admin shutdown failed: ${errText(e)}`);
  }
  await sleep(700);
  if (child !== null) {
    try {
      child.kill();
    } catch {
      // already gone
    }
  }
  await sleep(500);
  child = null;
  managed.set(false);
  status.set(null);
  phase.set("stopped");
  busy.set(false);
  notify("info", "Server stopped.");
}

export async function restart(): Promise<void> {
  await stop();
  await sleep(500);
  await start();
}

export async function saveConfig(): Promise<void> {
  const c = cfg();
  if (!c) return;
  busy.set(true);
  try {
    const res = await api<{ saved: boolean; restart_required: boolean; config: Config }>("/admin/config", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(c),
    });
    cfg.set(res.config);
    dirty.set(false);
    await refresh();
    if (res.restart_required) {
      notify("info", "Saved. Restart the server to apply the new bind address or port.", 9000);
    } else {
      notify("success", "Settings saved.");
    }
  } catch (e) {
    notify("error", `Save failed: ${errText(e)}`, 9000);
  } finally {
    busy.set(false);
  }
}

export async function regenerateToken(): Promise<void> {
  try {
    const res = await api<{ token: string }>("/admin/token", { method: "POST" });
    const c = cfg();
    if (c) cfg.set({ ...c, token: res.token });
    notify("success", "New bearer token generated.");
  } catch (e) {
    notify("error", `Cannot regenerate the token: ${errText(e)}`, 9000);
  }
}

export async function loadTools(): Promise<void> {
  try {
    const res = await api<any>("/mcp", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "tools/list", params: {} }),
    });
    const list: any[] = (res && res.result && res.result.tools) || [];
    tools.set(list.map((t) => ({ name: String(t.name), description: String(t.description ?? "") })));
  } catch {
    // server not running
  }
}

export async function copy(text: string, message = "Copied to clipboard"): Promise<void> {
  try {
    await Clipboard.writeText(text);
    notify("success", message);
  } catch (e) {
    notify("error", `Clipboard failed: ${errText(e)}`);
  }
}

export function patchConfig(patch: Partial<Config>): void {
  const c = cfg();
  if (!c) return;
  cfg.set({ ...c, ...patch });
  dirty.set(true);
}

export function patchSection<K extends keyof Config>(key: K, patch: Partial<Config[K]>): void {
  const c = cfg();
  if (!c) return;
  cfg.set({ ...c, [key]: { ...(c[key] as any), ...(patch as any) } } as Config);
  dirty.set(true);
}

export function toggleTool(name: string, enabled: boolean): void {
  const c = cfg();
  if (!c) return;
  const set = new Set(c.tools.disabled);
  if (enabled) set.delete(name);
  else set.add(name);
  patchConfig({ tools: { disabled: Array.from(set) } });
}

export function toggleToolGroup(names: string[], enabled: boolean): void {
  const c = cfg();
  if (!c) return;
  const set = new Set(c.tools.disabled);
  for (const n of names) {
    if (enabled) set.delete(n);
    else set.add(n);
  }
  patchConfig({ tools: { disabled: Array.from(set) } });
}

export function clearLogs(): void {
  logs.set([]);
}

export async function startTunnel(kind: string): Promise<void> {
  try {
    await api("/admin/tunnel", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ action: "start", kind }),
    });
    notify("info", "Starting the tunnel...");
    await sleep(1500);
    await refresh();
    const t = status()?.tunnel as any;
    if (t && t.url) notify("success", `Public URL: ${t.url}`, 14000);
    else if (t && t.error) notify("error", String(t.error), 12000);
  } catch (e) {
    const msg = errText(e);
    if (msg.includes("token_required")) {
      notify("error", "Enable the bearer token before exposing the server through a tunnel.", 10000);
    } else {
      notify("error", `Tunnel failed: ${msg}`, 12000);
    }
    addLog("WARN", `tunnel start failed: ${msg}`);
  }
}

export async function stopTunnel(): Promise<void> {
  try {
    await api("/admin/tunnel", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ action: "stop" }),
    });
    await refresh();
    notify("info", "Tunnel stopped.");
  } catch (e) {
    notify("error", `Cannot stop the tunnel: ${errText(e)}`, 9000);
  }
}
