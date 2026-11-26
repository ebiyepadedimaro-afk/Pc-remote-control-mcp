export interface ScreenshotConfig {
  format: string;
  jpeg_quality: number;
  max_width: number;
  max_height: number;
  match_input_coordinates: boolean;
}

export interface ShellConfig {
  enabled: boolean;
  default_timeout_ms: number;
  max_timeout_ms: number;
  max_output_bytes: number;
  blocklist: string[];
}

export interface FilesConfig {
  root: string;
  allow_write: boolean;
  allow_delete: boolean;
  max_read_bytes: number;
  max_write_bytes: number;
}

export interface DownloadsConfig {
  enabled: boolean;
  max_bytes: number;
  timeout_ms: number;
  allow_private_hosts: boolean;
}

export interface InputConfig {
  backend: string;
}

export interface BrowserConfig {
  path: string;
  headless: boolean;
  window_width: number;
  window_height: number;
  profile_dir: string;
}

export interface DocsConfig {
  max_chars: number;
  soffice_path: string;
  pdftotext_path: string;
  tesseract_path: string;
}

export interface AgentsConfig {
  enabled: boolean;
  max_jobs: number;
  max_runtime_ms: number;
}

export interface SystemConfig {
  power_enabled: boolean;
  admin_enabled: boolean;
  terminal_enabled: boolean;
  tunnel_enabled: boolean;
  max_delay_secs: number;
}

export interface TunnelConfig {
  kind: string;
  auto_start: boolean;
  ngrok_authtoken: string;
  tailscale_socket: string;
}

export interface ToolsConfig {
  disabled: string[];
}

export interface Config {
  bind: string;
  port: number;
  token_enabled: boolean;
  token: string;
  tool_prefix: string;
  screenshot: ScreenshotConfig;
  shell: ShellConfig;
  files: FilesConfig;
  downloads: DownloadsConfig;
  input: InputConfig;
  browser: BrowserConfig;
  docs: DocsConfig;
  agents: AgentsConfig;
  system: SystemConfig;
  tunnel: TunnelConfig;
  tools: ToolsConfig;
  log_lines: number;
}

export interface Status {
  ok: boolean;
  name: string;
  version: string;
  platform: string;
  arch: string;
  hostname?: string | null;
  uptime_secs: number;
  now_ms: number;
  transport: string;
  mcp_url: string;
  mcp_url_local: string;
  admin_url: string;
  lan_urls: string[];
  bind: string;
  port: number;
  token_enabled: boolean;
  token: string | null;
  approval: string;
  tool_prefix: string;
  tools: { total: number; enabled: number; disabled: string[] };
  stats: {
    mcp_requests: number;
    tool_calls: number;
    tool_errors: number;
    last_client: string;
    last_tool: string;
    last_error: string;
  };
  session: Record<string, any>;
  backends: Record<string, any>;
  capabilities: Record<string, any>;
  tunnel: Record<string, any>;
  config_path: string;
  config: Config;
}

export interface LogEntry {
  seq: number;
  ts: number;
  level: string;
  msg: string;
}

export interface UiSettings {
  serverPath: string;
  port: number;
  autostart: boolean;
}

export interface ToolDef {
  name: string;
  description: string;
}
