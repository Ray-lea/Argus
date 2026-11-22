"use client";
import { useSyncExternalStore } from "react";
import { adapters, disks, gpus, machine, transferred } from "./model";

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
// The monitors' readings by kind, as the components page offers them (components.rs
// `option_rows`): a row per device where there are several, after a row of all of
// them for disks and connections; the main GPU's row holds the GPU readings without
// a device. A reading of one device is "metric@device".
const on = (keys: string[], device?: string) => (device ? keys.map((k) => k + "@" + device) : keys);
const gpuKeys = (integrated: boolean) => ["gpu", "gpuTemp", ...(integrated ? [] : ["gpuMem"]), "gpuClock", "gpuPower", ...(integrated ? [] : ["gpuFan"])];
export type MonitorRow = { title: string; detail: string; keys: string[] };
const disksSize = (bytes: number) => (bytes / 1024 ** 4).toFixed(1) + " TB";
export const monitorGroups: { name: string; rows: MonitorRow[]; keys: string[] }[] = [
  { name: "CPU", rows: [{ title: "CPU", detail: machine.cpu, keys: ["cpu", "temp", "clock", "power"] }] },
  { name: "内存", rows: [{ title: "内存", detail: "32.0 GB", keys: ["memoryPercent", "mem"] }] },
  {
    name: "GPU",
    rows: [...gpus]
      .sort((a, b) => b.dedicated - a.dedicated)
      .map((g, n) => {
        const memory = "专用显存 " + transferred(g.dedicated);
        return n === 0
          ? { title: g.model, detail: memory + " · 默认显卡", keys: gpuKeys(g.integrated) }
          : { title: g.model, detail: memory, keys: on(gpuKeys(g.integrated), g.id) };
      }),
  },
  {
    name: "磁盘",
    rows:
      disks.length > 1
        ? [
            { title: "磁盘智能显示", detail: "速度为合计，活动时间与温度取最高", keys: ["disk", "diskActive", "diskTemp"] },
            ...disks.map((d) => ({ title: d.volumes.join(" "), detail: d.model + " · " + disksSize(d.size), keys: on(["disk", "diskActive", "diskTemp"], d.id) })),
          ]
        : [{ title: "磁盘", detail: disks[0].volumes.join(" ") + " · " + disks[0].model + " · " + disksSize(disks[0].size), keys: ["disk", "diskActive", "diskTemp"] }],
  },
  {
    name: "网络",
    rows:
      adapters.length > 1
        ? [
            { title: "全部网卡", detail: "速率为合计", keys: ["net"] },
            ...adapters.map((a) => ({ title: a.name, detail: a.model, keys: on(["net"], a.id) })),
          ]
        : [{ title: "网络", detail: adapters[0].name + " · " + adapters[0].model, keys: ["net"] }],
  },
  { name: "其他", rows: [{ title: "其他", detail: "", keys: ["battery", "uptime"] }] },
].map((g) => ({ ...g, keys: g.rows.flatMap((r) => r.keys) }));
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
  watch: { pid: number; start: number; stop: number | null } | null;

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
