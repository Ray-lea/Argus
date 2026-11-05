/** @jsxImportSource @/components/replica/i18n */
"use client";
// The process columns as the app has them (src-gpui/src/frontend/columns.rs
// and columns/dialog.rs): the catalog by category, the visitor's choice and
// order, what each column shows for a row, the header's menu, the "Select
// columns" dialog, and dragging a header to another place.
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
  type RefObject,
} from "react";
import { flushSync } from "react-dom";
import { translate } from "./i18n";
import { Icon, type UiIcon } from "./icons";
import { bytes, heat, noise, percent, rate, thousands, type Process } from "./model";
import type { MenuEntry } from "./Pages";
import type { Row } from "./Processes";
import { Button, Segments, useIsoLayoutEffect } from "./widgets";

export type Category = "general" | "cpu" | "memory" | "io" | "network" | "gpu";
type Format = "name" | "text" | "id" | "count" | "percent" | "memory" | "space" | "rate" | "duration" | "time" | "bool";
export interface ColumnDef {
  key: string;
  category: Category;
  format: Format;
  /** The width its format needs, reserved before any reading arrives. */
  width: number;
  /** Groups show the sum of their members. */
  aggregate: boolean;
  label: string;
  title: string;
  /** A live reading, tinted by its load; peaks and running totals are not. */
  heat?: "percent" | "memory" | "rate";
}
const col = (
  key: string,
  category: Category,
  format: Format,
  width: number,
  aggregate: boolean,
  label: string,
  title: string,
  tint?: ColumnDef["heat"],
): ColumnDef => ({ key, category, format, width, aggregate, label, title, heat: tint });

export const CATEGORIES: Category[] = ["general", "cpu", "memory", "io", "network", "gpu"];
export const categoryLabel: Record<Category, string> = {
  general: "基本信息",
  cpu: "CPU",
  memory: "内存",
  io: "磁盘与 I/O",
  network: "网络",
  gpu: "GPU",
};
const categoryIcon: Record<Category, UiIcon> = {
  general: "processes",
  cpu: "chip",
  memory: "memory",
  io: "disk",
  network: "network",
  gpu: "gpu",
};

/** The name takes the room the others leave, never less than this. */
export const NAME_MIN = 180;
export const COLUMNS: ColumnDef[] = [
  col("name", "general", "name", NAME_MIN, false, "名称", "应用名称或文件描述；次级文字显示可执行文件名。"),
  col("imageName", "general", "text", 180, false, "映像名称", "可执行文件的名称。"),
  col("pid", "general", "id", 72, false, "PID", "进程 ID"),
  col("ppid", "general", "id", 82, false, "父进程 ID", "创建时记录的父进程 ID。"),
  col("path", "general", "text", 300, false, "映像路径", "进程可执行文件的完整路径。"),
  col("description", "general", "text", 220, false, "描述", "可执行文件描述或打包应用名称。"),
  col("company", "general", "text", 200, false, "公司名称", "可执行文件版本信息中的公司名称。"),
  col("sessionId", "general", "id", 80, false, "会话 ID", "进程所属的 Windows 会话编号。"),
  col("createTime", "general", "time", 160, false, "启动时间", "进程创建时间，以本地时间显示。"),
  col("threads", "general", "count", 76, true, "线程", "线程数"),
  col("handles", "general", "count", 86, true, "句柄", "句柄数"),
  col("suspended", "general", "bool", 90, false, "已暂停", "进程的全部线程处于暂停等待状态。"),
  col("efficiency", "general", "bool", 100, false, "效率模式", "进程的 EcoQoS 节能状态。"),
  col("cpu", "cpu", "percent", 70, true, "CPU", "占全部处理器能力的百分比", "percent"),
  col("cpuTime", "cpu", "duration", 100, true, "CPU 时间", "累计用户态与内核态 CPU 时间；分组显示成员合计。"),
  col("cpuUserTime", "cpu", "duration", 115, true, "用户 CPU 时间", "进程累计使用的用户态 CPU 时间。"),
  col("cpuKernelTime", "cpu", "duration", 115, true, "内核 CPU 时间", "进程累计使用的内核态 CPU 时间。"),
  col("basePriority", "cpu", "id", 100, false, "基本优先级", "Windows 报告的进程基本优先级数值。"),
  col("mem", "memory", "memory", 96, true, "内存", "专用工作集：进程独占的物理内存", "memory"),
  col("workingSet", "memory", "memory", 110, true, "工作集", "当前驻留的物理内存，含共享页面；分组为成员工作集之和。", "memory"),
  col("peakWorkingSet", "memory", "memory", 125, false, "工作集峰值", "单进程自启动以来的工作集峰值；展开应用分组查看各进程。"),
  col("commit", "memory", "memory", 110, true, "提交大小", "进程专用的已提交内存。", "memory"),
  col("virtualSize", "memory", "space", 150, false, "虚拟地址空间", "单进程预留与已提交的虚拟地址范围；已提交内存见“提交大小”。"),
  col("pageFaults", "memory", "count", 110, true, "缺页次数", "累计缺页次数，包含软缺页与硬缺页。"),
  col("diskRead", "io", "rate", 104, true, "磁盘读取", "每秒磁盘读取字节数，不包含网络及其他设备 I/O", "rate"),
  col("diskWrite", "io", "rate", 104, true, "磁盘写入", "每秒磁盘写入字节数，不包含网络及其他设备 I/O", "rate"),
  col("io", "io", "rate", 92, true, "I/O", "读、写及其他 I/O 合计，包含文件、网络和设备访问", "rate"),
  col("ioRead", "io", "rate", 110, true, "I/O 读取", "每秒 I/O 读取字节数，含文件、网络和设备访问。", "rate"),
  col("ioWrite", "io", "rate", 110, true, "I/O 写入", "每秒 I/O 写入字节数，含文件、网络和设备访问。", "rate"),
  col("ioOther", "io", "rate", 110, true, "其他 I/O", "其他 I/O 操作每秒传输的字节数。", "rate"),
  col("ioReadTotal", "io", "memory", 130, true, "累计 I/O 读取", "进程生存期内累计读取的 I/O 字节数。"),
  col("ioWriteTotal", "io", "memory", 130, true, "累计 I/O 写入", "进程生存期内累计写入的 I/O 字节数。"),
  col("ioOtherTotal", "io", "memory", 130, true, "累计其他 I/O", "进程生存期内其他 I/O 累计传输的字节数。"),
  col("net", "network", "rate", 92, true, "网络", "每秒发送和接收的字节数", "rate"),
  col("netReceived", "network", "rate", 110, true, "网络接收", "进程每秒接收的网络流量。", "rate"),
  col("netSent", "network", "rate", 110, true, "网络发送", "进程每秒发送的网络流量。", "rate"),
  col("gpu", "gpu", "percent", 70, true, "GPU", "进程在最忙的 GPU 引擎上占用的时间比例", "percent"),
  col("gpuEngine", "gpu", "text", 140, false, "GPU 引擎", "进程当前最忙的 GPU 引擎。"),
  col("gpuDedicated", "gpu", "memory", 130, true, "专用 GPU 内存", "进程当前的专用显存驻留读数；分组显示成员计数合计。", "memory"),
  col("gpuShared", "gpu", "memory", 130, true, "共享 GPU 内存", "进程当前的共享 GPU 内存驻留读数；分组显示成员计数合计。", "memory"),
];
const byKey = new Map(COLUMNS.map((c) => [c.key, c]));
export const columnDef = (key: string) => byKey.get(key);
const DEFAULTS = ["name", "pid", "cpu", "mem", "diskRead", "diskWrite", "io", "gpu", "net", "threads", "handles"];
const VIRTUAL_ALIASES = "虚拟内存 虚拟字节 地址预留";

/** Text columns read from the start of their cell, figures from its end. */
export const alignsEnd = (c: ColumnDef) => !["name", "text", "time", "bool"].includes(c.format);
/** Text columns start ascending and figures descending, the useful first view. */
export const descendingFirst = (key: string) => {
  const c = byKey.get(key);
  return !!c && !["name", "text", "id", "time"].includes(c.format);
};
/** A column label in the page's language: "启动时间" names the column, not the details' "Started". */
export const columnLabel = (c: ColumnDef) => translate(c.label, "column");

/** The columns of a category ("all" for every one), or those matching every
 *  word of a search by key, name or description. */
function matching(category: string, query: string) {
  const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
  return COLUMNS.filter((c) => {
    if (!terms.length) return category === "all" || c.category === category;
    const text = [c.key, columnLabel(c), translate(c.title), c.key === "virtualSize" ? translate(VIRTUAL_ALIASES) : ""]
      .join(" ")
      .toLowerCase();
    const compact = text.replace(/\s+/g, "");
    return terms.every((term) => text.includes(term) || compact.includes(term));
  });
}

// ---------------------------------------------------------------------------
// The choice: shown columns in their order, the name always first; every
// column keeps a place, so one hidden and shown again returns to it.
export interface ColumnPrefs {
  selected: string[];
  order: string[];
  autoWidth: boolean;
}
function normalize(selected: string[], order: string[], autoWidth: boolean): ColumnPrefs {
  const seen = new Set(["name"]);
  const all = ["name"];
  for (const key of [...order, ...selected, ...COLUMNS.map((c) => c.key)])
    if (byKey.has(key) && !seen.has(key)) {
      seen.add(key);
      all.push(key);
    }
  const chosen = new Set([...selected, "name"]);
  return { selected: all.filter((key) => chosen.has(key)), order: all, autoWidth };
}
export const defaultPrefs = () => normalize(DEFAULTS, [], true);
export function setSelected(p: ColumnPrefs, key: string, on: boolean): ColumnPrefs {
  if (key === "name" || !byKey.has(key) || p.selected.includes(key) === on) return p;
  return normalize(on ? [...p.selected, key] : p.selected.filter((k) => k !== key), p.order, p.autoWidth);
}
/** Moves a shown column before another (past the last for null), never before the name. */
export function moveBefore(p: ColumnPrefs, key: string, before: string | null): ColumnPrefs {
  if (key === "name" || key === before || before === "name" || !p.selected.includes(key)) return p;
  if (before !== null && !byKey.has(before)) return p;
  const order = p.order.filter((k) => k !== key);
  const at = before === null ? order.length : order.indexOf(before);
  order.splice(at < 0 ? order.length : at, 0, key);
  const next = normalize(p.selected, order, p.autoWidth);
  return next.order.join() === p.order.join() ? p : next;
}
export const visibleColumns = (p: ColumnPrefs) => p.selected.map((key) => byKey.get(key)!);

// ---------------------------------------------------------------------------
// What a column shows for a row: made-up but steady readings for the demo's
// processes. Groups sum what adds up; text agrees across members or stays
// empty; instance properties (PID, start time, state…) show on single rows.
const MB = 1024 * 1024;
const TB = 1024 ** 4;
const priorityBase: Record<string, number> = { 实时: 24, 高: 13, 高于正常: 10, 正常: 8, 低于正常: 6, 低: 4 };
const quiet = new Set(["cargo.exe", "rustc.exe", "rust-analyzer.exe", "argus.exe"]);
const userTime = (p: Process) => Math.round(p.cpuTime * (0.58 + noise(p.pid * 3 + 1, 0) * 0.3));
const describe = (p: Process) => (p.glyph === "kernel" ? "" : p.exe === "svchost.exe" ? "Windows 服务主机进程" : p.name);
const company = (p: Process) =>
  p.glyph === "kernel" || quiet.has(p.exe) ? "" : p.exe === "node.exe" ? "Node.js" : "Microsoft Corporation";
const addressSpace = (p: Process) => {
  if (p.glyph === "kernel") return p.mem * 1.6 + 4 * MB;
  const chromium = p.glyph === "browser" || p.glyph === "code";
  return 2 * TB + (chromium ? noise(p.pid * 7 + 3, 0) * 1.3 * TB : 0) + p.mem * 4 + noise(p.pid * 7, 0) * 256 * MB;
};
const faults = (p: Process, tick: number) =>
  Math.round((p.mem / 4096) * (1.4 + noise(p.pid * 11, 0) * 1.6)) +
  (p.suspended ? 0 : tick * Math.round((p.cpu ?? 0) * 40 + 2));
function ioTotal(p: Process, tick: number, seed: number, share: number) {
  const disk = p.disk ?? 0;
  const base = (p.mem * 1.6 + disk * 900 * MB + 24 * MB) * share * (0.5 + noise(p.pid * 13 + seed, 0));
  return base + (p.suspended ? 0 : tick * (disk * 0.22 * MB + 3 * 1024) * share);
}
const dedicated = (p: Process) => (p.gpu ? Math.round(p.gpu * 24 + 12) * MB : 0);
const sharedGpu = (p: Process) => (p.gpu ? Math.round(p.gpu * 5 + 6) * MB : 0);
const hms = (seconds: number) =>
  `${Math.floor(seconds / 3600)}:${String(Math.floor((seconds % 3600) / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`;

export interface CellContext {
  byPid: Map<number, Process>;
  tick: number;
  efficiency: Set<number>;
  priority: Map<number, string>;
}
export interface Cell {
  text: string;
  sort: number | string;
  faint?: boolean;
  heat?: number;
}
export function cellOf(c: ColumnDef, row: Row, ctx: CellContext): Cell {
  const v = row.values;
  const heading = row.kind === "group";
  const members = () => row.pids.map((pid) => ctx.byPid.get(pid)).filter((p): p is Process => !!p);
  const one = !heading && row.count === 1 ? ctx.byPid.get(row.pids[0]) : undefined;
  const instance = <T,>(f: (p: Process) => T) => (one ? f(one) : null);
  const common = (f: (p: Process) => string) => {
    if (heading) return null;
    const list = members();
    const first = list.length ? f(list[0]) : "";
    return list.every((p) => f(p) === first) ? first : null;
  };
  const total = (f: (p: Process) => number) => members().reduce((a, p) => a + f(p), 0);
  const ioRead = v.read * 1.04 + v.net * 0.012;
  const ioWrite = v.write * 1.04 + v.net * 0.006;
  let value: number | string | null;
  switch (c.key) {
    case "imageName": value = common((p) => p.exe); break;
    case "pid": value = instance((p) => p.pid); break;
    case "ppid": value = instance((p) => p.ppid); break;
    case "path": value = common((p) => p.path); break;
    case "description": value = common(describe); break;
    case "company": value = common(company); break;
    case "sessionId": value = instance((p) => (p.group === "windows" && p.user.startsWith("NT") ? 0 : 1)); break;
    case "createTime": value = instance((p) => p.started); break;
    case "threads": value = total((p) => p.threads); break;
    case "handles": value = total((p) => p.handles); break;
    case "suspended": value = instance((p) => (p.suspended ? "是" : "否")); break;
    case "efficiency": value = instance((p) => (ctx.efficiency.has(p.pid) ? "是" : "否")); break;
    case "cpu": value = v.cpu; break;
    case "cpuTime": value = total((p) => p.cpuTime); break;
    case "cpuUserTime": value = total(userTime); break;
    case "cpuKernelTime": value = total((p) => p.cpuTime - userTime(p)); break;
    case "basePriority": value = instance((p) => priorityBase[ctx.priority.get(p.pid) ?? "正常"] ?? 8); break;
    case "mem": value = v.mem; break;
    case "workingSet": value = v.mem * 1.38; break;
    case "peakWorkingSet": value = instance((p) => p.mem * (1.46 + noise(p.pid * 5 + 2, 0) * 0.5)); break;
    case "commit": value = v.mem * 1.17; break;
    case "virtualSize": value = instance(addressSpace); break;
    case "pageFaults": value = total((p) => faults(p, ctx.tick)); break;
    case "diskRead": value = v.read; break;
    case "diskWrite": value = v.write; break;
    case "io": value = v.io; break;
    case "ioRead": value = ioRead; break;
    case "ioWrite": value = ioWrite; break;
    case "ioOther": value = Math.max(0, v.io - ioRead - ioWrite); break;
    case "ioReadTotal": value = total((p) => ioTotal(p, ctx.tick, 1, 1)); break;
    case "ioWriteTotal": value = total((p) => ioTotal(p, ctx.tick, 2, 0.55)); break;
    case "ioOtherTotal": value = total((p) => ioTotal(p, ctx.tick, 3, 0.12)); break;
    case "net": value = v.net; break;
    case "netReceived": value = v.net * 0.78; break;
    case "netSent": value = v.net * 0.22; break;
    case "gpu": value = v.gpu; break;
    case "gpuEngine": value = !heading && v.gpu >= 0.05 ? "GPU 0 - 3D" : null; break;
    case "gpuDedicated": value = total(dedicated); break;
    case "gpuShared": value = total(sharedGpu); break;
    default: value = null;
  }
  const textual = ["text", "time", "bool"].includes(c.format);
  if (value === null || value === "") return { text: "", sort: textual ? "" : -1 };
  if (typeof value === "string") return { text: value, sort: value };
  switch (c.format) {
    case "percent":
      return { text: percent(value), sort: value, faint: value < 0.05, heat: heat("percent", value) };
    case "memory":
      return { text: value ? bytes(value) : "0", sort: value, faint: !value, heat: c.heat ? heat("memory", value) : 0 };
    case "rate":
      return { text: rate(value), sort: value, faint: value < 1, heat: heat("rate", value) };
    case "count":
      return { text: thousands(value), sort: value };
    case "duration":
      return { text: hms(value), sort: value };
    case "space":
      return { text: value >= TB ? (value / TB).toFixed(2) + " TB" : bytes(value), sort: value };
    case "id":
      return { text: String(value), sort: value, faint: true };
    default:
      return { text: String(value), sort: value };
  }
}
/** What rows sort by in a column other than the name, PID and the live readings. */
export const columnSortValue = (key: string, row: Row, ctx: CellContext) => {
  const c = byKey.get(key);
  return c ? cellOf(c, row, ctx).sort : 0;
};

// ---------------------------------------------------------------------------
// The header's menu: the column under the pointer sorted or hidden; every
// column by category, checked where it is (the submenu stays open); then the
// dialog, automatic widths and the defaults.
export function headerMenu(o: {
  column: string | null;
  prefs: ColumnPrefs;
  onSort: (key: string, descending: boolean) => void;
  onToggle: (key: string) => void;
  onChoose: () => void;
  onAutoWidth: () => void;
  onReset: () => void;
}): MenuEntry[] {
  const items: MenuEntry[] = [];
  const at = o.column ? byKey.get(o.column) : undefined;
  if (at) {
    items.push(
      { label: "升序排序", icon: "sortAscending", run: () => o.onSort(at.key, false) },
      { label: "降序排序", icon: "sortDescending", run: () => o.onSort(at.key, true) },
      {
        label: `隐藏“${columnLabel(at)}”列`,
        icon: "eyeOff",
        disabled: at.key === "name",
        run: () => o.onToggle(at.key),
      },
      "-",
    );
  }
  for (const category of CATEGORIES) {
    const children: MenuEntry[] = COLUMNS.filter((c) => c.category === category).map((c) => ({
      label: columnLabel(c),
      checked: o.prefs.selected.includes(c.key),
      disabled: c.key === "name",
      stay: true,
      run: () => o.onToggle(c.key),
    }));
    const checked = children.filter((c) => c !== "-" && c.checked).length;
    items.push({
      label: categoryLabel[category],
      icon: categoryIcon[category],
      detail: `${checked}/${children.length}`,
      children,
    });
  }
  items.push(
    "-",
    { label: "选择列…", icon: "columns", run: o.onChoose },
    { label: "自动宽度", checked: o.prefs.autoWidth, run: o.onAutoWidth },
    { label: "恢复默认列", icon: "reset", run: o.onReset },
  );
  return items;
}

// ---------------------------------------------------------------------------
// FLIP: an item laid out at a new place is drawn back where it was and
// glides to where it now is (flip.rs), 220 ms on the entrance curve.
const GLIDE = 220;
function ease(t: number) {
  // cubic-bezier(0.1, 0.9, 0.2, 1)
  const at = (u: number, a: number, b: number) => 3 * (1 - u) * (1 - u) * u * a + 3 * (1 - u) * u * u * b + u * u * u;
  let lo = 0,
    hi = 1,
    u = t;
  for (let i = 0; i < 22; i++) {
    u = (lo + hi) / 2;
    if (at(u, 0.1, 0.2) < t) lo = u;
    else hi = u;
  }
  return at(u, 0.9, 1);
}
const stillMotion = () => typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
class Flips {
  private glides = new Map<string, { from: number; start: number }>();
  /** `key` was laid out `by` further along: drawn back where it was, it glides from there. */
  moved(key: string, by: number, now: number) {
    this.glides.set(key, { from: this.offset(key, now) - by, start: now });
  }
  quiet(key: string) {
    this.glides.delete(key);
  }
  offset(key: string, now: number) {
    const g = this.glides.get(key);
    if (!g) return 0;
    const t = (now - g.start) / GLIDE;
    return t <= 0 ? g.from : t >= 1 ? 0 : g.from * (1 - ease(t));
  }
  active(now: number) {
    for (const [key, g] of this.glides) if (now >= g.start + GLIDE) this.glides.delete(key);
    return this.glides.size > 0;
  }
}

// ---------------------------------------------------------------------------
// Widths: each column's format width; with automatic widths a header label
// that needs more room gets it, in 8 px steps (tables/widths.rs).
let measure: CanvasRenderingContext2D | null | undefined;
function labelWidth(text: string, scale: number) {
  measure ??= typeof document === "undefined" ? null : document.createElement("canvas").getContext("2d");
  if (!measure) return 0;
  measure.font = `500 ${12 * scale}px "Segoe UI Variable Text", "Segoe UI", system-ui, "Microsoft YaHei UI", sans-serif`;
  return measure.measureText(text).width;
}
export function useColumnWidths(columns: ColumnDef[], autoWidth: boolean, scale: number) {
  // The server knows no fonts: the first render keeps the format widths.
  const [ready, setReady] = useState(false);
  useEffect(() => setReady(true), []);
  return useMemo(() => {
    const widths = new Map<string, number>();
    for (const c of columns) {
      let w = c.width;
      if (ready && autoWidth && c.key !== "name") {
        // The label, the cell's padding and the sort mark beside it.
        const need = Math.ceil(labelWidth(columnLabel(c), scale) + 20 + 14);
        if (need > w) w += Math.ceil((need - w) / 8) * 8;
      }
      widths.set(c.key, w);
    }
    return widths;
  }, [columns, autoWidth, scale, ready]);
}

// ---------------------------------------------------------------------------
// Sideways scrolling: the rows scroll, the header follows in the same frame.
// A horizontal wheel or Shift+wheel glides there, as the app's does.
export function useTableScroll(
  table: RefObject<HTMLDivElement | null>,
  head: RefObject<HTMLDivElement | null>,
  body: RefObject<HTMLDivElement | null>,
) {
  useEffect(() => {
    const t = table.current,
      h = head.current,
      b = body.current;
    if (!t || !h || !b) return;
    let target = 0,
      frame = 0;
    const sync = () => {
      if (h.scrollLeft !== b.scrollLeft) h.scrollLeft = b.scrollLeft;
      t.toggleAttribute("data-scrolled", b.scrollLeft > 0.5);
    };
    const step = () => {
      const gap = target - b.scrollLeft;
      b.scrollLeft += Math.abs(gap) <= 1 ? gap : Math.sign(gap) * Math.max(1, Math.round(Math.abs(gap) * 0.3));
      sync();
      frame = Math.abs(target - b.scrollLeft) > 0.5 && b.scrollLeft > 0 && b.scrollLeft < b.scrollWidth - b.clientWidth ? requestAnimationFrame(step) : 0;
    };
    const wheel = (event: WheelEvent) => {
      const along = event.shiftKey ? event.deltaX || event.deltaY : Math.abs(event.deltaX) > Math.abs(event.deltaY) ? event.deltaX : 0;
      const room = b.scrollWidth - b.clientWidth;
      if (!along || room <= 0) return;
      event.preventDefault();
      const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? b.clientWidth : 1;
      target = Math.max(0, Math.min(room, (frame ? target : b.scrollLeft) + along * unit));
      if (stillMotion()) {
        b.scrollLeft = target;
        sync();
      } else if (!frame) frame = requestAnimationFrame(step);
    };
    b.addEventListener("scroll", sync, { passive: true });
    t.addEventListener("wheel", wheel, { passive: false });
    sync();
    return () => {
      cancelAnimationFrame(frame);
      b.removeEventListener("scroll", sync);
      t.removeEventListener("wheel", wheel);
    };
  }, [table, head, body]);
}

// ---------------------------------------------------------------------------
// Dragging a header (tables.rs `follow_header`): the column follows the
// pointer within the scrolled columns, raised on a strip from the header to
// the bottom of the rows; once its middle passes a neighbour's, the two trade
// places at once (the order is saved) and the one passed glides to its new
// place. Let go, the column glides into its slot, still over its strip.
interface HeaderDrag {
  key: string;
  grab: number;
  pointer: number;
  start: number;
  lo: number;
  hi: number;
  origin: number;
  places: Map<string, number>;
}
export interface Lifted {
  key: string;
  width: number;
  /** The scrolled columns' lane, in the table's own pixels. */
  left: number;
  right: number;
  bottom: number;
}
const reach = (d: HeaderDrag) => d.start + d.pointer - d.grab;
const dragAt = (d: HeaderDrag) => Math.min(Math.max(reach(d), d.lo), Math.max(d.hi, d.lo));
function placesOf(keys: string[], widths: Map<string, number>, origin: number) {
  const places = new Map<string, number>();
  let x = origin;
  for (const key of keys) {
    if (key === "name") continue;
    places.set(key, x);
    x += widths.get(key) ?? 0;
  }
  return places;
}
export function useHeaderDrag(o: {
  table: RefObject<HTMLDivElement | null>;
  head: RefObject<HTMLDivElement | null>;
  body: RefObject<HTMLDivElement | null>;
  columns: ColumnDef[];
  widths: Map<string, number>;
  onMove: (key: string, before: string | null) => void;
}) {
  const latest = useRef(o);
  useIsoLayoutEffect(() => {
    latest.current = o;
  });
  const [lifted, setLifted] = useState<Lifted | null>(null);
  const raised = useRef<Lifted | null>(null);
  const drag = useRef<HeaderDrag | null>(null);
  const flips = useRef(new Flips());
  const frame = useRef(0);
  const swallow = useRef(false);
  const release = useRef<() => void>(() => {});
  useEffect(
    () => () => {
      cancelAnimationFrame(frame.current);
      release.current();
    },
    [],
  );

  // Where each column is drawn this frame, as variables the moving cells read.
  const paint = () => {
    const { table, body, columns, widths } = latest.current;
    const t = table.current,
      b = body.current;
    if (!t || !b) return false;
    const now = performance.now();
    const d = drag.current;
    const lift = raised.current;
    let lifted = 0;
    for (const c of columns) {
      if (c.key === "name") continue;
      const dx = d && d.key === c.key ? dragAt(d) - (d.places.get(c.key) ?? 0) : flips.current.offset(c.key, now);
      t.style.setProperty(`--dx-${c.key}`, `${dx}px`);
      if (lift?.key === c.key) lifted = dx;
    }
    if (lift) {
      const places = d?.places ?? placesOf(columns.map((c) => c.key), widths, lift.left);
      t.style.setProperty("--a-strip-x", `${(places.get(lift.key) ?? 0) + lifted - b.scrollLeft - lift.left}px`);
    }
    return !!d || flips.current.active(now);
  };
  const loop = () => {
    frame.current = 0;
    if (paint()) frame.current = requestAnimationFrame(loop);
    else if (raised.current) {
      raised.current = null;
      setLifted(null);
    }
  };
  const kick = () => {
    if (!frame.current) frame.current = requestAnimationFrame(loop);
  };

  const begin = (key: string, x: number) => {
    const { table, head, body, columns, widths } = latest.current;
    const t = table.current!,
      b = body.current!;
    const name = head.current?.querySelector<HTMLElement>(".a-col-name");
    // The header's name starts after its 4 px inset; the rows' name, as wide, at 0.
    const origin = 4 + (name?.offsetWidth ?? NAME_MIN - 4);
    const keys = columns.map((c) => c.key);
    const places = placesOf(keys, widths, origin);
    const width = widths.get(key) ?? 0;
    const last = keys[keys.length - 1];
    const right = (places.get(last) ?? origin) + (widths.get(last) ?? 0);
    drag.current = { key, grab: x, pointer: x, start: places.get(key) ?? origin, lo: origin, hi: right - width, origin, places };
    flips.current.quiet(key);
    const lift: Lifted = {
      key,
      width,
      left: origin,
      right: b.offsetWidth - b.clientWidth,
      bottom: t.offsetHeight - b.offsetTop - b.clientHeight,
    };
    raised.current = lift;
    flushSync(() => setLifted(lift));
  };
  const follow = (d: HeaderDrag) => {
    const { columns, widths, onMove } = latest.current;
    let keys = columns.map((c) => c.key);
    const w = (key: string) => widths.get(key) ?? 0;
    const now = performance.now();
    let changed = false;
    for (;;) {
      const i = keys.indexOf(d.key);
      if (i < 0) break;
      const middle = reach(d) + w(d.key) / 2;
      const middleOf = (j: number) => (d.places.get(keys[j]) ?? 0) + w(keys[j]) / 2;
      const to = i + 1 < keys.length && middle > middleOf(i + 1) ? i + 1 : i > 1 && middle < middleOf(i - 1) ? i - 1 : -1;
      if (to < 0) break;
      const next = [...keys];
      [next[i], next[to]] = [next[to], next[i]];
      const after = placesOf(next, widths, d.origin);
      if (!stillMotion())
        for (const [key, place] of after) {
          const before = d.places.get(key);
          if (key !== d.key && before !== undefined && before !== place) flips.current.moved(key, place - before, now);
        }
      d.places = after;
      keys = next;
      changed = true;
    }
    if (changed) {
      const i = keys.indexOf(d.key);
      flushSync(() => onMove(d.key, keys[i + 1] ?? null));
    }
  };
  const settle = () => {
    const d = drag.current;
    if (!d) return;
    drag.current = null;
    swallow.current = true;
    const place = d.places.get(d.key) ?? d.start;
    if (!stillMotion()) flips.current.moved(d.key, place - dragAt(d), performance.now());
    paint();
    kick();
  };

  /** A press on a header: a move of a few pixels starts dragging its column. */
  const press = (event: ReactPointerEvent<HTMLElement>, key: string) => {
    swallow.current = false;
    if (event.button !== 0 || key === "name" || drag.current) return;
    const { table, body } = latest.current;
    const t = table.current,
      b = body.current;
    if (!t || !b) return;
    const xOf = (e: { clientX: number }) => {
      const box = t.getBoundingClientRect();
      return (e.clientX - box.left) / (t.offsetWidth ? box.width / t.offsetWidth : 1) + b.scrollLeft;
    };
    const x0 = xOf(event);
    let started = false;
    const move = (e: PointerEvent) => {
      const x = xOf(e);
      if (!started) {
        if (Math.abs(x - x0) < 4) return;
        started = true;
        begin(key, x0);
      }
      const d = drag.current;
      if (!d) return;
      d.pointer = x;
      follow(d);
      paint();
      kick();
    };
    const done = () => {
      release.current();
      if (started) settle();
    };
    release.current = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", done);
      window.removeEventListener("pointercancel", done);
      release.current = () => {};
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", done);
    window.addEventListener("pointercancel", done);
  };
  /** Whether the click that ends a drag should be ignored (it is not a sort). */
  const dragged = () => {
    const was = swallow.current;
    swallow.current = false;
    return was;
  };
  // While a column is raised, the moving cells take their offsets.
  const style = lifted
    ? o.columns
        .filter((c) => c.key !== "name")
        .map((c) => `.a-ptable[data-moving] .a-col-${c.key}{translate:var(--dx-${c.key},0px) 0}`)
        .join("") + `.a-ptable[data-moving] .a-col-${lifted.key}{position:relative;z-index:2}`
    : "";
  return { press, dragged, lifted, style };
}

// ---------------------------------------------------------------------------
// The "Select columns" dialog: the columns by category to check on the left,
// the shown ones in order on the right to drag. Nothing applies before OK.
export function ColumnsDialog({
  prefs,
  onApply,
  onClose,
}: {
  prefs: ColumnPrefs;
  onApply: (next: ColumnPrefs) => void;
  onClose: () => void;
}) {
  const [draft, setDraft] = useState(prefs);
  const [category, setCategory] = useState<"all" | Category>("all");
  const [query, setQuery] = useState("");
  const field = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const searching = !!query.trim();
  const shown = searching ? "all" : category;
  const listed = matching(shown, query);
  const sections = CATEGORIES.map((c) => [c, listed.filter((x) => x.category === c)] as const).filter(([, l]) => l.length);
  // Another search starts from the top of the list.
  useIsoLayoutEffect(() => {
    if (list.current) list.current.scrollTop = 0;
  }, [query]);
  const chosen = (key: string) => draft.selected.includes(key);
  return (
    <div className="a-dialog is-columns" role="dialog" tabIndex={-1} aria-modal="true" aria-labelledby="a-dialog-title">
      <div className="a-dialog-body a-columns-body">
        <div className="a-columns-head">
          <h3 id="a-dialog-title">选择列</h3>
          <p>勾选要显示的列；拖动右侧列表或表头可调整顺序。</p>
        </div>
        <label className="a-field">
          <input
            ref={field}
            type="search"
            value={query}
            placeholder="搜索列名或说明"
            aria-label="搜索列名或说明"
            spellCheck={false}
            autoComplete="off"
            onChange={(e) => setQuery(e.target.value)}
          />
          {query && (
            <button
              type="button"
              className="a-search-clear"
              aria-label="清除搜索"
              title="清除搜索"
              onClick={() => {
                setQuery("");
                field.current?.focus();
              }}
            >
              <Icon name="dismiss" size={12} />
            </button>
          )}
        </label>
        <div className="a-columns-panes">
          <div className="a-columns-choices">
            <div className="a-columns-cats">
              <Segments
                label="分类"
                value={shown}
                onChange={(next) => {
                  setCategory(next);
                  setQuery("");
                }}
                items={[
                  { value: "all" as const, label: "全部" },
                  ...CATEGORIES.map((c) => ({ value: c, label: categoryLabel[c] })),
                ]}
              />
            </div>
            {/* Another category's choices fade in rather than replace the last at once. */}
            <div className="a-columns-list a-scroll" ref={list} key={shown}>
              {sections.length === 0 && <p className="a-columns-none">没有匹配的列</p>}
              {sections.map(([category, columns]) => {
                const keys = columns.map((c) => c.key).filter((key) => key !== "name");
                const all = keys.every(chosen);
                const inCategory = COLUMNS.filter((c) => c.category === category);
                return (
                  <section className="a-columns-section" key={category}>
                    <div className="a-columns-section-head">
                      <Icon name={categoryIcon[category]} size={14} />
                      <span className="a-columns-section-name">{categoryLabel[category]}</span>
                      <span className="a-columns-section-count">
                        {inCategory.filter((c) => chosen(c.key)).length}/{inCategory.length}
                      </span>
                      {keys.length > 0 && (
                        <Button
                          small
                          tone="ghost"
                          onClick={() => setDraft((d) => keys.reduce((p, key) => setSelected(p, key, !all), d))}
                        >
                          {all ? "清空" : "全选"}
                        </Button>
                      )}
                    </div>
                    {columns.map((c) => {
                      const fixed = c.key === "name";
                      const on = chosen(c.key);
                      return (
                        <button
                          key={c.key}
                          type="button"
                          role="checkbox"
                          aria-checked={on}
                          aria-disabled={fixed || undefined}
                          className="a-columns-choice"
                          title={c.title}
                          onClick={() => !fixed && setDraft((d) => setSelected(d, c.key, !on))}
                        >
                          <span className={"a-columns-box" + (on ? " is-on" : "") + (fixed ? " is-fixed" : "")}>
                            {on && <Icon name="check" size={12} />}
                          </span>
                          <span className="a-columns-name" data-i18n="column">
                            {c.label}
                          </span>
                          <span className="a-columns-desc">{c.title}</span>
                          {fixed && <span className="a-columns-tag">固定</span>}
                        </button>
                      );
                    })}
                  </section>
                );
              })}
            </div>
          </div>
          <OrderList draft={draft} onDraft={setDraft} />
        </div>
      </div>
      <div className="a-dialog-foot a-columns-foot">
        <Button small tone="ghost" icon="reset" onClick={() => setDraft(defaultPrefs())}>
          恢复默认列
        </Button>
        <div className="a-columns-actions">
          <button type="button" className="a-btn is-accent" data-primary onClick={() => onApply(draft)}>
            确定
          </button>
          <button type="button" className="a-btn" onClick={onClose}>
            取消
          </button>
        </div>
      </div>
    </div>
  );
}

/** A place in the order list, and the distance from one place to the next. */
const PLACE = 32;
const PITCH = PLACE + 2;
/** Where a dragged place moves once its middle passes a neighbour's: before
 *  the place now at the result (`count` past the last), never before the name. */
function orderStep(index: number, count: number, middle: number) {
  const middleOf = (at: number) => at * PITCH + PLACE / 2;
  if (index + 1 < count && middle > middleOf(index + 1)) return index + 2;
  if (index > 1 && middle < middleOf(index - 1)) return index - 1;
  return null;
}
interface OrderDrag {
  key: string;
  grab: number;
  pointer: number;
  start: number;
}
const orderAt = (d: OrderDrag) => d.start + d.pointer - d.grab;

/** The shown columns in their order, the fixed name first. A dragged place
 *  leaves a tinted slot and is drawn raised under the pointer; the others make
 *  room, gliding to their new places. */
function OrderList({ draft, onDraft }: { draft: ColumnPrefs; onDraft: (next: ColumnPrefs) => void }) {
  const keys = draft.selected;
  const list = useRef<HTMLDivElement>(null);
  const rows = useRef(new Map<string, HTMLDivElement>());
  const flips = useRef(new Flips());
  const placed = useRef(new Map<string, number>());
  const frame = useRef(0);
  const dragRef = useRef<OrderDrag | null>(null);
  const [drag, setDrag] = useState<OrderDrag | null>(null);
  const latest = useRef({ draft, onDraft });
  useIsoLayoutEffect(() => {
    latest.current = { draft, onDraft };
  });
  const paint = () => {
    frame.current = 0;
    const now = performance.now();
    for (const [key, el] of rows.current) {
      const dy = flips.current.offset(key, now);
      el.style.translate = dy ? `0 ${dy}px` : "";
    }
    if (flips.current.active(now)) frame.current = requestAnimationFrame(paint);
  };
  useEffect(() => () => cancelAnimationFrame(frame.current), []);
  // A place laid out somewhere new glides there from where it was; one that
  // joins the list fades in.
  useIsoLayoutEffect(() => {
    const now = performance.now();
    const lifted = dragRef.current?.key;
    const still = stillMotion();
    keys.forEach((key, index) => {
      const place = index * PITCH;
      const before = placed.current.get(key);
      placed.current.set(key, place);
      if (before === undefined) {
        if (!still) rows.current.get(key)?.animate({ opacity: [0, 1] }, { duration: 160, easing: "cubic-bezier(0.1, 0.9, 0.2, 1)" });
        return;
      }
      if (key === lifted || still) flips.current.quiet(key);
      else if (before !== place) flips.current.moved(key, place - before, now);
    });
    for (const key of [...placed.current.keys()]) if (!keys.includes(key)) placed.current.delete(key);
    cancelAnimationFrame(frame.current);
    paint();
  }, [keys]);

  const press = (event: ReactPointerEvent<HTMLDivElement>, key: string, index: number) => {
    if (event.button !== 0 || (event.target as Element).closest("button")) return;
    const el = list.current;
    if (!el) return;
    event.preventDefault();
    const yOf = (e: { clientY: number }) => {
      const box = el.getBoundingClientRect();
      return (e.clientY - box.top) / (el.offsetHeight ? box.height / el.offsetHeight : 1) + el.scrollTop;
    };
    const y0 = yOf(event);
    let started = false;
    const move = (e: PointerEvent) => {
      const y = yOf(e);
      if (!started) {
        if (Math.abs(y - y0) < 3) return;
        started = true;
        flips.current.quiet(key);
        dragRef.current = { key, grab: y0, pointer: y0, start: index * PITCH };
      }
      const d = { ...dragRef.current!, pointer: y };
      dragRef.current = d;
      // Trade places with each neighbour whose middle it has passed.
      let next = latest.current.draft;
      for (;;) {
        const order = next.selected;
        const step = orderStep(order.indexOf(key), order.length, orderAt(d) + PLACE / 2);
        if (step === null) break;
        const moved = moveBefore(next, key, order[step] ?? null);
        if (moved === next) break;
        next = moved;
      }
      flushSync(() => {
        if (next !== latest.current.draft) latest.current.onDraft(next);
        setDrag(d);
      });
    };
    const done = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", done);
      window.removeEventListener("pointercancel", done);
      const d = dragRef.current;
      dragRef.current = null;
      if (!started || !d) return;
      // The place glides from under the pointer into its slot.
      const place = latest.current.draft.selected.indexOf(d.key) * PITCH;
      placed.current.set(d.key, place);
      if (!stillMotion()) flips.current.moved(d.key, place - orderAt(d), performance.now());
      setDrag(null);
      if (!frame.current) frame.current = requestAnimationFrame(paint);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", done);
    window.addEventListener("pointercancel", done);
  };

  const face = (c: ColumnDef) => {
    const fixed = c.key === "name";
    return (
      <>
        <Icon name={fixed ? "lock" : "grip"} size={14} />
        <span className="a-order-name" data-i18n="column">
          {c.label}
        </span>
        <span className="a-order-tag">{fixed ? "固定" : categoryLabel[c.category]}</span>
      </>
    );
  };
  const count = keys.length;
  const raised = drag && byKey.get(drag.key);
  return (
    <div className="a-columns-order">
      <div className="a-columns-order-head">
        <b>显示顺序</b>
        <span>{`已选 ${count} 列`}</span>
      </div>
      <div className={"a-order-list a-scroll" + (drag ? " is-dragging" : "")} ref={list}>
        {keys.map((key, index) => {
          const c = byKey.get(key)!;
          const fixed = key === "name";
          const lifted = drag?.key === key;
          const up = index > 1 ? keys[index - 1] : undefined;
          const down = index + 1 < count;
          return (
            <div
              key={key}
              ref={(el) => {
                if (el) rows.current.set(key, el);
                else rows.current.delete(key);
              }}
              className={"a-order-row" + (fixed ? "" : " is-movable") + (lifted ? " is-slot" : "")}
              onPointerDown={fixed ? undefined : (event) => press(event, key, index)}
            >
              {face(c)}
              {!fixed && (
                <span className="a-order-tools">
                  <Button
                    small
                    tone="ghost"
                    icon="chevronUp"
                    title="上移"
                    disabled={!up}
                    onClick={() => up && onDraft(moveBefore(draft, key, up))}
                  />
                  <Button
                    small
                    tone="ghost"
                    icon="chevronDown"
                    title="下移"
                    disabled={!down}
                    onClick={() => down && onDraft(moveBefore(draft, key, keys[index + 2] ?? null))}
                  />
                  <Button small tone="ghost" icon="dismiss" title="移除" onClick={() => onDraft(setSelected(draft, key, false))} />
                </span>
              )}
            </div>
          );
        })}
        {drag && raised && (
          <div
            className="a-order-raised"
            style={{ top: Math.max(0, Math.min((count - 1) * PITCH, orderAt(drag))) }}
            aria-hidden="true"
          >
            {face(raised)}
          </div>
        )}
      </div>
    </div>
  );
}
