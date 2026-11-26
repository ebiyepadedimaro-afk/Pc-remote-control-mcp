export function pad2(n: number): string {
  return String(n).padStart(2, "0");
}

export function formatTime(ms: number): string {
  const d = new Date(ms);
  return `${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}`;
}

export function formatUptime(secs: number): string {
  const s = Math.max(0, Math.floor(secs));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ${m % 60}m`;
  return `${Math.floor(h / 24)}d ${h % 24}h`;
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const kb = n / 1024;
  if (kb < 1024) return `${kb.toFixed(1)} KB`;
  const mb = kb / 1024;
  if (mb < 1024) return `${mb.toFixed(1)} MB`;
  return `${(mb / 1024).toFixed(2)} GB`;
}

export function phaseLabel(phase: string): string {
  switch (phase) {
    case "running":
      return "Running";
    case "external":
      return "Running (external)";
    case "starting":
      return "Starting...";
    case "stopped":
      return "Stopped";
    default:
      return "Unknown";
  }
}

export interface ToolGroup {
  key: string;
  label: string;
  names: string[];
}

export const TOOL_GROUPS: ToolGroup[] = [
  { key: "screen", label: "Screen", names: ["screenshot", "get_screen_state"] },
  { key: "mouse", label: "Mouse", names: ["mouse_move", "mouse_click", "mouse_scroll", "mouse_drag"] },
  { key: "keyboard", label: "Keyboard", names: ["key_press", "type_text"] },
  { key: "clipboard", label: "Clipboard", names: ["get_clipboard", "set_clipboard"] },
  {
    key: "files",
    label: "Files",
    names: [
      "list_directory",
      "read_file",
      "write_file",
      "delete_path",
      "move_path",
      "copy_path",
      "make_directory",
      "file_info",
      "download_from_url",
    ],
  },
  { key: "shell", label: "Shell", names: ["shell_exec"] },
  {
    key: "system",
    label: "Windows & processes",
    names: ["system_info", "list_processes", "kill_process", "list_windows", "focus_window", "lock_screen"],
  },
  { key: "apps", label: "Apps", names: ["open_url", "open_path", "launch_app", "notify"] },
  {
    key: "browser",
    label: "Browser & web",
    names: [
      "browser_open",
      "browser_navigate",
      "browser_get_content",
      "browser_click",
      "browser_type",
      "browser_screenshot",
      "browser_eval",
      "browser_tabs",
      "browser_close",
      "fetch_page",
    ],
  },
  { key: "docs", label: "Documents", names: ["doc_read", "doc_info", "doc_convert", "doc_ocr"] },
  {
    key: "agents",
    label: "Coding agents",
    names: ["agent_list", "agent_start", "agent_status", "agent_run", "agent_stop", "agent_jobs"],
  },
  { key: "power", label: "Power & administration", names: ["power", "admin_exec", "service", "terminal_run"] },
];

export function groupEnabled(disabled: string[], group: ToolGroup): boolean {
  return !group.names.some((n) => disabled.includes(n));
}
