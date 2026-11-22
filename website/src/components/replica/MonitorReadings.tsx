/** @jsxImportSource @/components/replica/i18n */
"use client";
import { Icon } from "./icons";
import { useId, useState, type CSSProperties, type DragEvent, type ReactNode } from "react";
import { adapterAt, adapters, diskAt, disks, gpuAt, gpus, sampleAt } from "./model";
import { byMetric } from "./metrics";
import { fill } from "./historyTime";
import { translate } from "./i18n";
import { monitorGroups, setMonitor, useShared, type MonitorKind } from "./store";
export const taskbarStyles = ["标签", "曲线", "胶囊", "大数字"];
export const desktopStyles = ["列表", "曲线", "仪表"];
const styleKeys: Record<string, string> = {
  标签: "label",
  曲线: "curve",
  胶囊: "capsule",
  大数字: "number",
  列表: "list",
  仪表: "gauge",
  横条: "strip",
};
// What each reading is called (components.json `metric`): its name on the
// desktop monitor, its short name on the taskbar, its option on the page,
// and its tag after another reading of its CPU or GPU.
const metric: Record<string, { name: string; short: string; option: string; tag?: string; family: number }> = {
  cpu: { name: "CPU", short: "CPU", option: "利用率", tag: "利用率", family: 0 },
  temp: { name: "CPU 温度", short: "CPU", option: "温度", tag: "温度", family: 0 },
  clock: { name: "CPU 频率", short: "CPU", option: "频率", tag: "频率", family: 0 },
  power: { name: "CPU 功耗", short: "CPU", option: "功耗", tag: "功耗", family: 0 },
  memoryPercent: { name: "内存", short: "内存", option: "占比", family: 1 },
  mem: { name: "内存用量", short: "内存", option: "用量", family: 1 },
  gpu: { name: "GPU", short: "GPU", option: "利用率", tag: "利用率", family: 2 },
  gpuTemp: { name: "GPU 温度", short: "GPU", option: "温度", tag: "温度", family: 2 },
  gpuMem: { name: "显存", short: "显存", option: "显存", tag: "显存", family: 2 },
  gpuClock: { name: "GPU 频率", short: "GPU", option: "频率", tag: "频率", family: 2 },
  gpuPower: { name: "GPU 功耗", short: "GPU", option: "功耗", tag: "功耗", family: 2 },
  gpuFan: { name: "GPU 风扇", short: "风扇", option: "风扇", tag: "风扇", family: 2 },
  disk: { name: "磁盘", short: "", option: "读写速度", family: 3 },
  diskActive: { name: "磁盘活动时间", short: "磁盘", option: "活动时间", family: 3 },
  diskTemp: { name: "磁盘温度", short: "磁盘", option: "温度", family: 3 },
  net: { name: "网络", short: "", option: "收发速率", family: 4 },
  battery: { name: "电池", short: "电池", option: "电量", family: 5 },
  uptime: { name: "正常运行时间", short: "运行", option: "正常运行时间", family: 6 },
};
const tones = ["cpu", "mem", "gpu", "io", "net", "bat", "ink"];
type Part = { mark: string; text: string; widest: string };
export type Gauge = {
  id: string;
  name: string;
  option: string;
  short: string;
  family: number;
  parts: Part[];
  fraction: number | null;
  bar: boolean;
  high: boolean;
  /** Recent values, 0–1, oldest first: one series, or a pair's two. */
  trend: number[][] | null;
  /** The device it is of, none for all of them (or the main GPU). */
  device: string;
};
const KB = 1024,
  MB = KB * 1024,
  GB = MB * 1024;
// A rate as the monitors write it (theme::bytes with `rate`).
const rateText = (v: number) =>
  v < 1 ? "0 KB/s" : v < MB ? Math.max(1, Math.round(v / KB)) + " KB/s" : v < GB ? (v / MB).toFixed(1) + " MB/s" : (v / GB).toFixed(2) + " GB/s";
const watts = (v: number) => (v < 10 ? v.toFixed(1) : Math.round(v).toString()) + " W";
// The readings, by the tick: a disk or connection reading without a device sums
// the rates and takes the busiest or hottest; a GPU reading without one is the main GPU's.
const sum = (list: number[]) => list.reduce((x, y) => x + y, 0);
const diskOf = (device: string | undefined, t: number) => {
  const list = disks.filter((d) => !device || d.id === device).map((d) => diskAt(d.index, t));
  return { read: sum(list.map((d) => d.read)), write: sum(list.map((d) => d.write)), active: Math.max(...list.map((d) => d.active)), temperature: Math.max(...list.map((d) => d.temperature)) };
};
const nicOf = (device: string | undefined, t: number) => {
  const list = adapters.filter((a) => !device || a.id === device).map((a) => adapterAt(a.index, t));
  return { rx: sum(list.map((a) => a.rx)), tx: sum(list.map((a) => a.tx)) };
};
const gpuOf = (device: string | undefined) => gpus.find((g) => g.id === device) ?? [...gpus].sort((x, y) => y.dedicated - x.dedicated)[0];
const reader = (id: string, device?: string): ((t: number) => number) => {
  const g = gpuOf(device);
  const read: Record<string, (t: number) => number> = {
    cpu: (t) => sampleAt(t).cpu,
    temp: (t) => 62 + sampleAt(t).cpu * 0.32,
    clock: (t) => 2.5 + sampleAt(t).cpu * 0.035,
    power: (t) => 28 + sampleAt(t).cpu * 0.82,
    memoryPercent: (t) => sampleAt(t).mem,
    mem: (t) => (sampleAt(t).mem * 32 * GB) / 100,
    gpu: (t) => gpuAt(g.index, t).util,
    gpuTemp: (t) => gpuAt(g.index, t).temperature,
    gpuMem: (t) => (gpuAt(g.index, t).dedicated / g.dedicated) * 100,
    gpuClock: (t) => gpuAt(g.index, t).clock / 1000,
    gpuPower: (t) => gpuAt(g.index, t).power,
    gpuFan: (t) => 1350 + gpuAt(g.index, t).util * 5,
    disk: (t) => diskOf(device, t).read,
    diskActive: (t) => diskOf(device, t).active,
    diskTemp: (t) => diskOf(device, t).temperature,
    net: (t) => nicOf(device, t).rx,
    battery: () => 84,
  };
  return read[id] ?? (() => 0);
};
const second = (id: string, device?: string) => (t: number) => (id === "disk" ? diskOf(device, t).write : nicOf(device, t).tx);
// A GPU's name without its maker's marks where a model number stays (monitors.rs `gpu_name`).
const gpuName = (name: string) => {
  let out = name.replace(/((R|TM|tm))/g, "").split(/s+/).filter(Boolean).join(" ");
  for (const maker of ["NVIDIA ", "GeForce ", "AMD ", "Radeon ", "Intel "])
    if (out.startsWith(maker) && /d/.test(out.slice(maker.length))) out = out.slice(maker.length);
  return out;
};
// What a device is called before its readings on the desktop monitor, and on the taskbar.
function deviceNames(id: string, device: string) {
  const disk = disks.find((d) => d.id === device);
  if (disk) return { name: disk.volumes.join(" "), short: disk.volumes[0] };
  const nic = adapters.find((a) => a.id === device);
  if (nic) return { name: nic.name, short: nic.name.length > 6 ? nic.name.slice(0, 5) + "…" : nic.name };
  const g = gpuOf(device);
  return { name: gpuName(g.model), short: "GPU" + (g.index + 1) };
}
// A reading's gauge at `tick` (monitors.rs `gauges`): "metric" or "metric@device".
export function gauge(key: string, tick: number): Gauge {
  const [id, device] = key.split("@");
  const m = metric[id],
    get = reader(id, device),
    v = get(tick);
  const paired = id === "disk" || id === "net";
  const named = device ? deviceNames(id, device) : null;
  const base = {
    id: key,
    name: named ? (paired ? named.name : fill("{device} {metric}", { device: named.name, metric: translate(m.option) })) : m.name,
    option: m.option,
    short: named ? named.short : m.short,
    family: m.family,
    device: device ?? "",
  };
  const one = (text: string, widest: string, fraction: number | null, high = false) => ({
    ...base,
    parts: [{ mark: "", text, widest }],
    fraction,
    bar: fraction !== null,
    high,
  });
  const history = (read: (t: number) => number, scale: (v: number) => number) =>
    Array.from({ length: 60 }, (_, i) => Math.min(1, Math.max(0, scale(read(tick - 59 + i)))));
  const peak = (read: (t: number) => number) => Math.max(...Array.from({ length: 60 }, (_, i) => read(tick - 59 + i)), 1) * 1.1;
  switch (id) {
    case "cpu":
    case "memoryPercent":
    case "gpu":
    case "gpuMem":
    case "diskActive":
    case "battery":
      return { ...one(Math.round(v) + "%", "100%", v / 100, id !== "battery" && Math.round(v) >= 80), trend: [history(get, (x) => x / 100)] };
    case "temp":
    case "gpuTemp":
    case "diskTemp":
      return { ...one(Math.round(v) + " °C", "100 °C", v / 100, Math.round(v) >= (id === "diskTemp" ? 70 : 80)), trend: [history(get, (x) => x / 100)] };
    case "mem":
      return { ...one((v / GB).toFixed(1) + " GB", "32.0 GB", v / (32 * GB), v / (32 * GB) >= 0.8), trend: [history(get, (x) => x / (32 * GB))] };
    case "clock":
    case "gpuClock":
      return { ...one(v.toFixed(2) + " GHz", "8.88 GHz", null), trend: [history(get, (x) => x / (peak(get) * 1.05))] };
    case "power":
    case "gpuPower":
      return { ...one(watts(v), "888 W", null), trend: [history(get, (x) => x / peak(get))] };
    case "gpuFan":
      return { ...one(Math.round(v) + " RPM", "8888 RPM", null), trend: [history(get, (x) => x / peak(get))] };
    case "disk":
    case "net": {
      const other = second(id, device),
        top = Math.max(peak(get), peak(other));
      const marks = id === "disk" ? ["读", "写"] : ["↓", "↑"];
      return {
        ...base,
        parts: [
          { mark: marks[0], text: rateText(v), widest: "88.8 MB/s" },
          { mark: marks[1], text: rateText(other(tick)), widest: "88.8 MB/s" },
        ],
        fraction: null,
        bar: false,
        high: false,
        trend: [history(get, (x) => x / top), history(other, (x) => x / top)],
      };
    }
    default:
      return { ...one("5 天 14 小时", "88 天 88 小时", null), trend: null };
  }
}
// The readings in turn that are of one device: the groups the monitor shows.
export function groupsOf(list: Gauge[]) {
  const out: Gauge[][] = [];
  for (const g of list) {
    const last = out.at(-1);
    if (last && g.family !== 6 && last[0].family === g.family && last[0].device === g.device) last.push(g);
    else out.push([g]);
  }
  return out;
}
const tone = (g: Gauge) => "var(--" + (g.family === 6 ? "a-ink" : "w-" + tones[g.family]) + ")";
// A value: figures, then its unit smaller and quieter, on one baseline (desk.rs `value`).
function pieces(text: string) {
  const out: [string, boolean][] = [];
  for (const run of text.match(/[\d.,:\-—]+|[^\d\s.,:\-—]+(?: [^\d\s]+)*/g) ?? []) out.push([run, !/^[\d.,:\-—]+$/.test(run)]);
  return out;
}
function Value({ text, size, high, widest, whole }: { text: string; size: [number, number]; high?: boolean; widest?: string; whole?: boolean }) {
  const draw = (t: string) =>
    (whole ? ([[t, false]] as [string, boolean][]) : pieces(t)).map(([piece, unit], i) => (
      <span key={i} className={unit ? "w-unit" : "w-fig"} style={{ fontSize: unit ? size[1] : size[0] }}>
        {piece}
      </span>
    ));
  return (
    <span className="w-value" data-high={high || undefined}>
      <span>{draw(text)}</span>
      {widest && <span aria-hidden="true">{draw(widest)}</span>}
    </span>
  );
}
const BIG: [number, number] = [20, 12.5],
  RATE: [number, number] = [16, 11.5],
  TALL: [number, number] = [17, 12],
  SMALL: [number, number] = [14, 11.5],
  KEYED: [number, number] = [13.5, 11.5];
function Rate({ g, part, size }: { g: Gauge; part: Part; size: [number, number] }) {
  return (
    <span className="w-rate">
      <em style={{ color: tone(g) }}>{part.mark}</em>
      <Value text={part.text} size={size} widest={part.widest} />
    </span>
  );
}
function Bar({ g, height }: { g: Gauge; height: number }) {
  return (
    <i className="w-bar" style={{ height, borderRadius: height / 2 }}>
      <i style={{ width: (g.fraction ?? 0) * 100 + "%", background: g.high ? "var(--w-high)" : tone(g) }} />
    </i>
  );
}
function Stats({ list, columns }: { list: Gauge[]; columns: number }) {
  return (
    <div className="w-stats" style={{ gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))` }}>
      {list.map((g) => (
        <div key={g.id}>
          <small>{g.option}</small>
          <span>
            {g.parts.length === 1 ? (
              <Value text={g.parts[0].text} size={SMALL} high={g.high} />
            ) : (
              g.parts.map((p) => <Rate key={p.mark} g={g} part={p} size={SMALL} />)
            )}
          </span>
        </div>
      ))}
    </div>
  );
}
// A curve over its box: grid lines at a quarter, half and three quarters; the
// first series a line over a fading area, a pair's second a fainter line.
function Plot({ trend, color }: { trend: number[][]; color: string }) {
  const at = (values: number[]) =>
    values.map((v, i) => `${((i + 60 - values.length) / 59) * 100},${(1 + 98 * (1 - Math.min(1, Math.max(0, v)))).toFixed(2)}`).join(" ");
  const id = "wg" + useId().replace(/:/g, "");
  return (
    <svg className="w-plot" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
      <defs>
        <linearGradient id={id} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor={color} stopOpacity="0.26" />
          <stop offset="1" stopColor={color} stopOpacity="0.04" />
        </linearGradient>
      </defs>
      {[25, 50, 75].map((y) => (
        <line key={y} x1="0" x2="100" y1={y} y2={y} stroke="var(--a-stroke)" vectorEffect="non-scaling-stroke" />
      ))}
      <polygon points={`0,100 ${at(trend[0])} 100,100`} fill={`url(#${id})`} />
      {trend.map((values, i) => (
        <polyline
          key={i}
          points={at(values)}
          fill="none"
          stroke={color}
          strokeOpacity={i ? 0.55 : 0.95}
          strokeWidth={i ? 1.5 : 2}
          vectorEffect="non-scaling-stroke"
        />
      ))}
    </svg>
  );
}
// A group of the list: its first reading's name, bar and value; a row with a
// bar for each other reading that has a scale; the rest in three columns.
function ListGroup({ list }: { list: Gauge[] }) {
  const [first, ...others] = list;
  const rows = others.filter((g) => g.bar && g.parts.length === 1),
    rest = others.filter((g) => !(g.bar && g.parts.length === 1));
  return (
    <div className="w-list">
      {first.parts.length === 2 ? (
        <div className="w-line is-head">
          <span className="w-name">{first.name}</span>
          {first.parts.map((p) => (
            <Rate key={p.mark} g={first} part={p} size={RATE} />
          ))}
        </div>
      ) : (
        <div className="w-line is-head">
          <span className="w-name">{first.name}</span>
          {first.bar ? <Bar g={first} height={6} /> : <i className="w-fill" />}
          <span className="w-end">
            <Value text={first.parts[0].text} size={BIG} high={first.high} />
          </span>
        </div>
      )}
      {rows.map((g) => (
        <div className="w-line" key={g.id}>
          <span className="w-name">{g.option}</span>
          <Bar g={g} height={5} />
          <span className="w-end">
            <Value text={g.parts[0].text} size={SMALL} high={g.high} />
          </span>
        </div>
      ))}
      {rest.length > 0 && <Stats list={rest} columns={3} />}
    </div>
  );
}
function CurveGroup({ list }: { list: Gauge[] }) {
  const [first, ...others] = list;
  return (
    <div className="w-curves">
      <div className="w-curve-text">
        {first.parts.length === 2 ? (
          <div className="w-curve-head is-pair">
            <span className="w-name">{first.name}</span>
            <span>
              {first.parts.map((p) => (
                <Rate key={p.mark} g={first} part={p} size={RATE} />
              ))}
            </span>
          </div>
        ) : (
          <div className="w-curve-head">
            <span className="w-name">{first.name}</span>
            <Value text={first.parts[0].text} size={BIG} high={first.high} />
          </div>
        )}
        {others.map((g) => (
          <div className="w-keyed" key={g.id}>
            <span className="w-name">{g.option}</span>
            <span>
              {g.parts.length === 1 ? (
                <Value text={g.parts[0].text} size={KEYED} high={g.high} />
              ) : (
                g.parts.map((p) => <Rate key={p.mark} g={g} part={p} size={KEYED} />)
              )}
            </span>
          </div>
        ))}
      </div>
      {first.trend && (
        <div className="w-curve-box">
          <Plot trend={first.trend} color={tone(first)} />
        </div>
      )}
    </div>
  );
}
// A ring 46 px across filled clockwise from the top.
function Ring({ g }: { g: Gauge }) {
  const r = 20,
    c = 2 * Math.PI * r,
    f = Math.min(1, Math.max(0, g.fraction ?? 0));
  return (
    <svg className="w-ring" width="46" height="46" viewBox="0 0 46 46" aria-hidden="true">
      <circle cx="23" cy="23" r={r} fill="none" stroke="var(--a-stroke-strong)" strokeWidth="6" />
      {f > 0 && (
        <circle
          cx="23"
          cy="23"
          r={r}
          fill="none"
          stroke={g.high ? "var(--w-high)" : tone(g)}
          strokeWidth="6"
          strokeLinecap="round"
          strokeDasharray={`${c * f} ${c}`}
          transform="rotate(-90 23 23)"
        />
      )}
    </svg>
  );
}
function Tile({ list, wide }: { list: Gauge[]; wide: boolean }) {
  const [first, ...rest] = list;
  return (
    <div className="w-tile" data-wide={wide || undefined}>
      {first.parts.length === 2 ? (
        <div className="w-tile-pair">
          <span className="w-name">{first.name}</span>
          <div>
            {first.parts.map((p) => (
              <span key={p.mark}>
                <Rate g={first} part={p} size={TALL} />
              </span>
            ))}
          </div>
        </div>
      ) : (
        <div className="w-tile-top">
          {first.bar && <Ring g={first} />}
          <div>
            <span className="w-name">{first.name}</span>
            <span className="w-big">
              <Value text={first.parts[0].text} size={BIG} high={first.high} />
            </span>
          </div>
        </div>
      )}
      {rest.length > 0 && <Stats list={rest} columns={wide ? 4 : 2} />}
    </div>
  );
}
// The strip: a meter for each group's first reading, then a name over a value for each.
function Strip({ groups }: { groups: Gauge[][] }) {
  return (
    <div className="w-strip">
      {groups.map((list) => (
        <div className="w-strip-group" key={list[0].id}>
          {list[0].bar && (
            <i className="w-meter">
              <i style={{ height: (list[0].fraction ?? 0) * 100 + "%", background: list[0].high ? "var(--w-high)" : tone(list[0]) }} />
            </i>
          )}
          {list.flatMap((g, at) => {
            const title = at === 0 ? g.name : g.option;
            return g.parts.length === 1
              ? [
                  <span className="w-cell" key={g.id}>
                    <small>{title}</small>
                    <Value text={g.parts[0].text} size={TALL} high={g.high} widest={g.parts[0].widest} />
                  </span>,
                ]
              : g.parts.map((p, which) => (
                  <span className="w-cell" key={g.id + p.mark}>
                    <small>
                      {which === 0 && title}
                      <em style={{ color: tone(g) }}>{p.mark}</em>
                    </small>
                    <Value text={p.text} size={TALL} widest={p.widest} />
                  </span>
                ));
          })}
        </div>
      ))}
    </div>
  );
}
type Cell = { label: string; name: string; run: number; value: string; widest: string; high: boolean };
type TaskGroup = { cells: Cell[]; tone: string; history: number[] };
// The taskbar's cells: a single value under its short name, a pair of rates
// under their marks; a CPU or GPU reading after another of its device also
// has its parameter's name (monitors.rs `taskbar_groups`).
function taskGroups(list: Gauge[]): TaskGroup[] {
  let run = 0;
  return list.map((g, index) => {
    const before = list[index - 1];
    const follows = (g.family === 0 || g.family === 2) && !!before && before.family === g.family;
    if (!follows) run = index + 1;
    return {
      tone: tone(g),
      history: g.trend?.[0] ?? [],
      cells:
        g.parts.length === 1
          ? [{ label: g.short, name: follows ? (metric[g.id].tag ?? "") : "", run, value: g.parts[0].text, widest: g.parts[0].widest, high: g.high }]
          : g.parts.map((p) => ({ label: g.short + p.mark, name: "", run, value: p.text, widest: p.widest, high: false })),
    };
  });
}
// Columns of two rows: a group's cells stay together, single cells share a column.
function stacks(groups: TaskGroup[]) {
  const columns: [Cell, TaskGroup][][] = [];
  let open: number | null = null;
  for (const g of groups) {
    if (g.cells.length === 1) {
      if (open !== null) {
        columns[open].push([g.cells[0], g]);
        open = null;
      } else {
        open = columns.length;
        columns.push([[g.cells[0], g]]);
      }
    } else for (let i = 0; i < g.cells.length; i += 2) columns.push(g.cells.slice(i, i + 2).map((c) => [c, g] as [Cell, TaskGroup]));
  }
  return columns;
}
const tag = (c: Cell) => c.name || c.label;
function Room({ value, widest, size, unit, high }: { value: string; widest: string; size: number; unit?: number; high?: boolean }) {
  return <Value text={value} size={[size, unit ?? size]} high={high} widest={widest} whole={unit === undefined} />;
}
function TaskFace({ style, groups }: { style: string; groups: TaskGroup[] }) {
  if (style === "曲线")
    return (
      <>
        {groups.map((g, n) => (
          <span className="w-tb-tile" key={n} style={{ "--tone": g.tone } as CSSProperties} data-pair={g.cells.length > 1 || undefined}>
            {g.cells.length === 1 ? (
              <span className="w-tb-row">
                <span className="w-tb-name">{tag(g.cells[0])}</span>
                <Room value={g.cells[0].value} widest={g.cells[0].widest} size={14} unit={12} high={g.cells[0].high} />
              </span>
            ) : (
              g.cells.map((c) => (
                <span className="w-tb-row" key={c.label}>
                  <em>{c.label}</em>
                  <Room value={c.value} widest={c.widest} size={12} />
                </span>
              ))
            )}
            {g.history.length > 1 && <TaskCurve values={g.history} />}
          </span>
        ))}
      </>
    );
  if (style === "胶囊")
    return (
      <>
        {groups.map((g, n) => (
          <span className="w-tb-pill" key={n} style={{ "--tone": g.tone } as CSSProperties}>
            {g.cells.length === 1 ? (
              <>
                <i />
                <span className="w-tb-name">{tag(g.cells[0])}</span>
                <Room value={g.cells[0].value} widest={g.cells[0].widest} size={14} unit={12} high={g.cells[0].high} />
              </>
            ) : (
              g.cells.map((c) => (
                <span className="w-tb-rate" key={c.label}>
                  <em>{c.label}</em>
                  <Room value={c.value} widest={c.widest} size={14} unit={12} />
                </span>
              ))
            )}
          </span>
        ))}
      </>
    );
  if (style === "大数字")
    return (
      <>
        {groups.map((g, n) => (
          <span className="w-tb-figure" key={n} style={{ "--tone": g.tone } as CSSProperties} data-pair={g.cells.length > 1 || undefined}>
            {g.cells.length === 1 ? (
              <>
                <em>{tag(g.cells[0])}</em>
                <Room value={g.cells[0].value} widest={g.cells[0].widest} size={20} unit={12} high={g.cells[0].high} />
              </>
            ) : (
              g.cells.map((c) => (
                <span className="w-tb-row" key={c.label}>
                  <em>{c.label}</em>
                  <Room value={c.value} widest={c.widest} size={14} unit={12} />
                </span>
              ))
            )}
          </span>
        ))}
      </>
    );
  return (
    <>
      {stacks(groups).map((column, n) => (
        <span className="w-tb-column" key={n}>
          {column.map(([c], row) => {
            const before = row > 0 ? column[row - 1][0] : null;
            const shown = before && before.run === c.run ? tag(c) : c.label;
            return (
              <span className="w-tb-row" key={c.label + row}>
                {shown && <span className="w-tb-name">{shown}</span>}
                <Room value={c.value} widest={c.widest} size={15} high={c.high} />
              </span>
            );
          })}
        </span>
      ))}
    </>
  );
}
function TaskCurve({ values }: { values: number[] }) {
  const pts = values.map((v, i) => `${(i / (values.length - 1)) * 100},${(100 - Math.min(1, Math.max(0, v)) * 100).toFixed(2)}`).join(" ");
  return (
    <svg className="w-tb-curve" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
      <polygon points={`0,100 ${pts} 100,100`} fill="var(--tone)" fillOpacity="0.22" />
      <polyline points={pts} fill="none" stroke="var(--tone)" strokeOpacity="0.9" strokeWidth="1" vectorEffect="non-scaling-stroke" />
    </svg>
  );
}
export function MiniCurve({ id, tick }: { id: string; tick: number }) {
  const m = byMetric[id],
    vals = Array.from({ length: 36 }, (_, i) => m.read(tick + i - 35));
  const max = m.unit === "%" ? 100 : Math.max(...vals, 1) * 1.08;
  const path = vals.map((v, i) => i * 2 + "," + (22 - (21 * v) / max).toFixed(1)).join(" ");
  return (
    <svg className="w-mini" viewBox="0 0 70 22" preserveAspectRatio="none" aria-hidden="true">
      <polygon points={"0,22 " + path + " 70,22"} fill="currentColor" opacity=".15" />
      <polyline points={path} fill="none" stroke="currentColor" strokeWidth="1.3" vectorEffect="non-scaling-stroke" />
    </svg>
  );
}
export function MonitorFace({
  kind,
  style,
  tick,
  preview = false,
}: {
  kind: MonitorKind;
  style: string;
  tick: number;
  preview?: boolean;
}) {
  const { widgets } = useShared();
  const cfg = widgets[kind];
  let ids = cfg.order.filter((id) => cfg.keys.includes(id) && metric[id]);
  // The taskbar's previews show its first two readings (monitors.rs `taskbar_previews`).
  if (preview && kind === "taskbar") ids = ids.length ? ids.slice(0, 2) : ["cpu", "memoryPercent"];
  const list = ids.map((id) => gauge(id, tick));
  const groups = groupsOf(list);
  const grouping = { 卡片: "cards", 色条: "stripes", 分隔线: "rules" }[widgets.desktop.grouping] ?? "cards";
  return (
    <div className="w-face" data-kind={kind} data-style={styleKeys[style]} data-grouping={kind === "desktop" ? grouping : undefined}>
      {kind === "taskbar" ? (
        <TaskFace style={style} groups={taskGroups(list)} />
      ) : style === "横条" ? (
        <Strip groups={groups} />
      ) : style === "仪表" ? (
        groups.map((g, n) => <Tile key={g[0].id} list={g} wide={n === groups.length - 1 && groups.length % 2 === 1} />)
      ) : (
        groups.map((g) => (
          <div className="w-group-box" key={g[0].id} style={{ "--stripe": tone(g[0]) } as CSSProperties}>
            {style === "曲线" ? <CurveGroup list={g} /> : <ListGroup list={g} />}
          </div>
        ))
      )}
    </div>
  );
}
export function Styles({ kind, tick = 0 }: { kind: MonitorKind; tick?: number }) {
  const cfg = useShared().widgets[kind];
  return (
    <div className="w-styles">
      {(kind === "desktop" ? desktopStyles : taskbarStyles).map((s) => (
        <button type="button" key={s} aria-pressed={cfg.style === s} onClick={() => setMonitor(kind, { style: s })}>
          <span className="w-preview">
            <MonitorFace kind={kind} style={s} tick={tick} preview />
          </span>
          <strong>
            <i />
            {s}
          </strong>
        </button>
      ))}
    </div>
  );
}
// The readings checklist (components.rs `metric_card`): a block per kind, a row
// per device where there are several; in the taskbar monitor's panel, a kind's
// devices as tabs, or as its options when each has one reading.
const groupTones: Record<string, string> = { CPU: "cpu", 内存: "mem", GPU: "gpu", 磁盘: "io", 网络: "net", 其他: "bat" };
export function Readings({ kind, compact = false }: { kind: MonitorKind; compact?: boolean }) {
  const cfg = useShared().widgets[kind];
  const [lifted, setLifted] = useState<string | null>(null);
  const [tabs, setTabs] = useState<Record<string, number>>({});
  const ordered = [...monitorGroups].sort(
    (a, b) => Math.min(...a.keys.map((k) => cfg.order.indexOf(k))) - Math.min(...b.keys.map((k) => cfg.order.indexOf(k))),
  );
  const reorder = (move: string[], before: string) => {
    const next = cfg.order.filter((k) => !move.includes(k));
    next.splice(next.indexOf(before), 0, ...move);
    setMonitor(kind, { order: next });
  };
  const drop = (handler: (e: DragEvent) => void) => ({
    onDragOver: (e: DragEvent) => e.preventDefault(),
    onDrop: (e: DragEvent) => {
      setLifted(null);
      handler(e);
    },
  });
  const option = (keys: string[], id: string, text: string, title?: string): ReactNode => {
    const on = cfg.keys.includes(id);
    const sortable = keys.length > 1;
    return (
      <label
        key={id}
        title={title || undefined}
        draggable={sortable}
        data-lifted={lifted === id || undefined}
        data-on={on || undefined}
        onDragStart={(e) => {
          e.stopPropagation();
          e.dataTransfer.setData("argus-metric", id);
          setLifted(id);
        }}
        onDragEnd={() => setLifted(null)}
        {...drop((e) => {
          const from = e.dataTransfer.getData("argus-metric");
          if (keys.includes(from) && from !== id) reorder([from], id);
        })}
      >
        <input
          type="checkbox"
          checked={on}
          onChange={(e) => setMonitor(kind, { keys: e.target.checked ? [...cfg.keys, id] : cfg.keys.filter((k) => k !== id) })}
        />
        {compact && (
          <i className="w-chip-box">
            <Icon name="check" size={11} />
          </i>
        )}
        {text}
      </label>
    );
  };
  const options = (keys: string[]) => cfg.order.filter((id) => keys.includes(id)).map((id) => option(keys, id, metric[id.split("@")[0]].option));
  const grip = (g: (typeof monitorGroups)[number]) => ({
    draggable: true,
    onDragStart: (e: DragEvent) => {
      e.dataTransfer.setData("argus-category", g.name);
      setLifted(g.name);
    },
    onDragEnd: () => setLifted(null),
  });
  return (
    <div className="w-readings">
      {ordered.map((g) => {
        const block = {
          className: "w-group",
          "data-lifted": lifted === g.name || undefined,
          style: { "--tone": "var(--w-" + groupTones[g.name] + ")" } as CSSProperties,
          ...drop((e) => {
            const source = ordered.find((o) => o.name === e.dataTransfer.getData("argus-category"));
            if (source && source !== g)
              reorder(
                cfg.order.filter((k) => source.keys.includes(k)),
                cfg.order.find((k) => g.keys.includes(k))!,
              );
          }),
        };
        if (!compact)
          return (
            <div key={g.name} {...block}>
              {g.rows.map((row, n) => (
                <div className="w-row" key={row.title}>
                  {n === 0 ? (
                    <span className="w-grip" {...grip(g)}>
                      <Icon name="grip" size={14} />
                    </span>
                  ) : (
                    <span />
                  )}
                  <span className="w-name">
                    <b>{row.title}</b>
                    {row.detail && <small>{row.detail}</small>}
                  </span>
                  <div className="w-options">{options(row.keys)}</div>
                </div>
              ))}
            </div>
          );
        const single = g.rows.length > 1 && g.rows.every((row) => row.keys.length === 1);
        const at = Math.min(tabs[g.name] ?? 0, g.rows.length - 1);
        return (
          <div key={g.name} {...block}>
            <span className="w-grip" {...grip(g)}>
              <i />
              <b>{g.name}</b>
            </span>
            <div className="w-kind">
              {g.rows.length > 1 && !single && (
                <div className="w-tabs" role="tablist">
                  {g.rows.map((row, n) => (
                    <button
                      type="button"
                      role="tab"
                      key={row.title}
                      title={row.detail || undefined}
                      aria-selected={n === at}
                      data-chosen={row.keys.some((k) => cfg.keys.includes(k)) || undefined}
                      onClick={() => setTabs((t) => ({ ...t, [g.name]: n }))}
                    >
                      {row.title}
                    </button>
                  ))}
                </div>
              )}
              <div className="w-options">
                {single
                  ? g.rows.map((row) => option(g.keys, row.keys[0], row.title, row.detail))
                  : options(g.rows[g.rows.length > 1 ? at : 0].keys)}
              </div>
            </div>
          </div>
        );
      })}
    </div>
  );
}
