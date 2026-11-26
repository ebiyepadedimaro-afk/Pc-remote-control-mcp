import { Scroll, signal, Text, View } from "nsh";
import { Button, Select } from "nsh/base";
import { Card } from "../components";
import { formatTime } from "../format";
import { clearLogs, logs } from "../store";

function levelClass(level: string): string {
  const l = level.toUpperCase();
  if (l.includes("ERROR")) return "text-[#f87171]";
  if (l.includes("WARN")) return "text-[#fbbf24]";
  if (l === "PROC") return "text-[#60a5fa]";
  if (l === "UI") return "text-[#c084fc]";
  return "text-[#4ade80]";
}

export default function LogsPage() {
  const filter = signal("all");
  const entries = () =>
    logs()
      .slice()
      .reverse()
      .filter((e) => {
        const f = filter();
        if (f === "all") return true;
        return e.level.toLowerCase().includes(f);
      });

  return (
    <Card title="Logs" subtitle="Server log stream, newest first">
      <View class="flex-row flex-wrap items-center gap-2">
        <Select
          items={[
            { value: "all", title: "All levels" },
            { value: "info", title: "Info" },
            { value: "warn", title: "Warnings" },
            { value: "error", title: "Errors" },
            { value: "proc", title: "Process output" },
            { value: "ui", title: "Desktop app" },
          ]}
          value={filter()}
          onChange={(e) => filter.set(e.value ?? "all")}
          class="rounded-md border border-input bg-background px-3 py-1.5 text-xs text-foreground"
        />
        <View class="flex-1" />
        <Text class="text-xs text-muted-foreground">{entries().length} lines</Text>
        <Button class="rounded-md border border-border px-2 py-1 hover:bg-accent" onPress={() => clearLogs()}>
          <Text class="text-xs text-muted-foreground">Clear view</Text>
        </Button>
      </View>
      <View class="rounded-lg border border-border bg-background p-2">
        <If
          when={entries().length === 0}
          else={
            <Scroll class="h-96" contentClass="gap-0.5">
              <Each of={entries()} key={(e) => e.seq}>
                {(e) => (
                  <View class="flex-row items-start gap-2 py-0.5">
                    <Text class="font-mono text-xs text-muted-foreground">{formatTime(e().ts)}</Text>
                    <Text class={`w-12 font-mono text-xs ${levelClass(e().level)}`}>{e().level}</Text>
                    <Text class="flex-1 font-mono text-xs text-foreground">{e().msg}</Text>
                  </View>
                )}
              </Each>
            </Scroll>
          }
        >
          <Text class="p-2 text-xs text-muted-foreground">No log lines yet.</Text>
        </If>
      </View>
    </Card>
  );
}
