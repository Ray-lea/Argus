const KB = 1024;
const MB = KB * 1024;
const GB = MB * 1024;

/** Process memory, Task Manager style: MB below 1 GB. */
export function fmtMem(bytes: number): string {
  if (bytes >= GB) return `${(bytes / GB).toFixed(bytes >= 10 * GB ? 1 : 2)} GB`;
  return `${(bytes / MB).toFixed(1)} MB`;
}

export function fmtGB(bytes: number, digits = 1): string {
  return (bytes / GB).toFixed(digits);
}

/** Byte rate; zero renders as a bare "0" so idle rows stay quiet. */
export function fmtRate(bytesPerSec: number): string {
  if (bytesPerSec < 1) return "0";
  if (bytesPerSec < MB) return `${Math.max(1, Math.round(bytesPerSec / KB))} KB/s`;
  if (bytesPerSec < GB) return `${(bytesPerSec / MB).toFixed(1)} MB/s`;
  return `${(bytesPerSec / GB).toFixed(2)} GB/s`;
}

/** Splits a rate into number and unit, for large figures. */
export function rateParts(bytesPerSec: number): [string, string] {
  if (bytesPerSec < MB) return [(bytesPerSec / KB).toFixed(0), "KB/s"];
  if (bytesPerSec < GB) return [(bytesPerSec / MB).toFixed(1), "MB/s"];
  return [(bytesPerSec / GB).toFixed(2), "GB/s"];
}

export function fmtCpu(pct: number): string {
  if (pct < 0.05) return "0";
  if (pct < 10) return pct.toFixed(1);
  return pct.toFixed(0);
}

const nf = new Intl.NumberFormat("zh-CN");
export const fmtCount = (n: number) => nf.format(n);

export function fmtUptime(ms: number): string {
  const s = Math.floor(ms / 1000);
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d > 0) return `${d} 天 ${h} 小时`;
  if (h > 0) return `${h} 小时 ${m} 分`;
  return `${m} 分 ${s % 60} 秒`;
}

/** CPU time as h:mm:ss. */
export function fmtCpuTime(ms: number): string {
  const s = Math.floor(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return `${h}:${String(m).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

const dtf = new Intl.DateTimeFormat("zh-CN", {
  month: "numeric",
  day: "numeric",
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
});
export const fmtDateTime = (unixMs: number) => (unixMs > 0 ? dtf.format(unixMs) : "系统启动时");

/** Network rate in bits per second, the unit adapters are sold in. */
export function fmtBits(bytesPerSec: number): string {
  const bits = bytesPerSec * 8;
  if (bits < 1) return "0 Kbps";
  if (bits < 1e6) return `${(bits / 1e3).toFixed(bits < 1e4 ? 1 : 0)} Kbps`;
  if (bits < 1e9) return `${(bits / 1e6).toFixed(bits < 1e7 ? 2 : 1)} Mbps`;
  return `${(bits / 1e9).toFixed(2)} Gbps`;
}

export function bitsParts(bytesPerSec: number): [string, string] {
  const [n, unit] = fmtBits(bytesPerSec).split(" ");
  return [n, unit];
}

export function fmtLinkSpeed(bitsPerSec: number): string {
  if (bitsPerSec >= 1e9) return `${+(bitsPerSec / 1e9).toFixed(1)} Gbps`;
  return `${Math.round(bitsPerSec / 1e6)} Mbps`;
}

export function fmtGHz(mhz: number): string {
  return `${(mhz / 1000).toFixed(2)} GHz`;
}

/** Capacity in binary units, the way Windows reports drive sizes. */
export function fmtCapacity(bytes: number): string {
  if (bytes >= 1024 * GB) return `${(bytes / (1024 * GB)).toFixed(1)} TB`;
  return `${Math.round(bytes / GB)} GB`;
}

export function fmtCache(bytes: number): string {
  if (bytes >= MB) return `${(bytes / MB).toFixed(1)} MB`;
  return `${Math.round(bytes / KB)} KB`;
}
