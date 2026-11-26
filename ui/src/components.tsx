import { Text, View } from "nsh";
import { Button, Input, Select, Switch } from "nsh/base";
import { copy } from "./store";

export function Card(props: { title?: string; subtitle?: string; className?: string; children?: any }) {
  return (
    <View class={`rounded-xl border border-border bg-card p-4 gap-3 ${props.className ?? ""}`}>
      <If when={props.title}>
        {(title) => (
          <View class="gap-0.5">
            <Text class="text-sm font-semibold text-card-foreground">{title()}</Text>
            <If when={props.subtitle}>
              {(subtitle) => <Text class="text-xs text-muted-foreground">{subtitle()}</Text>}
            </If>
          </View>
        )}
      </If>
      {props.children}
    </View>
  );
}

export function Row(props: { label: string; value?: string; mono?: boolean; hint?: string; children?: any }) {
  return (
    <View class="gap-1">
      <Text class="text-xs font-medium text-muted-foreground">{props.label}</Text>
      <If when={props.value}>
        {(value) => (
          <Text class={`text-sm text-foreground ${props.mono ? "font-mono text-xs break-all" : ""}`}>{value()}</Text>
        )}
      </If>
      <If when={props.hint}>{(hint) => <Text class="text-xs text-muted-foreground">{hint()}</Text>}</If>
      <If when={props.children}>
        {(child) => <View class="flex-row flex-wrap items-center gap-2">{child()}</View>}
      </If>
    </View>
  );
}

export function CopyButton(props: { text: string }) {
  return (
    <Button
      class="rounded-md border border-border px-2 py-1 hover:bg-accent active:bg-accent"
      onPress={() => void copy(props.text)}
    >
      <Text class="text-xs text-muted-foreground">Copy</Text>
    </Button>
  );
}

export function Pill(props: { text: string; tone?: "green" | "red" | "amber" | "muted" }) {
  const tone = props.tone ?? "muted";
  const cls =
    tone === "green"
      ? "bg-[#16a34a]/15 text-[#4ade80]"
      : tone === "red"
        ? "bg-[#dc2626]/15 text-[#f87171]"
        : tone === "amber"
          ? "bg-[#d97706]/15 text-[#fbbf24]"
          : "bg-muted text-muted-foreground";
  return <Text class={`rounded-full px-2 py-0.5 text-xs ${cls}`}>{props.text}</Text>;
}

export function SwitchRow(props: {
  label: string;
  hint?: string;
  checked: boolean;
  onChange: (value: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <View class="flex-row items-center justify-between gap-4 py-1">
      <View class="flex-1 gap-0.5">
        <Text class="text-sm text-foreground">{props.label}</Text>
        <If when={props.hint}>{(hint) => <Text class="text-xs text-muted-foreground">{hint()}</Text>}</If>
      </View>
      <Switch checked={props.checked} onChange={(e) => props.onChange(e.checked)} disabled={props.disabled} />
    </View>
  );
}

export function TextField(props: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  hint?: string;
  placeholder?: string;
  masked?: boolean;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <View class={`gap-1.5 flex-1 min-w-52 ${props.className ?? ""}`}>
      <Text class="text-xs text-muted-foreground">{props.label}</Text>
      <Input
        value={props.value}
        onChange={(e) => props.onChange(e.value)}
        placeholder={props.placeholder}
        masked={props.masked}
        disabled={props.disabled}
        class="rounded-md border border-input bg-background px-3 py-2 text-sm text-foreground"
      />
      <If when={props.hint}>{(hint) => <Text class="text-xs text-muted-foreground">{hint()}</Text>}</If>
    </View>
  );
}

export function NumberField(props: {
  label: string;
  value: number;
  onChange: (value: number) => void;
  hint?: string;
  disabled?: boolean;
}) {
  return (
    <TextField
      label={props.label}
      value={String(props.value)}
      onChange={(v) => {
        const n = parseInt(v.trim(), 10);
        props.onChange(Number.isFinite(n) ? n : 0);
      }}
      hint={props.hint}
      disabled={props.disabled}
    />
  );
}

export function SelectField(props: {
  label: string;
  value: string;
  options: { value: string; title: string }[];
  onChange: (value: string) => void;
  hint?: string;
  disabled?: boolean;
}) {
  return (
    <View class="gap-1.5 flex-1 min-w-44">
      <Text class="text-xs text-muted-foreground">{props.label}</Text>
      <Select
        items={props.options}
        value={props.value}
        onChange={(e) => props.onChange(e.value ?? props.options[0]?.value ?? "")}
        disabled={props.disabled}
        placeholder="Choose..."
        class="rounded-md border border-input bg-background px-3 py-2 text-sm text-foreground"
      />
      <If when={props.hint}>{(hint) => <Text class="text-xs text-muted-foreground">{hint()}</Text>}</If>
    </View>
  );
}

export function Stat(props: { label: string; value: string }) {
  return (
    <View class="flex-1 min-w-24 rounded-lg border border-border bg-background px-3 py-2 gap-0.5">
      <Text class="text-xs text-muted-foreground">{props.label}</Text>
      <Text class="text-base font-semibold text-foreground">{props.value}</Text>
    </View>
  );
}

export function Chips(props: { items: string[]; empty?: string }) {
  return (
    <View class="flex-row flex-wrap gap-1.5">
      <If when={props.items.length > 0} else={<Text class="text-xs text-muted-foreground">{props.empty ?? "none"}</Text>}>
        <Each of={props.items} key="index">
          {(item) => <Pill text={item()} tone="green" />}
        </Each>
      </If>
    </View>
  );
}

export function CodeBlock(props: { text: string; label?: string }) {
  return (
    <View class="gap-2 rounded-lg border border-border bg-background p-3">
      <View class="flex-row items-center justify-between gap-2">
        <Text class="text-xs font-medium text-muted-foreground">{props.label ?? "Command"}</Text>
        <CopyButton text={props.text} />
      </View>
      <Text class="font-mono text-xs text-foreground">{props.text}</Text>
    </View>
  );
}
