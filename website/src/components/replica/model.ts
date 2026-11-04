// Mock data for the interactive window. System totals replay the repository's
// one-minute workload recording; process rows share those totals out, so the
// columns add up the way they do in the app.
import trace from "../../../../tools/window-probe/fixtures/readme-trace.json";

export type Group = "app" | "background" | "windows";
export type Glyph =
  | "app"
  | "kernel"
  | "argus"
  | "browser"
  | "code"
  | "folder"
  | "terminal"
  | "console"
  | "pwsh"
  | "paint"
  | "note"
  | "gear"
  | "cloud"
  | "calc"
  | "phone"
  | "shield"
  | "search"
  | "node"
  | "service";

export interface Process {
  pid: number;
  ppid: number;
  name: string;
  exe: string;
  group: Group;
  glyph: Glyph;
  mem: number;
  cpu?: number;
  disk?: number;
  net?: number;
  gpu?: number;
  suspended?: boolean;
  path: string;
  user: string;
  integrity: string;
  args?: string;
  threads: number;
  handles: number;
  started: string;
  /** Accumulated CPU time in seconds. */
  cpuTime: number;
}

export interface Sample {
  cpu: number;
  kernel: number;
  mem: number;
  read: number;
  write: number;
  active: number;
  rx: number;
  tx: number;
  gpu: number;
}

// History ranges: the toolbar (Replica.tsx) and the page (Pages.tsx) both use them.
export type HistoryRange = "1h" | "6h" | "1d" | "7d";
export const historyRanges: { value: HistoryRange; label: string; hours: number; tick: number }[] = [
  { value: "1h", label: "1 小时", hours: 1, tick: 10 },
  { value: "6h", label: "6 小时", hours: 6, tick: 60 },
  { value: "1d", label: "1 天", hours: 24, tick: 180 },
  { value: "7d", label: "7 天", hours: 168, tick: 1440 },
];

export const totalMemory = 32 * 1024 ** 3;
export const span = 60;
// What a curve receives: the shown span, one interval to scroll and two hidden samples beyond each edge (Curve.tsx).
export const samples = span + 6;
const MB = 1024 * 1024;
const user = "ARGUS-DEMO\\演示用户";
const sys = "NT AUTHORITY\\SYSTEM";
const local = "NT AUTHORITY\\LOCAL SERVICE";
const network = "NT AUTHORITY\\NETWORK SERVICE";
const win = "C:\\Windows\\System32\\";

const recorded: Sample[] = trace.map((p) => ({
  cpu: p.cpu,
  kernel: p.kernel,
  mem: p.memoryPercent,
  read: p.readKiB * 1024,
  write: p.writeKiB * 1024,
  active: p.active,
  rx: p.rxKiB * 1024,
  tx: p.txKiB * 1024,
  gpu: p.gpu,
}));
export const sampleAt = (tick: number) =>
  recorded[((tick % recorded.length) + recorded.length) % recorded.length];

// Deterministic noise in [0, 1): the server and the browser draw the same first frame.
export function noise(seed: number, tick: number) {
  let h = Math.imul(seed ^ 0x9e3779b9, 0x85ebca6b) ^ Math.imul(tick, 0xc2b2ae35);
  h ^= h >>> 15;
  h = Math.imul(h, 0x2c1b3c6d);
  h ^= h >>> 12;
  h = Math.imul(h, 0x297a2d39);
  h ^= h >>> 15;
  return (h >>> 0) / 4294967296;
}
const smooth = (seed: number, tick: number) =>
  (noise(seed, tick) + noise(seed, tick - 1) * 0.6 + noise(seed, tick - 2) * 0.3) /
  1.9;

const p = (
  pid: number,
  ppid: number,
  name: string,
  exe: string,
  group: Group,
  glyph: Glyph,
  memMB: number,
  extra: Partial<Process> = {},
): Process => ({
  pid,
  ppid,
  name,
  exe,
  group,
  glyph,
  mem: memMB * MB,
  path:
    group === "windows" ? win + exe : "C:\\Program Files\\" + name + "\\" + exe,
  user: group === "windows" ? sys : user,
  integrity: group === "windows" ? "系统" : "中",
  threads: 8 + ((pid * 7) % 40),
  handles: 120 + ((pid * 13) % 900),
  started: "11/3 08:" + String(10 + (pid % 49)).padStart(2, "0") + ":" + String(pid % 60).padStart(2, "0"),
  cpuTime: (pid * 37) % 5400,
  ...extra,
});

const edge = "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe";
const code =
  "C:\\Users\\demo\\AppData\\Local\\Programs\\Microsoft VS Code\\Code.exe";
const cargoBin = "C:\\Users\\demo\\.cargo\\bin\\";
export const processes: Process[] = [
  // Apps
  p(9876, 6120, "Microsoft Edge", "msedge.exe", "app", "browser", 268, { cpu: 1.2, path: edge, threads: 64, handles: 2140 }),
  p(9900, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 4.2, { path: edge, args: "--type=crashpad-handler" }),
  p(9912, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 310, { cpu: 1.6, gpu: 6, path: edge, args: "--type=gpu-process" }),
  p(9940, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 42, { cpu: 0.4, net: 7, path: edge, args: "--type=utility --utility-sub-type=network.mojom.NetworkService" }),
  p(9964, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 18, { disk: 0.6, path: edge, args: "--type=utility --utility-sub-type=storage.mojom.StorageService" }),
  p(10012, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 12, { cpu: 0.3, path: edge, args: "--type=utility --utility-sub-type=audio.mojom.AudioService" }),
  p(10140, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 420, { cpu: 3.2, path: edge, args: "--type=renderer" }),
  p(10188, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 236, { cpu: 1.4, path: edge, args: "--type=renderer" }),
  p(10232, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 168, { cpu: 0.6, path: edge, args: "--type=renderer" }),
  p(10276, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 96, { cpu: 0.2, path: edge, args: "--type=renderer" }),
  p(10320, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 81, { cpu: 0.1, path: edge, args: "--type=renderer" }),
  p(11240, 6120, "Visual Studio Code", "Code.exe", "app", "code", 210, { cpu: 0.8, path: code, threads: 52, handles: 1260 }),
  p(11268, 11240, "Visual Studio Code", "Code.exe", "app", "code", 122, { cpu: 0.6, gpu: 1.2, path: code, args: "--type=gpu-process" }),
  p(11296, 11240, "Visual Studio Code", "Code.exe", "app", "code", 384, { cpu: 1.1, path: code, args: "--type=renderer" }),
  p(11324, 11240, "Visual Studio Code", "Code.exe", "app", "code", 540, { cpu: 2.4, disk: 1.2, path: code, args: "--type=utility --utility-sub-type=node.mojom.NodeService" }),
  p(11352, 11240, "Visual Studio Code", "Code.exe", "app", "code", 61, { cpu: 0.2, disk: 0.8, path: code, args: "--type=utility (fileWatcher)" }),
  p(11380, 11240, "Visual Studio Code", "Code.exe", "app", "code", 30, { path: code, args: "--type=utility (ptyHost)" }),
  p(6120, 6060, "Windows 资源管理器", "explorer.exe", "app", "folder", 167.5, { cpu: 0.4, disk: 0.4, path: "C:\\Windows\\explorer.exe", threads: 118, handles: 4260 }),
  p(12880, 6120, "终端", "WindowsTerminal.exe", "app", "terminal", 96, { cpu: 0.3, gpu: 0.4, path: "C:\\Program Files\\WindowsApps\\Microsoft.WindowsTerminal\\WindowsTerminal.exe" }),
  p(14868, 6120, "画图", "mspaint.exe", "app", "paint", 134.2, { cpu: 0.2, path: "C:\\Program Files\\WindowsApps\\Microsoft.Paint\\PaintApp\\mspaint.exe" }),
  p(15204, 6120, "记事本", "Notepad.exe", "app", "note", 48, { cpu: 0.1, path: "C:\\Program Files\\WindowsApps\\Microsoft.WindowsNotepad\\Notepad\\Notepad.exe" }),
  p(16072, 6120, "Argus", "argus.exe", "app", "argus", 17.8, { cpu: 0.4, gpu: 0.6, path: "C:\\Program Files\\Argus\\argus.exe", threads: 28, handles: 610 }),
  p(17320, 1196, "设置", "SystemSettings.exe", "app", "gear", 31, { suspended: true, path: "C:\\Windows\\ImmersiveControlPanel\\SystemSettings.exe" }),
  // Background processes
  p(13004, 12880, "PowerShell 7", "pwsh.exe", "background", "pwsh", 78, { cpu: 0.1, path: "C:\\Program Files\\PowerShell\\7\\pwsh.exe" }),
  p(13010, 13004, "控制台窗口主进程", "conhost.exe", "background", "console", 3.1, { path: win + "conhost.exe" }),
  p(13954, 13004, "cargo", "cargo.exe", "background", "app", 32.1, { cpu: 0.6, disk: 1.6, path: cargoBin + "cargo.exe", args: "build --release" }),
  p(14102, 13954, "rustc", "rustc.exe", "background", "app", 1096, { cpu: 9, disk: 3.4, path: cargoBin + "rustc.exe", args: "--crate-name argus_core --edition=2024" }),
  p(14130, 13954, "rustc", "rustc.exe", "background", "app", 756.5, { cpu: 6, disk: 2.1, path: cargoBin + "rustc.exe", args: "--crate-name argus --edition=2024" }),
  p(13577, 11324, "rust-analyzer", "rust-analyzer.exe", "background", "app", 1016, { cpu: 2.2, disk: 0.9, path: "C:\\Users\\demo\\.vscode\\extensions\\rust-analyzer\\server\\rust-analyzer.exe" }),
  p(13729, 13004, "Node.js JavaScript Runtime", "node.exe", "background", "node", 84.1, { cpu: 0.4, net: 0.4, path: "C:\\Program Files\\nodejs\\node.exe", args: "vite --port 5173" }),
  p(8920, 6120, "OneDrive", "OneDrive.exe", "background", "cloud", 96, { cpu: 0.2, disk: 0.8, net: 2.6, path: "C:\\Users\\demo\\AppData\\Local\\Microsoft\\OneDrive\\OneDrive.exe", args: "/background" }),
  p(18283, 1196, "计算器", "CalculatorApp.exe", "background", "calc", 20.2, { suspended: true, path: "C:\\Program Files\\WindowsApps\\Microsoft.WindowsCalculator\\CalculatorApp.exe" }),
  p(9420, 1196, "手机连接", "PhoneExperienceHost.exe", "background", "phone", 64, { cpu: 0.1, net: 0.3, path: "C:\\Program Files\\WindowsApps\\Microsoft.YourPhone\\PhoneExperienceHost.exe" }),
  // Windows processes
  p(4, 0, "System", "System", "windows", "kernel", 0.1, { cpu: 0.6, disk: 2.4, path: "System", threads: 284, handles: 7820 }),
  p(148, 4, "Registry", "Registry", "windows", "kernel", 9.4, { path: "Registry" }),
  p(2864, 4, "内存压缩", "Memory Compression", "windows", "kernel", 640, { path: "Memory Compression" }),
  p(520, 4, "Windows 会话管理器", "smss.exe", "windows", "app", 0.4),
  p(812, 760, "客户端服务器运行时进程", "csrss.exe", "windows", "app", 1.9, { cpu: 0.1 }),
  p(916, 896, "客户端服务器运行时进程", "csrss.exe", "windows", "app", 2.6, { cpu: 0.2 }),
  p(904, 760, "Windows 启动应用程序", "wininit.exe", "windows", "app", 1.3),
  p(1004, 896, "Windows 登录应用程序", "winlogon.exe", "windows", "app", 2.4),
  p(1068, 904, "服务和控制器应用", "services.exe", "windows", "app", 6.8, { cpu: 0.1 }),
  p(1088, 904, "本地安全机构进程", "lsass.exe", "windows", "app", 9.6, { cpu: 0.1 }),
  p(1420, 1004, "桌面窗口管理器", "dwm.exe", "windows", "app", 140.1, { cpu: 2, gpu: 9, user: "Window Manager\\DWM-1" }),
  p(1236, 904, "Usermode Font Driver Host", "fontdrvhost.exe", "windows", "app", 1.6, { user: "Font Driver Host\\UMFD-0" }),
  p(1196, 1068, "服务主机：DCOM 服务器进程启动器", "svchost.exe", "windows", "service", 14.2, { cpu: 0.1, args: "-k DcomLaunch -p" }),
  p(1784, 1068, "服务主机：DNS Client", "svchost.exe", "windows", "service", 7.6, { net: 0.2, user: network, args: "-k NetworkService -p -s Dnscache" }),
  p(2072, 1068, "服务主机：DHCP Client", "svchost.exe", "windows", "service", 3.9, { net: 0.1, user: local, args: "-k LocalServiceNetworkRestricted -p -s Dhcp" }),
  p(2336, 1068, "服务主机：Windows Event Log", "svchost.exe", "windows", "service", 18.5, { cpu: 0.1, disk: 0.6, user: local, args: "-k LocalServiceNetworkRestricted -p -s EventLog" }),
  p(2896, 1068, "服务主机：Windows Audio", "svchost.exe", "windows", "service", 8.8, { cpu: 0.2, user: local, args: "-k LocalServiceNetworkRestricted -p -s AudioSrv" }),
  p(3192, 1068, "服务主机：Windows Update", "svchost.exe", "windows", "service", 26.6, { cpu: 0.3, disk: 1.4, net: 1.4, args: "-k netsvcs -p -s wuauserv" }),
  p(3376, 1068, "服务主机：SysMain", "svchost.exe", "windows", "service", 22.4, { disk: 1.8, args: "-k LocalSystemNetworkRestricted -p -s SysMain" }),
  p(2988, 1068, "后台处理程序子系统应用", "spoolsv.exe", "windows", "service", 6.9),
  p(3480, 1068, "Antimalware Service Executable", "MsMpEng.exe", "windows", "shield", 212, { cpu: 1.4, disk: 2.6, path: "C:\\ProgramData\\Microsoft\\Windows Defender\\Platform\\MsMpEng.exe" }),
  p(3280, 1068, "Microsoft Windows Search Indexer", "SearchIndexer.exe", "windows", "search", 38.4, { cpu: 0.3, disk: 1.6 }),
  p(7012, 1196, "搜索", "SearchHost.exe", "windows", "search", 121, { cpu: 0.1, user, integrity: "低" }),
  p(7104, 1196, "开始", "StartMenuExperienceHost.exe", "windows", "app", 61.5, { user, integrity: "低" }),
  p(7740, 1196, "Windows Shell Experience 主机", "ShellExperienceHost.exe", "windows", "app", 44.8, { user, integrity: "低" }),
  p(7320, 1196, "运行时代理", "RuntimeBroker.exe", "windows", "app", 8.4, { user }),
  p(7564, 1196, "运行时代理", "RuntimeBroker.exe", "windows", "app", 5.1, { user }),
  p(5544, 1196, "Windows 任务的主机进程", "taskhostw.exe", "windows", "app", 9.2, { user }),
  p(5400, 1068, "Shell Infrastructure Host", "sihost.exe", "windows", "app", 14.6, { cpu: 0.1, user }),
  p(6280, 5400, "CTF 加载程序", "ctfmon.exe", "windows", "app", 11.7, { cpu: 0.1, user }),
  p(11980, 2896, "Windows 音频设备图形隔离", "audiodg.exe", "windows", "app", 12.3, { cpu: 0.3, user: local }),
];

export const groupLabel: Record<Group, string> = {
  app: "应用",
  background: "后台进程",
  windows: "Windows 进程",
};

export interface Reading {
  cpu: number;
  mem: number;
  read: number;
  write: number;
  io: number;
  gpu: number;
  net: number;
}

// Share the recorded system totals among the processes that have weight in each.
export function readings(tick: number, list: Process[]) {
  const s = sampleAt(tick);
  const share = (key: "cpu" | "disk" | "net" | "gpu", seed: number) => {
    const weights = list.map((proc) => {
      const base = proc[key] ?? 0;
      if (!base || proc.suspended) return 0;
      return base * (0.35 + 1.3 * smooth(proc.pid * 31 + seed, tick));
    });
    const sum = weights.reduce((a, b) => a + b, 0) || 1;
    return weights.map((w) => w / sum);
  };
  const cpu = share("cpu", 1),
    disk = share("disk", 2),
    net = share("net", 3),
    gpu = share("gpu", 4);
  const out = new Map<number, Reading>();
  list.forEach((proc, i) => {
    const drift = 1 + (smooth(proc.pid * 17 + 9, Math.floor(tick / 4)) - 0.5) * 0.04;
    const readShare = disk[i] * (0.4 + noise(proc.pid, tick) * 0.6);
    out.set(proc.pid, {
      cpu: s.cpu * cpu[i],
      mem: proc.mem * drift,
      read: s.read * readShare,
      write: s.write * disk[i],
      io: (s.read * readShare + s.write * disk[i]) * 1.08 + s.rx * net[i] * 0.02,
      gpu: s.gpu * gpu[i] * 0.9,
      net: (s.rx + s.tx) * net[i],
    });
  });
  return out;
}

const heatRules = {
  percent: (v: number) => v / 40,
  memory: (v: number) => v / (2 * 1024 ** 3),
  rate: (v: number) => Math.log10(v / MB + 1) / 2,
};
const heatMinimum = { percent: 1, memory: 200 * MB, rate: MB };
export function heat(kind: keyof typeof heatRules, value: number) {
  if (!(value >= heatMinimum[kind])) return 0;
  return Math.min(1, Math.max(0, heatRules[kind](value))) * 0.34;
}

// Sizes and rates as the app writes them (theme::memory, theme::bytes).
export function bytes(value: number) {
  if (value < 1024) return Math.round(value) + " B";
  if (value < MB) return Math.round(value / 1024) + " KB";
  if (value < 1024 * MB) return (value / MB).toFixed(1) + " MB";
  const gb = value / (1024 * MB);
  return (gb >= 10 ? gb.toFixed(1) : gb.toFixed(2)) + " GB";
}
export function rate(value: number) {
  if (value < 1) return "0";
  if (value < MB) return Math.max(1, Math.round(value / 1024)) + " KB/s";
  if (value < 1024 * MB) return (value / MB).toFixed(1) + " MB/s";
  return (value / (1024 * MB)).toFixed(2) + " GB/s";
}
// Throughput axis tops: the app's fixed steps (charts/rate.rs), at least 100 KB/s.
export function rateCeiling(peak: number, network = false) {
  const kilo = network ? [100, 500] : [100, 200, 500];
  const rest = network ? [1, 5, 10, 50, 100, 500] : [1, 10, 50, 100, 200, 500];
  for (const [size, steps] of [[1024, kilo], [MB, rest], [1024 * MB, rest]] as [number, number[]][])
    for (const step of steps) if (step * size >= peak) return step * size;
  return peak;
}
export function rateLabel(top: number) {
  if (top >= 1024 * MB) return Math.round(top / (1024 * MB)) + " GB/s";
  if (top >= MB) return Math.round(top / MB) + " MB/s";
  return Math.round(top / 1024) + " KB/s";
}
export function percent(value: number) {
  if (value < 0.05) return "0";
  return value < 10 ? value.toFixed(1) : Math.round(value).toString();
}
export const thousands = (value: number) =>
  Math.round(value).toLocaleString("en-US");

// Logical processors 0–11 belong to six hyper-threaded performance cores, 12–19 are efficiency cores.
export const logical = Array.from({ length: 20 }, (_, i) => ({
  index: i,
  kind: i < 12 ? ("p" as const) : ("e" as const),
}));
export function coreLoad(index: number, tick: number) {
  const s = sampleAt(tick);
  const bias = index < 12 ? 1.18 : 0.74;
  const v = s.cpu * bias * (0.45 + 1.1 * smooth(index * 97 + 5, tick));
  return Math.min(100, v);
}
export function clock(kind: "p" | "e", tick: number) {
  const s = sampleAt(tick);
  const base = kind === "p" ? 3.3 : 2.6;
  return base + (s.cpu / 100) * 0.9 + noise(kind === "p" ? 11 : 12, tick) * 0.12;
}

export interface Service {
  display: string;
  name: string;
  pid?: number;
  start: "auto" | "autoDelayed" | "manual" | "disabled";
  description: string;
}
export const services: Service[] = [
  { display: "Background Intelligent Transfer Service", name: "BITS", start: "manual", description: "利用空闲网络带宽在后台传输文件。" },
  { display: "DHCP Client", name: "Dhcp", pid: 2072, start: "auto", description: "为此计算机注册并更新 IP 地址和 DNS 记录。" },
  { display: "DNS Client", name: "Dnscache", pid: 1784, start: "auto", description: "缓存 DNS 查询结果，并注册此计算机的完整名称。" },
  { display: "Microsoft Defender Antivirus Service", name: "WinDefend", pid: 3480, start: "auto", description: "帮助用户防止恶意软件及其他潜在的垃圾软件。" },
  { display: "Print Spooler", name: "Spooler", pid: 2988, start: "auto", description: "将打印作业排入队列并处理与打印机的交互。" },
  { display: "Remote Registry", name: "RemoteRegistry", start: "disabled", description: "使远程用户能修改此计算机上的注册表设置。" },
  { display: "SysMain", name: "SysMain", pid: 3376, start: "auto", description: "维护和提高一段时间内的系统性能。" },
  { display: "Task Scheduler", name: "Schedule", pid: 1196, start: "auto", description: "按计划自动执行任务。" },
  { display: "Windows Audio", name: "AudioSrv", pid: 2896, start: "auto", description: "管理基于 Windows 的程序的音频。" },
  { display: "Windows Event Log", name: "EventLog", pid: 2336, start: "auto", description: "管理事件和事件日志。" },
  { display: "Windows Search", name: "WSearch", pid: 3280, start: "autoDelayed", description: "为文件、电子邮件和其他内容提供索引。" },
  { display: "Windows Update", name: "wuauserv", pid: 3192, start: "manual", description: "启用检测、下载和安装 Windows 更新。" },
  { display: "Xbox Live 身份验证管理器", name: "XblAuthManager", start: "manual", description: "提供身份验证和授权服务。" },
  { display: "Netlogon", name: "Netlogon", start: "manual", description: "为用户和服务身份验证维护安全通道。" },
];
export const startLabel = {
  auto: "自动",
  autoDelayed: "自动（延迟启动）",
  manual: "手动",
  disabled: "禁用",
};

export interface StartupItem {
  name: string;
  publisher: string;
  enabled: boolean;
  location: string;
  command: string;
  missing?: boolean;
  glyph: Glyph;
}
export const startupItems: StartupItem[] = [
  { name: "Argus", publisher: "Argus contributors", enabled: true, location: "注册表（当前用户）", command: '"C:\\Program Files\\Argus\\argus.exe" --hidden', glyph: "argus" },
  { name: "Contoso Updater", publisher: "—", enabled: true, location: "注册表（所有用户，32 位）", command: '"C:\\Program Files (x86)\\Contoso\\updater.exe" /silent', missing: true, glyph: "app" },
  { name: "OneDrive", publisher: "Microsoft Corporation", enabled: true, location: "注册表（当前用户）", command: '"C:\\Users\\demo\\AppData\\Local\\Microsoft\\OneDrive\\OneDrive.exe" /background', glyph: "cloud" },
  { name: "SecurityHealth", publisher: "Microsoft Corporation", enabled: true, location: "注册表（所有用户）", command: "C:\\WINDOWS\\system32\\SecurityHealthSystray.exe", glyph: "shield" },
  { name: "手机连接", publisher: "Microsoft Corporation", enabled: false, location: "应用包（当前用户）", command: '"C:\\Program Files\\WindowsApps\\Microsoft.YourPhone\\PhoneExperienceHost.exe"', glyph: "phone" },
  { name: "终端", publisher: "Microsoft Corporation", enabled: false, location: "应用包（当前用户）", command: '"C:\\Program Files\\WindowsApps\\Microsoft.WindowsTerminal\\WindowsTerminal.exe"', glyph: "terminal" },
];

export interface Connection {
  pid: number;
  protocol: "TCP" | "UDP";
  local: string;
  localPort: number;
  remote?: string;
  remotePort?: number;
  state?: "listen" | "established" | "timeWait" | "closeWait";
}
export const stateLabel = {
  listen: "监听",
  established: "已连接",
  timeWait: "等待超时",
  closeWait: "等待关闭",
};
export const connections: Connection[] = [
  { pid: 9940, protocol: "TCP", local: "192.0.2.24", localPort: 52144, remote: "203.0.113.18", remotePort: 443, state: "established" },
  { pid: 9940, protocol: "TCP", local: "192.0.2.24", localPort: 52151, remote: "198.51.100.7", remotePort: 443, state: "established" },
  { pid: 9940, protocol: "TCP", local: "2001:db8::24", localPort: 52610, remote: "2001:db8:85a3::8a2e", remotePort: 443, state: "established" },
  { pid: 9940, protocol: "TCP", local: "127.0.0.1", localPort: 52388, remote: "127.0.0.1", remotePort: 5173, state: "established" },
  { pid: 13729, protocol: "TCP", local: "127.0.0.1", localPort: 5173, remote: "127.0.0.1", remotePort: 52388, state: "established" },
  { pid: 13729, protocol: "TCP", local: "127.0.0.1", localPort: 5173, state: "listen" },
  { pid: 8920, protocol: "TCP", local: "192.0.2.24", localPort: 52210, remote: "203.0.113.40", remotePort: 443, state: "established" },
  { pid: 3192, protocol: "TCP", local: "192.0.2.24", localPort: 52633, remote: "203.0.113.77", remotePort: 443, state: "closeWait" },
  { pid: 9420, protocol: "TCP", local: "192.0.2.24", localPort: 52502, remote: "198.51.100.23", remotePort: 443, state: "timeWait" },
  { pid: 1196, protocol: "TCP", local: "0.0.0.0", localPort: 135, state: "listen" },
  { pid: 4, protocol: "TCP", local: "0.0.0.0", localPort: 445, state: "listen" },
  { pid: 1088, protocol: "TCP", local: "0.0.0.0", localPort: 49664, state: "listen" },
  { pid: 9940, protocol: "UDP", local: "0.0.0.0", localPort: 5353 },
  { pid: 1784, protocol: "UDP", local: "0.0.0.0", localPort: 5355 },
  { pid: 2072, protocol: "UDP", local: "192.0.2.24", localPort: 68 },
];

export const machine = {
  cpu: "Intel Core i5-13500HX",
  gpu: "NVIDIA GeForce RTX 4060 Laptop GPU",
  disk: "Samsung SSD 990 PRO 2TB",
  wifi: "Intel Wi-Fi 6E AX211 160MHz",
};
