import { test, expect } from "nsh/test";
import { TOOL_GROUPS, formatBytes, formatTime, formatUptime, groupEnabled } from "./format";

test("uptime formatting", () => {
  expect(formatUptime(45)).toBe("45s");
  expect(formatUptime(125)).toBe("2m 5s");
  expect(formatUptime(3661)).toBe("1h 1m");
  expect(formatUptime(90061)).toBe("1d 1h");
});

test("byte formatting", () => {
  expect(formatBytes(512)).toBe("512 B");
  expect(formatBytes(2048)).toBe("2.0 KB");
  expect(formatBytes(5 * 1024 * 1024)).toBe("5.0 MB");
});

test("time formatting pads", () => {
  expect(formatTime(new Date(2026, 0, 2, 3, 4, 5).getTime())).toBe("03:04:05");
});

test("tool groups toggle as a unit", () => {
  const group = TOOL_GROUPS[0];
  expect(groupEnabled([], group)).toBe(true);
  expect(groupEnabled([group.names[0]], group)).toBe(false);
  expect(groupEnabled(group.names, group)).toBe(false);
});
