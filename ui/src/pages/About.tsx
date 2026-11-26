import { Scroll, Text, View } from "nsh";
import { Card, CodeBlock, CopyButton, Pill, Row } from "../components";
import { cfg, mcpUrl, settings, status, tools } from "../store";

export default function AboutPage() {
  const httpConfig = () => {
    const entry: any = { url: mcpUrl() };
    const token = status()?.token ?? cfg()?.token ?? "";
    if (status()?.token_enabled && token !== "") {
      entry.headers = { Authorization: `Bearer ${token}` };
    }
    return JSON.stringify({ mcpServers: { "pc-remote-control": entry } }, null, 2);
  };
  const stdioConfig = () => {
    return JSON.stringify(
      {
        mcpServers: {
          "pc-remote-control": {
            command: settings().serverPath || "pcrc-server",
            args: ["--stdio"],
          },
        },
      },
      null,
      2,
    );
  };

  return (
    <View class="gap-3">
      <Card title="PC Remote Control MCP">
        <Text class="text-sm text-muted-foreground">
          A remote-control MCP server that runs on this computer: screenshots and input, files, shell, window and
          process management, browser automation over the Chrome DevTools Protocol, document extraction and
          conversion, coding-agent CLI jobs, and power/administration tools. The desktop UI is a native nsh app
          (GTK on Linux, AppKit on macOS, Win32 on Windows).
        </Text>
        <View class="flex-row flex-wrap gap-2">
          <Pill text={status() ? `server v${status()!.version}` : "server stopped"} tone={status() ? "green" : "muted"} />
          <Pill text="MCP · Streamable HTTP · stateless JSON" />
          <Pill text={`${tools().length} tools`} />
          <Pill text={status() ? `${status()!.platform}/${status()!.arch}` : "—"} />
        </View>
        <Row label="MCP endpoint" value={mcpUrl()} mono>
          <CopyButton text={mcpUrl()} />
        </Row>
        <If when={status()}>
          {(s) => <Row label="Config file" value={s().config_path} mono />}
        </If>
      </Card>

      <Card title="Connect an MCP client" subtitle="HTTP transport (recommended)">
        <CodeBlock label="Claude Desktop / Cursor / generic HTTP client (.mcp.json)" text={httpConfig()} />
        <Text class="text-xs text-muted-foreground">
          Connections are approved automatically. With a bearer token enabled, add the Authorization header shown in
          the snippet. For the web connectors of Claude, ChatGPT or Gemini, expose the server beyond loopback through
          a tunnel and enable the token.
        </Text>
        <CodeBlock label="stdio transport (no HTTP, for desktop clients)" text={stdioConfig()} />
      </Card>

      <Card title="Tools" subtitle="Registered on the running server">
        <If when={tools().length === 0}>
          <Text class="text-xs text-muted-foreground">Start the server to list its tools.</Text>
        </If>
        <If when={tools().length > 0}>
          <Scroll class="h-72" contentClass="gap-1.5">
            <Each of={tools()} key={(t) => t.name}>
              {(t) => (
                <View class="gap-0.5 rounded-md border border-border px-2.5 py-1.5">
                  <Text class="font-mono text-xs text-foreground">{t().name}</Text>
                  <Text class="text-xs text-muted-foreground">{t().description}</Text>
                </View>
              )}
            </Each>
          </Scroll>
        </If>
      </Card>

      <Card title="Permissions">
        <Text class="text-xs text-muted-foreground">
          Linux / Wayland: screenshots use the xdg-desktop-portal; full mouse and keyboard control needs write access
          to /dev/uinput (the Server page shows the exact setup). X11 sessions work without extra permissions for X11
          windows. macOS: grant Screen Recording (screenshots) and Accessibility (input) in System Settings.
          Windows: no extra permissions for screen or input; elevated commands show a UAC prompt.
        </Text>
        <Text class="text-xs text-muted-foreground">
          Always-approve is the MCP connection model: the server never shows an approval dialog. Keep the bind address
          at 127.0.0.1, or enable the bearer token before exposing it.
        </Text>
      </Card>
    </View>
  );
}
