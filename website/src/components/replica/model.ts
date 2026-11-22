// Mock data for the interactive window. System totals are made up afresh for
// every sample (sampleAt); process rows share those totals out, so the
// columns add up the way they do in the app.

import metadata from "../../../../resources/app.json";
export const appVersion = metadata.version;
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
  /** The Windows session, where the account does not tell it (sessionOf). */
  session?: number;
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
export type HistoryRange = "5m" | "1h" | "3h" | "6h" | "1d" | "3d" | "7d" | "30d" | "all";
export const historyRanges: { value: HistoryRange; label: string; hours: number; tick: number }[] = [
  { value: "5m", label: "5 分钟", hours: 1 / 12, tick: 1 },
  { value: "1h", label: "1 小时", hours: 1, tick: 10 },
  { value: "3h", label: "3 小时", hours: 3, tick: 30 },
  { value: "6h", label: "6 小时", hours: 6, tick: 60 },
  { value: "1d", label: "1 天", hours: 24, tick: 180 },
  { value: "3d", label: "3 天", hours: 72, tick: 720 },
  { value: "7d", label: "7 天", hours: 168, tick: 1440 },
  { value: "30d", label: "30 天", hours: 720, tick: 7200 },
  { value: "all", label: "全部", hours: 720, tick: 7200 },
];

export const totalMemory = 32 * 1024 ** 3;
export const span = 60;
// What a curve receives: the shown span, one interval to scroll and two hidden samples beyond each edge (Curve.tsx).
export const samples = span + 6;
const MB = 1024 * 1024;
const user = "ARGUS-DEMO\\演示用户";
/** The signed-in account's name, as the user groups title it. */
export const userName = "演示用户";
/** A process's Windows session: services run in session 0, the signed-in user's programs in 1. */
export const sessionOf = (p: Process) => p.session ?? (p.user.startsWith("NT") ? 0 : 1);
const sys = "NT AUTHORITY\\SYSTEM";
const local = "NT AUTHORITY\\LOCAL SERVICE";
const network = "NT AUTHORITY\\NETWORK SERVICE";
const win = "C:\\Windows\\System32\\";

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
// Noise that eases from one value to the next over `period` samples.
function drift(seed: number, tick: number, period: number) {
  const x = tick / period,
    i = Math.floor(x),
    f = x - i,
    s = f * f * (3 - 2 * f);
  return noise(seed, i) * (1 - s) + noise(seed, i + 1) * s;
}
// A load in [0, 1) that changes slowly, with quicker changes on top.
const load = (seed: number, tick: number) =>
  drift(seed, tick, 23) * 0.5 + drift(seed + 1, tick, 7) * 0.32 + drift(seed + 2, tick, 2.5) * 0.13 + noise(seed + 3, tick) * 0.05;
// Now and then (in `chance` of the blocks of `every` samples) a burst of a few
// samples: it rises at once and falls back over its length. 0 outside them.
function burst(seed: number, tick: number, every: number, chance: number) {
  const block = Math.floor(tick / every);
  let peak = 0;
  for (const b of [block - 1, block]) {
    if (noise(seed, b) >= chance) continue;
    const length = 2 + Math.floor(noise(seed + 2, b) * 5);
    const at = tick - (b * every + Math.floor(noise(seed + 1, b) * every));
    if (at < 0 || at > length) continue;
    const shape = at === 0 ? 0.65 : 1 - (at - 1) / length;
    peak = Math.max(peak, shape * (0.5 + noise(seed + 3, b) * 0.5));
  }
  return peak;
}
// A restless load for the processor: quicker swings carry more weight.
const restless = (seed: number, tick: number) =>
  drift(seed, tick, 19) * 0.36 + drift(seed + 1, tick, 6) * 0.3 + drift(seed + 2, tick, 2.2) * 0.2 + noise(seed + 3, tick) * 0.14;
// A busy desktop, made up: never the same minute twice, yet any sample can be
// drawn again from its tick alone. The processor has short bursts of two
// kinds and, now and then, a stretch of sustained work.
function draw(tick: number): Sample {
  const busy = burst(110, tick, 9, 0.5);
  const work = Math.max(0, drift(105, tick, 45) - 0.55) / 0.45;
  const cpu = Math.min(97, 9 + restless(100, tick) * 40 + work * 24 + busy * 40 + burst(115, tick, 17, 0.4) * 26);
  const read = 160 + load(140, tick) ** 2 * 1600 + burst(150, tick, 9, 0.25) * 22000;
  const write = 1500 + load(160, tick) ** 2 * 7000 + burst(170, tick, 11, 0.45) * 46000;
  const rx = 9500 + load(180, tick) * 9000 + burst(190, tick, 10, 0.4) * 9000;
  const tx = 5000 + load(200, tick) * 6500 + burst(210, tick, 10, 0.35) * 3000;
  return {
    cpu,
    kernel: cpu * (0.3 + drift(120, tick, 9) * 0.18),
    mem: 39 + drift(130, tick, 40) * 3.2 + busy * 0.8,
    read: read * 1024,
    write: write * 1024,
    active: Math.min(100, 1.5 + (read + write) / 1500),
    rx: rx * 1024,
    tx: tx * 1024,
    gpu: Math.min(98, 22 + load(220, tick) * 38 + burst(230, tick, 14, 0.5) * 30),
  };
}
// Each tick is read by many cells and charts; the recent ones are kept.
const drawn = new Map<number, Sample>();
export function sampleAt(tick: number) {
  let sample = drawn.get(tick);
  if (!sample) {
    sample = draw(tick);
    drawn.set(tick, sample);
    if (drawn.size > 512) drawn.delete(drawn.keys().next().value as number);
  }
  return sample;
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
): Process => {
  const owner = extra.user ?? (group === "windows" ? sys : user);
  // Services and the boot chain start with Windows; the rest once the user signs in.
  const boot = owner.startsWith("NT") || owner.includes("UMFD-0");
  const clock = boot ? `8:0${1 + (pid % 3)}` : "8:" + String(10 + (pid % 49)).padStart(2, "0");
  return {
    pid,
    ppid,
    name,
    exe,
    group,
    glyph,
    mem: memMB * MB,
    path: group === "windows" ? win + exe : "C:\\Program Files\\" + name + "\\" + exe,
    user: owner,
    integrity: owner.startsWith("NT") ? "系统" : "中",
    threads: 8 + ((pid * 7) % 40),
    handles: 120 + ((pid * 13) % 900),
    // Windows' short date and time, as the app writes them (regional.rs).
    started: `2026/11/3 ${clock}:${String(pid % 60).padStart(2, "0")}`,
    cpuTime: (pid * 37) % 5400,
    ...extra,
  };
};
// A service host: its name follows the services it runs (named below, after the services).
const svcHost = (pid: number, memMB: number, args: string, extra: Partial<Process> = {}) =>
  p(pid, 1068, "", "svchost.exe", "windows", "service", memMB, { args, ...extra });

const edge = "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe";
const chrome = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const code = "C:\\Users\\demo\\AppData\\Local\\Programs\\Microsoft VS Code\\Code.exe";
const cargoBin = "C:\\Users\\demo\\.cargo\\bin\\";
const analyzer = "C:\\Users\\demo\\.vscode\\extensions\\rust-lang.rust-analyzer-0.3.2196-win32-x64\\server\\";
const driverStore = "C:\\WINDOWS\\System32\\DriverStore\\FileRepository\\";
const defender = "C:\\ProgramData\\Microsoft\\Windows Defender\\Platform\\4.18.24090.11-0\\";
const systemApps = "C:\\Windows\\SystemApps\\";
const nvidia = driverStore + "nv_dispig.inf_amd64_5d3f0a2c1b7e9a41\\Display.NvContainer\\NVDisplay.Container.exe";
const realtek = driverStore + "realtekservice.inf_amd64_8a2f4c1e6d3b5a70\\RtkAudUService64.exe";
const teams = "C:\\Program Files\\WindowsApps\\MSTeams_25290.205.4069.4894_x64__8wekyb3d8bbwe\\ms-teams.exe";
const webview = "C:\\Program Files (x86)\\Microsoft\\EdgeWebView\\Application\\131.0.2903.112\\msedgewebview2.exe";
const discord = "C:\\Users\\demo\\AppData\\Local\\Discord\\app-1.0.9175\\Discord.exe";
const steamHelper = "C:\\Program Files (x86)\\Steam\\bin\\cef\\cef.win7x64\\steamwebhelper.exe";
const toys = "C:\\Program Files\\PowerToys\\";
const webExperience = "C:\\Program Files\\WindowsApps\\MicrosoftWindows.Client.WebExperience_524.33400.0.0_x64__cw5n1h2txyewy\\";
const terminalPackage = "C:\\Program Files\\WindowsApps\\Microsoft.WindowsTerminal_1.21.3231.0_x64__8wekyb3d8bbwe\\";
const dwmUser = "Window Manager\\DWM-1";
// A Windows 11 developer laptop: the system's own processes and services,
// driver helpers, and the signed-in user's apps with their children.
export const processes: Process[] = [
  // Apps
  p(9876, 6120, "Microsoft Edge", "msedge.exe", "app", "browser", 268, { cpu: 1.2, path: edge, threads: 64, handles: 2140 }),
  p(9900, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 4.2, { path: edge, args: "--type=crashpad-handler" }),
  p(9912, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 236, { cpu: 1.6, gpu: 6, path: edge, args: "--type=gpu-process" }),
  p(9940, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 42, { cpu: 0.4, net: 7, path: edge, args: "--type=utility --utility-sub-type=network.mojom.NetworkService" }),
  p(9964, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 18, { disk: 0.6, path: edge, args: "--type=utility --utility-sub-type=storage.mojom.StorageService" }),
  p(10012, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 12, { cpu: 0.3, path: edge, args: "--type=utility --utility-sub-type=audio.mojom.AudioService" }),
  p(10140, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 340, { cpu: 3.2, path: edge, args: "--type=renderer" }),
  p(10188, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 236, { cpu: 1.4, path: edge, args: "--type=renderer" }),
  p(10232, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 168, { cpu: 0.6, path: edge, args: "--type=renderer" }),
  p(10276, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 96, { cpu: 0.2, path: edge, args: "--type=renderer" }),
  p(10320, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 81, { cpu: 0.1, path: edge, args: "--type=renderer" }),
  p(10364, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 188, { cpu: 0.9, path: edge, args: "--type=renderer" }),
  p(10408, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 142, { cpu: 0.3, path: edge, args: "--type=renderer" }),
  p(10452, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 64, { path: edge, args: "--type=renderer" }),
  p(10496, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 58, { path: edge, args: "--type=renderer" }),
  p(10540, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 46, { cpu: 0.1, path: edge, args: "--type=renderer --extension-process" }),
  p(10584, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 52, { path: edge, args: "--type=renderer" }),
  p(10628, 9876, "Microsoft Edge", "msedge.exe", "app", "browser", 38, { path: edge, args: "--type=utility --utility-sub-type=video_capture.mojom.VideoCaptureService" }),
  p(14400, 6120, "Google Chrome", "chrome.exe", "app", "browser", 148, { cpu: 0.6, path: chrome, threads: 58, handles: 1720 }),
  p(14440, 14400, "Google Chrome", "chrome.exe", "app", "browser", 6.1, { path: chrome, args: "--type=crashpad-handler" }),
  p(14480, 14400, "Google Chrome", "chrome.exe", "app", "browser", 186, { cpu: 0.8, gpu: 3, path: chrome, args: "--type=gpu-process" }),
  p(14520, 14400, "Google Chrome", "chrome.exe", "app", "browser", 36, { net: 2.4, path: chrome, args: "--type=utility --utility-sub-type=network.mojom.NetworkService" }),
  p(14560, 14400, "Google Chrome", "chrome.exe", "app", "browser", 14, { disk: 0.3, path: chrome, args: "--type=utility --utility-sub-type=storage.mojom.StorageService" }),
  p(14600, 14400, "Google Chrome", "chrome.exe", "app", "browser", 132, { cpu: 0.7, path: chrome, args: "--type=renderer" }),
  p(14640, 14400, "Google Chrome", "chrome.exe", "app", "browser", 74, { cpu: 0.2, path: chrome, args: "--type=renderer" }),
  p(14680, 14400, "Google Chrome", "chrome.exe", "app", "browser", 24, { path: chrome, args: "--type=renderer --extension-process" }),
  p(11240, 6120, "Visual Studio Code", "Code.exe", "app", "code", 210, { cpu: 0.8, path: code, threads: 52, handles: 1260 }),
  p(11268, 11240, "Visual Studio Code", "Code.exe", "app", "code", 122, { cpu: 0.6, gpu: 1.2, path: code, args: "--type=gpu-process" }),
  p(11296, 11240, "Visual Studio Code", "Code.exe", "app", "code", 330, { cpu: 1.1, path: code, args: "--type=renderer" }),
  p(11324, 11240, "Visual Studio Code", "Code.exe", "app", "code", 470, { cpu: 2.4, disk: 1.2, path: code, args: "--type=utility --utility-sub-type=node.mojom.NodeService" }),
  p(11352, 11240, "Visual Studio Code", "Code.exe", "app", "code", 61, { cpu: 0.2, disk: 0.8, path: code, args: "--type=utility (fileWatcher)" }),
  p(11380, 11240, "Visual Studio Code", "Code.exe", "app", "code", 30, { path: code, args: "--type=utility (ptyHost)" }),
  p(11408, 11324, "Visual Studio Code", "Code.exe", "app", "code", 248, { cpu: 0.7, path: code, args: '--max-old-space-size=3072 "c:\\Users\\demo\\AppData\\Local\\Programs\\Microsoft VS Code\\resources\\app\\extensions\\node_modules\\typescript\\lib\\tsserver.js" --useInferredProjectPerProjectRoot' }),
  p(11436, 11324, "Visual Studio Code", "Code.exe", "app", "code", 96, { cpu: 0.2, path: code, args: '"c:\\Users\\demo\\.vscode\\extensions\\dbaeumer.vscode-eslint-3.0.10\\server\\out\\eslintServer.js" --node-ipc' }),
  p(11464, 11240, "Visual Studio Code", "Code.exe", "app", "code", 72, { path: code, args: "--type=utility --utility-sub-type=shared.mojom.SharedProcess" }),
  p(6120, 6060, "Windows 资源管理器", "explorer.exe", "app", "folder", 167.5, { cpu: 0.4, disk: 0.4, path: "C:\\Windows\\explorer.exe", threads: 118, handles: 4260 }),
  p(12880, 6120, "终端", "WindowsTerminal.exe", "app", "terminal", 96, { cpu: 0.3, gpu: 0.4, path: terminalPackage + "WindowsTerminal.exe", threads: 44, handles: 980 }),
  p(14868, 6120, "画图", "mspaint.exe", "app", "paint", 134.2, { cpu: 0.2, path: "C:\\Program Files\\WindowsApps\\Microsoft.Paint_11.2410.28.0_x64__8wekyb3d8bbwe\\PaintApp\\mspaint.exe" }),
  p(15204, 6120, "记事本", "Notepad.exe", "app", "note", 48, { cpu: 0.1, path: "C:\\Program Files\\WindowsApps\\Microsoft.WindowsNotepad_11.2410.21.0_x64__8wekyb3d8bbwe\\Notepad\\Notepad.exe" }),
  p(16072, 6120, "Argus", "argus.exe", "app", "argus", 17.8, { cpu: 0.4, gpu: 0.6, path: "C:\\Program Files\\Argus\\argus.exe", threads: 28, handles: 610 }),
  p(17320, 1196, "设置", "SystemSettings.exe", "app", "gear", 31, { suspended: true, path: "C:\\Windows\\ImmersiveControlPanel\\SystemSettings.exe", integrity: "低" }),
  p(16800, 6120, "Microsoft Teams", "ms-teams.exe", "app", "app", 118, { cpu: 0.3, net: 0.6, path: teams, threads: 58, handles: 1840 }),
  p(16840, 16800, "WebView2：Microsoft Teams", "msedgewebview2.exe", "app", "browser", 92, { cpu: 0.2, net: 1.8, path: webview, args: "--embedded-browser-webview=1 --webview-exe-name=ms-teams.exe", threads: 46, handles: 1320 }),
  p(16880, 16840, "WebView2：Microsoft Teams", "msedgewebview2.exe", "app", "browser", 168, { cpu: 0.5, gpu: 2.2, path: webview, args: "--type=gpu-process --webview-exe-name=ms-teams.exe" }),
  p(16920, 16840, "WebView2：Microsoft Teams", "msedgewebview2.exe", "app", "browser", 34, { cpu: 0.1, net: 0.8, path: webview, args: "--type=utility --utility-sub-type=network.mojom.NetworkService --webview-exe-name=ms-teams.exe" }),
  p(16960, 16840, "WebView2：Microsoft Teams", "msedgewebview2.exe", "app", "browser", 286, { cpu: 1.1, path: webview, args: "--type=renderer --webview-exe-name=ms-teams.exe" }),
  p(17000, 16840, "WebView2：Microsoft Teams", "msedgewebview2.exe", "app", "browser", 126, { cpu: 0.2, path: webview, args: "--type=renderer --webview-exe-name=ms-teams.exe" }),
  p(17040, 16840, "WebView2：Microsoft Teams", "msedgewebview2.exe", "app", "browser", 21, { path: webview, args: "--type=utility --utility-sub-type=storage.mojom.StorageService --webview-exe-name=ms-teams.exe" }),
  p(16200, 6120, "Discord", "Discord.exe", "app", "app", 96, { cpu: 0.3, net: 0.4, path: discord, threads: 66, handles: 1180 }),
  p(16240, 16200, "Discord", "Discord.exe", "app", "app", 142, { cpu: 0.4, gpu: 1.6, path: discord, args: "--type=gpu-process" }),
  p(16280, 16200, "Discord", "Discord.exe", "app", "app", 28, { net: 0.9, path: discord, args: "--type=utility --utility-sub-type=network.mojom.NetworkService" }),
  p(16320, 16200, "Discord", "Discord.exe", "app", "app", 198, { cpu: 0.8, path: discord, args: "--type=renderer" }),
  p(16360, 16200, "Discord", "Discord.exe", "app", "app", 9.6, { path: discord, args: "--type=crashpad-handler" }),
  // Background processes
  // One fictional utility illustrates the resource-leak warning.
  p(18340, 6120, "Contoso Sync", "ContosoSync.exe", "background", "app", 42, { cpu: 0.2, handles: 38240, path: "C:\\Program Files\\Contoso\\ContosoSync.exe" }),
  p(13004, 12880, "PowerShell 7", "pwsh.exe", "background", "pwsh", 78, { cpu: 0.1, path: "C:\\Program Files\\PowerShell\\7\\pwsh.exe" }),
  p(13010, 13004, "控制台窗口主进程", "conhost.exe", "background", "console", 3.1, { path: win + "conhost.exe" }),
  p(12920, 12880, "Console Window Host", "OpenConsole.exe", "background", "console", 4.6, { path: terminalPackage + "OpenConsole.exe" }),
  p(12960, 12880, "Console Window Host", "OpenConsole.exe", "background", "console", 4.2, { path: terminalPackage + "OpenConsole.exe" }),
  p(13520, 12880, "PowerShell 7", "pwsh.exe", "background", "pwsh", 64, { path: "C:\\Program Files\\PowerShell\\7\\pwsh.exe", args: "-NoLogo" }),
  p(13660, 12880, "Windows Subsystem for Linux", "wsl.exe", "background", "terminal", 6.2, { path: "C:\\Program Files\\WSL\\wsl.exe", args: "-d Ubuntu-24.04" }),
  p(13954, 13004, "cargo", "cargo.exe", "background", "app", 32.1, { cpu: 0.6, disk: 1.6, path: cargoBin + "cargo.exe", args: "build --release" }),
  p(14102, 13954, "rustc", "rustc.exe", "background", "app", 880, { cpu: 9, disk: 3.4, path: cargoBin + "rustc.exe", args: "--crate-name argus_core --edition=2024" }),
  p(14130, 13954, "rustc", "rustc.exe", "background", "app", 610, { cpu: 6, disk: 2.1, path: cargoBin + "rustc.exe", args: "--crate-name argus --edition=2024" }),
  p(13577, 11324, "rust-analyzer", "rust-analyzer.exe", "background", "app", 860, { cpu: 2.2, disk: 0.9, path: analyzer + "rust-analyzer.exe" }),
  p(13600, 13577, "rust-analyzer-proc-macro-srv", "rust-analyzer-proc-macro-srv.exe", "background", "app", 48, { cpu: 0.1, path: analyzer + "rust-analyzer-proc-macro-srv.exe" }),
  p(13729, 13004, "Node.js JavaScript Runtime", "node.exe", "background", "node", 84.1, { cpu: 0.4, net: 0.4, path: "C:\\Program Files\\nodejs\\node.exe", args: "vite --port 5173" }),
  p(8920, 6120, "OneDrive", "OneDrive.exe", "background", "cloud", 96, { cpu: 0.2, disk: 0.8, net: 2.6, path: "C:\\Users\\demo\\AppData\\Local\\Microsoft\\OneDrive\\OneDrive.exe", args: "/background" }),
  p(9000, 8920, "Microsoft OneDriveFile Co-Authoring Executable", "FileCoAuth.exe", "background", "cloud", 11.4, { path: "C:\\Users\\demo\\AppData\\Local\\Microsoft\\OneDrive\\24.226.1110.0004\\FileCoAuth.exe", args: "-Embedding" }),
  p(18283, 1196, "计算器", "CalculatorApp.exe", "background", "calc", 20.2, { suspended: true, path: "C:\\Program Files\\WindowsApps\\Microsoft.WindowsCalculator_11.2410.0.0_x64__8wekyb3d8bbwe\\CalculatorApp.exe", integrity: "低" }),
  p(9420, 1196, "手机连接", "PhoneExperienceHost.exe", "background", "phone", 64, { cpu: 0.1, net: 0.3, path: "C:\\Program Files\\WindowsApps\\Microsoft.YourPhone_1.24112.89.0_x64__8wekyb3d8bbwe\\PhoneExperienceHost.exe" }),
  p(9620, 1196, "Widgets", "Widgets.exe", "background", "app", 58, { path: webExperience + "Dashboard\\Widgets.exe", integrity: "低" }),
  p(9660, 1196, "Widget Service", "WidgetService.exe", "background", "app", 14.2, { path: webExperience + "WidgetService.exe" }),
  p(10700, 6120, "PowerToys Runner", "PowerToys.exe", "background", "app", 42, { cpu: 0.1, path: toys + "PowerToys.exe", threads: 30, handles: 720 }),
  p(10740, 10700, "PowerToys.Awake", "PowerToys.Awake.exe", "background", "app", 22, { path: toys + "PowerToys.Awake.exe" }),
  p(10780, 10700, "PowerToys.PowerLauncher", "PowerToys.PowerLauncher.exe", "background", "app", 62, { path: toys + "PowerToys.PowerLauncher.exe" }),
  p(10820, 10700, "PowerToys.FancyZones", "PowerToys.FancyZones.exe", "background", "app", 16, { path: toys + "PowerToys.FancyZones.exe" }),
  p(15600, 6120, "Steam", "steam.exe", "background", "app", 74, { cpu: 0.1, net: 0.3, path: "C:\\Program Files (x86)\\Steam\\steam.exe", args: "-silent", threads: 118, handles: 1960 }),
  p(15640, 15600, "Steam Client WebHelper", "steamwebhelper.exe", "background", "app", 66, { path: steamHelper, args: "-lang=schinese -cachedir=C:\\Users\\demo\\AppData\\Local\\Steam\\htmlcache" }),
  p(15680, 15640, "Steam Client WebHelper", "steamwebhelper.exe", "background", "app", 62, { gpu: 0.2, path: steamHelper, args: "--type=gpu-process" }),
  p(15720, 15640, "Steam Client WebHelper", "steamwebhelper.exe", "background", "app", 24, { net: 0.2, path: steamHelper, args: "--type=utility --utility-sub-type=network.mojom.NetworkService" }),
  p(15760, 15640, "Steam Client WebHelper", "steamwebhelper.exe", "background", "app", 86, { cpu: 0.1, path: steamHelper, args: "--type=renderer" }),
  p(15800, 15640, "Steam Client WebHelper", "steamwebhelper.exe", "background", "app", 41, { path: steamHelper, args: "--type=renderer" }),
  // Drivers' helpers, WSL and the privileged service
  p(2740, 1068, "NVIDIA Container", "NVDisplay.Container.exe", "background", "app", 9.8, { path: nvidia, user: sys, args: "-s NVDisplay.ContainerLocalSystem -f C:\\ProgramData\\NVIDIA\\NVDisplay.ContainerLocalSystem.log -l 3 -d", threads: 22, handles: 610 }),
  p(9700, 2740, "NVIDIA Container", "NVDisplay.Container.exe", "background", "app", 31, { cpu: 0.1, path: nvidia, user: sys, session: 1, args: "-p 30000 -cfg NVDisplay.ContainerLocalSystem\\LocalSystem", threads: 46, handles: 1080 }),
  p(4040, 1068, "Intel(R) Graphics Command Center Service", "OneApp.IGCC.WinService.exe", "background", "app", 19.6, { path: "C:\\Program Files\\WindowsApps\\AppUp.IntelGraphicsExperience_1.100.5536.0_x64__8j3eq9eme6ctt\\Service\\OneApp.IGCC.WinService.exe", user: sys }),
  p(4084, 1068, "Realtek HD Audio Universal Service", "RtkAudUService64.exe", "background", "app", 3.4, { path: realtek, user: sys }),
  p(9740, 4084, "Realtek HD Audio Universal Service", "RtkAudUService64.exe", "background", "app", 5.8, { path: realtek, args: "-background" }),
  p(4128, 1068, "Intel(R) Dynamic Application Loader Host Interface", "jhi_service.exe", "background", "app", 1.4, { path: driverStore + "dal.inf_amd64_3c9e1f7a2b4d6e85\\jhi_service.exe", user: sys }),
  p(4392, 1068, "Intel(R) Content Protection HDCP Service", "IntelCpHDCPSvc.exe", "background", "app", 1.8, { path: driverStore + "iigd_dch.inf_amd64_6b2e9d4f1a7c3e58\\IntelCpHDCPSvc.exe", user: sys }),
  p(4480, 1068, "Intel(R) Management Engine WMI Provider Registration", "WMIRegistrationService.exe", "background", "app", 2.1, { path: driverStore + "mewmiprov.inf_amd64_4f7a1c3e9b2d6a05\\WMIRegistrationService.exe", user: sys }),
  p(5128, 1068, "Argus", "argus-service-4812-1762127411.exe", "background", "argus", 6.4, { cpu: 0.1, path: "C:\\Program Files\\Argus Privileged\\argus-service-4812-1762127411.exe", args: "--argus-service", user: sys, threads: 12, handles: 260 }),
  p(8204, 1068, "WSL Service", "wslservice.exe", "background", "app", 12.6, { path: "C:\\Program Files\\WSL\\wslservice.exe", user: sys }),
  p(8248, 1068, "Hyper-V Host Compute Service", "vmcompute.exe", "background", "app", 9.1, { path: win + "vmcompute.exe", user: sys }),
  p(8312, 8248, "VmmemWSL", "vmmemWSL", "background", "kernel", 1280, { cpu: 1.6, disk: 1.1, net: 0.3, path: "vmmemWSL", user: sys, threads: 20, handles: 0 }),
  // Windows processes
  p(4, 0, "System", "System", "windows", "kernel", 0.1, { cpu: 0.6, disk: 2.4, path: "System", threads: 284, handles: 7820 }),
  p(148, 4, "Registry", "Registry", "windows", "kernel", 9.4, { path: "Registry", threads: 4, handles: 0 }),
  p(140, 4, "Secure System", "Secure System", "windows", "kernel", 42, { path: "Secure System", threads: 0, handles: 0 }),
  p(2864, 4, "内存压缩", "Memory Compression", "windows", "kernel", 480, { path: "Memory Compression", threads: 18, handles: 0 }),
  p(520, 4, "Windows 会话管理器", "smss.exe", "windows", "app", 0.4, { threads: 2, handles: 53 }),
  p(812, 760, "客户端服务器运行时进程", "csrss.exe", "windows", "app", 1.9, { cpu: 0.1, threads: 12, handles: 640 }),
  p(916, 896, "客户端服务器运行时进程", "csrss.exe", "windows", "app", 2.6, { cpu: 0.2, session: 1, threads: 16, handles: 1080 }),
  p(904, 760, "Windows 启动应用程序", "wininit.exe", "windows", "app", 1.3, { threads: 1, handles: 160 }),
  p(1004, 896, "Windows 登录应用程序", "winlogon.exe", "windows", "app", 2.4, { session: 1, threads: 4, handles: 290 }),
  p(1068, 904, "服务和控制器应用", "services.exe", "windows", "app", 6.8, { cpu: 0.1, threads: 9, handles: 980 }),
  p(1088, 904, "本地安全机构进程", "lsass.exe", "windows", "app", 9.6, { cpu: 0.1, threads: 11, handles: 1820 }),
  p(1080, 904, "Credential Guard & VBS Key Isolation", "LsaIso.exe", "windows", "app", 1.1, { threads: 2, handles: 40 }),
  p(1236, 904, "Usermode Font Driver Host", "fontdrvhost.exe", "windows", "app", 1.6, { user: "Font Driver Host\\UMFD-0", session: 0, threads: 5, handles: 38 }),
  p(1244, 1004, "Usermode Font Driver Host", "fontdrvhost.exe", "windows", "app", 4.2, { user: "Font Driver Host\\UMFD-1", threads: 6, handles: 44 }),
  p(1420, 1004, "桌面窗口管理器", "dwm.exe", "windows", "app", 140.1, { cpu: 2, gpu: 9, user: dwmUser, integrity: "系统", threads: 24, handles: 3200 }),
  svcHost(1196, 14.2, "-k DcomLaunch -p", { cpu: 0.1, threads: 18, handles: 1640 }),
  svcHost(1296, 11.8, "-k RPCSS -p", { user: network, threads: 14, handles: 1310 }),
  svcHost(1340, 2.6, "-k DcomLaunch -p -s LSM"),
  svcHost(1512, 3.8, "-k netsvcs -p -s gpsvc"),
  svcHost(1560, 6.2, "-k netsvcs -p -s Schedule", { cpu: 0.1 }),
  svcHost(1652, 3.1, "-k netsvcs -p -s ProfSvc"),
  svcHost(1744, 4.6, "-k netsvcs -p -s UserManager"),
  svcHost(1784, 7.6, "-k NetworkService -p -s Dnscache", { net: 0.2, user: network }),
  svcHost(2072, 3.9, "-k LocalServiceNetworkRestricted -p -s Dhcp", { net: 0.1, user: local }),
  svcHost(2120, 5.2, "-k NetworkService -p -s NlaSvc", { user: network }),
  svcHost(2164, 1.9, "-k LocalService -p -s nsi", { user: local }),
  svcHost(2208, 8.4, "-k LocalServiceNoNetworkFirewall -p", { cpu: 0.1, user: local }),
  svcHost(2252, 12.1, "-k LocalServiceNoNetwork -p", { user: local }),
  svcHost(2336, 18.5, "-k LocalServiceNetworkRestricted -p -s EventLog", { cpu: 0.1, disk: 0.6, user: local }),
  svcHost(2420, 6.8, "-k NetworkService -p -s CryptSvc", { user: network }),
  svcHost(2464, 4.1, "-k NetworkService -p -s LanmanWorkstation", { user: network }),
  svcHost(2548, 6.9, "-k LocalSystemNetworkRestricted -p -s AudioEndpointBuilder"),
  svcHost(2604, 9.7, "-k netsvcs -p -s Winmgmt", { cpu: 0.1 }),
  svcHost(2636, 3.3, "-k LocalServiceNetworkRestricted -p -s Wcmsvc", { user: local }),
  svcHost(2680, 5.4, "-k LocalSystemNetworkRestricted -p -s WlanSvc", { net: 0.1 }),
  svcHost(2896, 8.8, "-k LocalServiceNetworkRestricted -p -s AudioSrv", { cpu: 0.2, user: local }),
  svcHost(3020, 2.9, "-k LocalService -p -s bthserv", { user: local }),
  svcHost(3104, 24.8, "-k appmodel -p -s StateRepository", { cpu: 0.1, disk: 0.3 }),
  svcHost(3192, 26.6, "-k netsvcs -p -s wuauserv", { cpu: 0.3, disk: 1.4, net: 1.4 }),
  svcHost(3236, 9.4, "-k netsvcs -p -s UsoSvc"),
  svcHost(3376, 22.4, "-k LocalSystemNetworkRestricted -p -s SysMain", { disk: 1.8 }),
  svcHost(3512, 4.9, "-k netsvcs -p -s LanmanServer"),
  svcHost(3600, 3.7, "-k NetSvcs -p -s iphlpsvc"),
  svcHost(3720, 3.2, "-k LocalServiceNetworkRestricted -p -s wscsvc", { user: local }),
  svcHost(3828, 7.1, "-k LocalService -p -s CDPSvc", { user: local }),
  svcHost(3864, 5.6, "-k netsvcs -p -s WpnService", { net: 0.1 }),
  svcHost(4344, 2.5, "-k LocalServiceAndNoImpersonation -p -s SSDPSRV", { user: local }),
  svcHost(4980, 4.8, "-k appmodel -p -s camsvc"),
  svcHost(5180, 8.9, "-k netsvcs -p -s TokenBroker"),
  svcHost(5640, 9.2, "-k UnistackSvcGroup", { user, session: 1 }),
  svcHost(6740, 11.6, "-k NetworkService -p -s DoSvc", { disk: 0.4, net: 0.6, user: network }),
  p(1880, 1068, "Windows Driver Foundation - User-mode Driver Framework Host Process", "WUDFHost.exe", "windows", "app", 3.6, { user: local }),
  p(2724, 2680, "Windows Wireless LAN 802.11 Extensibility Framework", "wlanext.exe", "windows", "app", 2.8),
  p(2732, 2724, "控制台窗口主进程", "conhost.exe", "windows", "console", 1.2),
  p(2988, 1068, "后台处理程序子系统应用", "spoolsv.exe", "windows", "service", 6.9, { threads: 10, handles: 520 }),
  p(3280, 1068, "Microsoft Windows Search Indexer", "SearchIndexer.exe", "windows", "search", 38.4, { cpu: 0.3, disk: 1.6, args: "/Embedding" }),
  p(13240, 3280, "Microsoft Windows Search Protocol Host", "SearchProtocolHost.exe", "windows", "search", 4.1, { disk: 0.2 }),
  p(13284, 3280, "Microsoft Windows Search Filter Host", "SearchFilterHost.exe", "windows", "search", 2.6),
  p(3480, 1068, "Antimalware Service Executable", "MsMpEng.exe", "windows", "shield", 180, { cpu: 1.4, disk: 2.6, net: 0.1, path: defender + "MsMpEng.exe", threads: 46, handles: 1480 }),
  p(3448, 1068, "Antimalware Core Service", "MpDefenderCoreService.exe", "windows", "shield", 18.6, { path: defender + "MpDefenderCoreService.exe" }),
  p(4512, 1068, "Microsoft Network Realtime Inspection Service", "NisSrv.exe", "windows", "shield", 8.4, { path: defender + "NisSrv.exe", user: local }),
  p(4560, 1068, "Windows Security Health Service", "SecurityHealthService.exe", "windows", "shield", 7.8),
  p(9180, 6120, "Windows Security notification icon", "SecurityHealthSystray.exe", "windows", "shield", 2.4, { user }),
  p(4248, 1068, "System Guard Runtime Monitor Broker Service", "SgrmBroker.exe", "windows", "app", 4.6),
  p(4700, 1196, "WMI Provider Host", "WmiPrvSE.exe", "windows", "app", 9.8, { cpu: 0.1, path: win + "wbem\\WmiPrvSE.exe", user: network }),
  p(6900, 1196, "WMI Provider Host", "WmiPrvSE.exe", "windows", "app", 6.1, { path: win + "wbem\\WmiPrvSE.exe" }),
  p(5024, 1196, "Device Association Framework Provider Host", "dasHost.exe", "windows", "app", 4.3, { user: local }),
  p(5400, 1068, "Shell Infrastructure Host", "sihost.exe", "windows", "app", 14.6, { cpu: 0.1, user }),
  p(5544, 1560, "Windows 任务的主机进程", "taskhostw.exe", "windows", "app", 9.2, { user }),
  p(6280, 5400, "CTF 加载程序", "ctfmon.exe", "windows", "app", 11.7, { cpu: 0.1, user }),
  p(7012, 1196, "搜索", "SearchHost.exe", "windows", "search", 96, { cpu: 0.1, user, integrity: "低", path: systemApps + "MicrosoftWindows.Client.CBS_cw5n1h2txyewy\\SearchHost.exe" }),
  p(7104, 1196, "开始", "StartMenuExperienceHost.exe", "windows", "app", 61.5, { user, integrity: "低", path: systemApps + "Microsoft.Windows.StartMenuExperienceHost_cw5n1h2txyewy\\StartMenuExperienceHost.exe" }),
  p(7740, 1196, "Windows Shell Experience 主机", "ShellExperienceHost.exe", "windows", "app", 44.8, { user, integrity: "低", path: systemApps + "ShellExperienceHost_cw5n1h2txyewy\\ShellExperienceHost.exe" }),
  p(8140, 1196, "Windows 输入体验", "TextInputHost.exe", "windows", "app", 38.2, { user, integrity: "低", path: systemApps + "MicrosoftWindows.Client.CBS_cw5n1h2txyewy\\TextInputHost.exe" }),
  p(7320, 1196, "运行时代理", "RuntimeBroker.exe", "windows", "app", 8.4, { user }),
  p(7564, 1196, "运行时代理", "RuntimeBroker.exe", "windows", "app", 5.1, { user }),
  p(9240, 1196, "运行时代理", "RuntimeBroker.exe", "windows", "app", 12.3, { user }),
  p(10560, 1196, "运行时代理", "RuntimeBroker.exe", "windows", "app", 3.9, { user }),
  p(11160, 1196, "运行时代理", "RuntimeBroker.exe", "windows", "app", 6.8, { user }),
  p(8460, 1196, "COM Surrogate", "dllhost.exe", "windows", "app", 6.7, { user }),
  p(5960, 1196, "COM Surrogate", "dllhost.exe", "windows", "app", 3.2),
  p(11120, 1196, "COM Surrogate", "dllhost.exe", "windows", "app", 4.4, { user }),
  p(11200, 1196, "Background Task Host", "backgroundTaskHost.exe", "windows", "app", 9.6, { user, integrity: "低" }),
  p(9580, 1196, "Application Frame Host", "ApplicationFrameHost.exe", "windows", "app", 21.4, { user }),
  p(9020, 1196, "MoUSO Core Worker Process", "MoUsoCoreWorker.exe", "windows", "app", 14.8, { disk: 0.2 }),
  p(9460, 1196, "Windows Defender SmartScreen", "smartscreen.exe", "windows", "shield", 9.6, { user }),
  p(10120, 1196, "User OOBE Broker", "UserOOBEBroker.exe", "windows", "app", 2.6, { user, path: win + "oobe\\UserOOBEBroker.exe" }),
  p(10640, 1196, "Component Package Support Server", "CompPkgSrv.exe", "windows", "app", 3.4, { user }),
  p(17360, 1196, "System Settings Broker", "SystemSettingsBroker.exe", "windows", "gear", 7.3, { user }),
  p(11980, 2896, "Windows 音频设备图形隔离", "audiodg.exe", "windows", "app", 12.3, { cpu: 0.3, user: local }),
];

export const groupLabel: Record<Group, string> = {
  app: "应用",
  background: "后台进程",
  windows: "Windows 进程",
};

// Hiding system processes (the app's visibility.rs): Windows components leave
// the lists unless their program is kept, and so do the programs named to
// hide. Names are exact executable names, compared without case.
export interface VisibilityRules {
  keep: string[];
  hide: string[];
}
export const defaultVisibility: VisibilityRules = {
  keep: ["cmd.exe", "conhost.exe", "explorer.exe", "openconsole.exe", "powershell.exe", "pwsh.exe"],
  hide: [],
};
export function hidesProcess(rules: VisibilityRules, p: Process) {
  const name = p.exe.toLowerCase();
  return !rules.keep.includes(name) && (p.group === "windows" || rules.hide.includes(name));
}
/** The names of a list as typed, or null where one is a path or a pattern. */
export function programNames(value: string): string[] | null {
  const names = new Set<string>();
  for (const name of value.split(/[,;\n\r，；]/).map((s) => s.trim()).filter(Boolean)) {
    // eslint-disable-next-line no-control-regex
    if (name.length > 260 || /[\u0000-\u001f\\/:*?"<>|]/.test(name)) return null;
    names.add(name.toLowerCase());
    if (names.size > 128) return null;
  }
  return [...names].sort();
}

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
// Memory as the app's process columns and details show it (theme::memory):
// megabytes with one decimal under a gigabyte, never KB.
export function memory(value: number) {
  const gb = value / (1024 * MB);
  if (gb >= 10) return gb.toFixed(1) + " GB";
  if (gb >= 1) return gb.toFixed(2) + " GB";
  return (value / MB).toFixed(1) + " MB";
}
// Bytes moved or reserved (theme::bytes, not a rate): whole B and KB, one decimal from MB up.
export function transferred(value: number) {
  if (value === 0) return "0";
  const names = ["B", "KB", "MB", "GB", "TB"];
  let amount = value,
    unit = 0;
  while (amount >= 1024 && unit < 4) {
    amount /= 1024;
    unit++;
  }
  return (unit <= 1 ? Math.round(amount).toString() : amount.toFixed(1)) + " " + names[unit];
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
// The sensor readings every page shares (vitals, Performance, History, the
// monitors): the package's temperature as the kernel sensors read it, 50–68 °C
// from idle to load, and its power; the main GPU's; the battery, slowly
// draining at about 16.5 W.
export const cpuTemperature = (t: number) => 50 + sampleAt(t).cpu * 0.16 + noise(31, t) * 2;
export const cpuPower = (t: number) => 28 + sampleAt(t).cpu * 0.82 + noise(32, t) * 4;
export const gpuTemperature = (t: number) => 50 + noise(7, t) * 4;
export const gpuPower = (t: number) => 18 + sampleAt(t).gpu * 0.2;
export const batteryLevel = (t: number) => 84 - (((t % 600) + 600) % 600) / 300;
export const batteryDischarge = (t: number) => 16.5 + noise(71, Math.floor(t / 3)) * 1.4;

export interface Service {
  display: string;
  name: string;
  pid?: number;
  start: "auto" | "autoDelayed" | "manual" | "disabled";
  description: string;
  /** The image path, where it is not a service host in the Windows folder. */
  path?: string;
}
export const servicePath = (s: Service) => s.path ?? "C:\\WINDOWS\\system32\\svchost.exe -k netsvcs -p";
// Windows' own services, as a Windows 11 laptop lists them: those running name
// their service host or program above; a share are stopped, manual or disabled.
export const services: Service[] = [
  { display: "Argus 特权服务", name: "ArgusPrivileged", pid: 5128, start: "auto", description: "为授权的本机账户提供 Argus 进程和服务管理操作。", path: '"C:\\Program Files\\Argus Privileged\\argus-service-4812-1762127411.exe" --argus-service' },
  { display: "Background Intelligent Transfer Service", name: "BITS", start: "manual", description: "利用空闲网络带宽在后台传输文件。" },
  { display: "Base Filtering Engine", name: "BFE", pid: 2208, start: "auto", description: "管理防火墙和 Internet 协议安全（IPsec）策略，并实施用户模式筛选。", path: "C:\\WINDOWS\\system32\\svchost.exe -k LocalServiceNoNetworkFirewall -p" },
  { display: "Bluetooth Support Service", name: "bthserv", pid: 3020, start: "manual", description: "支持发现和关联远程蓝牙设备。", path: "C:\\WINDOWS\\system32\\svchost.exe -k LocalService -p" },
  { display: "Capability Access Manager Service", name: "camsvc", pid: 4980, start: "manual", description: "管理应用对摄像头、麦克风和位置等功能的访问。", path: "C:\\WINDOWS\\system32\\svchost.exe -k appmodel -p" },
  { display: "Connected Devices Platform Service", name: "CDPSvc", pid: 3828, start: "autoDelayed", description: "此服务用于连接的设备平台方案。", path: "C:\\WINDOWS\\system32\\svchost.exe -k LocalService -p" },
  { display: "Connected Devices Platform User Service_5f2a1", name: "CDPUserSvc_5f2a1", pid: 5640, start: "auto", description: "此用户服务用于连接的设备平台方案。", path: "C:\\WINDOWS\\system32\\svchost.exe -k UnistackSvcGroup" },
  { display: "Cryptographic Services", name: "CryptSvc", pid: 2420, start: "auto", description: "确认 Windows 文件的签名，管理受信任的根证书并自动更新根证书。", path: "C:\\WINDOWS\\system32\\svchost.exe -k NetworkService -p" },
  { display: "DCOM Server Process Launcher", name: "DcomLaunch", pid: 1196, start: "auto", description: "启动 COM 和 DCOM 服务器以响应对象激活请求。", path: "C:\\WINDOWS\\system32\\svchost.exe -k DcomLaunch -p" },
  { display: "Delivery Optimization", name: "DoSvc", pid: 6740, start: "autoDelayed", description: "执行内容传递优化任务。", path: "C:\\WINDOWS\\System32\\svchost.exe -k NetworkService -p" },
  { display: "DHCP Client", name: "Dhcp", pid: 2072, start: "auto", description: "为此计算机注册并更新 IP 地址和 DNS 记录。", path: "C:\\WINDOWS\\system32\\svchost.exe -k LocalServiceNetworkRestricted -p" },
  { display: "Diagnostic Policy Service", name: "DPS", pid: 2252, start: "auto", description: "为 Windows 组件提供问题检测、疑难解答和解决方案。", path: "C:\\WINDOWS\\System32\\svchost.exe -k LocalServiceNoNetwork -p" },
  { display: "DNS Client", name: "Dnscache", pid: 1784, start: "auto", description: "缓存 DNS 查询结果，并注册此计算机的完整名称。", path: "C:\\WINDOWS\\system32\\svchost.exe -k NetworkService -p" },
  { display: "Downloaded Maps Manager", name: "MapsBroker", start: "autoDelayed", description: "供应用程序访问已下载地图的 Windows 服务。", path: "C:\\WINDOWS\\System32\\svchost.exe -k NetworkService -p" },
  { display: "Group Policy Client", name: "gpsvc", pid: 1512, start: "auto", description: "应用管理员通过组策略为计算机和用户配置的设置。" },
  { display: "Hyper-V Host Compute Service", name: "vmcompute", pid: 8248, start: "manual", description: "为运行容器和虚拟机提供支持。", path: "C:\\WINDOWS\\system32\\vmcompute.exe" },
  { display: "Intel(R) Content Protection HDCP Service", name: "cplspcon", pid: 4392, start: "auto", description: "Intel(R) Content Protection HDCP Service - enables communication with Content Protection HDCP HW", path: "C:\\WINDOWS\\System32\\DriverStore\\FileRepository\\iigd_dch.inf_amd64_6b2e9d4f1a7c3e58\\IntelCpHDCPSvc.exe" },
  { display: "Intel(R) Dynamic Application Loader Host Interface Service", name: "jhi_service", pid: 4128, start: "autoDelayed", description: "Intel(R) Dynamic Application Loader Host Interface Service - Allows applications to access the local Intel (R) DAL", path: "C:\\WINDOWS\\System32\\DriverStore\\FileRepository\\dal.inf_amd64_3c9e1f7a2b4d6e85\\jhi_service.exe" },
  { display: "Intel(R) Graphics Command Center Service", name: "igccservice", pid: 4040, start: "auto", description: "Service for Intel(R) Graphics Command Center", path: '"C:\\Program Files\\WindowsApps\\AppUp.IntelGraphicsExperience_1.100.5536.0_x64__8j3eq9eme6ctt\\Service\\OneApp.IGCC.WinService.exe"' },
  { display: "Intel(R) Management Engine WMI Provider Registration", name: "WMIRegistrationService", pid: 4480, start: "auto", description: "Intel(R) Management Engine WMI Provider Registration", path: "C:\\WINDOWS\\System32\\DriverStore\\FileRepository\\mewmiprov.inf_amd64_4f7a1c3e9b2d6a05\\WMIRegistrationService.exe" },
  { display: "IP Helper", name: "iphlpsvc", pid: 3600, start: "auto", description: "使用 IPv6 转换技术和 IP-HTTPS 提供隧道连接。" },
  { display: "Local Session Manager", name: "LSM", pid: 1340, start: "auto", description: "管理本地用户会话的核心 Windows 服务。", path: "C:\\WINDOWS\\system32\\svchost.exe -k DcomLaunch -p" },
  { display: "Microsoft Defender Antivirus Network Inspection Service", name: "WdNisSvc", pid: 4512, start: "manual", description: "帮助防止针对网络协议中已知漏洞和新发现漏洞的入侵企图。", path: '"C:\\ProgramData\\Microsoft\\Windows Defender\\Platform\\4.18.24090.11-0\\NisSrv.exe"' },
  { display: "Microsoft Defender Antivirus Service", name: "WinDefend", pid: 3480, start: "auto", description: "帮助用户防止恶意软件及其他潜在的垃圾软件。", path: '"C:\\ProgramData\\Microsoft\\Windows Defender\\Platform\\4.18.24090.11-0\\MsMpEng.exe"' },
  { display: "Microsoft Defender Core Service", name: "MDCoreSvc", pid: 3448, start: "auto", description: "为 Microsoft Defender 防病毒提供核心保护功能。", path: '"C:\\ProgramData\\Microsoft\\Windows Defender\\Platform\\4.18.24090.11-0\\MpDefenderCoreService.exe"' },
  { display: "Netlogon", name: "Netlogon", start: "manual", description: "为用户和服务身份验证维护安全通道。", path: "C:\\WINDOWS\\system32\\lsass.exe" },
  { display: "Network Location Awareness", name: "NlaSvc", pid: 2120, start: "auto", description: "收集并存储网络的配置信息，并在信息改变时通知程序。", path: "C:\\WINDOWS\\System32\\svchost.exe -k NetworkService -p" },
  { display: "Network Store Interface Service", name: "nsi", pid: 2164, start: "auto", description: "向用户模式客户端传递网络通知。", path: "C:\\WINDOWS\\system32\\svchost.exe -k LocalService -p" },
  { display: "NVIDIA Display Container LS", name: "NVDisplay.ContainerLocalSystem", pid: 2740, start: "auto", description: "Container service for NVIDIA root features", path: '"C:\\WINDOWS\\System32\\DriverStore\\FileRepository\\nv_dispig.inf_amd64_5d3f0a2c1b7e9a41\\Display.NvContainer\\NVDisplay.Container.exe" -s NVDisplay.ContainerLocalSystem' },
  { display: "Plug and Play", name: "PlugPlay", pid: 1196, start: "manual", description: "使计算机在极少或没有用户输入的情况下识别并适应硬件更改。", path: "C:\\WINDOWS\\system32\\svchost.exe -k DcomLaunch -p" },
  { display: "Power", name: "Power", pid: 1196, start: "auto", description: "管理电源策略和电源策略通知的传递。", path: "C:\\WINDOWS\\system32\\svchost.exe -k DcomLaunch -p" },
  { display: "Print Spooler", name: "Spooler", pid: 2988, start: "auto", description: "将打印作业排入队列并处理与打印机的交互。", path: "C:\\WINDOWS\\System32\\spoolsv.exe" },
  { display: "Realtek Audio Universal Service", name: "RtkAudioUniversalService", pid: 4084, start: "auto", description: "Realtek Audio Universal Service", path: '"C:\\WINDOWS\\System32\\DriverStore\\FileRepository\\realtekservice.inf_amd64_8a2f4c1e6d3b5a70\\RtkAudUService64.exe"' },
  { display: "Remote Desktop Services", name: "TermService", start: "manual", description: "允许用户以交互方式连接到远程计算机。", path: "C:\\WINDOWS\\System32\\svchost.exe -k NetworkService" },
  { display: "Remote Procedure Call (RPC)", name: "RpcSs", pid: 1296, start: "auto", description: "COM 和 DCOM 服务器的服务控制管理器。", path: "C:\\WINDOWS\\system32\\svchost.exe -k rpcss -p" },
  { display: "Remote Registry", name: "RemoteRegistry", start: "disabled", description: "使远程用户能修改此计算机上的注册表设置。", path: "C:\\WINDOWS\\system32\\svchost.exe -k localService -p" },
  { display: "Security Center", name: "wscsvc", pid: 3720, start: "autoDelayed", description: "监视并报告计算机上的安全运行状况设置。", path: "C:\\WINDOWS\\System32\\svchost.exe -k LocalServiceNetworkRestricted -p" },
  { display: "Server", name: "LanmanServer", pid: 3512, start: "auto", description: "支持此计算机通过网络共享文件、打印机和命名管道。" },
  { display: "SSDP Discovery", name: "SSDPSRV", pid: 4344, start: "manual", description: "发现使用 SSDP 协议的网络设备和服务，如 UPnP 设备。", path: "C:\\WINDOWS\\system32\\svchost.exe -k LocalServiceAndNoImpersonation -p" },
  { display: "State Repository Service", name: "StateRepository", pid: 3104, start: "manual", description: "为应用程序模型提供所需的基础结构支持。", path: "C:\\WINDOWS\\system32\\svchost.exe -k appmodel -p" },
  { display: "SysMain", name: "SysMain", pid: 3376, start: "auto", description: "维护和提高一段时间内的系统性能。", path: "C:\\WINDOWS\\system32\\svchost.exe -k LocalSystemNetworkRestricted -p" },
  { display: "Task Scheduler", name: "Schedule", pid: 1560, start: "auto", description: "按计划自动执行任务。" },
  { display: "Update Orchestrator Service", name: "UsoSvc", pid: 3236, start: "autoDelayed", description: "管理 Windows 更新。如果已停止，设备将无法下载和安装最新更新。" },
  { display: "User Manager", name: "UserManager", pid: 1744, start: "auto", description: "提供多用户交互所需的运行时组件。" },
  { display: "User Profile Service", name: "ProfSvc", pid: 1652, start: "auto", description: "负责加载和卸载用户配置文件。" },
  { display: "Web Account Manager", name: "TokenBroker", pid: 5180, start: "manual", description: "Web 帐户管理器利用此服务实现单一登录到应用和服务。" },
  { display: "Windows Audio", name: "AudioSrv", pid: 2896, start: "auto", description: "管理基于 Windows 的程序的音频。", path: "C:\\WINDOWS\\System32\\svchost.exe -k LocalServiceNetworkRestricted -p" },
  { display: "Windows Audio Endpoint Builder", name: "AudioEndpointBuilder", pid: 2548, start: "auto", description: "管理 Windows 音频服务的音频设备。", path: "C:\\WINDOWS\\System32\\svchost.exe -k LocalSystemNetworkRestricted -p" },
  { display: "Windows Connection Manager", name: "Wcmsvc", pid: 2636, start: "auto", description: "根据电脑当前可用的网络连接选项自动连接或断开。", path: "C:\\WINDOWS\\system32\\svchost.exe -k LocalServiceNetworkRestricted -p" },
  { display: "Windows Defender Firewall", name: "mpssvc", pid: 2208, start: "auto", description: "阻止未经授权的用户通过 Internet 或网络访问此计算机。", path: "C:\\WINDOWS\\system32\\svchost.exe -k LocalServiceNoNetworkFirewall -p" },
  { display: "Windows Error Reporting Service", name: "WerSvc", start: "manual", description: "在程序停止运行或停止响应时报告错误，并提供现有的解决方案。", path: "C:\\WINDOWS\\System32\\svchost.exe -k WerSvcGroup" },
  { display: "Windows Event Log", name: "EventLog", pid: 2336, start: "auto", description: "管理事件和事件日志。", path: "C:\\WINDOWS\\System32\\svchost.exe -k LocalServiceNetworkRestricted -p" },
  { display: "Windows Management Instrumentation", name: "Winmgmt", pid: 2604, start: "auto", description: "提供访问操作系统、设备、应用程序和服务管理信息的通用接口和对象模型。" },
  { display: "Windows Push Notifications System Service", name: "WpnService", pid: 3864, start: "auto", description: "在会话 0 中运行，托管通知平台和连接提供程序。" },
  { display: "Windows Search", name: "WSearch", pid: 3280, start: "autoDelayed", description: "为文件、电子邮件和其他内容提供索引。", path: "C:\\WINDOWS\\system32\\SearchIndexer.exe /Embedding" },
  { display: "Windows Security Service", name: "SecurityHealthService", pid: 4560, start: "manual", description: "处理统一的设备保护和运行状况信息。", path: "C:\\WINDOWS\\system32\\SecurityHealthService.exe" },
  { display: "Windows Time", name: "W32Time", start: "manual", description: "维护网络上所有客户端和服务器的日期和时间同步。", path: "C:\\WINDOWS\\system32\\svchost.exe -k LocalService" },
  { display: "Windows Update", name: "wuauserv", pid: 3192, start: "manual", description: "启用检测、下载和安装 Windows 更新。" },
  { display: "WLAN AutoConfig", name: "WlanSvc", pid: 2680, start: "auto", description: "配置、发现、连接和断开无线局域网（WLAN）。", path: "C:\\WINDOWS\\system32\\svchost.exe -k LocalSystemNetworkRestricted -p" },
  { display: "Workstation", name: "LanmanWorkstation", pid: 2464, start: "auto", description: "使用 SMB 协议创建并维护到远程服务器的客户端网络连接。", path: "C:\\WINDOWS\\System32\\svchost.exe -k NetworkService -p" },
  { display: "WSL Service", name: "WslService", pid: 8204, start: "auto", description: "WSL Service", path: '"C:\\Program Files\\WSL\\wslservice.exe"' },
  { display: "Xbox Live 身份验证管理器", name: "XblAuthManager", start: "manual", description: "提供身份验证和授权服务。" },
];
// A service host is named after the services it runs, as the app titles it
// (model.rs `service_title`): one or two by name, more summed up.
for (const proc of processes) {
  if (proc.exe !== "svchost.exe") continue;
  const names = services.filter((s) => s.pid === proc.pid).map((s) => s.display);
  proc.name = "服务主机：" + (names.length <= 2 ? names.join("、") : `${names.slice(0, 2).join("、")} 等 ${names.length} 个服务`);
}
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
  /** The registry value or file name, where it is not the name. */
  entry?: string;
  /** Why its state cannot be changed: the app manages it, or it could not be read. */
  blocked?: "managed" | "unavailable";
}
export const startupItems: StartupItem[] = [
  { name: "Argus", publisher: "Argus contributors", enabled: true, location: "注册表（当前用户）", command: '"C:\\Program Files\\Argus\\argus.exe" --hidden', glyph: "argus" },
  { name: "Contoso Updater", publisher: "—", enabled: true, location: "注册表（所有用户，32 位）", command: '"C:\\Program Files (x86)\\Contoso\\updater.exe" /silent', missing: true, glyph: "app" },
  { name: "OneDrive", publisher: "Microsoft Corporation", enabled: true, location: "注册表（当前用户）", command: '"C:\\Users\\demo\\AppData\\Local\\Microsoft\\OneDrive\\OneDrive.exe" /background', glyph: "cloud" },
  { name: "SecurityHealth", publisher: "Microsoft Corporation", enabled: true, location: "注册表（所有用户）", command: "C:\\WINDOWS\\system32\\SecurityHealthSystray.exe", glyph: "shield" },
  { name: "Realtek HD Audio Universal Service", publisher: "Realtek Semiconductor", enabled: true, location: "注册表（所有用户）", command: '"C:\\WINDOWS\\System32\\DriverStore\\FileRepository\\realtekservice.inf_amd64_8a2f4c1e6d3b5a70\\RtkAudUService64.exe" -background', glyph: "app", entry: "RtkAudUService" },
  { name: "Discord", publisher: "Discord Inc.", enabled: true, location: "注册表（当前用户）", command: '"C:\\Users\\demo\\AppData\\Local\\Discord\\Update.exe" --processStart Discord.exe', glyph: "app" },
  { name: "Steam", publisher: "Valve Corporation", enabled: false, location: "注册表（当前用户）", command: '"C:\\Program Files (x86)\\Steam\\steam.exe" -silent', glyph: "app" },
  { name: "Microsoft Edge", publisher: "Microsoft Corporation", enabled: false, location: "注册表（当前用户）", command: '"C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe" --no-startup-window --win-session-start', glyph: "browser", entry: "MicrosoftEdgeAutoLaunch_6A1C8E2F0B4D93A57E21C0F9D8B3A645" },
  { name: "PowerToys", publisher: "Microsoft Corporation", enabled: true, location: "启动文件夹（当前用户）", command: '"C:\\Program Files\\PowerToys\\PowerToys.exe"', glyph: "app", entry: "PowerToys.lnk" },
  { name: "Send to OneNote", publisher: "Microsoft Corporation", enabled: false, location: "启动文件夹（所有用户）", command: '"C:\\Program Files\\Microsoft Office\\root\\Office16\\ONENOTEM.EXE" /tsr', glyph: "note", entry: "Send to OneNote.lnk" },
  { name: "Microsoft Teams", publisher: "Microsoft Corporation", enabled: true, location: "应用包（当前用户）", command: '"C:\\Program Files\\WindowsApps\\MSTeams_25290.205.4069.4894_x64__8wekyb3d8bbwe\\ms-teams.exe"', glyph: "app", entry: "TeamsTfwStartupTask" },
  { name: "Contoso Sync", publisher: "Contoso Ltd.", enabled: false, location: "应用包（当前用户）", command: '"C:\\Program Files\\WindowsApps\\Contoso.Sync_2.4.0.0_x64__c0nt0s0syncq2\\ContosoSync.exe"', glyph: "app", entry: "SyncStartupTask", blocked: "unavailable" },
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
// The laptop's sockets: Windows' listeners (RPC, SMB, the RPC dynamic ports,
// Delivery Optimization), name resolution and discovery over UDP, the apps'
// HTTPS and QUIC traffic, and a dev server on the loopback. Addresses come
// from the documentation ranges (RFC 5737, RFC 3849).
const lanAddress = "192.0.2.24";
const lanAddress6 = "2001:db8::24";
const tcpSocket = (pid: number, local: string, localPort: number, state: Connection["state"], remote?: string, remotePort?: number): Connection => ({ pid, protocol: "TCP", local, localPort, remote, remotePort, state });
const udpSocket = (pid: number, local: string, localPort: number): Connection => ({ pid, protocol: "UDP", local, localPort });
export const connections: Connection[] = [
  // Listening
  tcpSocket(1296, "0.0.0.0", 135, "listen"),
  tcpSocket(1296, "::", 135, "listen"),
  tcpSocket(4, lanAddress, 139, "listen"),
  tcpSocket(4, "0.0.0.0", 445, "listen"),
  tcpSocket(4, "::", 445, "listen"),
  tcpSocket(3828, "0.0.0.0", 5040, "listen"),
  tcpSocket(6740, "0.0.0.0", 7680, "listen"),
  tcpSocket(6740, "::", 7680, "listen"),
  tcpSocket(1088, "0.0.0.0", 49664, "listen"),
  tcpSocket(1088, "::", 49664, "listen"),
  tcpSocket(904, "0.0.0.0", 49665, "listen"),
  tcpSocket(904, "::", 49665, "listen"),
  tcpSocket(2336, "0.0.0.0", 49666, "listen"),
  tcpSocket(2336, "::", 49666, "listen"),
  tcpSocket(1560, "0.0.0.0", 49667, "listen"),
  tcpSocket(2988, "0.0.0.0", 49668, "listen"),
  tcpSocket(1068, "0.0.0.0", 49669, "listen"),
  tcpSocket(1068, "::", 49669, "listen"),
  tcpSocket(13729, "127.0.0.1", 5173, "listen"),
  tcpSocket(15600, "127.0.0.1", 27060, "listen"),
  tcpSocket(15600, "0.0.0.0", 27036, "listen"),
  // The browser and the apps
  tcpSocket(9940, lanAddress, 52144, "established", "203.0.113.18", 443),
  tcpSocket(9940, lanAddress, 52151, "established", "198.51.100.7", 443),
  tcpSocket(14520, lanAddress, 52166, "established", "203.0.113.52", 443),
  tcpSocket(14520, lanAddress, 52190, "established", "198.51.100.84", 443),
  tcpSocket(9940, lanAddress6, 52610, "established", "2001:db8:85a3::8a2e", 443),
  tcpSocket(9940, lanAddress6, 52618, "established", "2001:db8:4f2::1d", 443),
  tcpSocket(9940, "127.0.0.1", 52388, "established", "127.0.0.1", 5173),
  tcpSocket(13729, "127.0.0.1", 5173, "established", "127.0.0.1", 52388),
  tcpSocket(16920, lanAddress, 52240, "established", "203.0.113.120", 443),
  tcpSocket(16920, lanAddress, 52244, "established", "198.51.100.35", 443),
  tcpSocket(16800, lanAddress, 52236, "established", "203.0.113.121", 443),
  tcpSocket(16280, lanAddress, 52312, "established", "198.51.100.140", 443),
  tcpSocket(16280, lanAddress, 52318, "established", "198.51.100.141", 443),
  tcpSocket(15600, lanAddress, 52402, "established", "203.0.113.200", 27030),
  tcpSocket(15720, lanAddress, 52410, "established", "203.0.113.201", 443),
  tcpSocket(8920, lanAddress, 52210, "established", "203.0.113.40", 443),
  tcpSocket(3480, lanAddress, 52288, "established", "198.51.100.64", 443),
  tcpSocket(3864, lanAddress, 52101, "established", "203.0.113.9", 443),
  tcpSocket(9620, lanAddress, 52470, "established", "198.51.100.98", 443),
  tcpSocket(6740, lanAddress, 52520, "established", "203.0.113.77", 443),
  tcpSocket(3192, lanAddress, 52633, "closeWait", "203.0.113.77", 443),
  tcpSocket(9420, lanAddress, 52502, "timeWait", "198.51.100.23", 443),
  tcpSocket(9940, lanAddress, 52128, "timeWait", "203.0.113.18", 443),
  tcpSocket(16280, lanAddress, 52300, "closeWait", "198.51.100.140", 443),
  // UDP: name resolution, discovery, DHCP, NetBIOS and the browser's QUIC
  udpSocket(4, lanAddress, 137),
  udpSocket(4, lanAddress, 138),
  udpSocket(2072, lanAddress, 68),
  udpSocket(1784, "0.0.0.0", 5353),
  udpSocket(1784, "::", 5353),
  udpSocket(1784, "0.0.0.0", 5355),
  udpSocket(1784, "::", 5355),
  udpSocket(5024, "0.0.0.0", 3702),
  udpSocket(5024, "::", 3702),
  udpSocket(4344, "127.0.0.1", 1900),
  udpSocket(4344, lanAddress, 1900),
  udpSocket(9940, "0.0.0.0", 5353),
  udpSocket(9940, "0.0.0.0", 61344),
  udpSocket(9940, "::", 61345),
  udpSocket(16280, "0.0.0.0", 50007),
];

export const machine = {
  cpu: "Intel Core i5-13500HX",
  gpu: "NVIDIA GeForce RTX 4060 Laptop GPU",
  disk: "Samsung SSD 990 PRO 2TB",
  wifi: "Intel(R) Wi-Fi 6E AX211 160MHz",
};

// The machine's disks, network adapters and GPUs, each kind in the order the
// app lists it (hardware.disks, nics, gpus). `id` is the device's id on the
// performance page (Processes.tsx `Device`); `color` its curves' token.
const GiB = 1024 ** 3;
export const disks = [
  { id: "disk", index: 0, name: "磁盘 0 (C:)", model: machine.disk, volumes: ["C:"], ssd: true, bus: "NVMe", kind: "SSD (NVMe)", size: 2_000_398_934_016, rpm: null, color: "var(--a-io)" },
  { id: "disk1", index: 1, name: "磁盘 1 (D:)", model: "ST2000LM015-2E8174", volumes: ["D:"], ssd: false, bus: "SATA", kind: "HDD (SATA)", size: 2_000_398_934_016, rpm: 5400, color: "var(--a-io)" },
] as const;
export const adapters = [
  { id: "wifi", index: 0, name: "WLAN", model: machine.wifi, kind: "wifi", link: 1_201_000_000, color: "var(--a-net)" },
  { id: "wsl", index: 1, name: "vEthernet (WSL)", model: "Hyper-V Virtual Ethernet Adapter", kind: "virtual", link: 10_000_000_000, color: "var(--a-net)" },
] as const;
export const gpus = [
  {
    id: "gpu", index: 0, name: "GPU 0", model: machine.gpu, integrated: false, dedicated: 8 * GiB, shared: 16 * GiB,
    engines: ["3D", "Copy", "Video Encode", "Video Decode", "Compute_0", "Security"], color: "var(--a-gpu)",
  },
  {
    // The processor's integrated GPU: it uses the system's memory, none of its own.
    id: "gpu1", index: 1, name: "GPU 1", model: "Intel(R) UHD Graphics", integrated: true, dedicated: 0, shared: 16 * GiB,
    engines: ["3D", "Copy", "Video Decode", "Video Processing", "Video Encode"], color: "var(--a-gpu)",
  },
] as const;
export type DiskId = (typeof disks)[number]["id"];
export type AdapterId = (typeof adapters)[number]["id"];
export type GpuId = (typeof gpus)[number]["id"];

// The devices' readings are drawn from the tick alone and read by many charts
// at every sample, each over the last minute: the recent ones are kept, as
// the samples are (sampleAt).
function kept<T>(store: Map<number, T>, key: number, draw: () => T) {
  let value = store.get(key);
  if (value === undefined) {
    value = draw();
    store.set(key, value);
    if (store.size > 1024) store.delete(store.keys().next().value as number);
  }
  return value;
}
const keptTotals = new Map<number, ReturnType<typeof drawTotals>>();
const keptDisks = new Map<number, ReturnType<typeof drawDisk>>();
const keptAdapters = new Map<number, ReturnType<typeof drawAdapter>>();
const keptGpus = new Map<number, ReturnType<typeof drawGpu>>();
/** The whole machine at a sample (vitals.rs, tray.rs): every disk's and adapter's rates summed,
 * the busiest disk, and the busiest GPU (the first while equally busy). */
export function machineTotals(t: number) {
  return kept(keptTotals, t, () => drawTotals(t));
}
/** A disk's readings at a sample: rates in bytes/s, active time (%), response (ms), temperature (°C). */
export function diskAt(index: number, tick: number) {
  return kept(keptDisks, tick * 8 + index, () => drawDisk(index, tick));
}
/** An adapter's receive and send rates at a sample (bytes/s). */
export function adapterAt(index: number, tick: number) {
  return kept(keptAdapters, tick * 8 + index, () => drawAdapter(index, tick));
}
/** A GPU's readings at a sample: utilization and each engine's (%, in `gpus[index].engines` order),
 * memory used (bytes), temperature (°C), power (W) and core clock (MHz). */
export function gpuAt(index: number, tick: number) {
  return kept(keptGpus, tick * 8 + index, () => drawGpu(index, tick));
}
function drawTotals(t: number) {
  const diskReadings = disks.map((d) => ({ id: d.id, ...diskAt(d.index, t) }));
  const adapterReadings = adapters.map((a) => adapterAt(a.index, t));
  const gpuReadings = gpus.map((g) => ({ id: g.id, ...gpuAt(g.index, t) }));
  const rx = adapterReadings.reduce((sum, r) => sum + r.rx, 0);
  const tx = adapterReadings.reduce((sum, r) => sum + r.tx, 0);
  const busiestGpu = gpuReadings.reduce((a, b) => (b.util > a.util ? b : a));
  return {
    disk: diskReadings.reduce((sum, r) => sum + r.read + r.write, 0),
    busiestDisk: diskReadings.reduce((a, b) => (b.active > a.active ? b : a)),
    rx,
    tx,
    net: rx + tx,
    gpu: busiestGpu.util,
    busiestGpu,
  };
}
/** The GPU engine the busiest of `pids` on the GPU runs on, as the app's GPU engine column names it:
 * the desktop's composition and a video in the browser on the integrated GPU, the rest on the NVIDIA one. */
export function gpuEngine(pids: number[]) {
  const busiest = processes.filter((p) => p.gpu && pids.includes(p.pid)).sort((a, b) => (b.gpu ?? 0) - (a.gpu ?? 0))[0];
  if (busiest?.exe === "dwm.exe") return "GPU 1 - 3D";
  if (busiest?.exe === "msedge.exe" && busiest.args?.includes("gpu-process")) return "GPU 1 - Video Decode";
  return "GPU 0 - 3D";
}
function drawDisk(index: number, tick: number) {
  if (index === 0) {
    const s = sampleAt(tick);
    return { read: s.read, write: s.write, active: s.active, response: 0.2 + noise(51, tick) * 0.4, temperature: 41 };
  }
  // The laptop hard disk: idle mostly, now and then a burst of reads.
  const trickle = noise(251, tick) < 0.12 ? 16 + noise(252, tick) * 240 : 0;
  const read = (burst(250, tick, 14, 0.3) * 7200 + trickle) * 1024;
  const write = noise(253, tick) < 0.04 ? (64 + noise(254, tick) * 320) * 1024 : 0;
  const busy = read + write;
  return {
    read,
    write,
    active: busy ? Math.min(100, 0.4 + (busy / 1024 ** 2) * 6.5) : 0,
    response: busy ? 6 + noise(255, tick) * 9 : 0,
    temperature: 38,
  };
}
function drawAdapter(index: number, tick: number) {
  if (index === 0) {
    const s = sampleAt(tick);
    return { rx: s.rx, tx: s.tx };
  }
  // WSL's virtual switch: a trickle, now and then a little more.
  return {
    rx: (0.4 + load(270, tick) * 2.6 + burst(272, tick, 25, 0.3) * 36) * 1024,
    tx: (0.2 + load(274, tick) * 1.4 + burst(276, tick, 25, 0.3) * 9) * 1024,
  };
}
function drawGpu(index: number, tick: number) {
  if (index === 0) {
    const util = sampleAt(tick).gpu;
    return {
      util,
      engines: [
        util,
        noise(61, tick) < 0.8 ? 0 : noise(66, tick) * 0.8,
        12 + noise(62, Math.floor(tick / 3)) * 14,
        0,
        util * 0.22,
        0,
      ],
      dedicated: (3.3 + util / 900 + noise(41, tick) * 0.04) * GiB,
      shared: (0.3 + noise(42, Math.floor(tick / 5)) * 0.03) * GiB,
      temperature: 50 + noise(7, tick) * 4,
      power: 18 + util * 0.2,
      clock: 1650 + util * 4 + noise(43, tick) * 30,
    };
  }
  // The integrated GPU: the desktop's composition and a video playing.
  const desktop = 3 + load(280, tick) * 6 + burst(281, tick, 12, 0.4) * 9;
  const decode = 9 + load(282, tick) * 6;
  const engines = [desktop, noise(283, tick) < 0.7 ? 0 : 0.4 + noise(284, tick) * 1.6, decode, 3 + load(285, tick) * 3, 0];
  return {
    util: Math.max(...engines),
    engines,
    dedicated: 0,
    shared: (0.62 + load(286, tick) * 0.12) * GiB,
    temperature: 54 + load(287, tick) * 7,
    power: 0.5 + desktop * 0.06 + decode * 0.03,
    clock: Math.round((300 + desktop * 45) / 50) * 50,
  };
}
