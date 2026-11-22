"use client";
import { useSyncExternalStore } from "react";

// Shared between the parts of the home page: the accent picker recolours the
// window in the first screen, the comparison rows can open one of its pages,
// and the taskbar shows or minimizes it.
export interface Accent {
  light: string;
  dark: string;
}
export type Page =
  | "processes"
  | "performance"
  | "services"
  | "startup"
  | "connections"
  | "history"
  | "components"
  | "settings"
  | "about";
// A row of the comparison asks for a place in the window: a page; a view of
// the process list; a device of the performance page, with its details page
// (CPU, disk) or the CPU's per-core view; a range of the history.
export interface Request {
  page: Page;
  view?: "apps" | "tree" | "list";
  device?: "cpu" | "memory" | "disk" | "wifi" | "gpu" | "battery";
  details?: boolean;
  cores?: boolean;
  range?: "1h" | "6h" | "1d" | "7d";
  pid?: number;
  at: number;
}
export const monitorGroups = [
  { name: "CPU", keys: ["cpu", "temp", "clock", "power"] },
  { name: "内存", keys: ["memoryPercent", "mem"] },
  { name: "GPU", keys: ["gpu", "gpuTemp", "gpuMem", "gpuClock", "gpuPower", "gpuFan"] },
  { name: "磁盘智能显示", keys: ["disk", "diskActive", "diskTemp"] },
  { name: "全部网卡", keys: ["net"] },
  { name: "其他", keys: ["battery", "uptime"] },
];
const monitor = (keys: string[]) => ({
  enabled: false,
  style: "标签",
  keys,
  order: monitorGroups.flatMap((g) => g.keys),
});
export const initialWidgets = {
  section: "托盘",
  icon: "走势",
  card: true,
  trayMenu: "快捷面板",
  closeToTray: true,
  restore: "保留当前页面",
  slow: 5000,
  desktop: {
    ...monitor(["cpu", "memoryPercent", "gpu", "disk", "net"]),
    style: "列表",
    layout: "面板",
    grouping: "卡片",
    locked: false,
    topmost: true,
    through: false,
    opacity: 100,
  },
  taskbar: { ...monitor(["cpu", "memoryPercent", "net"]), place: "托盘旁" },
};
export type WidgetPrefs = typeof initialWidgets;
export function setWidgets(patch: Partial<WidgetPrefs>) {
  store.set({ widgets: { ...store.get().widgets, ...patch } });
}
export type MonitorKind = "desktop" | "taskbar";
export function setMonitor<K extends MonitorKind>(kind: K, patch: Partial<WidgetPrefs[K]>) {
  setWidgets({ [kind]: { ...store.get().widgets[kind], ...patch } });
}
interface State {
  widgets: WidgetPrefs;
  paused: boolean;
  lanes: { id: number; key: string; overlay?: string }[];
  recording: {
    enabled: boolean;
    until: number | null;
    off: string[];
    count: number;
    days: number;
    size: number;
    detail: string;
    location: string;
  };
  historyBand: boolean;
  leakHints: boolean;
  ignoreLeak: boolean;
  watch: { pid: number; program: boolean; start: number; stop: number | null } | null;

  accent: Accent | null;
  request: Request | null;
  // The taskbar shows or minimizes the window; the guided demo looks up a file
  // as if it had been dropped on the window.
  command:
    | { action: "show" | "minimize" | "pause" | "exit"; at: number }
    | { action: "lock"; names: string[]; at: number }
    | null;
}
let state: State = {
  widgets: initialWidgets,
  paused: false,
  lanes: [{ id: 1, key: "cpu", overlay: "temp" }],
  recording: {
    enabled: true,
    until: null,
    off: [],
    count: 20,
    days: 30,
    size: 2048,
    detail: "7 天后合并为每分钟",
    location: "%LOCALAPPDATA%\\dev.argus.taskmanager\\history",
  },
  historyBand: true,
  leakHints: true,
  ignoreLeak: false,
  watch: null,
  accent: null,
  request: null,
  command: null,
};
const listeners = new Set<() => void>();
export const store = {
  get: () => state,
  set(patch: Partial<State>) {
    state = { ...state, ...patch };
    for (const listener of listeners) listener();
  },
  subscribe(listener: () => void) {
    listeners.add(listener);
    return () => listeners.delete(listener);
  },
};
export const useShared = () => useSyncExternalStore(store.subscribe, store.get, store.get);
