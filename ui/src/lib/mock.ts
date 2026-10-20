// Synthetic data for running the UI in a plain browser (`pnpm dev`).
// Never bundled into the Tauri build's hot path: ipc.ts only imports it outside Tauri.

import { canonical, judge, parseHotkey } from "./hotkey";
import { type Language, type Locale, localeFor, t } from "./i18n";
import { mockCpuSensors } from "./mock-cpu";
import type {
  ActionError,
  AppInfo,
  Connection,
  CpuInfo,
  DiskInfo,
  GpuInfo,
  Hardware,
  HistoryMoment,
  HistoryPoint,
  HistoryUsage,
  LockReport,
  NicInfo,
  PriorityClass,
  ProcessDetails,
  ProcessRow,
  ServiceInfo,
  SettingsView,
  Snapshot,
  StartupItem,
  TreeKillReport,
  UpdateStatus,
} from "./types";

const MB = 1024 * 1024;
const GB = 1024 * MB;
const AMD_FIXTURE = new URLSearchParams(location.search).get("cpu") === "amd";
const DISK_UNAVAILABLE = new URLSearchParams(location.search).has("diskUnavailable");
const CORES = AMD_FIXTURE ? 32 : 20;
const BOOT = Date.now() - (5 * 86400 + 14 * 3600) * 1000;

interface MockProc extends ProcessRow {
  load: number;
  baseMem: number;
  ioLevel: number;
  /** Typical GPU use and the engine it lands on. */
  gpuLoad: number;
  gpuOn: string;
  /** Samples taken so far; disk rates need two, as through the privileged service. */
  samples: number;
}

const procs: MockProc[] = [];
let nextPid = 600;

// What an English Windows reports for the descriptions above, for `?lang=en-US`.
const ENGLISH_TEXT: Record<string, string> = {
  "Windows 会话管理器": "Windows Session Manager",
  客户端服务器运行时进程: "Client Server Runtime Process",
  "Windows 启动应用程序": "Windows Start-Up Application",
  服务和控制器应用: "Services and Controller app",
  "Windows 服务主进程": "Host Process for Windows Services",
  桌面窗口管理器: "Desktop Window Manager",
  "Windows 资源管理器": "Windows Explorer",
  搜索: "Search",
  微信: "WeChat",
  计算器: "Calculator",
  控制台窗口主进程: "Console Window Host",
  "缓存 DNS 查询结果，并注册此计算机的完整名称。":
    "The DNS Client service caches Domain Name System (DNS) names and registers the full computer name.",
  "为此计算机注册并更新 IP 地址和 DNS 记录。":
    "Registers and updates IP addresses and DNS records for this computer.",
  "管理事件和事件日志。": "This service manages events and event logs.",
  "按计划自动执行任务。": "Enables a user to configure and schedule automated tasks on this computer.",
  "为用户体验主题提供管理。": "Provides user experience theme management.",
  "管理基于 Windows 的程序的音频。": "Manages audio for Windows-based programs.",
  "利用空闲网络带宽在后台传输文件。": "Transfers files in the background using idle network bandwidth.",
  "启用检测、下载和安装 Windows 更新。":
    "Enables the detection, download, and installation of updates for Windows.",
  "将打印作业排入队列并处理与打印机的交互。": "Spools print jobs and handles interaction with the printer.",
  "为文件、电子邮件和其他内容提供索引。":
    "Provides content indexing, property caching, and search results for files, e-mail, and other content.",
  "维护和提高一段时间内的系统性能。": "Maintains and improves system performance over time.",
  "帮助用户防止恶意软件及其他潜在的垃圾软件。":
    "Helps protect users from malware and other potentially unwanted software.",
  "支持此计算机通过网络的文件、打印和命名管道共享。":
    "Supports file, print, and named-pipe sharing over the network for this computer.",
  "为用户和服务身份验证维护安全通道。":
    "Maintains a secure channel between this computer and the domain controller.",
  "使远程用户能修改此计算机上的注册表设置。":
    "Enables remote users to modify registry settings on this computer.",
};
/** Text from Windows (file and service descriptions) in the previewed language. */
const fromWindows = <T extends string | null>(text: T): T =>
  text !== null && mockSettings.locale === "en-US" ? ((ENGLISH_TEXT[text] ?? text) as T) : text;

function add(
  name: string,
  description: string | null,
  company: string | null,
  o: {
    ppid?: number;
    mem?: number;
    load?: number;
    io?: number;
    pid?: number;
    dir?: string;
    gpu?: number;
    gpuOn?: string;
  } = {},
): MockProc {
  if (o.pid === undefined) nextPid += 4 + Math.floor(Math.random() * 300);
  const pid = o.pid ?? nextPid;
  const inheritedPath =
    o.dir === undefined ? procs.find((p) => p.pid === o.ppid && p.name === name)?.path : undefined;
  const p: MockProc = {
    pid,
    ppid: o.ppid ?? 0,
    createTime: BOOT + procs.length * 900 + Math.floor(Math.random() * 600),
    name,
    path: description === null ? null : (inheritedPath ?? `${o.dir ?? "C:\\Program Files\\App"}\\${name}`),
    description: fromWindows(description),
    company,
    icon: null,
    cpu: 0,
    cpuTime: Math.floor(Math.random() * 3_600_000),
    privateWs: 0,
    workingSet: 0,
    commit: 0,
    ioRead: 0,
    ioWrite: 0,
    ioOther: 0,
    diskRead: 0,
    diskWrite: 0,
    gpu: 0,
    gpuEngine: null,
    gpuDedicated: 0,
    gpuShared: 0,
    netSent: 0,
    netReceived: 0,
    threads: 4 + Math.floor(Math.random() * 40),
    handles: 120 + Math.floor(Math.random() * 1500),
    sessionId: 1,
    basePriority: 8,
    suspended: false,
    efficiency: false,
    appWindow: false,
    windowsProcess: o.pid === 4 || o.ppid === 4 || (o.dir?.startsWith("C:\\Windows") ?? false),
    load: o.load ?? 0,
    baseMem: (o.mem ?? 8) * MB,
    ioLevel: o.io ?? 0,
    gpuLoad: o.gpu ?? 0,
    gpuOn: o.gpuOn ?? "3D",
    samples: 0,
  };
  procs.push(p);
  return p;
}

function seed() {
  const sys32 = "C:\\Windows\\System32";
  const ms = "Microsoft Corporation";
  const system = add("System", null, null, { pid: 4, mem: 0.1, load: 0.4, io: 2 });
  add("Registry", null, null, { ppid: system.pid, mem: 60 });
  add("Memory Compression", null, null, { ppid: system.pid, mem: 780 });
  add("smss.exe", "Windows 会话管理器", ms, { ppid: system.pid, mem: 1, dir: sys32 });
  add("csrss.exe", "客户端服务器运行时进程", ms, { mem: 2, dir: sys32 });
  const wininit = add("wininit.exe", "Windows 启动应用程序", ms, { mem: 1.5, dir: sys32 });
  const services = add("services.exe", "服务和控制器应用", ms, { ppid: wininit.pid, mem: 9, dir: sys32 });
  add("lsass.exe", "Local Security Authority Process", ms, { ppid: wininit.pid, mem: 14, dir: sys32 });
  for (let i = 0; i < 46; i++) {
    add("svchost.exe", "Windows 服务主进程", ms, {
      ppid: services.pid,
      mem: 2 + Math.random() * 40,
      load: Math.random() < 0.1 ? 0.3 : 0,
      io: Math.random() < 0.1 ? 1 : 0,
      dir: sys32,
    });
  }
  add("dwm.exe", "桌面窗口管理器", ms, { mem: 140, load: 1.4, dir: sys32, gpu: 4 });
  const explorer = add("explorer.exe", "Windows 资源管理器", ms, { mem: 320, load: 0.4, dir: "C:\\Windows" });
  const search = add("SearchHost.exe", "搜索", ms, { mem: 110, dir: "C:\\Windows\\SystemApps" });
  search.efficiency = true;
  search.basePriority = 4;

  const chrome = add("chrome.exe", "Google Chrome", "Google LLC", {
    ppid: explorer.pid,
    mem: 420,
    load: 1.2,
    io: 3,
    dir: "C:\\Program Files\\Google\\Chrome\\Application",
    gpu: 6,
  });
  for (let i = 0; i < 21; i++) {
    add("chrome.exe", "Google Chrome", "Google LLC", {
      ppid: chrome.pid,
      mem: 30 + Math.random() * 380,
      load: Math.random() < 0.25 ? 1.5 + Math.random() * 3 : 0.05,
      io: Math.random() < 0.2 ? 2 : 0,
      dir: "C:\\Program Files\\Google\\Chrome\\Application",
      // One tab plays a video.
      ...(i === 3 ? { gpu: 9, gpuOn: "Video Decode" } : {}),
    });
  }
  const code = add("Code.exe", "Visual Studio Code", ms, {
    ppid: explorer.pid,
    mem: 380,
    load: 0.8,
    dir: "C:\\Users\\me\\AppData\\Local\\Programs\\Microsoft VS Code",
    gpu: 1.2,
  });
  for (let i = 0; i < 9; i++)
    add("Code.exe", "Visual Studio Code", ms, { ppid: code.pid, mem: 40 + Math.random() * 300, load: 0.2 });
  add("rust-analyzer.exe", "rust-analyzer", null, { ppid: code.pid, mem: 1900, load: 4, io: 4 });
  add("node.exe", "Node.js JavaScript Runtime", "Node.js", { ppid: code.pid, mem: 160, load: 0.6 });
  const cargo = add("cargo.exe", "cargo", null, { ppid: code.pid, mem: 60, load: 0.4, io: 2 });
  add("rustc.exe", "rustc", null, { ppid: cargo.pid, mem: 1100, load: 22, io: 30 });
  add("rustc.exe", "rustc", null, { ppid: cargo.pid, mem: 760, load: 14, io: 18 });

  const qq = add("QQ.exe", "QQ", "Tencent", { ppid: explorer.pid, mem: 470, load: 0.3 });
  for (let i = 0; i < 3; i++) add("QQ.exe", "QQ", "Tencent", { ppid: qq.pid, mem: 60 + Math.random() * 200 });
  const wx = add("Weixin.exe", "微信", "Tencent", { ppid: explorer.pid, mem: 260, load: 0.2 });
  for (let i = 0; i < 4; i++)
    add("WeChatAppEx.exe", "WeChatAppEx", "Tencent", { ppid: wx.pid, mem: 50 + Math.random() * 120 });
  const steam = add("steam.exe", "Steam", "Valve Corporation", {
    ppid: explorer.pid,
    mem: 180,
    load: 0.2,
    io: 1,
  });
  for (let i = 0; i < 5; i++)
    add("steamwebhelper.exe", "Steam Client WebHelper", "Valve Corporation", {
      ppid: steam.pid,
      mem: 40 + Math.random() * 160,
    });
  const argus = add("argus.exe", "Argus", null, { ppid: explorer.pid, mem: 34, load: 0.3 });
  for (const host of [argus, search]) {
    const browser = add("msedgewebview2.exe", "Microsoft Edge WebView2", ms, {
      ppid: host.pid,
      mem: 30,
      dir: "C:\\Program Files (x86)\\Microsoft\\EdgeWebView\\Application\\Runtime",
    });
    browser.webView = {
      browserPid: browser.pid,
      browserCreateTime: browser.createTime,
      hostName: host.name,
      hostDescription: host.description,
      hostIcon: host.icon,
      role: "browser",
    };
    for (const role of ["renderer", "gpu", "network", "storage", "crashpad"] as const) {
      const child = add("msedgewebview2.exe", "Microsoft Edge WebView2", ms, {
        ppid: browser.pid,
        mem: 20 + Math.random() * 60,
        load: role === "renderer" ? 0.6 : 0,
        gpu: role === "gpu" ? 2 : 0,
      });
      child.webView = { ...browser.webView, role };
    }
  }
  const spotify = add("Spotify.exe", "Spotify", "Spotify Ltd", {
    ppid: explorer.pid,
    mem: 210,
    load: 0.5,
    io: 1,
  });
  add("Everything.exe", "Everything", "voidtools", { ppid: explorer.pid, mem: 350 });
  const paused = add("Calculator.exe", "计算器", ms, { ppid: explorer.pid, mem: 38 });
  paused.suspended = true;

  // The processes that own a window on the taskbar, listed under "Apps".
  for (const p of [explorer, chrome, code, qq, wx, steam, argus, spotify]) p.appWindow = true;
}

let elapsed = 1;
function tick(): Snapshot {
  // Occasional short-lived processes, so arrivals and exits show up.
  if (Math.random() < 0.15)
    add("conhost.exe", "控制台窗口主进程", "Microsoft Corporation", { mem: 6, load: 0.5 });
  if (Math.random() < 0.12) {
    const i = procs.findIndex((p) => p.name === "conhost.exe");
    if (i >= 0) procs.splice(i, 1);
  }

  let cpu = 0;
  let mem = 0;
  let io = 0;
  for (const p of procs) {
    const spike = Math.random() < 0.01 ? Math.random() * 6 : 0;
    p.cpu = p.suspended ? 0 : Math.max(0, p.load * (0.5 + Math.random()) + spike);
    p.cpuTime += p.cpu * elapsed * 10 * CORES;
    p.privateWs = Math.max(0.1 * MB, p.baseMem * (0.97 + Math.random() * 0.06));
    p.workingSet = p.privateWs * 1.4;
    p.commit = p.privateWs * 1.2;
    const level = p.ioLevel * MB * (Math.random() < 0.6 ? Math.random() : 0);
    p.ioRead = level * 0.7;
    p.ioWrite = level * 0.3;
    p.diskRead = p.suspended ? 0 : level * 0.4;
    p.diskWrite = p.suspended ? 0 : level * 0.2;
    if (DISK_UNAVAILABLE || p.samples === 0) {
      p.diskRead = null;
      p.diskWrite = null;
    }
    p.samples++;
    p.ioOther = level > 0 ? 2048 : 0;
    p.gpu = p.suspended || !p.gpuLoad ? 0 : Math.min(100, p.gpuLoad * (0.5 + Math.random()));
    p.gpuEngine = p.gpu > 0 ? `GPU 0 - ${p.gpuOn}` : null;
    p.gpuDedicated = p.gpuLoad * 40 * MB;
    p.gpuShared = p.gpuLoad * 6 * MB;
    // Network follows the processes that also do I/O, mostly downloads.
    const net = p.ioLevel && !p.suspended && Math.random() < 0.7 ? p.ioLevel * 120_000 * Math.random() : 0;
    p.netReceived = net;
    p.netSent = net * 0.15;
    cpu += p.cpu;
    mem += p.privateWs;
    io += p.ioRead + p.ioWrite + p.ioOther;
  }
  cpu = Math.min(100, cpu);
  const memTotal = 32 * GB;
  const memUsed = Math.min(memTotal * 0.95, mem + 6 * GB);
  return {
    seq: Math.floor(elapsed++),
    timestamp: Date.now(),
    hardware: mockHardware(cpu, io),
    system: {
      cpu,
      cpuKernel: cpu * (0.25 + Math.random() * 0.1),
      cores: Array.from({ length: CORES }, () =>
        Math.min(100, Math.max(0, cpu * (0.3 + Math.random() * 1.6))),
      ),
      logicalCpus: CORES,
      memTotal,
      memUsed,
      memAvailable: memTotal - memUsed,
      commitUsed: memUsed * 1.25,
      commitLimit: memTotal * 1.5,
      cached: 7.4 * GB,
      kernelPaged: 640 * MB,
      kernelNonpaged: 310 * MB,
      processCount: procs.length,
      threadCount: procs.reduce((a, p) => a + p.threads, 0),
      handleCount: procs.reduce((a, p) => a + p.handles, 0),
      uptimeMs: Date.now() - BOOT,
      ioRead: io * 0.7,
      ioWrite: io * 0.3,
      ioOther: 0,
    },
    processes: procs.map(
      ({ load: _l, baseMem: _b, ioLevel: _i, gpuLoad: _g, gpuOn: _o, samples: _s, ...row }) => ({
        ...row,
      }),
    ),
    // As if running elevated, so the network column shows in the browser preview.
    processNetwork: true,
  };
}

const wave = (base: number, amp: number) => Math.max(0, base + (Math.random() - 0.5) * 2 * amp);
const CPU_INFO: CpuInfo = {
  name: AMD_FIXTURE ? "Mock Ryzen 9 9950X" : "Mock Core i5-13500HX",
  vendor: AMD_FIXTURE ? "AuthenticAMD" : "GenuineIntel",
  family: AMD_FIXTURE ? 0x1a : 6,
  model: AMD_FIXTURE ? 0x44 : 0xbf,
  stepping: AMD_FIXTURE ? 0 : 2,
  microcode: AMD_FIXTURE ? 0xb404021 : 0x4122,
  instructions: [
    "SSE",
    "SSE2",
    "SSE3",
    "SSSE3",
    "SSE4.1",
    "SSE4.2",
    "AVX",
    "AVX2",
    "FMA",
    "AES",
    "BMI1",
    "BMI2",
    ...(AMD_FIXTURE ? ["AVX-512F", "AVX-512BW", "AVX-512VL", "AVX-512VNNI"] : []),
  ],
  baseMhz: AMD_FIXTURE ? 4300 : 2500,
  baseSource: "windows",
  sockets: 1,
  cores: AMD_FIXTURE ? 16 : 14,
  performanceCores: AMD_FIXTURE ? 0 : 6,
  efficiencyCores: AMD_FIXTURE ? 0 : 8,
  logical: CORES,
  l1: 1_228_800,
  l2: 11_534_336,
  l3: 25_165_824,
  virtualization: true,
  // 6 hyper-threaded P-cores, then 8 E-cores.
  threads: Array.from({ length: CORES }, (_, i) => ({
    group: 0,
    processor: i,
    ...(AMD_FIXTURE || i < 12 ? { core: i >> 1, efficient: false } : { core: i - 6, efficient: true }),
  })),
};
const BATTERY_INFO = {
  id: "0",
  name: "CT-4410",
  manufacturer: "Contoso",
  serial: "0001",
  chemistry: "LION",
  designedCapacity: 80_000,
  fullCapacity: 72_000,
  cycleCount: 120,
  manufactured: null,
};
let batteryPct = 84;
const DISKS: DiskInfo[] = [
  {
    index: 0,
    model: "Samsung SSD 990 PRO 2TB",
    size: 2e12,
    bus: "nvme",
    ssd: true,
    removable: false,
    volumes: ["C:"],
  },
  {
    index: 1,
    model: "WDC WD40EZAZ-00SF3B0",
    size: 4e12,
    bus: "sata",
    ssd: false,
    removable: false,
    volumes: ["D:", "E:"],
  },
];
const NICS: NicInfo[] = [
  {
    id: "1",
    name: "WLAN",
    description: "Intel(R) Wi-Fi 6E AX211 160MHz",
    kind: "wifi",
    linkSpeed: 1_201_000_000,
    // Locally administered MAC and documentation prefixes (RFC 3849): nothing real.
    mac: "02-00-00-00-00-01",
    ipv4: ["192.168.1.23", "10.8.0.2"],
    // More than the view lists before its "show all" button.
    ipv6: [
      "2001:db8::a14",
      "2001:db8::8a2e:370:7334",
      "2001:db8::1c9d:5f2e:6b41:e07a",
      "fd00::51",
      "fd00::7a3c:91ff:fe40:22b0",
      "fe80::7a3c:91ff:fe40:22b0%12",
    ],
  },
  {
    id: "2",
    name: "vEthernet (WSL)",
    description: "Hyper-V Virtual Ethernet Adapter",
    kind: "virtual",
    linkSpeed: 10_000_000_000,
    mac: null,
    ipv4: ["172.25.80.1"],
    ipv6: [],
  },
];
const GPU_INFO: GpuInfo = {
  id: "7001",
  name: "NVIDIA GeForce RTX 4060 Laptop GPU",
  dedicatedTotal: 8 * GB,
  sharedTotal: 16 * GB,
  engines: ["3D", "Copy", "Video Encode", "Video Decode", "Compute_0", "Security"],
  temperatureMax: 105,
};

function mockHardware(cpu: number, io: number): Hardware {
  const g3d = wave(28, 14);
  batteryPct = Math.max(5, batteryPct - 0.02);
  return {
    cpu: CPU_INFO,
    cpuMhz: Math.round(2400 * (1 + cpu / 120) + Math.random() * 300),
    coreMhz: CPU_INFO.threads.map((t) => Math.round((t.efficient ? 2800 : 3500) + wave(0, 120))),
    thermal: [{ name: "TZ01", celsius: wave(55 + cpu / 4, 1.5) }],
    cpuIdle: CPU_INFO.threads.map(() => ({ c1: 10, c2: 5, c3: 30 })),
    cpuSensors: mockSettings.kernelSensors ? mockCpuSensors(CPU_INFO, cpu, AMD_FIXTURE) : null,
    power: { package: wave(30 + cpu * 0.8, 3), cores: wave(22 + cpu * 0.7, 3), graphics: null, memory: null },
    memory: {
      installed: 32 * GB,
      speed: 5600,
      slotsUsed: 2,
      slotsTotal: 2,
      formFactor: "SODIMM",
      kind: "DDR5",
    },
    disks: DISKS.map((info, i) => ({
      info,
      active: i === 0 ? Math.min(100, (io / (60 * MB)) * 100 + wave(2, 2)) : wave(1, 1),
      read: i === 0 ? io * 0.7 : wave(20_000, 20_000),
      write: i === 0 ? io * 0.3 : 0,
      responseMs: i === 0 ? wave(0.3, 0.2) : wave(6, 3),
      temperature: i === 0 ? wave(48, 1) : null,
      temperatureWarning: i === 0 ? 90 : null,
    })),
    nics: NICS.map((info, i) => ({
      info,
      rx: i === 0 ? wave(2.4 * MB, 1.5 * MB) : wave(4_000, 4_000),
      tx: i === 0 ? wave(320_000, 200_000) : wave(2_000, 2_000),
    })),
    gpus: [
      {
        info: GPU_INFO,
        util: g3d,
        engines: [g3d, wave(2, 2), wave(18, 6), 0, wave(4, 4), 0],
        dedicatedUsed: wave(3.4 * GB, 0.05 * GB),
        sharedUsed: wave(280 * MB, 10 * MB),
        temperature: wave(52, 1.5),
        fanRpm: null,
        power: null,
        boardPower: wave(18 + g3d * 0.6, 2),
        boardPowerLimit: 115,
        powerSource: "nvml",
        clockMhz: Math.round(1800 + g3d * 12),
        clockMaxMhz: 3105,
        voltageMv: null,
        memoryClockMhz: 7001,
      },
    ],
    batteries: [
      {
        info: BATTERY_INFO,
        percent: batteryPct,
        capacity: Math.round((BATTERY_INFO.fullCapacity * batteryPct) / 100),
        rate: -Math.round(wave(18_000, 4_000)),
        voltage: Math.round(15_200 + batteryPct * 12),
        state: "discharging",
        pluggedIn: false,
        critical: false,
        remaining: Math.round((batteryPct / 100) * 3 * 3600),
        temperature: null,
      },
    ],
  };
}

// `?lang=en-US` previews a language, `?onboarding` the onboarding.
const query = new URLSearchParams(location.search);
export async function runTask(command: string, elevated: boolean): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 250));
  if (command.toLowerCase() === "missing.exe") throw { kind: "notFound", code: 2 };
  if (elevated && query.get("task") === "cancelled") throw { kind: "cancelled", code: 1223 };
}

/** The version the mock app runs. */
export const VERSION = "0.1.0";
const resolve = (language: Language): Locale =>
  language === "system" ? localeFor(navigator.language) : language;
const language = (query.get("lang") ?? "system") as Language;

const mockSettings: SettingsView = {
  language,
  onboarding: query.has("onboarding") ? 0 : 1,
  // ?updated plays the first start after an update from 0.0.9; a fresh install has seen none.
  seenVersion: query.has("updated") ? "0.0.9" : query.has("onboarding") ? "" : VERSION,
  locale: resolve(language),
  theme: "system",
  closeToTray: true,
  minimizedInterval: 5000,
  trayInterval: 5000,
  hotkey: "Ctrl+Alt+Escape",
  hotkeyEnabled: true,
  rules: [],
  history: true,
  kernelSensors: query.has("sensors"),
  updateCheck: true,
  updateChannel: "stable",
  updateRoute: "auto",
  updateProxy: "https://gh-proxy.com/",
  // `?whatsnew=off`: the first start after an update only says so.
  whatsNew: query.get("whatsnew") !== "off",
  autostart: false,
  // `?hotkey=taken`: every combination is taken, the saved one too.
  hotkeyError: query.get("hotkey") === "taken" ? { kind: "taken", shortcut: "Ctrl+Alt+Escape" } : null,
  defaultHotkey: "Ctrl+Alt+Escape",
};

/** What set_hotkey does, with the same rules. */
export async function setHotkey(hotkey: string): Promise<SettingsView> {
  const parsed = parseHotkey(hotkey);
  const result = judge(hotkey);
  if (typeof parsed === "string" || result.level === "block") {
    throw { kind: "invalid", shortcut: hotkey, issue: result.level === "block" ? result.issue : parsed };
  }
  if (query.get("hotkey") === "taken") throw { kind: "taken", shortcut: hotkey };
  return settings.update({ hotkey: canonical(parsed), hotkeyEnabled: true, hotkeyError: null });
}

/** What set_hotkey_enabled does. */
export async function setHotkeyEnabled(enabled: boolean): Promise<SettingsView> {
  if (enabled && query.get("hotkey") === "taken") throw { kind: "taken", shortcut: mockSettings.hotkey };
  return settings.update({ hotkeyEnabled: enabled, hotkeyError: null });
}

export const settings = {
  get: async (): Promise<SettingsView> => ({ ...mockSettings }),
  update: async (patch: Partial<SettingsView>): Promise<SettingsView> => {
    Object.assign(mockSettings, patch);
    mockSettings.locale = resolve(mockSettings.language);
    return { ...mockSettings };
  },
};

/** Browser-only fixture for pending/failure UI; no installer or service calls. */
export async function setKernelSensors(enabled: boolean): Promise<SettingsView> {
  if (enabled) {
    await new Promise((resolve) => setTimeout(resolve, 1200));
    if (query.get("sensorSetup") === "failure") throw new Error(t("settings.sensors.error"));
  }
  return settings.update({ kernelSensors: enabled });
}

let timer: ReturnType<typeof setInterval> | undefined;
let interval = 1000;
let isPaused = false;
let listener: ((s: Snapshot) => void) | null = null;

function restart() {
  clearInterval(timer);
  timer = setInterval(() => {
    if (!isPaused && listener) listener(tick());
  }, interval);
}

export async function subscribe(cb: (s: Snapshot) => void) {
  if (procs.length === 0) seed();
  listener = cb;
  cb(tick());
  restart();
}

export const appInfo = async (): Promise<AppInfo> => ({
  version: VERSION,
  elevated: false,
  accent: null,
  mica: false,
  // ?updated plays the first start after an update.
  updated: query.has("updated"),
  // ?taskmgr plays a start by the task manager replacement.
  taskManager: query.has("taskmgr"),
  locale: mockSettings.locale,
});

// Starts idle; "检查更新" plays a check and a download, then offers the update.
// ?update=ready starts with it offered. Installing plays a declined
// administrator prompt.
const MOCK_RELEASES = [
  {
    version: "0.2.0",
    date: "2026-10-20",
    notes: [
      "### 新功能",
      "",
      "- **ui:** 新版本就绪后，标题栏显示更新按钮，可查看各版本的更新内容 ([a1b2c3d](https://github.com/Ray-lea/Argus/commit/a1b2c3d))",
      "- **core:** 采集每个磁盘的队列长度 ([b2c3d4e](https://github.com/Ray-lea/Argus/commit/b2c3d4e))",
      "",
      "### 修复",
      "",
      "- **core:** 修复未装 Intel 显卡的电脑上程序运行一段时间后崩溃的问题 ([db0aac5](https://github.com/Ray-lea/Argus/commit/db0aac5))",
      "- **host:** 窗口最大化时标题栏右上角不再出现两组重叠的按钮 ([ad831d5](https://github.com/Ray-lea/Argus/commit/ad831d5))",
      "",
      "### 性能",
      "",
      "- 托盘常驻时的唤醒次数减少一半 ([c3d4e5f](https://github.com/Ray-lea/Argus/commit/c3d4e5f))",
    ].join("\n"),
  },
  {
    version: "0.1.1",
    date: "2026-10-18",
    notes:
      "### 修复\n\n- **ui:** 窗口最大化时贴着屏幕顶边也能拖动 ([3240521](https://github.com/Ray-lea/Argus/commit/3240521))",
  },
  { version: "0.1.0", date: "2026-10-16", notes: "Argus 的首个公开版本。" },
];
const offeredAtStart = new URLSearchParams(location.search).get("update") === "ready";
let updateStatus: UpdateStatus = {
  state: offeredAtStart ? "ready" : "idle",
  version: offeredAtStart ? "0.2.0" : null,
  releases: offeredAtStart ? MOCK_RELEASES : null,
  notes: null,
  date: null,
  received: offeredAtStart ? 6_412_000 : 0,
  total: offeredAtStart ? 6_412_000 : null,
  error: null,
  checkedAt: null,
  source: null,
  portable: false,
};
let updateListener: ((s: UpdateStatus) => void) | null = null;
const setUpdate = (patch: Partial<UpdateStatus>) => {
  updateStatus = { ...updateStatus, ...patch };
  updateListener?.({ ...updateStatus });
};

export const updates = {
  status: async (): Promise<UpdateStatus> => ({ ...updateStatus }),
  subscribe: (cb: (s: UpdateStatus) => void) => {
    updateListener = cb;
  },
  check: async (): Promise<UpdateStatus> => {
    setUpdate({ state: "checking", error: null });
    setTimeout(() => {
      const total = 2_150_000;
      setUpdate({
        state: "downloading",
        version: "0.2.0",
        releases: MOCK_RELEASES,
        notes: null,
        date: Date.now() - 86_400_000,
        received: 0,
        total,
        checkedAt: Date.now(),
        source: "GitHub",
      });
      let received = 0;
      const step = setInterval(() => {
        received = Math.min(total, received + 430_000);
        setUpdate({ received });
        if (received >= total) {
          clearInterval(step);
          setUpdate({ state: "ready" });
        }
      }, 250);
    }, 700);
    return { ...updateStatus };
  },
  install: async () => {
    setUpdate({ state: "installing" });
    await new Promise((resolve) => setTimeout(resolve, 1200));
    setUpdate({ state: "ready" });
    throw { cancelled: true, message: "已取消管理员授权，未安装更新" };
  },
};

// `?service=incompatible` plays a service of an older protocol, left running
// when Argus was updated without it: what it would do runs with Argus's own
// rights, denied where those fall short, until the service is updated or
// reinstalled.
let serviceIncompatible = query.get("service") === "incompatible";
let serviceInstalled = serviceIncompatible;
export const privilegedService = {
  status: async (): Promise<import("./types").PrivilegedServiceStatus> => ({
    state: serviceInstalled ? "running" : "notInstalled",
    authorized: serviceInstalled,
    version: serviceInstalled && !serviceIncompatible ? VERSION : null,
    error: serviceIncompatible
      ? mockSettings.locale === "en-US"
        ? "The privileged service version is incompatible; update the service in Settings"
        : "特权服务版本不兼容，请在设置中更新服务"
      : null,
    incompatible: serviceIncompatible,
  }),
  manage: async (action: import("./types").PrivilegedServiceAction) => {
    // Stands in for the administrator prompt and the service starting.
    await new Promise((done) => setTimeout(done, 1500));
    serviceInstalled = action !== "uninstall";
    serviceIncompatible = false;
    return privilegedService.status();
  },
};

/** An operation the privileged service would run, denied to Argus's own rights. */
export const denied = (): ActionError => ({ kind: serviceIncompatible ? "serviceOutdated" : "accessDenied" });

// ?service=running|outdated|stopped|unreachable starts with the service
// installed and in that state (the disk columns' hints follow it) until it is
// managed in Settings. Unlike `incompatible` above, `outdated` is an older
// version of the same protocol: it still connects.
type ServiceFixture = () => Partial<import("./types").PrivilegedServiceStatus>;
const SERVICE_FIXTURES: Record<string, ServiceFixture> = {
  running: () => ({}),
  outdated: () => ({ version: "0.0.9" }),
  stopped: () => ({ state: "stopped", version: null }),
  unreachable: () => ({
    version: null,
    error:
      mockSettings.locale === "en-US"
        ? "The privileged service didn't respond in time; refresh the status to confirm the result"
        : "特权服务响应超时，请刷新状态后确认操作结果",
  }),
};
let serviceFixture: ServiceFixture | undefined = SERVICE_FIXTURES[query.get("service") ?? ""];
if (serviceFixture) {
  const { status, manage } = privilegedService;
  serviceInstalled = true;
  privilegedService.status = async () => ({ ...(await status()), ...serviceFixture?.() });
  privilegedService.manage = (action) => {
    serviceFixture = undefined;
    return manage(action);
  };
}

let taskManagerEnabled = false;
export const taskManager = {
  status: async (): Promise<import("./types").TaskManagerStatus> => ({
    state: taskManagerEnabled ? "enabled" : "disabled",
    canEnable: !taskManagerEnabled,
    canRestore: taskManagerEnabled,
    recoveryPath: "C:\\Program Files\\Argus Privileged\\argus-taskmgr-recover.exe",
    detail:
      mockSettings.locale === "en-US"
        ? taskManagerEnabled
          ? "Task Manager shortcuts open Argus."
          : "Windows Task Manager is in use."
        : taskManagerEnabled
          ? "任务管理器入口已交给 Argus。"
          : "当前使用 Windows 任务管理器。",
  }),
  manage: async (action: "enable" | "restore") => {
    taskManagerEnabled = action === "enable";
    return taskManager.status();
  },
};

export async function setInterval_(ms: number) {
  interval = ms;
  restart();
}
export { setInterval_ as setInterval };

export async function setPaused(p: boolean) {
  isPaused = p;
}

export async function refresh() {
  listener?.(tick());
}

export async function details(pid: number, path: string | null): Promise<ProcessDetails> {
  const p = procs.find((x) => x.pid === pid);
  await new Promise((r) => setTimeout(r, 60));
  return {
    pid,
    accessible: pid > 1000,
    commandLine: path ? `"${path}" --type=renderer --lang=zh-CN --enable-features=Mock` : null,
    user: pid > 1000 ? "DESKTOP\\me" : "NT AUTHORITY\\SYSTEM",
    integrity: pid > 1000 ? "medium" : "system",
    elevated: pid <= 1000,
    architecture: "x64",
    priorityClass: "normal",
    version: {
      description: p?.description ?? null,
      company: p?.company ?? null,
      productName: p?.description ?? null,
      fileVersion: "1.2.3.4",
    },
  };
}

export async function kill(pid: number) {
  const i = procs.findIndex((p) => p.pid === pid);
  if (i < 0) throw { kind: "notFound" };
  if (pid < 1000) throw denied();
  procs.splice(i, 1);
  listener?.(tick());
}

const BASE_PRIORITY: Record<PriorityClass, number> = {
  idle: 4,
  belowNormal: 6,
  normal: 8,
  aboveNormal: 10,
  high: 13,
  realtime: 24,
};

export async function setPriority(pid: number, priority: PriorityClass): Promise<PriorityClass> {
  const p = procs.find((x) => x.pid === pid);
  if (!p) throw { kind: "notFound" };
  if (pid < 1000) throw denied();
  // Like a standard user: "realtime" quietly becomes "high".
  const applied = priority === "realtime" ? "high" : priority;
  p.basePriority = BASE_PRIORITY[applied];
  listener?.(tick());
  return applied;
}

export async function setEfficiency(pid: number, on: boolean) {
  const p = procs.find((x) => x.pid === pid);
  if (!p) throw { kind: "notFound" };
  if (pid < 1000) throw denied();
  p.efficiency = on;
  if (on) p.basePriority = BASE_PRIORITY.idle;
  else if (p.basePriority === BASE_PRIORITY.idle) p.basePriority = BASE_PRIORITY.normal;
  listener?.(tick());
}

export async function killTree(pid: number): Promise<TreeKillReport> {
  if (!procs.some((p) => p.pid === pid)) throw { kind: "notFound" };
  const doomed = new Set([pid]);
  for (let grew = true; grew; ) {
    grew = false;
    for (const p of procs) {
      if (doomed.has(p.ppid) && !doomed.has(p.pid)) {
        doomed.add(p.pid);
        grew = true;
      }
    }
  }
  // As kill() does, PIDs below 1000 are denied. Like the host, the
  // descendants go first, and a denied root then fails the whole call.
  doomed.delete(pid);
  const failed = [...doomed].filter((p) => p < 1000).length;
  const before = procs.length;
  for (let i = procs.length - 1; i >= 0; i--) {
    const p = procs[i].pid;
    if ((doomed.has(p) || p === pid) && p >= 1000) procs.splice(i, 1);
  }
  listener?.(tick());
  if (pid < 1000) throw denied();
  return { terminated: before - procs.length, failed, serviceOutdated: failed > 0 && serviceIncompatible };
}

// ---- History -----------------------------------------------------------------------
// Synthetic, but shaped like a real week: a daily rhythm, short spikes, and
// nights when the machine (and Argus) was off.

const HOUR = 3_600_000;
// Moves to the present when the mock history is cleared.
let started = Date.now() - 6.5 * 24 * HOUR;
const isOff = (ts: number) => {
  const h = new Date(ts).getHours();
  return ts < started || (h >= 2 && h < 7);
};
const level = (ts: number) => {
  const h = new Date(ts).getHours() + new Date(ts).getMinutes() / 60;
  const day = Math.max(0, Math.sin(((h - 8) / 24) * 2 * Math.PI));
  const spike = Math.sin(ts / 97_000) > 0.97 ? 35 : 0;
  return 6 + 30 * day + 8 * Math.sin(ts / 600_000) + spike;
};
const point = (ts: number): HistoryPoint => {
  const cpu = Math.max(1, level(ts));
  return {
    ts,
    cpu,
    cpuPeak: Math.min(100, cpu * 1.6 + 4),
    gpu: Math.max(0, cpu * 0.5 - 2),
    memUsed: (12 + cpu / 10) * GB,
    memTotal: 32 * GB,
    disk: cpu * 180_000,
    net: cpu * 60_000,
  };
};

export const history = {
  range: async (): Promise<[number, number] | null> => [started, Date.now() - 5000],
  usage: async (): Promise<HistoryUsage> => {
    // About 450 bytes a frame while the machine was on.
    const frames = (Date.now() - started) / 5000;
    return { bytes: Math.round(frames * 450 * 0.8), first: frames > 1 ? started : null };
  },
  clear: async (): Promise<void> => {
    started = Date.now();
  },
  series: async (from: number, to: number, buckets: number): Promise<HistoryPoint[]> => {
    const out: HistoryPoint[] = [];
    const step = Math.max(5000, (to - from) / buckets);
    for (let ts = from + step / 2; ts < to; ts += step) if (!isOff(ts)) out.push(point(Math.round(ts)));
    return out;
  },
  moment: async (ts: number): Promise<HistoryMoment | null> => {
    if (isOff(ts) || ts > Date.now()) return null;
    if (procs.length === 0) seed();
    const system = point(ts - (ts % 5000));
    const scale = system.cpu / 20;
    const processes = procs
      .filter((p) => p.load > 0.3 || p.baseMem > 300 * MB)
      .slice(0, 12)
      .map((p) => ({
        name: p.name,
        pid: p.pid,
        cpu: p.load * scale,
        mem: p.baseMem,
        io: p.ioLevel * 200_000 * scale,
        gpu: p.gpuLoad * scale,
        net: p.ioLevel * 50_000 * scale,
      }));
    return { system, processes };
  },
};

// ---- M3: services, startup, connections, file locks -----------------------------

const SERVICE_NAMES: [string, string, string][] = [
  ["Dnscache", "DNS Client", "缓存 DNS 查询结果，并注册此计算机的完整名称。"],
  ["Dhcp", "DHCP Client", "为此计算机注册并更新 IP 地址和 DNS 记录。"],
  ["EventLog", "Windows Event Log", "管理事件和事件日志。"],
  ["Schedule", "Task Scheduler", "按计划自动执行任务。"],
  ["Themes", "Themes", "为用户体验主题提供管理。"],
  ["AudioSrv", "Windows Audio", "管理基于 Windows 的程序的音频。"],
  ["BITS", "Background Intelligent Transfer Service", "利用空闲网络带宽在后台传输文件。"],
  ["wuauserv", "Windows Update", "启用检测、下载和安装 Windows 更新。"],
  ["Spooler", "Print Spooler", "将打印作业排入队列并处理与打印机的交互。"],
  ["WSearch", "Windows Search", "为文件、电子邮件和其他内容提供索引。"],
  ["SysMain", "SysMain", "维护和提高一段时间内的系统性能。"],
  ["WinDefend", "Microsoft Defender Antivirus Service", "帮助用户防止恶意软件及其他潜在的垃圾软件。"],
  ["LanmanServer", "Server", "支持此计算机通过网络的文件、打印和命名管道共享。"],
  ["Netlogon", "Netlogon", "为用户和服务身份验证维护安全通道。"],
  ["RemoteRegistry", "Remote Registry", "使远程用户能修改此计算机上的注册表设置。"],
];

let mockServices: ServiceInfo[] | null = null;

function buildServices(): ServiceInfo[] {
  const hosts = procs.filter((p) => p.name === "svchost.exe");
  return SERVICE_NAMES.map(([name, displayName, description], i) => {
    const running = i % 5 !== 3;
    const host = hosts[i % Math.max(1, hosts.length)];
    return {
      name,
      displayName,
      description: fromWindows(description),
      state: running ? "running" : "stopped",
      startType: i % 7 === 6 ? "disabled" : i % 3 === 0 ? "manual" : i % 4 === 1 ? "autoDelayed" : "auto",
      pid: running && host ? host.pid : 0,
      path: `C:\\WINDOWS\\system32\\svchost.exe -k netsvcs -p`,
      account: "LocalSystem",
      canStop: running && i % 2 === 0,
      shared: i % 3 === 0,
    } satisfies ServiceInfo;
  });
}

const mockStartup: StartupItem[] = [
  {
    id: "hkcuRun|OneDrive",
    name: "OneDrive",
    command: '"C:\\Users\\me\\AppData\\Local\\Microsoft\\OneDrive\\OneDrive.exe" /background',
    path: "C:\\Users\\me\\AppData\\Local\\Microsoft\\OneDrive\\OneDrive.exe",
    location: "hkcuRun",
    machine: false,
    enabled: false,
    exists: true,
    publisher: "Microsoft Corporation",
    description: "Microsoft OneDrive",
    icon: null,
  },
  {
    id: "hkcuRun|Steam",
    name: "Steam",
    command: '"C:\\Program Files (x86)\\Steam\\steam.exe" -silent',
    path: "C:\\Program Files (x86)\\Steam\\steam.exe",
    location: "hkcuRun",
    machine: false,
    enabled: true,
    exists: true,
    publisher: "Valve Corporation",
    description: "Steam",
    icon: null,
  },
  {
    id: "hklmRun|SecurityHealth",
    name: "SecurityHealth",
    command: "C:\\WINDOWS\\system32\\SecurityHealthSystray.exe",
    path: "C:\\WINDOWS\\system32\\SecurityHealthSystray.exe",
    location: "hklmRun",
    machine: true,
    enabled: true,
    exists: true,
    publisher: "Microsoft Corporation",
    description: "Windows Security notification icon",
    icon: null,
  },
  {
    id: "hklmRun32|OldUpdater",
    name: "OldUpdater",
    command: '"C:\\Program Files (x86)\\Gone\\updater.exe" /silent',
    path: "C:\\Program Files (x86)\\Gone\\updater.exe",
    location: "hklmRun32",
    machine: true,
    enabled: true,
    exists: false,
    publisher: null,
    description: null,
    icon: null,
  },
  {
    id: "userFolder|Snipaste.lnk",
    name: "Snipaste",
    command: '"C:\\Program Files\\Snipaste\\Snipaste.exe"',
    path: "C:\\Program Files\\Snipaste\\Snipaste.exe",
    location: "userFolder",
    machine: false,
    enabled: true,
    exists: true,
    publisher: "Le Liu",
    description: "Snipaste",
    icon: null,
  },
];

function buildConnections(): Connection[] {
  const pick = (name: string) => procs.find((p) => p.name === name)?.pid ?? 4;
  const out: Connection[] = [
    {
      protocol: "tcp",
      ipv6: false,
      localAddress: "0.0.0.0",
      localPort: 135,
      remoteAddress: null,
      remotePort: null,
      state: "listen",
      pid: pick("svchost.exe"),
    },
    {
      protocol: "tcp",
      ipv6: false,
      localAddress: "0.0.0.0",
      localPort: 445,
      remoteAddress: null,
      remotePort: null,
      state: "listen",
      pid: 4,
    },
    {
      protocol: "tcp",
      ipv6: false,
      localAddress: "127.0.0.1",
      localPort: 5173,
      remoteAddress: null,
      remotePort: null,
      state: "listen",
      pid: pick("node.exe"),
    },
    {
      protocol: "tcp",
      ipv6: true,
      localAddress: "::",
      localPort: 135,
      remoteAddress: null,
      remotePort: null,
      state: "listen",
      pid: pick("svchost.exe"),
    },
    {
      protocol: "udp",
      ipv6: false,
      localAddress: "0.0.0.0",
      localPort: 5353,
      remoteAddress: null,
      remotePort: null,
      state: null,
      pid: pick("chrome.exe"),
    },
    {
      protocol: "udp",
      ipv6: true,
      localAddress: "::",
      localPort: 5355,
      remoteAddress: null,
      remotePort: null,
      state: null,
      pid: pick("svchost.exe"),
    },
  ];
  // Documentation ranges (RFC 5737).
  const remotes = ["203.0.113.14", "198.51.100.92", "192.0.2.7", "203.0.113.42", "198.51.100.69"];
  for (let i = 0; i < 24; i++) {
    out.push({
      protocol: "tcp",
      ipv6: false,
      localAddress: "192.168.1.23",
      localPort: 50000 + i * 37,
      remoteAddress: remotes[i % remotes.length],
      remotePort: i % 4 === 0 ? 80 : 443,
      state: i % 9 === 8 ? "timeWait" : "established",
      pid: pick(i % 3 === 0 ? "Code.exe" : i % 3 === 1 ? "chrome.exe" : "QQ.exe"),
    });
  }
  return out;
}

export const m3 = {
  services: async (): Promise<ServiceInfo[]> => {
    if (procs.length === 0) seed();
    mockServices ??= buildServices();
    return mockServices.map((s) => ({ ...s }));
  },
  controlService: async (name: string, action: "start" | "stop" | "restart") => {
    const s = mockServices?.find((x) => x.name === name);
    if (!s) throw { kind: "notFound" };
    if (name === "WinDefend") throw denied();
    s.state = action === "stop" ? "stopped" : "running";
    s.pid = action === "stop" ? 0 : (procs.find((p) => p.name === "svchost.exe")?.pid ?? 0);
  },
  setStartType: async (name: string, startType: ServiceInfo["startType"]) => {
    const s = mockServices?.find((x) => x.name === name);
    if (name === "WinDefend") throw denied();
    if (s) s.startType = startType;
  },
  startup: async (): Promise<StartupItem[]> => mockStartup.map((s) => ({ ...s })),
  setStartupEnabled: async (id: string, enabled: boolean) => {
    const item = mockStartup.find((s) => s.id === id);
    if (item?.machine) throw { kind: "accessDenied" };
    if (item) item.enabled = enabled;
  },
  connections: async (): Promise<Connection[]> => buildConnections(),
  findLockers: async (paths: string[]): Promise<LockReport> => {
    await new Promise((r) => setTimeout(r, 400));
    const take = (name: string) => procs.find((p) => p.name === name);
    const hits = [take("Code.exe"), take("explorer.exe")].filter((p): p is MockProc => !!p);
    return {
      lockers: hits.map((p) => ({
        pid: p.pid,
        createTime: p.createTime,
        appName: p.description ?? p.name,
        service: null,
        kind: p.name === "explorer.exe" ? "explorer" : "window",
        handleCount: 1,
        handlePath: paths[0] ?? null,
      })),
      filesChecked: paths.length * 12,
      unchecked: 0,
      truncated: false,
      handles: {
        available: true,
        viaService: false,
        elevated: false,
        checked: 2800,
        deniedProcesses: 2,
        skipped: 0,
        timeouts: 0,
        truncated: false,
        error: null,
      },
      restartError: null,
    };
  },
};
