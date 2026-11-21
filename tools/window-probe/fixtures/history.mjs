// The history page's part of the fixture, derived from the rest of it: the
// recorded channels, five and a half minutes of per-second points, the newest
// frame's processes and a range table, all relative to the fixture sample's
// time. Change the shape here and run it again instead of editing the JSON.
//
//   node tools/window-probe/fixtures/history.mjs           rewrite desktop.json
//   node tools/window-probe/fixtures/history.mjs --check   fail when it is out of date
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";

const GIB = 1024 ** 3;
const MIB = 1024 ** 2;
const DAY = 86400000;

// Deterministic smooth noise.
function hash(n) {
  n = Math.imul(n ^ (n >>> 16), 0x45d9f3b);
  n = Math.imul(n ^ (n >>> 16), 0x45d9f3b);
  return ((n ^ (n >>> 16)) >>> 0) / 4294967296;
}
function smooth(t, seed, period) {
  const x = t / period;
  const i = Math.floor(x);
  const f = x - i;
  const s = f * f * (3 - 2 * f);
  return hash(i * 7919 + seed) * (1 - s) + hash((i + 1) * 7919 + seed) * s;
}
const round = (v, d = 3) => Math.round(v * 10 ** d) / 10 ** d;

// Programs that run as several instances, added after the frame's own processes:
// name, PID, CPU percent, memory in MiB.
const EXTRA = [
  ["chrome.exe", 21400, 4.2, 612],
  ["chrome.exe", 21452, 1.6, 284],
  ["chrome.exe", 21508, 0.9, 198],
  ["chrome.exe", 21733, 0.4, 132],
  ["Code.exe", 15120, 2.8, 466],
  ["Code.exe", 15188, 0.7, 251],
  ["msedgewebview2.exe", 9820, 0.3, 96],
  ["msedgewebview2.exe", 9876, 0.2, 74],
  ["rustc.exe", 30212, 11.5, 1210],
  ["Steam.exe", 7710, 0.4, 188],
];

/** The fixture's history keys, from its sample time, hardware and the frame's base processes. */
export function historyData(fixture) {
  const now = fixture.snapshot.timestamp;
  const hw = fixture.snapshot.hardware;
  const first = now - 6 * DAY;

  // Channels: what was recorded, with device labels.
  const channel = (key, unit, group, label = null, device = null) => ({ key, unit, group, label, device, service: false, first, last: now });
  const channels = [
    ["cpu", "percent", "cpu"],
    ["cpuPeak", "percent", "cpu"],
    ["cpuKernel", "percent", "cpu"],
    ["cpuClock", "megahertz", "cpu"],
    ["cpuPower", "watts", "cpu"],
    ["cpuTemp", "celsius", "cpu"],
    ["memUsed", "bytes", "memory"],
    ["memTotal", "bytes", "memory"],
    ["commitUsed", "bytes", "memory"],
    ["commitLimit", "bytes", "memory"],
    ["memCached", "bytes", "memory"],
    ["memCompressed", "bytes", "memory"],
    ["pagedPool", "bytes", "memory"],
    ["nonpagedPool", "bytes", "memory"],
    ["processes", "count", "system"],
    ["threads", "count", "system"],
    ["handles", "count", "system"],
    ["diskRead", "bytesPerSecond", "disk"],
    ["diskWrite", "bytesPerSecond", "disk"],
    ["netReceive", "bytesPerSecond", "network"],
    ["netSend", "bytesPerSecond", "network"],
    ["gpu", "percent", "gpu"],
    ["battery", "percent", "battery"],
    ["batteryRate", "milliwatts", "battery"],
    ["batteryVoltage", "volts", "battery"],
  ].map(([key, unit, group]) => channel(key, unit, group));
  for (const disk of hw.disks) {
    const id = String(disk.info.index);
    for (const [part, unit] of [
      ["read", "bytesPerSecond"],
      ["write", "bytesPerSecond"],
      ["active", "percent"],
      ["response", "milliseconds"],
    ])
      channels.push(channel(`disk.${id}.${part}`, unit, "disk", disk.info.model, id));
    if (disk.temperature != null) channels.push(channel(`disk.${id}.temp`, "celsius", "disk", disk.info.model, id));
  }
  for (const nic of hw.nics)
    for (const part of ["receive", "send"]) channels.push(channel(`nic.${nic.info.id}.${part}`, "bytesPerSecond", "network", nic.info.name, nic.info.id));
  const gpu = hw.gpus[0];
  for (const [part, unit] of [
    ["util", "percent"],
    ["dedicated", "bytes"],
    ["dedicatedTotal", "bytes"],
    ["temp", "celsius"],
    ["power", "watts"],
  ])
    channels.push(channel(`gpu.${gpu.info.id}.${part}`, unit, "gpu", gpu.info.name, gpu.info.id));

  // Per-second points, ending a second before the sample.
  const BUCKET = 1000;
  const start = now - (now % 1000) - 330 * 1000;
  const ts = [];
  const span = [];
  const frame = [];
  const series = {};
  const put = (key, value) => {
    (series[key] ??= { mean: [] }).mean.push(value == null ? null : round(value));
  };
  for (let t = start; t <= now - 1000; t += BUCKET) {
    const s = t / 1000;
    const burst = smooth(s, 11, 23) > 0.72 ? 38 * smooth(s, 12, 4) ** 2 : 0;
    const cpu = 9 + 14 * smooth(s, 1, 37) + 6 * smooth(s, 2, 6) + burst;
    ts.push(t);
    span.push(BUCKET);
    frame.push(t - (t % 5000));
    put("cpu", cpu);
    put("cpuPeak", Math.min(100, cpu * (1.15 + 0.5 * smooth(s, 3, 3))));
    put("cpuTemp", 48 + 0.35 * cpu + 4 * smooth(s, 4, 41));
    put("memUsed", 13.1 * GIB + 0.9 * GIB * smooth(s, 5, 160) + 0.12 * GIB * smooth(s, 6, 9));
    put("memTotal", 32 * GIB);
    const reading = smooth(s, 7, 17) > 0.58 ? 38 * MIB * smooth(s, 8, 3) ** 2 : 0.4 * MIB * smooth(s, 9, 5);
    put("diskRead", reading);
    put("diskWrite", 3.2 * MIB * smooth(s, 10, 13) ** 3);
    put("netReceive", 1.4 * MIB * smooth(s, 13, 29) ** 2 + 60 * 1024 * smooth(s, 14, 3));
    put("netSend", 240 * 1024 * smooth(s, 15, 19) ** 2);
    put("gpu", 6 + 22 * smooth(s, 16, 31) ** 3);
    put("battery", 84 - ((now - t) / 3600000) * 9);
  }
  const points = { bucket: BUCKET, origin: now - (now % DAY), from: start, to: now, ts, span, frame, series, gaps: [], settled: ts.at(-1) };

  // The newest frame's processes. The frame's own processes (everything but
  // EXTRA) are the input: their name, PID and readings stay as they are.
  const created = now - 3 * 3600000;
  const added = new Set(EXTRA.map(([name, pid]) => `${name}/${pid}`));
  const base = fixture.historyFrame.processes
    .filter((p) => !added.has(`${p.name}/${p.pid}`))
    .map(({ name, pid, cpu, mem, io, gpu, net }, i) => ({ name, pid, cpu, mem, io, gpu, net, createTime: created - i * 60000 }));
  const processes = base
    .concat(
      EXTRA.map(([name, pid, cpu, mem], i) => ({
        name,
        pid,
        createTime: created + i * 90000,
        cpu,
        mem: mem * MIB,
        io: cpu * 0.6 * MIB,
        gpu: name === "chrome.exe" ? cpu * 0.8 : 0,
        net: name === "chrome.exe" ? 48000 * cpu : 0,
      })),
    )
    .map((p, i) => ({
      ...p,
      diskRead: p.io ? round(p.io * 0.55, 0) : 0,
      diskWrite: p.io ? round(p.io * 0.2, 0) : 0,
      gpuMem: p.gpu > 0 ? 180 * MIB + i * 12 * MIB : null,
      power: round((p.cpu / 100) * 20 + (p.gpu / 100) * 6, 3),
      threads: 8 + ((i * 7) % 60),
      handles: 220 + ((i * 131) % 2400),
    }));
  const historyFrame = {
    ts: now - (now % 5000) - 5000,
    span: 5000,
    system: { cpu: 18.4, cpuPeak: 31.2, memUsed: 13.4 * GIB, memTotal: 32 * GIB, diskRead: 2.1 * MIB, diskWrite: 0.6 * MIB, netReceive: 0.9 * MIB, netSend: 0.1 * MIB, gpu: 9.5, battery: 84 },
    processes,
  };

  // The range table: the same processes over five minutes, with trends.
  const TREND = 32;
  const seconds = 300;
  const historyTable = {
    from: now - 300000,
    to: now,
    recorded: 300000,
    processes: processes.map((p, i) => ({
      name: p.name,
      pid: p.pid,
      createTime: p.createTime,
      cpu: round(p.cpu * 0.9, 3),
      cpuMax: round(p.cpu * 2.4 + 2, 3),
      mem: Math.round(p.mem * 1.08),
      disk: Math.round((p.diskRead + p.diskWrite) * seconds),
      diskRead: Math.round(p.diskRead * seconds),
      diskWrite: Math.round(p.diskWrite * seconds),
      io: Math.round(p.io * seconds),
      net: Math.round(p.net * seconds),
      gpu: round(p.gpu * 0.9, 3),
      gpuMem: p.gpuMem,
      power: p.power,
      threads: p.threads,
      handles: p.handles,
      seen: seconds * 1000,
      first: now - 300000,
      last: now,
      trend: Array.from({ length: TREND }, (_, k) => round(Math.max(0, p.cpu * (0.4 + 1.2 * smooth(k, 50 + i, 5))), 3)),
    })),
  };

  return { historyChannels: channels, historyPoints: points, historyFrame, historyTable, historyRange: [first - 20 * 3600000, now] };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const file = new URL("desktop.json", import.meta.url);
  const fixture = JSON.parse(readFileSync(file, "utf8"));
  const data = historyData(fixture);
  const stale = Object.keys(data).filter((key) => !isDeepStrictEqual(fixture[key], data[key]));
  if (process.argv.includes("--check")) {
    if (stale.length) {
      console.error(`desktop.json: ${stale.join(", ")} differ from what history.mjs writes`);
      process.exit(1);
    }
    console.log("desktop.json: history data is up to date");
  } else if (stale.length) {
    writeFileSync(file, `${JSON.stringify({ ...fixture, ...data }, null, 2)}\n`);
    console.log(`desktop.json: rewrote ${stale.join(", ")}`);
  } else {
    console.log("desktop.json: nothing to change");
  }
}
