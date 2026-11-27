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
import { language, translate } from "./i18n";
import { Icon, type UiIcon } from "./icons";
import {
  appVersion,
  gpuEngine,
  heat,
  memory,
  noise,
  percent,
  rate,
  sessionOf,
  thousands,
  transferred,
  type Process,
  type Reading,
} from "./model";
import type { MenuEntry } from "./Pages";
import type { Row } from "./Processes";
import { Button, Segments, useIsoLayoutEffect } from "./widgets";

export type Category = "general" | "cpu" | "memory" | "io" | "network" | "gpu";
/** "events": a count per second, in whole numbers (columns.rs `EventRate`). */
type Format = "name" | "text" | "id" | "count" | "events" | "percent" | "memory" | "space" | "rate" | "duration" | "time" | "bool" | "power";
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
  heat?: "percent" | "memory" | "rate" | "power";
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
  col("name", "general", "name", 220, false, "名称", "应用名称或文件描述；次级文字显示可执行文件名。"),
  col("imageName", "general", "text", 180, false, "映像名称", "可执行文件的名称。"),
  col("pid", "general", "id", 72, false, "PID", "进程 ID"),
  col("ppid", "general", "id", 82, false, "父进程 ID", "创建时记录的父进程 ID。"),
  col("path", "general", "text", 300, false, "映像路径", "进程可执行文件的完整路径。"),
  col("description", "general", "text", 220, false, "描述", "可执行文件描述或打包应用名称。"),
  col("company", "general", "text", 200, false, "公司名称", "可执行文件版本信息中的公司名称。"),
  col("version", "general", "text", 130, false, "文件版本", "可执行文件版本信息中的文件版本。"),
  col("architecture", "general", "text", 110, false, "架构", "进程运行的处理器架构。"),
  col("elevated", "general", "bool", 90, false, "已提升", "进程是否以管理员权限运行。"),
  col("commandLine", "general", "text", 360, false, "命令行", "启动进程时使用的完整命令行，包括参数。"),
  col("sessionId", "general", "id", 80, false, "会话 ID", "进程所属的 Windows 会话编号。"),
  col("createTime", "general", "time", 160, false, "启动时间", "进程创建时间，以本地时间显示。"),
  col("threads", "general", "count", 76, true, "线程", "线程数"),
  col("peakThreads", "general", "count", 100, false, "线程峰值", "单进程自启动以来同时存在的最多线程数；展开应用分组查看各进程。"),
  col("handles", "general", "count", 86, true, "句柄", "句柄数"),
  col("children", "general", "count", 90, false, "子进程", "由该进程启动、仍在运行的进程数；展开应用分组查看各进程。"),
  col("runtime", "general", "duration", 110, false, "运行时长", "单进程自启动以来经过的时间；展开应用分组查看各进程。"),
  col("suspended", "general", "bool", 90, false, "已暂停", "进程的全部线程处于暂停等待状态。"),
  col("efficiency", "general", "bool", 100, false, "效率模式", "进程的 EcoQoS 节能状态。"),
  col("power", "general", "power", 120, true, "电源使用情况", "按 CPU、GPU 与磁盘活动估算的当前耗电水平；分组显示成员合计。", "power"),
  col("powerTrend", "general", "power", 140, true, "电源使用情况趋势", "最近约两分钟的平均耗电水平，按 CPU、GPU 与磁盘活动估算；分组显示成员合计。", "power"),
  col("cpu", "cpu", "percent", 70, true, "CPU", "占全部处理器能力的百分比", "percent"),
  col("cpuKernel", "cpu", "percent", 90, true, "内核 CPU", "CPU 占用中内核态的部分，按本次刷新间隔内核态与用户态 CPU 时间之比拆分；分组显示成员合计。", "percent"),
  col("cpuUser", "cpu", "percent", 90, true, "用户 CPU", "CPU 占用中用户态的部分，按本次刷新间隔内核态与用户态 CPU 时间之比拆分；分组显示成员合计。", "percent"),
  col("cpuTime", "cpu", "duration", 100, true, "CPU 时间", "累计用户态与内核态 CPU 时间；分组显示成员合计。"),
  col("cpuUserTime", "cpu", "duration", 115, true, "用户 CPU 时间", "进程累计使用的用户态 CPU 时间。"),
  col("cpuKernelTime", "cpu", "duration", 115, true, "内核 CPU 时间", "进程累计使用的内核态 CPU 时间。"),
  col("basePriority", "cpu", "id", 100, false, "基本优先级", "通常显示优先级类别，介于两类之间的基本优先级显示为数值。"),
  col("contextSwitchRate", "cpu", "events", 130, true, "每秒上下文切换", "进程各线程每秒的上下文切换次数；分组显示成员合计。"),
  col("contextSwitches", "cpu", "count", 120, true, "上下文切换次数", "进程现有线程累计的上下文切换次数，不含已退出的线程；分组显示成员合计。"),
  col("mem", "memory", "memory", 96, true, "内存", "专用工作集：进程独占的物理内存", "memory"),
  col("workingSet", "memory", "memory", 110, true, "工作集", "当前驻留的物理内存，含共享页面；分组为成员工作集之和。", "memory"),
  col("sharedWs", "memory", "memory", 120, true, "共享工作集", "工作集中可与其他进程共享的部分，即工作集减去专用工作集；分组为成员之和。"),
  col("peakWorkingSet", "memory", "memory", 125, false, "工作集峰值", "单进程自启动以来的工作集峰值；展开应用分组查看各进程。"),
  col("commit", "memory", "memory", 110, true, "提交大小", "进程专用的已提交内存。", "memory"),
  col("peakCommit", "memory", "memory", 125, false, "提交大小峰值", "单进程自启动以来专用已提交内存的峰值；展开应用分组查看各进程。"),
  col("virtualSize", "memory", "space", 150, false, "虚拟地址空间", "单进程预留与已提交的虚拟地址范围；已提交内存见“提交大小”。"),
  col("peakVirtualSize", "memory", "space", 160, false, "虚拟地址空间峰值", "单进程自启动以来预留与已提交虚拟地址范围的峰值；展开应用分组查看各进程。"),
  col("pagedPool", "memory", "memory", 100, true, "分页池", "记在该进程名下的内核分页池内存；分组显示成员合计。"),
  col("nonPagedPool", "memory", "memory", 110, true, "非分页池", "记在该进程名下的内核非分页池内存，始终驻留物理内存；分组显示成员合计。"),
  col("pageFaults", "memory", "count", 110, true, "缺页次数", "累计缺页次数，包含软缺页与硬缺页。"),
  col("pageFaultRate", "memory", "events", 120, true, "每秒缺页次数", "每秒缺页次数，包含软缺页与硬缺页；分组显示成员合计。"),
  col("hardFaults", "memory", "count", 110, true, "硬缺页次数", "累计硬缺页次数，即需从磁盘读入页面的缺页；分组显示成员合计。"),
  col("hardFaultRate", "memory", "events", 130, true, "每秒硬缺页次数", "每秒需从磁盘读入页面的缺页次数，持续偏高通常表示物理内存不足；分组显示成员合计。"),
  col("memoryPriority", "memory", "text", 120, false, "内存优先级", "内存紧张时回收该进程内存页的先后，越低越先回收。"),
  col("diskRead", "io", "rate", 104, true, "磁盘读取", "每秒磁盘读取字节数，不包含网络及其他设备 I/O", "rate"),
  col("diskWrite", "io", "rate", 104, true, "磁盘写入", "每秒磁盘写入字节数，不包含网络及其他设备 I/O", "rate"),
  col("diskReadOps", "io", "events", 130, true, "每秒磁盘读取次数", "每秒向磁盘发出的读取请求数，不包含网络及其他设备 I/O。"),
  col("diskWriteOps", "io", "events", 130, true, "每秒磁盘写入次数", "每秒向磁盘发出的写入请求数，不包含网络及其他设备 I/O。"),
  col("diskReadTotal", "io", "memory", 130, true, "累计磁盘读取", "进程生存期内累计从磁盘读取的字节数。"),
  col("diskWriteTotal", "io", "memory", 130, true, "累计磁盘写入", "进程生存期内累计写入磁盘的字节数。"),
  col("diskReadOpsTotal", "io", "count", 140, true, "累计磁盘读取次数", "进程生存期内累计的磁盘读取请求数。"),
  col("diskWriteOpsTotal", "io", "count", 140, true, "累计磁盘写入次数", "进程生存期内累计的磁盘写入请求数。"),
  col("io", "io", "rate", 92, true, "I/O", "读、写及其他 I/O 合计，包含文件、网络和设备访问", "rate"),
  col("ioRead", "io", "rate", 110, true, "I/O 读取", "每秒 I/O 读取字节数，含文件、网络和设备访问。", "rate"),
  col("ioWrite", "io", "rate", 110, true, "I/O 写入", "每秒 I/O 写入字节数，含文件、网络和设备访问。", "rate"),
  col("ioOther", "io", "rate", 110, true, "其他 I/O", "其他 I/O 操作每秒传输的字节数。", "rate"),
  col("ioReadOps", "io", "events", 140, true, "每秒 I/O 读取次数", "每秒 I/O 读取操作数，含文件、网络和设备访问。"),
  col("ioWriteOps", "io", "events", 140, true, "每秒 I/O 写入次数", "每秒 I/O 写入操作数，含文件、网络和设备访问。"),
  col("ioOtherOps", "io", "events", 140, true, "每秒其他 I/O 次数", "每秒读写以外的 I/O 操作数，如设备控制请求。"),
  col("ioReadTotal", "io", "memory", 130, true, "累计 I/O 读取", "进程生存期内累计读取的 I/O 字节数。"),
  col("ioWriteTotal", "io", "memory", 130, true, "累计 I/O 写入", "进程生存期内累计写入的 I/O 字节数。"),
  col("ioOtherTotal", "io", "memory", 130, true, "累计其他 I/O", "进程生存期内其他 I/O 累计传输的字节数。"),
  col("ioReadOpsTotal", "io", "count", 150, true, "累计 I/O 读取次数", "进程生存期内累计的 I/O 读取操作数。"),
  col("ioWriteOpsTotal", "io", "count", 150, true, "累计 I/O 写入次数", "进程生存期内累计的 I/O 写入操作数。"),
  col("ioOtherOpsTotal", "io", "count", 150, true, "累计其他 I/O 次数", "进程生存期内累计的其他 I/O 操作数。"),
  col("ioPriority", "io", "text", 110, false, "I/O 优先级", "进程磁盘与其他 I/O 请求的优先级提示。"),
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
/** Memory columns shown in bytes to the kilobyte (columns.rs `byte_count`): transfer totals and kernel pools. */
const byteCount = new Set(["ioReadTotal", "ioWriteTotal", "ioOtherTotal", "diskReadTotal", "diskWriteTotal", "pagedPool", "nonPagedPool"]);
const DEFAULTS = ["name", "pid", "cpu", "mem", "diskRead", "diskWrite", "io", "gpu", "net", "threads", "handles"];
const VIRTUAL_ALIASES = "虚拟内存 虚拟字节 地址预留";

/** Text columns read from the start of their cell, figures from its end. */
export const alignsEnd = (c: ColumnDef) => !["name", "text", "time", "bool", "power"].includes(c.format);
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
  /** The name takes the room the other columns leave (automatic widths only). */
  fullWidth: boolean;
  /** Widths dragged by hand; such a column keeps its width and takes no share. */
  widths: Record<string, number>;
}
type Sizing = Pick<ColumnPrefs, "autoWidth" | "fullWidth" | "widths">;
function normalize(selected: string[], order: string[], sizing: Sizing): ColumnPrefs {
  const seen = new Set(["name"]);
  const all = ["name"];
  for (const key of [...order, ...selected, ...COLUMNS.map((c) => c.key)])
    if (byKey.has(key) && !seen.has(key)) {
      seen.add(key);
      all.push(key);
    }
  const chosen = new Set([...selected, "name"]);
  return {
    selected: all.filter((key) => chosen.has(key)),
    order: all,
    autoWidth: sizing.autoWidth,
    fullWidth: sizing.fullWidth,
    widths: sizing.widths,
  };
}
export const defaultPrefs = () => normalize(DEFAULTS, [], { autoWidth: true, fullWidth: false, widths: {} });
export function setSelected(p: ColumnPrefs, key: string, on: boolean): ColumnPrefs {
  if (key === "name" || !byKey.has(key) || p.selected.includes(key) === on) return p;
  return normalize(on ? [...p.selected, key] : p.selected.filter((k) => k !== key), p.order, p);
}
/** A column dragged to `width`, or back to its automatic width (null). */
export function setWidth(p: ColumnPrefs, key: string, width: number | null): ColumnPrefs {
  const widths = { ...p.widths };
  if (width === null) delete widths[key];
  else widths[key] = Math.round(Math.min(1200, Math.max(48, width)));
  return { ...p, widths };
}
/** Moves a shown column before another (past the last for null), never before the name. */
export function moveBefore(p: ColumnPrefs, key: string, before: string | null): ColumnPrefs {
  if (key === "name" || key === before || before === "name" || !p.selected.includes(key)) return p;
  if (before !== null && !byKey.has(before)) return p;
  const order = p.order.filter((k) => k !== key);
  const at = before === null ? order.length : order.indexOf(before);
  order.splice(at < 0 ? order.length : at, 0, key);
  const next = normalize(p.selected, order, p);
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
const quiet = new Set(["cargo.exe", "rustc.exe", "rust-analyzer.exe", "rust-analyzer-proc-macro-srv.exe", "argus.exe"]);
const isQuiet = (p: Process) => quiet.has(p.exe) || p.exe.startsWith("argus-service-");
const userTime = (p: Process) => Math.round(p.cpuTime * (0.58 + noise(p.pid * 3 + 1, 0) * 0.3));
const describe = (p: Process) =>
  p.glyph === "kernel" ? "" : p.exe === "svchost.exe" ? "Windows 服务主机进程" : p.exe === "msedgewebview2.exe" ? "Microsoft Edge WebView2" : p.name;
// The publishers that are not Microsoft, by image name.
const companies: Record<string, string> = {
  "node.exe": "Node.js",
  "chrome.exe": "Google LLC",
  "Discord.exe": "Discord Inc.",
  "steam.exe": "Valve Corporation",
  "steamwebhelper.exe": "Valve Corporation",
  "NVDisplay.Container.exe": "NVIDIA Corporation",
  "RtkAudUService64.exe": "Realtek Semiconductor",
  "OneApp.IGCC.WinService.exe": "Intel Corporation",
  "jhi_service.exe": "Intel Corporation",
  "IntelCpHDCPSvc.exe": "Intel Corporation",
  "WMIRegistrationService.exe": "Intel Corporation",
  "ContosoSync.exe": "Contoso Ltd.",
};
export const company = (p: Process) =>
  p.glyph === "kernel" || isQuiet(p) ? "" : (companies[p.exe] ?? "Microsoft Corporation");
// Version resources: Windows' own programs share the system's, build tools
// carry none (made-up but plausible numbers).
const products: Record<string, [string, string]> = {
  "msedge.exe": ["Microsoft Edge", "131.0.2903.112"],
  "Code.exe": ["Visual Studio Code", "1.96.2"],
  "WindowsTerminal.exe": ["Windows Terminal", "1.21.3231.0"],
  "mspaint.exe": ["Microsoft Paint", "11.2410.28.0"],
  "Notepad.exe": ["Windows Notepad", "11.2410.21.0"],
  "argus.exe": ["Argus", appVersion],
  "pwsh.exe": ["PowerShell", "7.4.6.500"],
  "node.exe": ["Node.js", "22.12.0"],
  "OneDrive.exe": ["Microsoft OneDrive", "24.226.1110.0004"],
  "CalculatorApp.exe": ["Windows Calculator", "11.2410.0.0"],
  "PhoneExperienceHost.exe": ["Microsoft Phone Link", "1.24112.89.0"],
  "MsMpEng.exe": ["Microsoft Malware Protection", "4.18.24090.11"],
  "MpDefenderCoreService.exe": ["Microsoft Malware Protection", "4.18.24090.11"],
  "NisSrv.exe": ["Microsoft Malware Protection", "4.18.24090.11"],
  "chrome.exe": ["Google Chrome", "131.0.6778.205"],
  "msedgewebview2.exe": ["Microsoft Edge WebView2", "131.0.2903.112"],
  "ms-teams.exe": ["Microsoft Teams", "25290.205.4069.4894"],
  "Discord.exe": ["Discord", "1.0.9175"],
  "steam.exe": ["Steam", "9.81.27.40"],
  "steamwebhelper.exe": ["Steam Client WebHelper", "126.0.6478.183"],
  "OpenConsole.exe": ["Windows Terminal", "1.21.3231.0"],
  "wsl.exe": ["Windows Subsystem for Linux", "2.3.26.0"],
  "wslservice.exe": ["Windows Subsystem for Linux", "2.3.26.0"],
  "FileCoAuth.exe": ["Microsoft OneDrive", "24.226.1110.0004"],
  "Widgets.exe": ["Windows Web Experience Pack", "524.33400.0.0"],
  "WidgetService.exe": ["Windows Web Experience Pack", "524.33400.0.0"],
  "PowerToys.exe": ["PowerToys", "0.86.0"],
  "PowerToys.Awake.exe": ["PowerToys", "0.86.0"],
  "PowerToys.PowerLauncher.exe": ["PowerToys", "0.86.0"],
  "PowerToys.FancyZones.exe": ["PowerToys", "0.86.0"],
  "NVDisplay.Container.exe": ["NVIDIA Container", "1.40.3466.6012"],
  "RtkAudUService64.exe": ["Realtek HD Audio Universal Service", "1.0.0.762"],
  "OneApp.IGCC.WinService.exe": ["Intel(R) Graphics Command Center", "1.100.5536.0"],
  "jhi_service.exe": ["Intel(R) Dynamic Application Loader Host Interface", "2.1.24.1"],
  "IntelCpHDCPSvc.exe": ["Intel(R) Content Protection HDCP Service", "32.0.101.6299"],
  "WMIRegistrationService.exe": ["Intel(R) Management Engine Components", "2406.5.5.0"],
  "ContosoSync.exe": ["Contoso Sync", "2.4.0.0"],
};
export const versionInfo = (p: Process): [string, string] | null =>
  p.glyph === "kernel" || isQuiet(p)
    ? null
    : (products[p.exe] ?? (p.path.startsWith("C:\\Windows\\") ? ["Microsoft® Windows® Operating System", "10.0.26100.2605"] : null));
export const architecture = (p: Process) => (p.glyph === "kernel" ? "" : "64 位 (x64)");
/** The demo runs as administrator: Argus and the system's accounts hold elevated tokens. */
export const elevatedOf = (p: Process) => (p.glyph === "kernel" ? "" : p.exe === "argus.exe" || p.user.startsWith("NT AUTHORITY") ? "是" : "否");
export const commandLine = (p: Process) =>
  p.glyph === "kernel" ? "" : `"${p.path}"` + (p.args ? " " + p.args : "");
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
// The counters 0.7.0 added: made-up but steady, growing with each sample.
const kernelShare = (list: Process[]) => {
  const all = list.reduce((a, p) => a + p.cpuTime, 0);
  return all ? list.reduce((a, p) => a + p.cpuTime - userTime(p), 0) / all : 0.3;
};
const switchRate = (p: Process, tick: number) =>
  p.suspended ? 0 : Math.round((p.cpu ?? 0) * 260 + p.threads * 2.5 + noise(p.pid * 17, tick) * 40);
const switches = (p: Process, tick: number) => Math.round(p.cpuTime * 1100 + p.threads * 5200) + tick * switchRate(p, 0);
const hardFaultRate = (p: Process, tick: number) =>
  p.suspended || !(p.disk ?? 0) ? 0 : Math.round((p.disk ?? 0) * 0.6 * noise(p.pid * 23, tick));
const hardFaults = (p: Process, tick: number) => Math.round(faults(p, 0) * 0.004) + tick * hardFaultRate(p, 0);
// Disk traffic is a share of all I/O; requests average 32 KB read, 16 KB written.
const diskTotal = (p: Process, tick: number, seed: number) => ioTotal(p, tick, seed, 0.7);
/** Seconds since a start time "YYYY/M/D H:MM:SS" on the demo's day, at the demo's 10:24 plus the samples since. */
function runtimeOf(p: Process, tick: number) {
  const [h, m, s] = (p.started.split(" ")[1] ?? "0:0:0").split(":").map(Number);
  return Math.max(0, 10 * 3600 + 24 * 60 + tick - (h * 3600 + m * 60 + s));
}
const dedicated = (p: Process) => (p.gpu ? Math.round(p.gpu * 24 + 12) * MB : 0);
const sharedGpu = (p: Process) => (p.gpu ? Math.round(p.gpu * 5 + 6) * MB : 0);
/** The power estimates the demo makes up: now from a row's readings, the trend from its processes' steady loads. */
export const powerEstimate = (v: Reading) =>
  v.cpu * 0.2 + v.gpu * 0.04 + Math.min(1.5, (v.read + v.write) / (512 * 1024 ** 2));
export const trendEstimate = (p: Process) => (p.cpu ?? 0) * 0.2 + (p.gpu ?? 0) * 0.04 + (p.disk ?? 0) / 512;
const hms = (seconds: number) =>
  `${Math.floor(seconds / 3600)}:${String(Math.floor((seconds % 3600) / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`;

export interface CellContext {
  byPid: Map<number, Process>;
  tick: number;
  efficiency: Set<number>;
  priority: Map<number, string>;
  /** I/O and memory priority set from the menu, by "I/O 优先级" or "内存优先级" and the PID. */
  resource?: Record<string, string>;
  /** Processes the leak alerts mark (a handle leak in the demo). */
  leaks?: Set<number>;
}
/** What a cell without a reading shows (format.missing). */
export const MISSING = "—";
/** The levels of the priority columns, lowest first, as they sort (columns.rs `rank_levels`). */
export const IO_LEVELS = ["非常低", "低", "正常", "高", "关键"];
export const MEMORY_LEVELS = ["非常低", "低", "中", "低于正常", "正常"];
export const POWER_LEVELS = ["非常低", "低", "中", "高", "非常高"];
/** argus_core::power: where each level above the lowest begins. */
export const powerLevel = (estimate: number) => [0.1, 0.3, 0.8, 2].filter((n) => estimate >= n).length;
export const resourceLevel = (ctx: Pick<CellContext, "resource">, kind: "I/O 优先级" | "内存优先级", p: Process) =>
  p.glyph === "kernel" ? "" : (ctx.resource?.[kind + p.pid] ?? "正常");
export interface Cell {
  text: string;
  sort: number | string;
  faint?: boolean;
  heat?: number;
}
export function cellOf(c: ColumnDef, row: Row, ctx: CellContext): Cell {
  const v = row.values;
  const heading = row.kind !== "unit";
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
    case "version": value = common((p) => versionInfo(p)?.[1] ?? ""); break;
    case "architecture": value = common(architecture); break;
    case "elevated": value = instance(elevatedOf); break;
    case "commandLine": value = instance(commandLine); break;
    case "sessionId": value = instance(sessionOf); break;
    case "createTime": value = instance((p) => p.started); break;
    case "threads": value = total((p) => p.threads); break;
    case "peakThreads": value = instance((p) => p.threads + 2 + Math.round(noise(p.pid * 19, 0) * p.threads * 0.6)); break;
    case "handles": value = total((p) => p.handles); break;
    case "children": value = instance((p) => [...ctx.byPid.values()].filter((c) => c.ppid === p.pid && c.pid !== p.pid).length); break;
    case "runtime": value = instance((p) => runtimeOf(p, ctx.tick)); break;
    case "suspended": value = instance((p) => (p.suspended ? "是" : "否")); break;
    // The app names EcoQoS on and off (common.on / common.off).
    case "efficiency": value = instance((p) => (ctx.efficiency.has(p.pid) ? "开启" : "关闭")); break;
    case "power":
    case "powerTrend": {
      const estimate = c.key === "power" ? powerEstimate(v) : total(trendEstimate);
      // Faint at the lowest level; tinted by the level shown (theme::heat_alpha).
      const level = powerLevel(estimate);
      return { text: POWER_LEVELS[level], sort: estimate, faint: level === 0, heat: (level / 4) * 0.34 };
    }
    case "memoryPriority":
    case "ioPriority": {
      const levels = c.key === "ioPriority" ? IO_LEVELS : MEMORY_LEVELS;
      const name = common((p) => resourceLevel(ctx, c.key === "ioPriority" ? "I/O 优先级" : "内存优先级", p));
      if (!name) return { text: MISSING, sort: -1, faint: true };
      return { text: name, sort: levels.indexOf(name), faint: true };
    }
    case "cpu": value = v.cpu; break;
    case "cpuKernel": value = v.cpu * kernelShare(members()); break;
    case "cpuUser": value = v.cpu * (1 - kernelShare(members())); break;
    case "contextSwitchRate": value = total((p) => switchRate(p, ctx.tick)); break;
    case "contextSwitches": value = total((p) => switches(p, ctx.tick)); break;
    case "cpuTime": value = total((p) => p.cpuTime); break;
    case "cpuUserTime": value = total(userTime); break;
    case "cpuKernelTime": value = total((p) => p.cpuTime - userTime(p)); break;
    case "basePriority": {
      // The class by name, as the app shows it; sorted by its base priority.
      const name = instance((p) => ctx.priority.get(p.pid) ?? "正常");
      if (name === null) { value = null; break; }
      return { text: name, sort: priorityBase[name] ?? 8, faint: true };
    }
    case "mem": value = v.mem; break;
    case "workingSet": value = v.mem * 1.38; break;
    case "peakWorkingSet": value = instance((p) => p.mem * (1.46 + noise(p.pid * 5 + 2, 0) * 0.5)); break;
    case "sharedWs": value = v.mem * 0.38; break;
    case "commit": value = v.mem * 1.17; break;
    case "peakCommit": value = instance((p) => p.mem * 1.17 * (1.18 + noise(p.pid * 5 + 4, 0) * 0.4)); break;
    case "virtualSize": value = instance(addressSpace); break;
    case "peakVirtualSize": value = instance((p) => addressSpace(p) * (1.02 + noise(p.pid * 7 + 5, 0) * 0.08)); break;
    case "pagedPool": value = total((p) => p.handles * 180 + 96 * 1024); break;
    case "nonPagedPool": value = total((p) => p.handles * 22 + p.threads * 640 + 6 * 1024); break;
    case "pageFaults": value = total((p) => faults(p, ctx.tick)); break;
    case "pageFaultRate": value = total((p) => (p.suspended ? 0 : Math.round((p.cpu ?? 0) * 40 + 2))); break;
    case "hardFaults": value = total((p) => hardFaults(p, ctx.tick)); break;
    case "hardFaultRate": value = total((p) => hardFaultRate(p, ctx.tick)); break;
    case "diskRead": value = v.read; break;
    case "diskWrite": value = v.write; break;
    case "diskReadOps": value = v.read / (32 * 1024); break;
    case "diskWriteOps": value = v.write / (16 * 1024); break;
    case "diskReadTotal": value = total((p) => diskTotal(p, ctx.tick, 4)); break;
    case "diskWriteTotal": value = total((p) => diskTotal(p, ctx.tick, 5) * 0.55); break;
    case "diskReadOpsTotal": value = total((p) => Math.round(diskTotal(p, ctx.tick, 4) / (32 * 1024))); break;
    case "diskWriteOpsTotal": value = total((p) => Math.round((diskTotal(p, ctx.tick, 5) * 0.55) / (16 * 1024))); break;
    case "io": value = v.io; break;
    case "ioRead": value = ioRead; break;
    case "ioWrite": value = ioWrite; break;
    case "ioOther": value = Math.max(0, v.io - ioRead - ioWrite); break;
    case "ioReadOps": value = ioRead / (8 * 1024); break;
    case "ioWriteOps": value = ioWrite / (12 * 1024); break;
    case "ioOtherOps": value = total((p) => (p.suspended ? 0 : p.threads * 0.5 + (p.cpu ?? 0) * 24)); break;
    case "ioReadTotal": value = total((p) => ioTotal(p, ctx.tick, 1, 1)); break;
    case "ioWriteTotal": value = total((p) => ioTotal(p, ctx.tick, 2, 0.55)); break;
    case "ioOtherTotal": value = total((p) => ioTotal(p, ctx.tick, 3, 0.12)); break;
    case "ioReadOpsTotal": value = total((p) => Math.round(ioTotal(p, ctx.tick, 1, 1) / (8 * 1024))); break;
    case "ioWriteOpsTotal": value = total((p) => Math.round(ioTotal(p, ctx.tick, 2, 0.55) / (12 * 1024))); break;
    case "ioOtherOpsTotal": value = total((p) => Math.round(ioTotal(p, ctx.tick, 3, 0.12) / 512)); break;
    case "net": value = v.net; break;
    case "netReceived": value = v.net * 0.78; break;
    case "netSent": value = v.net * 0.22; break;
    case "gpu": value = v.gpu; break;
    case "gpuEngine": value = !heading && v.gpu >= 0.05 ? gpuEngine(row.pids) : null; break;
    case "gpuDedicated": value = total(dedicated); break;
    case "gpuShared": value = total(sharedGpu); break;
    default: value = null;
  }
  const textual = ["text", "time", "bool"].includes(c.format);
  // Text columns and identifiers are quiet (cells.rs `ProcessText`, `Identifier`).
  const quiet = c.format === "text" || c.format === "id";
  // No reading, or members that disagree: a dash, but a group's PID stays empty.
  if (value === null || value === "") return { text: c.key === "pid" ? "" : MISSING, sort: textual ? "" : -1, faint: quiet };
  if (typeof value === "string") return { text: value, sort: value, faint: c.format === "text" };
  switch (c.format) {
    case "percent":
      return { text: percent(value) + "%", sort: value, faint: value < 0.05, heat: heat("percent", value) };
    case "memory":
      return {
        text: byteCount.has(c.key) ? transferred(value) : memory(value),
        sort: value,
        heat: c.heat ? heat("memory", value) : 0,
      };
    case "rate":
      return { text: rate(value), sort: value, faint: value < 1, heat: heat("rate", value) };
    case "count":
      return { text: thousands(value), sort: value };
    case "events":
      return { text: thousands(Math.round(value)), sort: value, faint: value < 1 };
    case "duration":
      return { text: hms(value), sort: value };
    case "space":
      return { text: transferred(value), sort: value };
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
// The header's menu: in the tree, every branch opened or folded; the column
// under the pointer sorted or hidden; then the dialog, automatic widths and
// the defaults. Columns are chosen in the dialog only: with 0.7.0's columns
// a category no longer fits a menu that does not scroll.
export function headerMenu(o: {
  column: string | null;
  prefs: ColumnPrefs;
  /** The tree view: whether a branch can still open, whether one is open. */
  tree?: { canExpand: boolean; canCollapse: boolean; onAll: (expand: boolean) => void };
  onSort: (key: string, descending: boolean) => void;
  onToggle: (key: string) => void;
  onChoose: () => void;
  onAutoWidth: () => void;
  onFullWidth: () => void;
  /** As a double-click on the column's edge: back to its automatic width. */
  onFit: (key: string) => void;
  onReset: () => void;
}): MenuEntry[] {
  const items: MenuEntry[] = [];
  const tree = o.tree;
  if (tree)
    items.push(
      { label: "展开所有", icon: "expandAll", disabled: !tree.canExpand, run: () => tree.onAll(true) },
      { label: "折叠所有", icon: "collapseAll", disabled: !tree.canCollapse, run: () => tree.onAll(false) },
      "-",
    );
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
      {
        label: "自动调整此列宽度",
        icon: "fitWidth",
        disabled: o.prefs.autoWidth && !(at.key in o.prefs.widths),
        run: () => o.onFit(at.key),
      },
      "-",
    );
  }
  items.push(
    { label: "选择列…", icon: "columns", run: o.onChoose },
    { label: "自动宽度", checked: o.prefs.autoWidth, run: o.onAutoWidth },
    // Widths set by hand are kept as they are.
    { label: "全宽", checked: o.prefs.fullWidth, disabled: !o.prefs.autoWidth, run: o.onFullWidth },
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
// Widths (tables/widths.rs). With automatic widths every column follows what
// it shows, in 8 px steps, at least its header (the label beside the sort
// mark, and the figure above it). A figure column is as wide as the widest
// value its format shows in practice (`envelope`), or as every row needs, and
// only ever widens. The name and the text columns take what most rows need
// (at least 95% of them; lengths far past the rest are cut short) where room
// is short, up to what every row needs where there is room, the difference
// shared out. The rest of the table stays empty unless the name takes it
// ("全宽"). Without automatic widths the columns keep their designed widths.
// A width dragged by hand stays as it is.
let measure: CanvasRenderingContext2D | null | undefined;
const SANS = `"Segoe UI Variable Text", "Segoe UI", system-ui, "Microsoft YaHei UI", sans-serif`;
const DISPLAY = `"Segoe UI Variable Display", "Segoe UI Variable Text", "Segoe UI", system-ui, "Microsoft YaHei UI", sans-serif`;
function textWidth(text: string, scale: number, size = 13, weight = 400, family = SANS) {
  measure ??= typeof document === "undefined" ? null : document.createElement("canvas").getContext("2d");
  if (!measure || !text) return 0;
  measure.font = `${weight} ${size * scale}px ${family}`;
  return measure.measureText(text).width;
}
const labelWidth = (text: string, scale: number) => textWidth(text, scale, 12, 500);
/** The header's figure above its label: 15 px in the display face. */
const totalWidth = (text: string, scale: number) => textWidth(text, scale, 15, 400, DISPLAY);
const stepped = (width: number) => Math.ceil(width / 8) * 8;
/** Digits all measure alike: one measure serves every value with as many. */
const figures = (text: string) => text.replace(/\d/g, "8");
const numeric = (c: ColumnDef) => !["name", "text", "bool", "power"].includes(c.format);
const isText = (c: ColumnDef) => c.format === "name" || c.format === "text";
/** The widest values a figure column shows in practice (widths.rs `envelope`). */
function envelope(c: ColumnDef): string[] {
  switch (c.format) {
    case "percent":
      return ["100%", "8.8%"];
    case "memory":
      return byteCount.has(c.key) ? ["888.8 GB", "8888 KB"] : ["88.8 GB", "8.88 GB", "888.8 MB"];
    case "space":
      return ["888.8 TB", "888.8 GB"];
    case "rate":
      return ["8888 KB/s", "888.8 MB/s"];
    case "power":
      return POWER_LEVELS;
    default:
      return [];
  }
}
/** The widest totals above a column, reached within moments on a busy machine. */
const totalEnvelope = (c: ColumnDef) =>
  ["cpu", "mem", "gpu"].includes(c.key) ? ["100%"] : c.format === "percent" || c.format === "power" ? [] : envelope(c);
/** What most of `needs` fit in, and what all of them do. */
function spread(needs: number[]): [number, number] {
  if (!needs.length) return [0, 0];
  const sorted = [...needs].sort((a, b) => a - b);
  const at = (q: number) => sorted[Math.round((sorted.length - 1) * q)];
  // Tukey's fence, but at least nineteen values in twenty fit.
  const fence = Math.max(at(0.75) + 1.5 * (at(0.75) - at(0.25)), at(0.95));
  const most = Math.max(...sorted.filter((n) => n <= fence));
  return [most, sorted[sorted.length - 1]];
}
/** A name cell's text and marks, padding included (widths.rs `name_width`). */
function nameNeed(row: Row, ctx: CellContext, scale: number) {
  const GAP = 8,
    FOLD = 18,
    edges = 26;
  const inset = row.nested ? 20 : 0;
  const title = textWidth(translate(row.label), scale, row.kind === "unit" ? 13 : 14, row.kind !== "unit" || row.count > 1 ? 500 : 400);
  if (row.kind === "user") return edges + FOLD + GAP + 16 + GAP + title + GAP + labelWidth(translate(row.detail ?? ""), scale);
  if (row.kind === "group") return edges + inset + FOLD + GAP + title + GAP + Math.max(22, textWidth(String(row.count), scale, 11, 600) + 14);
  let need = edges + inset + row.depth * 20 + 14 + GAP + 16 + GAP + title;
  if (row.count > 1) need += GAP + Math.max(20, 12 + textWidth(String(row.count), scale, 11));
  if (row.detail) need += GAP + textWidth(translate(row.detail), scale);
  if (row.pids.some((pid) => ctx.efficiency.has(pid))) need += GAP + 12;
  if (row.suspended) need += GAP + 14 + textWidth(translate("已暂停"), scale, 11);
  if (row.pids.some((pid) => ctx.leaks?.has(pid))) need += GAP + 14 + textWidth(translate("句柄泄漏"), scale, 11);
  return need;
}
/** What a cell showing `text` needs, its padding included. */
const cellNeed = (c: ColumnDef, text: string, scale: number) =>
  text ? textWidth(numeric(c) ? figures(translate(text)) : translate(text), scale) + 20 : 0;
/** The narrowest and widest a column is made, whatever its contents (widths.rs `limits`). */
const limits = (key: string): [number, number] =>
  key === "name"
    ? [180, 520]
    : key === "path" || key === "commandLine"
      ? [120, 600]
      : key === "description"
        ? [100, 480]
        : key === "company" || key === "gpuEngine"
          ? [100, 360]
          : key === "imageName"
            ? [100, 320]
            : [64, 400];
export interface ColumnWidths {
  widths: Map<string, number>;
  /** The name takes the room the others leave, as before automatic widths fitted it. */
  fill: boolean;
}
export function useColumnWidths(
  columns: ColumnDef[],
  prefs: Sizing,
  scale: number,
  rows: Row[],
  ctx: CellContext,
  /** The table's width, in the window's own pixels. */
  room: number,
  /** The figures above the labels, by column. */
  totals: Record<string, string>,
): ColumnWidths {
  // The server knows no fonts: the first render keeps the designed widths.
  const [ready, setReady] = useState(false);
  useEffect(() => setReady(true), []);
  const fitting = ready && prefs.autoWidth;
  // Row needs follow the list measured and the language, not every reading:
  // the list as it is with every group open and nothing searched.
  const latest = useRef({ rows, ctx });
  latest.current = { rows, ctx };
  const shape = fitting ? rows.map((r) => r.key + (r.suspended ? "~" : "")).join() + "|" + columns.map((c) => c.key).join() : "";
  const needs = useMemo(() => {
    const out = new Map<string, [number, number]>();
    if (!shape) return out;
    const { rows, ctx } = latest.current;
    const units = rows.filter((r) => r.kind === "unit");
    for (const c of columns) {
      if (c.key === "name") {
        out.set(c.key, spread(rows.map((r) => nameNeed(r, ctx, scale))));
        continue;
      }
      const list = units.map((r) => cellNeed(c, cellOf(c, r, ctx).text, scale));
      if (isText(c)) out.set(c.key, spread(list));
      else {
        const most = Math.max(0, ...list, ...envelope(c).map((text) => cellNeed(c, text, scale)));
        out.set(c.key, [most, most]);
      }
    }
    return out;
  }, [shape, columns, scale, language()]);
  // A figure column only widens: a value or total growing a digit moves it once.
  const widest = useRef({ era: "", widths: new Map<string, number>() });
  const era = columns.map((c) => c.key).join() + "|" + scale + "|" + language() + "|" + fitting;
  if (widest.current.era !== era) widest.current = { era, widths: new Map() };
  const header = (c: ColumnDef) => {
    const label = labelWidth(columnLabel(c), scale) + 14 + 20;
    const shown = translate(totals[c.key] ?? "");
    let total = totalWidth(numeric(c) ? figures(shown) : shown, scale);
    for (const text of totalEnvelope(c)) total = Math.max(total, totalWidth(text, scale));
    // The name's figure has the search's filter mark beside it.
    return Math.max(label, total + (c.key === "name" ? 17 : 0) + 20);
  };
  const widths = new Map<string, number>();
  const grow = new Map<string, number>();
  for (const c of columns) {
    const manual = prefs.widths[c.key];
    let w = c.width;
    if (manual !== undefined) w = manual;
    else if (fitting) {
      const [lo, hi] = limits(c.key);
      const head = header(c);
      const floor = Math.max(head, lo),
        ceiling = Math.max(hi, head);
      const clamp = (n: number) => stepped(Math.min(ceiling, Math.max(floor, n)));
      const [most, all] = needs.get(c.key) ?? [0, 0];
      if (isText(c)) {
        w = clamp(most);
        const full = Math.max(w, clamp(all));
        if (full > w) grow.set(c.key, full - w);
      } else {
        w = Math.max(clamp(most), widest.current.widths.get(c.key) ?? 0);
        widest.current.widths.set(c.key, w);
      }
    }
    widths.set(c.key, w);
  }
  if (!ready) return { widths, fill: true };
  if (!fitting) return { widths, fill: false };
  // Room to spare widens the text columns toward what every row needs (widths.rs `allocate`).
  let left = room - [...widths.values()].reduce((a, w) => a + w, 0);
  const wanted = [...grow.values()].reduce((a, w) => a + w, 0);
  if (left > 0 && wanted > 0) {
    const share = Math.min(1, left / wanted);
    for (const [key, more] of grow) widths.set(key, widths.get(key)! + Math.floor(more * share));
    left = room - [...widths.values()].reduce((a, w) => a + w, 0);
  }
  if (prefs.fullWidth && prefs.widths.name === undefined && left > 0) widths.set("name", widths.get("name")! + Math.floor(left));
  return { widths, fill: false };
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
          <p>拖动右侧列表项或表头可调整列顺序。</p>
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
