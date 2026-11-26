import { effect, Image, mount, Scroll, signal, Text, View } from "nsh";
import { Button } from "nsh/base";
import { currentWindow, Tray, Window } from "nsh/desktop";
import appIcon from "./assets/icon.png";
import { phaseLabel } from "./src/format";
import AboutPage from "./src/pages/About";
import LogsPage from "./src/pages/Logs";
import ServerPage from "./src/pages/Server";
import SettingsPage from "./src/pages/Settings";
import { checkUpdates, init, notice, openUpdate, phase, status, updateDismissed, updateInfo } from "./src/store";

const TABS = [
  { id: "server", label: "Server" },
  { id: "settings", label: "Settings" },
  { id: "logs", label: "Logs" },
  { id: "about", label: "About" },
];

function showMainWindow() {
  const win = Window.get("main");
  if (win) {
    win.show();
    win.focus();
  }
}

function App() {
  const tab = signal("server");
  void init();
  const running = () => status() !== null;
  const win = currentWindow();

  effect(() => {
    win.set({
      title: "PC Remote Control",
      minSize: { width: 460, height: 620 },
      closeOnPress: false,
      closeOnEscape: false,
    });
    win.setBounds({ size: { width: 560, height: 900 } });
    const stop = win.on("closePress", () => win.hide());
    return () => stop();
  });

  effect(() => {
    Tray.set({
      icon: appIcon,
      tooltip: "PC Remote Control — MCP server",
      onClick: showMainWindow,
      menu: [
        { label: "Show window", onSelect: showMainWindow },
        { label: "Hide window", onSelect: () => win.hide() },
        { type: "separator" },
        { label: "Quit PC Remote Control", role: "quit" },
      ],
    });
    return () => Tray.set(null);
  });
  let content: any = null;
  let hadStatus = false;
  effect(() => {
    const s = status();
    if (s && !hadStatus) {
      hadStatus = true;
      content?.scrollTo({ y: 0, animated: false });
    }
    if (!s) hadStatus = false;
  });
  const go = (id: string) => {
    tab.set(id);
    content?.scrollTo({ y: 0, animated: false });
  };

  return (
    <View class="flex-1 bg-background text-foreground">
      <View class="flex-row items-center gap-3 border-b border-border px-4 py-3">
        <Image src={appIcon} class="h-8 w-8 rounded-lg" />
        <View class="gap-0.5">
          <Text class="text-base font-semibold text-foreground">PC Remote Control</Text>
          <Text class="text-[11px] text-muted-foreground">MCP desktop remote</Text>
        </View>
        <View class="flex-row items-center gap-1.5 rounded-full border border-border px-2 py-0.5">
          <View
            class={`h-2 w-2 rounded-full ${running() ? "bg-[#22c55e]" : phase() === "starting" ? "bg-[#f59e0b]" : "bg-[#ef4444]"}`}
          />
          <Text class="text-xs text-muted-foreground">{phaseLabel(phase())}</Text>
        </View>
        <View class="flex-1" />
        <Button
          class="rounded-md border border-border px-2 py-1 hover:bg-accent"
          onPress={() => win.hide()}
        >
          <Text class="text-xs text-muted-foreground">Hide to tray</Text>
        </Button>
      </View>

      <View class="flex-row gap-1 border-b border-border px-4 pb-2 pt-2">
        <Each of={TABS} key={(t) => t.id}>
          {(t) => (
            <Button
              selected={tab() === t().id}
              class="rounded-md px-3 py-1.5 hover:bg-accent selected:bg-secondary selected:text-secondary-foreground"
              onPress={() => go(t().id)}
            >
              <Text class="text-sm text-foreground">{t().label}</Text>
            </Button>
          )}
        </Each>
      </View>

      <View class="flex-1 gap-3 px-4 py-4">
        <If when={updateInfo()?.available && updateDismissed() !== String(updateInfo()?.latest)}>
          <View class="gap-2 rounded-lg border border-[#2563eb]/40 bg-[#2563eb]/10 px-3 py-2">
            <Text class="text-xs text-[#93c5fd]">
              Update available: v{String(updateInfo()?.latest ?? "")} (current v{String(updateInfo()?.current ?? "")}).
            </Text>
            <View class="flex-row flex-wrap gap-2">
              <Button class="rounded-md bg-primary px-3 py-1.5 hover:bg-primary/90" onPress={() => void openUpdate()}>
                <Text class="text-sm text-primary-foreground">Download update</Text>
              </Button>
              <Button
                class="rounded-md border border-border px-3 py-1.5 hover:bg-accent"
                onPress={() => updateDismissed.set(String(updateInfo()?.latest ?? ""))}
              >
                <Text class="text-sm text-foreground">Dismiss</Text>
              </Button>
              <Button class="rounded-md border border-border px-2 py-1 hover:bg-accent" onPress={() => void checkUpdates(true)}>
                <Text class="text-xs text-muted-foreground">Check again</Text>
              </Button>
            </View>
          </View>
        </If>
        <If when={notice()}>
          {(n) => (
            <View
              class={`rounded-lg border px-3 py-2 ${
                n().kind === "error"
                  ? "border-[#dc2626]/40 bg-[#dc2626]/10"
                  : n().kind === "success"
                    ? "border-[#16a34a]/40 bg-[#16a34a]/10"
                    : "border-border bg-muted"
              }`}
            >
              <Text
                class={`text-xs ${
                  n().kind === "error" ? "text-[#f87171]" : n().kind === "success" ? "text-[#4ade80]" : "text-muted-foreground"
                }`}
              >
                {n().text}
              </Text>
            </View>
          )}
        </If>
        <Scroll class="flex-1" contentClass="gap-3 pb-10" ref={(node) => { content = node; }}>
          {tab() === "server" ? (
            <ServerPage />
          ) : tab() === "settings" ? (
            <SettingsPage />
          ) : tab() === "logs" ? (
            <LogsPage />
          ) : (
            <AboutPage />
          )}
        </Scroll>
      </View>
    </View>
  );
}

mount(App);
