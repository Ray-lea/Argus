import {
  adapterAt,
  adapters,
  batteryDischarge,
  batteryLevel,
  clock,
  cpuPower,
  cpuTemperature,
  diskAt,
  disks,
  gpuAt,
  gpus,
  memory,
  noise,
  processes,
  sampleAt,
  type Sample,
} from "./model";

// The system totals the status bar shows (Replica.tsx).
const threadTotal = processes.reduce((a, p) => a + p.threads, 1180);
const handleTotal = processes.reduce((a, p) => a + p.handles, 52400);

// The readings the history page can draw, as the app's catalogue lists them
// (history/readings.rs `catalogue`): its order, short names for the tabs and
// the monitors, long names for the lanes, menus and tooltips, the device a
// reading belongs to and its own name there, colours, units and axes. The
// devices are the demo machine's (model.ts `disks`, `adapters`, `gpus`).
export type Axis = "percent" | "total" | "auto" | "temp" | "signed" | "count" | "fixed";
export type ProcessKey = "cpu" | "mem" | "disk" | "net" | "gpu";
export interface Metric {
  id: string;
  name: string;
  long: string;
  group: string;
  device?: string;
  part?: string;
  color: string;
  unit: string;
  axis: Axis;
  /** The axis top of a `total` axis; the low and high ends of a `fixed` one. */
  total?: number;
  fixed?: [number, number];
  /** The per-process value the table's trend and the tooltip follow. */
  process?: ProcessKey;
  /** Read through the privileged service. */
  service?: boolean;
  /** A pair's two parts (read and write, received and sent). */
  parts?: [string, string];
  /** Offered by the chooser (the monitors' own readings are not). */
  listed: boolean;
  /** Its value at a sample (`tick`), and a pair's second part. */
  read: (t: number) => number;
  second?: (t: number) => number;
}
type Reader = (s: Sample, t: number) => number;
type Extra = Partial<Omit<Metric, "id" | "name" | "group" | "color" | "unit" | "read" | "second">> & { second?: Reader };
function metric(id: string, name: string, group: string, color: string, unit: string, read: Reader, extra: Extra = {}): Metric {
  const { second, ...rest } = extra;
  return {
    id,
    name,
    long: name,
    group,
    color: "var(--a-" + color + ")",
    unit,
    axis: "auto",
    listed: true,
    ...rest,
    read: (t) => read(sampleAt(t), t),
    second: second && ((t) => second(sampleAt(t), t)),
  };
}
const gb = 1024 ** 3;
const sum = (list: number[]) => list.reduce((a, b) => a + b, 0);
// Each disk: its pair of rates under its name, then its active time, response time and temperature.
const diskReadings = disks.flatMap((d) => {
  const of = (part: string) => ({ device: d.name, part, long: d.name + " " + part });
  return [
    metric("disk:" + d.id, d.name, "存储", "io", "rate", (_, t) => diskAt(d.index, t).read, {
      ...of("读写"),
      parts: ["读取", "写入"],
      process: "disk",
      second: (_, t) => diskAt(d.index, t).write,
    }),
    metric(d.id + "Active", d.name + " 活动时间", "存储", "io", "%", (_, t) => diskAt(d.index, t).active, {
      ...of("活动时间"),
      axis: "fixed",
      fixed: [0, 100],
    }),
    metric(d.id + "Response", d.name + " 平均响应时间", "存储", "io", "ms", (_, t) => diskAt(d.index, t).response, of("平均响应时间")),
    metric(d.id + "Temp", d.name + " 温度", "存储", "temp", "°C", (_, t) => diskAt(d.index, t).temperature, { ...of("温度"), axis: "temp" }),
  ];
});
// Each adapter: its pair of rates under its name.
const nicReadings = adapters.map((a) =>
  metric("nic:" + a.id, a.name, "网络", "net", "rate", (_, t) => adapterAt(a.index, t).rx, {
    device: a.name,
    part: "收发",
    long: a.name + " 收发",
    parts: ["接收", "发送"],
    process: "net",
    second: (_, t) => adapterAt(a.index, t).tx,
  }),
);
// Each adapter's readings under "GPU n · model"; with two, their usage in two colours.
const gpuReadings = gpus.flatMap((g, n) => {
  const number = g.name,
    device = number + " · " + g.model;
  const of = (part: string) => ({ device, part, long: number + " " + part, process: "gpu" as const });
  const at = (t: number) => gpuAt(g.index, t);
  return [
    metric(g.id + "Util", number, "显卡", n % 2 ? "gpu" : "group-bg", "%", (_, t) => at(t).util, {
      ...of("使用率"),
      long: number + " 使用率 · " + g.model,
      axis: "percent",
    }),
    ...(g.dedicated
      ? [
          metric(g.id + "Mem", number + " 专用 GPU 内存", "显卡", "group-user-3", "bytes", (_, t) => at(t).dedicated, {
            ...of("专用 GPU 内存"),
            axis: "total",
            total: g.dedicated,
          }),
        ]
      : []),
    metric(g.id + "Shared", number + " 共享 GPU 内存", "显卡", "group-user-3", "bytes", (_, t) => at(t).shared, {
      ...of("共享 GPU 内存"),
      axis: "total",
      total: g.shared,
    }),
    metric(g.id + "Temp", number + " 温度", "显卡", "temp", "°C", (_, t) => at(t).temperature, { ...of("温度"), axis: "temp" }),
    metric(g.id + "Power", number + " 功耗", "显卡", "group-user-1", "W", (_, t) => at(t).power, of("功耗")),
    metric(g.id + "Clock", number + " 频率", "显卡", "cpu-e", "GHz", (_, t) => at(t).clock / 1000, of("频率")),
    ...(g.integrated
      ? []
      : [metric(g.id + "Fan", number + " 风扇", "显卡", "group-user-1", "RPM", (_, t) => 1350 + at(t).util * 5, of("风扇"))]),
  ];
});
export const metrics = [
  metric("cpu", "CPU", "处理器", "cpu", "%", (s) => s.cpu, { long: "CPU 使用率", axis: "percent", process: "cpu" }),
  metric("kernel", "内核时间", "处理器", "cpu-e", "%", (s) => s.kernel, { long: "CPU 内核时间", axis: "percent", process: "cpu" }),
  metric("clock", "CPU 频率", "处理器", "cpu-e", "GHz", (_, t) => clock("p", t), { process: "cpu" }),
  metric("power", "CPU 功耗", "处理器", "group-user-1", "W", (_, t) => cpuPower(t), { long: "CPU 封装功耗", process: "cpu" }),
  metric("temp", "CPU 温度", "处理器", "temp", "°C", (_, t) => cpuTemperature(t), { axis: "temp", process: "cpu", service: true }),
  metric("voltage", "CPU 电压", "处理器", "cpu-e", "V", (_, t) => 1.21 + noise(230, t) * 0.03, {
    long: "CPU 核心电压",
    axis: "fixed",
    fixed: [0.6, 1.4],
    process: "cpu",
    service: true,
  }),
  metric("fan", "风扇 CPU", "处理器", "group-user-1", "RPM", (s) => 1800 + 25 * s.cpu, { service: true }),
  metric("mem", "内存", "内存", "mem", "bytes", (s) => (s.mem * 32 * gb) / 100, {
    long: "内存使用量",
    axis: "total",
    total: 32 * gb,
    process: "mem",
  }),
  metric("commit", "已提交", "内存", "group-user-3", "bytes", (s) => s.mem * 0.41 * gb, {
    long: "已提交内存",
    axis: "total",
    total: 37 * gb,
    process: "mem",
  }),
  metric("cache", "已缓存", "内存", "group-user-3", "bytes", () => 7.7 * gb, { long: "已缓存内存", axis: "total", total: 32 * gb, process: "mem" }),
  metric("compressed", "已压缩", "内存", "group-user-3", "bytes", () => 0.5 * gb, {
    long: "已压缩内存",
    axis: "total",
    total: 32 * gb,
    process: "mem",
  }),
  metric("paged", "分页缓冲池", "内存", "group-user-3", "bytes", () => 0.625 * gb, { process: "mem" }),
  metric("nonpaged", "非分页缓冲池", "内存", "group-user-3", "bytes", () => 310 * 1024 ** 2, { process: "mem" }),
  // All disks together, then each.
  metric("disk", "磁盘", "存储", "io", "rate", (_, t) => sum(disks.map((d) => diskAt(d.index, t).read)), {
    long: "磁盘读写",
    parts: ["读取", "写入"],
    process: "disk",
    second: (_, t) => sum(disks.map((d) => diskAt(d.index, t).write)),
  }),
  ...diskReadings,
  metric("net", "网络", "网络", "net", "rate", (_, t) => sum(adapters.map((a) => adapterAt(a.index, t).rx)), {
    long: "网络收发",
    parts: ["接收", "发送"],
    process: "net",
    second: (_, t) => sum(adapters.map((a) => adapterAt(a.index, t).tx)),
  }),
  ...nicReadings,
  // The busiest engine of any adapter, then each adapter.
  metric("gpu", "GPU", "显卡", "gpu", "%", (_, t) => Math.max(...gpus.map((g) => gpuAt(g.index, t).util)), {
    long: "GPU 使用率",
    axis: "percent",
    process: "gpu",
  }),
  ...gpuReadings,
  metric("battery", "电池", "电池", "bat", "%", (_, t) => batteryLevel(t), { long: "电池电量", axis: "fixed", fixed: [0, 100] }),
  metric("charge", "电池功率", "电池", "group-user-1", "±W", (_, t) => -batteryDischarge(t), { long: "电池充放电功率", axis: "signed" }),
  metric("batteryV", "电池电压", "电池", "cpu-e", "V", () => 15.2),
  metric("batteryTemp", "电池温度", "电池", "temp", "°C", () => 32, { axis: "temp" }),
  metric("processes", "进程数", "系统", "ink-2", "", () => processes.length, { axis: "count" }),
  metric("threads", "线程数", "系统", "ink-2", "", () => threadTotal, { axis: "count" }),
  metric("handles", "句柄数", "系统", "ink-2", "", () => handleTotal, { axis: "count" }),
  // The monitors' own readings.
  metric("memoryPercent", "占比", "内存", "mem", "%", (s) => s.mem, { listed: false }),
  metric("uptime", "正常运行时间", "系统", "cpu", "", () => 134, { listed: false }),
];
export const byMetric = Object.fromEntries(metrics.map((m) => [m.id, m]));
export const groups = ["处理器", "内存", "存储", "网络", "显卡", "电池", "系统"];
// The tabs: CPU, memory, disk, network, GPU (each adapter when there are several), battery (readings.rs `tab_ids`).
export const tabMetrics = ["cpu", "mem", "disk", "net", ...(gpus.length > 1 ? gpus.map((g) => g.id + "Util") : ["gpu"]), "battery"].map(
  (id) => byMetric[id],
);

// The app's number formats (theme::cpu, theme::memory, charts::byte_rate…).
export const percentText = (v: number) => (v < 0.05 ? "0" : v >= 10 ? Math.round(v).toString() : v.toFixed(1));
export const thousands = (v: number) => Math.round(v).toLocaleString("en-US");
export function byteRate(v: number) {
  if (v < 1) return "0 KB/s";
  if (v < 1024 ** 2) return Math.max(1, Math.round(v / 1024)) + " KB/s";
  if (v < gb) return (v / 1024 ** 2).toFixed(1) + " MB/s";
  return (v / gb).toFixed(2) + " GB/s";
}
const tenths = (w: number) => (w < 10 ? w.toFixed(1) : Math.round(w).toString());
export function formatMetric(unit: string, value: number) {
  if (!Number.isFinite(value)) return "—";
  switch (unit) {
    case "%":
      return percentText(value) + "%";
    case "bytes":
      return memory(value);
    case "rate":
      return byteRate(value);
    case "°C":
      return Math.round(value) + "°C";
    case "GHz":
      return value.toFixed(2) + " GHz";
    case "W":
      return tenths(Math.max(0, value)) + " W";
    case "±W":
      return (value > 0.05 ? "+" : value < -0.05 ? "-" : "") + tenths(Math.abs(value)) + " W";
    case "V":
      return value.toFixed(3) + " V";
    case "RPM":
      return Math.round(value) + " RPM";
    case "ms":
      return value.toFixed(1) + " ms";
    default:
      return thousands(value);
  }
}
export function metricValue(m: Metric, tick: number) {
  if (m.id === "uptime") return "5 天 14 小时";
  return [m.read(tick), ...(m.second ? [m.second(tick)] : [])].map((v) => formatMetric(m.unit, v)).join(" / ");
}
// Values as wide as a reading's widest, for slots that keep their width (readings.rs `widest_samples`).
export function widest(unit: string) {
  const samples: Record<string, number[]> = {
    "%": [100, 88.8],
    bytes: [88.8 * gb, 8.88 * gb],
    rate: [888 * 1024, 88.8 * 1024 ** 2, 888.8 * 1024 ** 2],
    "°C": [100],
    GHz: [8.888],
    W: [888.8],
    "±W": [-188],
    V: [8.888],
    RPM: [8888],
    ms: [888.8],
    "": [888888],
  };
  return (samples[unit] ?? [0]).map((v) => formatMetric(unit, v)).reduce((a, b) => (b.length > a.length ? b : a));
}
