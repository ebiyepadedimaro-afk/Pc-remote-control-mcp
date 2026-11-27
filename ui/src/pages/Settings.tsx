import { signal, Text, View } from "nsh";
import { Button } from "nsh/base";
import { Card, CopyButton, NumberField, Row, SelectField, SwitchRow, TextField } from "../components";
import { TOOL_GROUPS, groupEnabled } from "../format";
import type { ToolDef } from "../types";
import {
  busy,
  cfg,
  dirty,
  patchConfig,
  patchSection,
  regenerateToken,
  restart,
  saveConfig,
  saveSettings,
  settings,
  status,
  toggleTool,
  toggleToolGroup,
} from "../store";

function num(v: string): number {
  const n = parseInt(v.trim(), 10);
  return Number.isFinite(n) ? n : 0;
}

export default function SettingsPage() {
  const showToken = signal(false);
  return (
    <View class="gap-3">
      <Card title="Desktop app" subtitle="How this app finds and starts the server">
        <View class="flex-row flex-wrap gap-3">
          <TextField
            label="Server binary"
            value={settings().serverPath}
            onChange={(v) => void saveSettings({ serverPath: v })}
            placeholder="pcrc-server (PATH)"
            hint="Leave empty to use `pcrc-server` from PATH. An absolute path must be listed in ui/nsh.json capabilities.process.execute."
            className="min-w-96"
          />
          <NumberField
            label="Connection port"
            value={settings().port}
            onChange={(v) => void saveSettings({ port: v })}
            hint="Port used to reach the admin API (the server's configured port)."
          />
        </View>
        <SwitchRow
          label="Start the server when this app opens"
          checked={settings().autostart}
          onChange={(v) => void saveSettings({ autostart: v })}
        />
      </Card>

      <If when={!cfg()}>
        <Card title="Server settings">
          <Text class="text-sm text-muted-foreground">
            Start the server to load and edit its settings. The server owns its config file; changes made here are
            written through the loopback admin API.
          </Text>
        </Card>
      </If>

      <If when={cfg()}>
        {(c) => (
          <View class="gap-3">
            <Card title="Server settings">
              <View class="flex-row items-center gap-3">
                <View class={`h-2 w-2 rounded-full ${dirty() ? "bg-[#f59e0b]" : "bg-[#22c55e]"}`} />
                <Text class="text-xs text-muted-foreground">{dirty() ? "Unsaved changes" : "Saved"}</Text>
                <View class="flex-1" />
                <Button
                  class="rounded-md bg-primary px-3 py-1.5 hover:bg-primary/90 disabled:opacity-50"
                  disabled={!dirty() || busy()}
                  onPress={() => void saveConfig()}
                >
                  <Text class="text-sm text-primary-foreground">Save settings</Text>
                </Button>
                <Button
                  class="rounded-md border border-border px-3 py-1.5 hover:bg-accent disabled:opacity-50"
                  disabled={busy() || status() === null}
                  onPress={() => void restart()}
                >
                  <Text class="text-sm text-foreground">Restart server</Text>
                </Button>
              </View>
              <Row label="Config file" value={status()?.config_path ?? ""} mono />
              <Text class="text-xs text-muted-foreground">
                Bind/port changes need a restart; all other changes apply immediately.
              </Text>
            </Card>

            <Card title="Network & approval">
              <View class="flex-row flex-wrap gap-3">
                <TextField label="Bind address" value={c().bind} onChange={(v) => patchConfig({ bind: v.trim() })} hint="127.0.0.1 keeps it local; 0.0.0.0 exposes it on the LAN." />
                <NumberField label="Port" value={c().port} onChange={(v) => patchConfig({ port: v })} />
                <TextField label="Tool prefix" value={c().tool_prefix} onChange={(v) => patchConfig({ tool_prefix: v.trim() })} hint="Default pc, giving names like pc_screenshot." />
              </View>
              <SwitchRow
                label="Require a bearer token"
                hint="Off: always approve every connection (no token). On: connections are approved automatically when the token matches."
                checked={c().token_enabled}
                onChange={(v) => patchConfig({ token_enabled: v })}
              />
              <Row label="Bearer token" value={showToken() ? c().token : "••••••••••••••••••••••••••••••••"} mono>
                <Button class="rounded-md border border-border px-2 py-1 hover:bg-accent" onPress={() => showToken.set(!showToken())}>
                  <Text class="text-xs text-muted-foreground">{showToken() ? "Hide" : "Show"}</Text>
                </Button>
                <CopyButton text={c().token} />
                <Button class="rounded-md border border-border px-2 py-1 hover:bg-accent" onPress={() => void regenerateToken()}>
                  <Text class="text-xs text-muted-foreground">Regenerate</Text>
                </Button>
              </Row>
            </Card>

            <Card title="Tools" subtitle="Disable tool groups an MCP client should never use, or single tools in the library below">
              <View class="flex-row flex-wrap gap-x-6">
                <Each of={TOOL_GROUPS} key={(g) => g.key}>
                  {(g) => (
                    <View class="min-w-56 flex-1">
                      <SwitchRow
                        label={g().label}
                        checked={groupEnabled(c().tools.disabled, g())}
                        onChange={(v) => toggleToolGroup(g().names, v)}
                      />
                    </View>
                  )}
                </Each>
              </View>
              <View class="flex-row flex-wrap gap-x-6">
                <Each of={status()?.tool_names ?? []} key={(n) => n}>
                  {(n) => (
                    <View class="min-w-56 flex-1">
                      <SwitchRow
                        label={n()}
                        checked={!c().tools.disabled.includes(n())}
                        onChange={(v) => toggleTool(n(), v)}
                      />
                    </View>
                  )}
                </Each>
              </View>
            </Card>

            <Card title="Screen & input">
              <View class="flex-row flex-wrap gap-3">
                <SelectField
                  label="Screenshot format"
                  value={c().screenshot.format}
                  options={[
                    { value: "png", title: "PNG" },
                    { value: "jpeg", title: "JPEG" },
                  ]}
                  onChange={(v) => patchSection("screenshot", { format: v })}
                />
                <NumberField label="JPEG quality" value={c().screenshot.jpeg_quality} onChange={(v) => patchSection("screenshot", { jpeg_quality: v })} />
                <NumberField label="Max width (0 = native)" value={c().screenshot.max_width} onChange={(v) => patchSection("screenshot", { max_width: v })} />
                <NumberField label="Max height (0 = native)" value={c().screenshot.max_height} onChange={(v) => patchSection("screenshot", { max_height: v })} />
                <SelectField
                  label="Input backend (Linux)"
                  value={c().input.backend}
                  options={[
                    { value: "auto", title: "Auto" },
                    { value: "x11", title: "X11 (XTEST)" },
                    { value: "uinput", title: "uinput (Wayland)" },
                  ]}
                  onChange={(v) => patchSection("input", { backend: v })}
                />
              </View>
              <SwitchRow
                label="Screenshots match input coordinates"
                hint="HiDPI captures are scaled down so a screenshot pixel matches the coordinates the input tools use."
                checked={c().screenshot.match_input_coordinates}
                onChange={(v) => patchSection("screenshot", { match_input_coordinates: v })}
              />
            </Card>

            <Card title="Shell & files">
              <SwitchRow label="Allow shell commands" checked={c().shell.enabled} onChange={(v) => patchSection("shell", { enabled: v })} />
              <View class="flex-row flex-wrap gap-3">
                <NumberField label="Shell timeout (ms)" value={c().shell.default_timeout_ms} onChange={(v) => patchSection("shell", { default_timeout_ms: v })} />
                <TextField
                  label="Shell blocklist (comma separated)"
                  value={c().shell.blocklist.join(", ")}
                  onChange={(v) =>
                    patchSection("shell", { blocklist: v.split(",").map((s) => s.trim()).filter((s) => s.length > 0) })
                  }
                  className="min-w-96"
                />
              </View>
              <View class="flex-row flex-wrap gap-3">
                <TextField
                  label="Files root (empty = whole filesystem)"
                  value={c().files.root}
                  onChange={(v) => patchSection("files", { root: v })}
                  className="min-w-80"
                />
                <TextField
                  label="Protected folders (comma separated)"
                  value={c().files.protected.join(", ")}
                  onChange={(v) =>
                    patchSection("files", {
                      protected: v.split(",").map((s) => s.trim()).filter((s) => s.length > 0),
                    })
                  }
                  hint="File tools refuse these folders even when the root is empty (~ is expanded)."
                  className="min-w-96"
                />
              </View>
              <SwitchRow label="Allow file writes" checked={c().files.allow_write} onChange={(v) => patchSection("files", { allow_write: v })} />
              <SwitchRow label="Allow file deletion" checked={c().files.allow_delete} onChange={(v) => patchSection("files", { allow_delete: v })} />
              <SwitchRow
                label="Allow downloads to private addresses"
                hint="Off blocks localhost/LAN targets for download_from_url."
                checked={c().downloads.allow_private_hosts}
                onChange={(v) => patchSection("downloads", { allow_private_hosts: v })}
              />
              <SwitchRow label="Allow downloads" checked={c().downloads.enabled} onChange={(v) => patchSection("downloads", { enabled: v })} />
            </Card>

            <Card title="Browser, documents & agents">
              <View class="flex-row flex-wrap gap-3">
                <TextField
                  label="Browser executable (empty = auto-detect)"
                  value={c().browser.path}
                  onChange={(v) => patchSection("browser", { path: v })}
                  placeholder="/usr/bin/google-chrome"
                  className="min-w-96"
                />
              </View>
              <SwitchRow label="Run the automation browser headless by default" checked={c().browser.headless} onChange={(v) => patchSection("browser", { headless: v })} />
              <View class="flex-row flex-wrap gap-3">
                <TextField label="soffice path" value={c().docs.soffice_path} onChange={(v) => patchSection("docs", { soffice_path: v })} placeholder="auto-detect" />
                <TextField label="pdftotext path" value={c().docs.pdftotext_path} onChange={(v) => patchSection("docs", { pdftotext_path: v })} placeholder="auto-detect" />
                <TextField label="tesseract path" value={c().docs.tesseract_path} onChange={(v) => patchSection("docs", { tesseract_path: v })} placeholder="auto-detect" />
              </View>
              <SwitchRow label="Allow coding-agent CLIs" checked={c().agents.enabled} onChange={(v) => patchSection("agents", { enabled: v })} />
              <View class="flex-row flex-wrap gap-3">
                <NumberField label="Max parallel agent jobs" value={c().agents.max_jobs} onChange={(v) => patchSection("agents", { max_jobs: v })} />
              </View>
            </Card>

            <Card title="Power & administration">
              <SwitchRow label="Allow shutdown / restart / hibernate / suspend / logout" checked={c().system.power_enabled} onChange={(v) => patchSection("system", { power_enabled: v })} />
              <SwitchRow label="Allow elevated commands and services" checked={c().system.admin_enabled} onChange={(v) => patchSection("system", { admin_enabled: v })} />
              <SwitchRow label="Allow running commands in a visible terminal" checked={c().system.terminal_enabled} onChange={(v) => patchSection("system", { terminal_enabled: v })} />
              <SwitchRow label="Allow remote-access tunnels (requires the bearer token)" checked={c().system.tunnel_enabled} onChange={(v) => patchSection("system", { tunnel_enabled: v })} />
              <View class="flex-row flex-wrap gap-3">
                <NumberField label="Max power delay (seconds)" value={c().system.max_delay_secs} onChange={(v) => patchSection("system", { max_delay_secs: v })} />
              </View>
            </Card>
          </View>
        )}
      </If>
    </View>
  );
}
