// Mirrors the serde types in argus-core. Keep in sync.

import type { Language, Locale } from "./i18n";

export interface ProcessRow {
  pid: number;
  ppid: number;
  /** Unix ms; with `pid`, identifies a process instance. */
  createTime: number;
  name: string;
  path: string | null;
  description: string | null;
  company: string | null;
  icon: number | null;
  /** Percent of total machine capacity. */
  cpu: number;
  /** Total CPU time, ms. */
  cpuTime: number;
  privateWs: number;
  workingSet: number;
  commit: number;
  ioRead: number;
  ioWrite: number;
  ioOther: number;
  /** The process's share of its busiest GPU engine, percent. */
  gpu: number;
  /** That engine, e.g. `GPU 0 - 3D`; null while the GPU is idle for it. */
  gpuEngine: string | null;
  /** GPU memory in use, bytes. */
  gpuDedicated: number;
  gpuShared: number;
  /** Network bytes per second; 0 unless `Snapshot.processNetwork`. */
  netSent: number;
  netReceived: number;
  threads: number;
  handles: number;
  sessionId: number;
  basePriority: number;
  suspended: boolean;
  /** Efficiency mode (EcoQoS) is on. */
  efficiency: boolean;
  /** Owns a taskbar window, which puts it under "Apps". */
  appWindow: boolean;
}

export interface SystemStats {
  cpu: number;
  /** Portion of `cpu` spent in kernel mode. */
  cpuKernel: number;
  cores: number[];
  logicalCpus: number;
  memTotal: number;
  memUsed: number;
  memAvailable: number;
  commitUsed: number;
  commitLimit: number;
  cached: number;
  kernelPaged: number;
  kernelNonpaged: number;
  processCount: number;
  threadCount: number;
  handleCount: number;
  uptimeMs: number;
  ioRead: number;
  ioWrite: number;
  ioOther: number;
}

export interface CpuInfo {
  name: string;
  baseMhz: number;
  sockets: number;
  cores: number;
  /** Hybrid CPUs only (0 otherwise). */
  performanceCores: number;
  efficiencyCores: number;
  logical: number;
  l1: number;
  l2: number;
  l3: number;
  virtualization: boolean;
  /** One per logical processor, indexed like `SystemStats.cores`. */
  threads: CpuThread[];
}

export interface CpuThread {
  /** Physical core (hyper-threads share one). */
  core: number;
  /** An efficiency core of a hybrid CPU. */
  efficient: boolean;
}

export interface ThermalZone {
  name: string;
  celsius: number;
}

/** RAPL power domains in watts; null where not metered. */
export interface CpuPower {
  package: number | null;
  cores: number | null;
  graphics: number | null;
  memory: number | null;
}

export interface BatteryInfo {
  id: string;
  name: string;
  manufacturer: string;
  serial: string;
  /** e.g. `LION`, `LiP` */
  chemistry: string;
  /** mWh */
  designedCapacity: number | null;
  fullCapacity: number | null;
  cycleCount: number | null;
  /** YYYY-MM-DD */
  manufactured: string | null;
}

export interface BatteryStats {
  info: BatteryInfo;
  percent: number;
  /** mWh remaining */
  capacity: number | null;
  /** mW into the battery; negative while discharging */
  rate: number | null;
  /** mV */
  voltage: number | null;
  state: "charging" | "discharging" | "full" | "idle";
  pluggedIn: boolean;
  critical: boolean;
  /** Seconds until empty (discharging only). */
  remaining: number | null;
  temperature: number | null;
}

export interface MemoryInfo {
  installed: number;
  /** MT/s */
  speed: number | null;
  slotsUsed: number;
  slotsTotal: number;
  /** e.g. `DIMM`; `onboard` for memory soldered to the board. */
  formFactor: string | null;
  kind: string | null;
}

export interface DiskInfo {
  index: number;
  model: string;
  size: number;
  bus: "nvme" | "sata" | "usb" | "sd" | "other";
  ssd: boolean | null;
  removable: boolean;
  volumes: string[];
}

export interface DiskStats {
  info: DiskInfo;
  /** Percent of time busy. */
  active: number;
  read: number;
  write: number;
  responseMs: number;
  /** °C */
  temperature: number | null;
  temperatureWarning: number | null;
}

export interface NicInfo {
  id: string;
  name: string;
  description: string;
  kind: "ethernet" | "wifi" | "virtual" | "other";
  /** bits per second */
  linkSpeed: number;
  mac: string | null;
  ipv4: string[];
  ipv6: string[];
}

export interface NicStats {
  info: NicInfo;
  /** bytes per second */
  rx: number;
  tx: number;
}

export interface GpuInfo {
  id: string;
  name: string;
  dedicatedTotal: number;
  sharedTotal: number;
  engines: string[];
  temperatureMax: number | null;
}

export interface GpuStats {
  info: GpuInfo;
  util: number;
  /** Aligned with `info.engines`. */
  engines: number[];
  dedicatedUsed: number;
  sharedUsed: number;
  temperature: number | null;
  fanRpm: number | null;
  power: number | null;
  clockMhz: number | null;
  clockMaxMhz: number | null;
  voltageMv: number | null;
  memoryClockMhz: number | null;
}

export interface Hardware {
  cpu: CpuInfo;
  cpuMhz: number | null;
  /** Per logical processor, MHz; empty when unknown. */
  coreMhz: number[];
  thermal: ThermalZone[];
  power: CpuPower;
  memory: MemoryInfo;
  disks: DiskStats[];
  nics: NicStats[];
  gpus: GpuStats[];
  batteries: BatteryStats[];
}

export interface Snapshot {
  seq: number;
  timestamp: number;
  system: SystemStats;
  hardware: Hardware;
  processes: ProcessRow[];
  /** Per-process network traffic is measured locally or by the privileged service. */
  processNetwork: boolean;
}

/** System totals of one recorded frame (or one chart bucket). */
export interface HistoryPoint {
  ts: number;
  cpu: number;
  cpuPeak: number;
  gpu: number;
  memUsed: number;
  memTotal: number;
  /** Bytes per second, all disks / all adapters. */
  disk: number;
  net: number;
}

/** A process that stood out in a recorded frame (averages over the frame). */
export interface HistoryProcess {
  name: string;
  pid: number;
  cpu: number;
  mem: number;
  io: number;
  gpu: number;
  net: number;
}

export interface HistoryMoment {
  system: HistoryPoint;
  processes: HistoryProcess[];
}

export type UpdateChannel = "stable" | "beta";
/** `auto`: GitHub first, then the proxies; `proxy`: only the chosen proxy. */
export type UpdateRoute = "auto" | "direct" | "proxy";

/** `system` follows the Windows app mode. */
export type Theme = "system" | "light" | "dark";

export interface SettingsView {
  language: Language;
  theme: Theme;
  /** The onboarding version last completed or skipped; 0 before the first. */
  onboarding: number;
  closeToTray: boolean;
  /** Accelerator such as `Ctrl+Alt+Escape`; empty when disabled. */
  hotkey: string;
  rules: ProcessRule[];
  /** Record history for the history page. */
  history: boolean;
  updateCheck: boolean;
  updateChannel: UpdateChannel;
  updateRoute: UpdateRoute;
  /** GitHub proxy prefix, e.g. `https://gh-proxy.com/`. */
  updateProxy: string;
  autostart: boolean;
  /** The language in effect (`language` resolved). */
  locale: Locale;
  hotkeyError: string | null;
}

export interface UpdateStatus {
  /** `available` only for the standalone exe, which updates by hand. */
  state:
    | "disabled"
    | "idle"
    | "checking"
    | "upToDate"
    | "available"
    | "downloading"
    | "ready"
    | "installing"
    | "error";
  version: string | null;
  /** Plain text; unlike the installer, not signed. */
  notes: string | null;
  /** Unix ms. */
  date: number | null;
  received: number;
  total: number | null;
  /** Why the last check failed, or why updates are disabled. */
  error: string | null;
  checkedAt: number | null;
  /** "GitHub" or the proxy's host. */
  source: string | null;
  portable: boolean;
}

/** What the recorded history takes on disk. */
export interface HistoryUsage {
  bytes: number;
  /** Unix ms of the oldest frame. */
  first: number | null;
}

/** Applied to a program each time it starts (and to running copies when saved). */
export interface ProcessRule {
  id: string;
  /** Executable file name, matched case-insensitively. */
  name: string;
  enabled: boolean;
  priority: PriorityClass | null;
  /** true turns efficiency mode on, false off, null leaves it alone. */
  efficiency: boolean | null;
}

export interface RuleStats {
  applied: number;
  /** Processes it could not change without administrator rights. */
  denied: number;
  failed: number;
}

export interface AccentColors {
  accent: string;
  light1: string;
  light2: string;
  light3: string;
  dark1: string;
  dark2: string;
  dark3: string;
}

export interface AppInfo {
  version: string;
  elevated: boolean;
  accent: AccentColors | null;
  /** Relaunched by the installer of an update. */
  updated: boolean;
  /** The UI language, resolved. */
  locale: Locale;
}

export interface PrivilegedServiceStatus {
  state: "notInstalled" | "running" | "stopped" | "startPending" | "stopPending" | "unavailable";
  authorized: boolean;
  version: string | null;
  error: string | null;
}

export interface TaskManagerStatus {
  state: "disabled" | "enabled" | "recoveryNeeded" | "conflict";
  canEnable: boolean;
  canRestore: boolean;
  recoveryPath: string;
  detail: string;
}

export type PrivilegedServiceAction = "install" | "update" | "start" | "uninstall";

export type Integrity = "untrusted" | "low" | "medium" | "mediumPlus" | "high" | "system" | "protected";
export type PriorityClass = "idle" | "belowNormal" | "normal" | "aboveNormal" | "high" | "realtime";

export interface ProcessDetails {
  pid: number;
  accessible: boolean;
  commandLine: string | null;
  user: string | null;
  integrity: Integrity | null;
  elevated: boolean | null;
  architecture: "x64" | "x86" | "arm64" | "arm" | null;
  priorityClass: PriorityClass | null;
  version: {
    description: string | null;
    company: string | null;
    productName: string | null;
    fileVersion: string | null;
  };
}

export type ActionError =
  | { kind: "accessDenied" }
  | { kind: "notFound" }
  | { kind: "critical" }
  | { kind: "refused" }
  | { kind: "os"; message: string };

export interface TreeKillReport {
  terminated: number;
  failed: number;
}

export type ServiceState =
  | "stopped"
  | "startPending"
  | "stopPending"
  | "running"
  | "continuePending"
  | "pausePending"
  | "paused";
export type ServiceStartType = "boot" | "system" | "auto" | "autoDelayed" | "manual" | "disabled";

export interface ServiceInfo {
  name: string;
  displayName: string;
  description: string | null;
  state: ServiceState;
  startType: ServiceStartType | null;
  /** Hosting process; 0 when not running. */
  pid: number;
  path: string | null;
  account: string | null;
  canStop: boolean;
  shared: boolean;
}

export interface StartupItem {
  id: string;
  name: string;
  command: string;
  path: string | null;
  location: "hkcuRun" | "hklmRun" | "hklmRun32" | "userFolder" | "commonFolder";
  /** Applies to all users; changing it needs administrator rights. */
  machine: boolean;
  enabled: boolean;
  exists: boolean;
  publisher: string | null;
  description: string | null;
  icon: number | null;
}

export type TcpState =
  | "listen"
  | "established"
  | "synSent"
  | "synReceived"
  | "finWait1"
  | "finWait2"
  | "closeWait"
  | "closing"
  | "lastAck"
  | "timeWait"
  | "closed";

export interface Connection {
  protocol: "tcp" | "udp";
  ipv6: boolean;
  localAddress: string;
  localPort: number;
  remoteAddress: string | null;
  remotePort: number | null;
  state: TcpState | null;
  pid: number;
}

export interface Locker {
  pid: number;
  createTime: number;
  appName: string;
  service: string | null;
  kind: "window" | "service" | "explorer" | "console" | "critical" | "other";
  handleCount: number;
  handlePath: string | null;
}

export interface HandleSummary {
  available: boolean;
  viaService: boolean;
  elevated: boolean;
  checked: number;
  deniedProcesses: number;
  skipped: number;
  timeouts: number;
  truncated: boolean;
  error: string | null;
}

export interface LockReport {
  lockers: Locker[];
  filesChecked: number;
  /** Files Windows would not let us inspect (system files used by protected processes). */
  unchecked: number;
  truncated: boolean;
  handles: HandleSummary | null;
  restartError: string | null;
}
