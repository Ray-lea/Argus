import { bytes, machine, rate, sampleAt, type Sample } from "./model";

export interface Metric {
  id: string;
  name: string;
  group: string;
  device?: string;
  color: string;
  unit: string;
  read: (s: Sample) => number;
  second?: (s: Sample) => number;
}

function metric(
  id: string,
  name: string,
  group: string,
  color: string,
  unit: string,
  read: Metric["read"],
  device?: string,
  second?: Metric["read"],
): Metric {
  return { id, name, group, color: "var(--a-" + color + ")", unit, read, device, second };
}
const gb = 1024 ** 3;
export const metrics = [
  metric("cpu", "CPU", "处理器", "cpu", "%", (s) => s.cpu),
  metric("mem", "内存", "内存", "mem", "bytes", (s) => (s.mem * 32 * gb) / 100),
  metric(
    "disk",
    "磁盘",
    "存储",
    "io",
    "rate",
    (s) => s.read,
    machine.disk,
    (s) => s.write,
  ),
  metric(
    "net",
    "网络",
    "网络",
    "net",
    "rate",
    (s) => s.rx,
    machine.wifi,
    (s) => s.tx,
  ),
  metric("gpu", "GPU", "显卡", "gpu", "%", (s) => s.gpu, machine.gpu),
  metric("battery", "电池电量", "电池", "bat", "%", () => 84),
  metric("temp", "CPU 温度", "处理器", "temp", "°C", (s) => 62 + s.cpu * 0.32),
  metric("clock", "CPU 频率", "处理器", "cpu-e", "GHz", (s) => 2.5 + s.cpu * 0.035),
  metric("power", "CPU 功耗", "处理器", "io", "W", (s) => 28 + s.cpu * 0.82),
  metric("kernel", "内核时间", "处理器", "cpu", "%", (s) => s.kernel),
  metric("voltage", "CPU 电压", "处理器", "cpu", "V", (s) => 0.75 + s.cpu * 0.007),
  metric("fan", "风扇", "处理器", "cpu", "RPM", (s) => 1800 + 25 * s.cpu),
  metric("memoryPercent", "占比", "内存", "mem", "%", (s) => s.mem),
  metric("commit", "已提交内存", "内存", "mem", "bytes", (s) => s.mem * 0.41 * gb),
  metric("cache", "已缓存内存", "内存", "mem", "bytes", () => 7.7 * gb),
  metric("compressed", "已压缩内存", "内存", "mem", "bytes", () => 0.5 * gb),
  metric("paged", "分页缓冲池", "内存", "mem", "bytes", () => 0.625 * gb),
  metric("nonpaged", "非分页缓冲池", "内存", "mem", "bytes", () => 310 * 1024 ** 2),
  metric(
    "disk0",
    "读写",
    "存储",
    "io",
    "rate",
    (s) => s.read,
    machine.disk,
    (s) => s.write,
  ),
  metric(
    "wlan",
    "收发",
    "网络",
    "net",
    "rate",
    (s) => s.rx,
    machine.wifi,
    (s) => s.tx,
  ),
  metric("diskActive", "磁盘活动时间", "存储", "io", "%", (s) => s.active, machine.disk),
  metric("diskTemp", "温度", "存储", "io", "°C", (s) => 38 + s.active * 0.15, machine.disk),
  metric("diskResponse", "平均响应时间", "存储", "io", "ms", (s) => 0.21 + s.active * 0.012, machine.disk),
  metric("gpuMem", "专用 GPU 内存", "显卡", "gpu", "bytes", (s) => (3.3 + s.gpu / 900) * gb, machine.gpu),
  metric("gpuShared", "共享 GPU 内存", "显卡", "gpu", "bytes", (s) => (0.3 + s.gpu / 1200) * gb, machine.gpu),
  metric("gpuTemp", "GPU 温度", "显卡", "gpu", "°C", (s) => 46 + s.gpu * 0.12, machine.gpu),
  metric("gpuPower", "GPU 功耗", "显卡", "gpu", "W", (s) => 18 + s.gpu * 0.2, machine.gpu),
  metric("gpuClock", "GPU 频率", "显卡", "gpu", "MHz", (s) => 900 + s.gpu * 3, machine.gpu),
  metric("gpuFan", "风扇", "显卡", "gpu", "RPM", (s) => 1350 + s.gpu * 5, machine.gpu),
  metric("charge", "电池功率", "电池", "bat", "W", () => 16.9),
  metric("batteryV", "电池电压", "电池", "bat", "V", () => 15.2),
  metric("batteryTemp", "温度", "电池", "bat", "°C", () => 32),
  metric("uptime", "正常运行时间", "其他", "cpu", "", () => 134),
  metric("processes", "进程数", "系统", "cpu", "", (s) => 218 + Math.round(s.cpu / 10)),
  metric("threads", "线程数", "系统", "cpu", "", (s) => 3100 + Math.round(s.cpu * 3)),
  metric("handles", "句柄数", "系统", "cpu", "", (s) => 88000 + Math.round(s.cpu * 5)),
];
export const byMetric = Object.fromEntries(metrics.map((m) => [m.id, m]));
export const tabMetrics = metrics.slice(0, 6);
export function formatMetric(unit: string, value: number) {
  if (unit === "bytes") return bytes(value);
  if (unit === "rate") return rate(value);
  const digits = ["GHz", "V", "ms"].includes(unit) ? 2 : 0;
  return value.toFixed(digits) + (unit && unit !== "%" ? " " : "") + unit;
}
export function metricValue(m: Metric, tick: number) {
  if (m.id === "uptime") return "5 天 14 小时";
  const s = sampleAt(tick);
  return [m.read(s), ...(m.second ? [m.second(s)] : [])].map((v) => formatMetric(m.unit, v)).join(" / ");
}
