import { signal, Text, View } from "nsh";
import { Button } from "nsh/base";
import { Card, Chips, CodeBlock, CopyButton, Pill, Row, SelectField, Stat, SwitchRow, TextField } from "../components";
import { formatUptime, phaseLabel } from "../format";
import {
  busy,
  cfg,
  dirty,
  managed,
  patchConfig,
  patchSection,
  phase,
  restart,
  saveConfig,
  saveSettings,
  settings,
  start,
  startTunnel,
  status,
  stop,
  stopTunnel,
} from "../store";

export default function ServerPage() {
  const showToken = signal(false);
  const tunnelKind = () => cfg()?.tunnel.kind ?? "auto";
  const tunnel = () => (status()?.tunnel ?? {}) as any;
  const tunnelRunning = () => tunnel().state === "running" && !!tunnel().url;
  const tunnelBusy = () => tunnel().state === "starting" || tunnel().state === "stopping";
  const installed = () => (tunnel().installed ?? {}) as any;
  return (
    <View class="gap-3">
      <Card title="Server" subtitle="MCP server running on this PC">
        <View class="gap-2">
          <View class="flex-row items-center gap-2">
            <View
              class={`h-2.5 w-2.5 rounded-full ${status() ? "bg-[#22c55e]" : phase() === "starting" ? "bg-[#f59e0b]" : "bg-[#ef4444]"}`}
            />
            <Text class="text-base font-medium text-foreground">{phaseLabel(phase())}</Text>
            <View class="flex-1" />
            <If when={status()}>
              {(s) => (
                <Text class="text-right text-[11px] text-muted-foreground">
                  v{s().version} · {s().platform}/{s().arch} · up {formatUptime(s().uptime_secs)}
                </Text>
              )}
            </If>
          </View>
          <View class="flex-row gap-2">
            <Button
              class="flex-1 items-center justify-center rounded-md bg-primary px-3 py-1.5 hover:bg-primary/90 disabled:opacity-50"
              disabled={busy() || status() !== null || phase() === "starting"}
              onPress={() => void start()}
            >
              <Text class="text-sm text-primary-foreground">Start</Text>
            </Button>
            <Button
              class="flex-1 items-center justify-center rounded-md border border-border px-3 py-1.5 hover:bg-accent disabled:opacity-50"
              disabled={busy() || status() === null}
              onPress={() => void restart()}
            >
              <Text class="text-sm text-foreground">Restart</Text>
            </Button>
            <Button
              class="flex-1 items-center justify-center rounded-md border border-border px-3 py-1.5 hover:bg-accent disabled:opacity-50"
              disabled={busy() || status() === null}
              onPress={() => void stop()}
            >
              <Text class="text-sm text-destructive">Stop</Text>
            </Button>
          </View>
        </View>
        <SwitchRow
          label="Start the server when this app opens"
          checked={settings().autostart}
          onChange={(v) => void saveSettings({ autostart: v })}
        />
        <If when={!status() && phase() !== "starting"}>
          <Text class="text-xs text-muted-foreground">
            Binary: {settings().serverPath || "pcrc-server (found on PATH)"}. Build the server and install it on your
            PATH (for example ~/.local/bin), or set an absolute path in Settings.
          </Text>
        </If>
        <If when={managed()}>
          <View class="flex-row gap-2">
            <Pill text="Managed by this app" />
          </View>
        </If>
      </Card>

      <If when={status()}>
        {(s) => (
          <View class="gap-3">
            <If when={!s().token_enabled && s().bind !== "127.0.0.1" && s().bind !== "::1"}>
              <View class="rounded-lg border border-[#d97706]/40 bg-[#d97706]/10 px-3 py-2">
                <Text class="text-xs text-[#fbbf24]">
                  The server is reachable beyond loopback with no token: every client that can connect is approved.
                  Enable the bearer token in Settings, or bind 127.0.0.1.
                </Text>
              </View>
            </If>

            <Card title="Connection">
              <View class="gap-1.5">
                <View class="flex-row items-center gap-2">
                  <Text class="text-xs font-medium text-muted-foreground">MCP endpoint</Text>
                  <If when={tunnelRunning()}>
                    <Pill text="public tunnel" tone="green" />
                  </If>
                </View>
                <View class="flex-row items-center gap-2 rounded-lg border border-border bg-background px-3 py-2">
                  <Text class="flex-1 font-mono text-xs text-foreground">{s().mcp_url}</Text>
                  <CopyButton text={s().mcp_url} />
                </View>
                <View class="flex-row items-center gap-2 rounded-lg border border-border bg-background px-3 py-2">
                  <Text class="flex-1 font-mono text-[11px] text-muted-foreground">{s().mcp_url_local}</Text>
                  <CopyButton text={s().mcp_url_local} />
                </View>
              </View>
              <Row
                label="Approval"
                value={
                  s().token_enabled
                    ? "Bearer token required — connections are approved automatically when it matches"
                    : "Always approve — no token, every client is accepted"
                }
              />
              <If when={s().token_enabled}>
                <Row
                  label="Bearer token"
                  value={showToken() ? String(s().token ?? cfg()?.token ?? "") : "••••••••••••••••••••••••••••••••"}
                  mono
                >
                  <Button
                    class="rounded-md border border-border px-2 py-1 hover:bg-accent"
                    onPress={() => showToken.set(!showToken())}
                  >
                    <Text class="text-xs text-muted-foreground">{showToken() ? "Hide" : "Show"}</Text>
                  </Button>
                  <CopyButton text={String(s().token ?? cfg()?.token ?? "")} />
                </Row>
              </If>
              <If when={s().lan_urls}>
                {(urls) => (
                  <View class="gap-2">
                    <Text class="text-sm text-foreground">Same network (LAN)</Text>
                    <Each of={urls()} key="index">
                      {(url) => (
                        <View class="flex-row items-center gap-2">
                          <Text class="flex-1 font-mono text-xs text-muted-foreground">{url()}</Text>
                          <CopyButton text={url()} />
                        </View>
                      )}
                    </Each>
                    <Text class="text-xs text-muted-foreground">
                      {urls().length === 0
                        ? "No LAN address detected."
                        : "Only reachable when the server is bound beyond loopback."}
                    </Text>
                  </View>
                )}
              </If>
              <Row label="stdio transport" value={`${settings().serverPath || "pcrc-server"} --stdio`} mono>
                <CopyButton text={`${settings().serverPath || "pcrc-server"} --stdio`} />
              </Row>
              <Row label="Admin API" value={s().admin_url} mono>
                <CopyButton text={s().admin_url} />
              </Row>
            </Card>

            <Card title="Remote access" subtitle="A public URL that works from another network">
              <If when={tunnelRunning()}>
                <View class="gap-2">
                  <Row label="Public URL" value={String(tunnel().url)} mono>
                    <CopyButton text={String(tunnel().url)} />
                  </Row>
                  <Row
                    label="Tunnel"
                    value={`${String(tunnel().kind)} · up ${formatUptime(Number(tunnel().uptime_secs ?? 0))}`}
                  />
                  <View class="flex-row gap-2">
                    <Button
                      class="rounded-md border border-border px-3 py-1.5 hover:bg-accent"
                      onPress={() => void stopTunnel()}
                    >
                      <Text class="text-sm text-destructive">Stop tunnel</Text>
                    </Button>
                  </View>
                </View>
              </If>
              <If when={!tunnelRunning()}>
                <View class="gap-3">
                  <Text class="text-xs text-muted-foreground">
                    Clients on the same network use the LAN URLs; clients anywhere else connect through this tunnel.
                    The tunnel points at this machine's loopback server; the bearer token still gates every request.
                  </Text>
                  <View class="flex-row flex-wrap items-end gap-3">
                    <SelectField
                      label="Tunnel client"
                      value={tunnelKind()}
                      options={[
                        { value: "auto", title: "Auto (first installed)" },
                        { value: "cloudflared", title: `cloudflared${installed().cloudflared ? " ✓" : ""}` },
                        { value: "tailscale", title: `tailscale${installed().tailscale ? " ✓" : ""}` },
                        { value: "ngrok", title: `ngrok${installed().ngrok ? " ✓" : ""}` },
                      ]}
                      onChange={(v) => patchSection("tunnel", { kind: v })}
                    />
                    <View class="flex-1" />
                    <Button
                      class="rounded-md bg-primary px-3 py-1.5 hover:bg-primary/90 disabled:opacity-50"
                      disabled={tunnelBusy() || !s().token_enabled}
                      onPress={() => void startTunnel(tunnelKind())}
                    >
                      <Text class="text-sm text-primary-foreground">
                        {tunnelBusy() ? "Starting..." : "Start tunnel"}
                      </Text>
                    </Button>
                  </View>
                  <Chips
                    items={["cloudflared", "tailscale", "ngrok"].filter((k) => installed()[k])}
                    empty="no tunnel client installed"
                  />
                  <SwitchRow
                    label="Start the tunnel automatically when the server starts"
                    hint="Applies on the next server start. The MCP endpoint above then shows the public URL."
                    checked={cfg()?.tunnel.auto_start ?? false}
                    onChange={(v) => patchSection("tunnel", { auto_start: v })}
                  />
                  <View class="flex-row flex-wrap items-end gap-3">
                    <TextField
                      label="ngrok authtoken"
                      value={cfg()?.tunnel.ngrok_authtoken ?? ""}
                      onChange={(v) => patchSection("tunnel", { ngrok_authtoken: v })}
                      placeholder="paste your authtoken"
                      masked
                      hint="From dashboard.ngrok.com/get-started/your-authtoken. Stored in the server config."
                      className="min-w-80"
                    />
                    <TextField
                      label="Tailscale socket (optional)"
                      value={cfg()?.tunnel.tailscale_socket ?? ""}
                      onChange={(v) => patchSection("tunnel", { tailscale_socket: v })}
                      placeholder="/path/to/tailscaled.sock"
                      hint="For a bundled/userspace tailscaled instead of a system install."
                      className="min-w-72"
                    />
                    <Button
                      class="rounded-md border border-border px-3 py-1.5 hover:bg-accent disabled:opacity-50"
                      disabled={!dirty() || busy()}
                      onPress={() => void saveConfig()}
                    >
                      <Text class="text-sm text-foreground">Save settings</Text>
                    </Button>
                  </View>
                  <If when={!s().token_enabled}>
                    <View class="gap-2 rounded-lg border border-[#d97706]/40 bg-[#d97706]/10 px-3 py-2">
                      <Text class="text-xs text-[#fbbf24]">
                        A tunnel exposes this PC's control endpoint beyond your network. Enable the bearer token first:
                        the server refuses to start a tunnel without it.
                      </Text>
                      <View class="flex-row gap-2">
                        <Button
                          class="rounded-md bg-primary px-3 py-1.5 hover:bg-primary/90"
                          onPress={() => {
                            patchConfig({ token_enabled: true });
                            void saveConfig();
                          }}
                        >
                          <Text class="text-sm text-primary-foreground">Enable bearer token</Text>
                        </Button>
                      </View>
                    </View>
                  </If>
                  <Text class="text-xs text-muted-foreground">
                    cloudflared quick tunnels need no account; tailscale gives a private tailnet URL; ngrok needs an
                    authtoken.
                  </Text>
                </View>
              </If>
            </Card>

            <Card title="Activity">
              <View class="flex-row flex-wrap gap-3">
                <Stat label="MCP requests" value={String(s().stats.mcp_requests)} />
                <Stat label="Tool calls" value={String(s().stats.tool_calls)} />
                <Stat label="Tool errors" value={String(s().stats.tool_errors)} />
                <Stat label="Tools enabled" value={`${s().tools.enabled}/${s().tools.total}`} />
              </View>
              <If when={s().stats.last_client}>
                <Text class="text-xs text-muted-foreground">Last client: {s().stats.last_client}</Text>
              </If>
              <If when={s().stats.last_tool}>
                <Text class="text-xs text-muted-foreground">Last tool: {s().stats.last_tool}</Text>
              </If>
              <If when={s().stats.last_error}>
                <Text class="text-xs text-[#f87171]">Last error: {s().stats.last_error}</Text>
              </If>
            </Card>

            <Card title="Capabilities" subtitle="What this machine can do right now">
              <Row label="Screenshot" value={String(s().backends?.screenshot ?? "unknown")} />
              <Row
                label="Input backend"
                value={String(s().backends?.input ?? "unknown")}
                hint={
                  s().backends?.uinput && !s().backends.uinput.writable && s().session?.wayland
                    ? "X11 input only reaches XWayland windows; enable uinput below for full control."
                    : undefined
                }
              />
              <Row
                label="Browser"
                value={
                  s().capabilities?.browser?.installed
                    ? `${s().capabilities.browser.name} (${s().capabilities.browser.headless ? "headless default" : "headed default"})`
                    : "not installed (browser tools need Chrome/Chromium/Edge)"
                }
              />
              <Row label="Documents">
                <Chips
                  items={["pdftotext", "soffice", "tesseract"].filter((k) => s().capabilities?.docs?.[k])}
                  empty="no converters found"
                />
              </Row>
              <Row label="Coding agents">
                <Chips
                  items={(s().capabilities?.agents?.agents ?? [])
                    .filter((a: any) => a.installed)
                    .map((a: any) => String(a.name))}
                  empty="no agent CLIs found"
                />
              </Row>
              <Row
                label="Session"
                value={`${s().session?.session_type ?? "?"}${s().session?.desktop ? " · " + s().session.desktop : ""}`}
              />
              <If when={s().backends?.uinput && !s().backends.uinput.writable}>
                <CodeBlock
                  label="Enable full input on Wayland (run once, then log in again)"
                  text={String(s().backends.uinput.instructions ?? "")}
                />
              </If>
            </Card>
          </View>
        )}
      </If>
    </View>
  );
}
