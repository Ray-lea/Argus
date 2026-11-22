/** @jsxImportSource @/components/replica/i18n */
"use client";
import { useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { store, useShared } from "./store";
import { Button, Segments, Select } from "./widgets";
import { Icon, ProcessIcon } from "./icons";
import { memory, processes, readings, heat, percent, type Process } from "./model";
import { asleep, clock, durationLabel, fill, momentLabel, monthDay, rangeLabel, type Span } from "./historyTime";
import { byteRate, thousands, type ProcessKey } from "./metrics";
import type { MenuEntry } from "./Pages";
import { company } from "./Columns";
import { processValue } from "./HistoryPlot";

// A process's private working set and commit over the watch, for its card (process_watch.rs).
const watched = (p: Process, t: number) => {
  const r = readings(t, [p]).get(p.pid)!;
  return { memory: r.mem, commit: r.mem * 1.18 + 24 * 1024 ** 2 };
};
function WatchChart({ series, start, end }: { series: { color: string; values: number[] }[]; start: number; end: number }) {
  const top = Math.max(1024 ** 2, ...series.flatMap((s) => s.values)) * 1.1;
  const path = (values: number[]) =>
    values.map((v, i) => (i ? "L" : "M") + ((i / Math.max(1, values.length - 1)) * 1000).toFixed(1) + " " + (100 - (v / top) * 100).toFixed(2)).join("");
  return (
    <div className="h-watch-chart">
      <svg viewBox="0 0 1000 100" preserveAspectRatio="none" aria-hidden="true">
        {[25, 50, 75].map((y) => (
          <path key={y} d={"M0 " + y + "H1000"} stroke="var(--a-stroke)" vectorEffect="non-scaling-stroke" />
        ))}
        {series.map((s) => (
          <path key={s.color} d={path(s.values)} fill="none" stroke={s.color} strokeWidth="1.5" vectorEffect="non-scaling-stroke" />
        ))}
      </svg>
      <small>{clock(start, false)}</small>
      <small>{clock(end, false)}</small>
    </div>
  );
}
export function WatchPanel({ tick, alive }: { tick: number; alive: Process[] }) {
  const { watch } = useShared();
  const [run, setRun] = useState<number | null>(null);
  if (!watch) return null;
  const first = processes.find((p) => p.pid === watch.pid)!;
  // Every run of the same image path is recorded on its own (process_watch.rs).
  const runs = processes.filter((proc) => proc.path === first.path);
  const p = runs.find((r) => r.pid === run) ?? first;
  const exited = !alive.some((v) => v.pid === p.pid);
  const until = watch.stop ?? tick;
  const active = watch.stop === null && !exited;
  const state = (proc: Process) =>
    !alive.some((v) => v.pid === proc.pid) ? "此运行实例已结束" : watch.stop !== null ? "已停止记录" : "正在记录";
  const count = Math.min(120, Math.max(2, until - watch.start + 1));
  const points = Array.from({ length: count }, (_, i) =>
    watched(p, watch.start + Math.round(((until - watch.start) * i) / (count - 1))),
  );
  const last = points.at(-1)!;
  return (
    <div className="h-watch">
      <header>
        <b>{`观察 ${p.exe}（PID ${p.pid}）`}</b>
        <small>{watch.stop === null && !runs.some((r) => alive.some((v) => v.pid === r.pid)) ? "等待后续运行" : state(p)}</small>
        <Button small disabled={!active} onClick={() => store.set({ watch: { ...watch, stop: tick } })}>
          停止记录
        </Button>
        <Button small onClick={() => store.set({ watch: null })}>
          删除记录
        </Button>
      </header>
      <div className="h-watch-legend">
        <span style={{ color: "var(--a-mem)" }}>{`专用工作集: ${memory(last.memory)}`}</span>
        <span style={{ color: "var(--a-cpu)" }}>{`专用提交量: ${memory(last.commit)}`}</span>
      </div>
      <>
          <Select
            label="选择运行实例"
            value={p.pid}
            onChange={setRun}
            options={runs.map((r) => ({ value: r.pid, label: `${r.started} · PID ${r.pid} · ${state(r)}` }))}
          />
          <div className="h-watch-row">
            <Button small disabled>
              更早的实例
            </Button>
            <Button small disabled>
              最新实例
            </Button>
          </div>
          <small className="h-watch-path">{p.path}</small>
          <div className="h-watch-row">
            <small>{`显示 ${runs.length}/${runs.length} 个实例；所选实例共 ${points.length} 个记录点`}</small>
            <Button small disabled>
              更早的记录
            </Button>
            <Button small disabled>
              最新记录
            </Button>
          </div>
      </>
      <WatchChart
        start={watch.start}
        end={until}
        series={[
          { color: "var(--a-mem)", values: points.map((v) => v.memory) },
          { color: "var(--a-cpu)", values: points.map((v) => v.commit) },
        ]}
      />
      <small className="h-watch-note">
        同一路径的各次运行分别记录。最多保留 7 天，达到容量上限时提前清理最旧记录。停止后保留数据，删除会结束观察并清除记录。重启 Argus 后需重新开始观察。
      </small>
    </div>
  );
}
// The recorded frames read lately (every process's readings at a second), kept so
// that the trends and ranges of a window moving with the clock read each only once.
const kept = new Map<number, ReturnType<typeof readings>>();
function frameAt(t: number) {
  let frame = kept.get(t);
  if (!frame) {
    frame = readings(t, processes);
    kept.set(t, frame);
    if (kept.size > 256) kept.delete(kept.keys().next().value as number);
  }
  return frame;
}
// The rows drawn: those in the body's viewport and a few either side, as the
// app's list lays out only the rows it shows; spacers of the same height stand
// for the rest. Rows are all one height (`.a-busy-card .a-tr`).
const ROW = 28,
  SPARE = 8;
type Line = { draw: () => ReactNode };
function useRowWindow() {
  const body = useRef<HTMLDivElement>(null);
  const [port, setPort] = useState({ first: 0, count: 24 });
  useLayoutEffect(() => {
    const el = body.current!;
    const update = () => {
      const first = Math.floor(el.scrollTop / ROW),
        count = Math.ceil(el.clientHeight / ROW) + 1;
      setPort((p) => (p.first === first && p.count === count ? p : { first, count }));
    };
    // Drawn before the frame the scroll shows, so that no row is missing from it.
    const scrolled = () => flushSync(update);
    update();
    const ro = new ResizeObserver(update);
    ro.observe(el);
    el.addEventListener("scroll", scrolled, { passive: true });
    return () => {
      ro.disconnect();
      el.removeEventListener("scroll", scrolled);
    };
  }, []);
  return [body, port] as const;
}
type Kind = "percent" | "bytes" | "rate" | "total" | "power" | "count" | "duration";
type Column = { key: string; kind: Kind; width: number; shown: boolean; additive: boolean; heat?: "percent" | "memory" | "rate" };
const column = (key: string, kind: Kind, width: number, shown: boolean, additive: boolean): Column => ({
  key,
  kind,
  width,
  shown,
  additive,
  heat: kind === "percent" ? "percent" : kind === "bytes" ? "memory" : kind === "rate" || kind === "total" ? "rate" : undefined,
});
// The columns of each mode (table.rs MOMENT and RANGE).
const MOMENT = [
  column("cpu", "percent", 64, true, true),
  column("mem", "bytes", 84, true, true),
  column("disk", "rate", 84, true, true),
  column("diskRead", "rate", 84, false, true),
  column("diskWrite", "rate", 84, false, true),
  column("io", "rate", 80, false, true),
  column("net", "rate", 84, true, true),
  column("gpu", "percent", 60, true, true),
  column("gpuMem", "bytes", 84, false, true),
  column("power", "power", 100, true, false),
  column("threads", "count", 64, false, true),
  column("handles", "count", 72, false, true),
];
const RANGE = [
  column("cpu", "percent", 82, true, true),
  column("cpuMax", "percent", 82, true, false),
  column("mem", "bytes", 86, true, false),
  column("disk", "total", 94, true, true),
  column("diskRead", "total", 94, false, true),
  column("diskWrite", "total", 94, false, true),
  column("io", "total", 84, false, true),
  column("net", "total", 94, true, true),
  column("gpu", "percent", 80, true, true),
  column("gpuMem", "bytes", 84, false, false),
  column("power", "power", 100, true, false),
  column("seen", "duration", 96, true, false),
  column("threads", "count", 64, false, false),
  column("handles", "count", 72, false, false),
];
const momentLabels: Record<string, string> = {
  cpu: "CPU",
  mem: "内存",
  disk: "磁盘",
  diskRead: "磁盘读取",
  diskWrite: "磁盘写入",
  io: "I/O",
  net: "网络",
  gpu: "GPU",
  gpuMem: "专用 GPU 内存",
  power: "电源使用情况",
  threads: "线程",
  handles: "句柄",
};
const rangeLabels: Record<string, string> = {
  cpu: "CPU 平均",
  cpuMax: "CPU 最高",
  mem: "内存最高",
  disk: "磁盘读写量",
  diskRead: "磁盘读取量",
  diskWrite: "磁盘写入量",
  io: "I/O 量",
  net: "网络收发量",
  gpu: "GPU 平均",
  gpuMem: "专用 GPU 内存最高",
  power: "电源使用情况",
  seen: "记录时长",
  threads: "线程",
  handles: "句柄",
};
const trendNames: Record<ProcessKey, string> = { cpu: "CPU", mem: "内存", disk: "磁盘", net: "网络", gpu: "GPU" };
const trendColors: Record<ProcessKey, string> = {
  cpu: "var(--a-cpu)",
  mem: "var(--a-mem)",
  disk: "var(--a-io)",
  net: "var(--a-net)",
  gpu: "var(--a-gpu)",
};
const sections = [
  ["app", "应用", "var(--a-group-apps)"],
  ["background", "后台进程", "var(--a-group-bg)"],
  ["windows", "Windows 进程", "var(--a-group-win)"],
] as const;
const POWER = ["非常低", "低", "中", "高", "非常高"];
type Row = { p: Process; values: Record<string, number>; trend: number[] };
// A process's trend: a small area scaled to its own highest value.
function Sparkline({ values, color }: { values: number[]; color: string }) {
  const top = Math.max(1e-9, ...values.filter(Number.isFinite));
  if (!values.some((v) => v > 0)) return <span className="a-td h-trend" />;
  const pts = values.map((v, i) => [(i / Math.max(1, values.length - 1)) * 1000, 100 - (Number.isFinite(v) ? v / top : 0) * 96]);
  const line = pts.map(([x, y], i) => (i ? "L" : "M") + x.toFixed(1) + " " + y.toFixed(1)).join("");
  return (
    <span className="a-td h-trend">
      <svg viewBox="0 0 1000 100" preserveAspectRatio="none" aria-hidden="true">
        <path d={line + "V100H0Z"} fill={color} opacity=".22" />
        <path d={line} fill="none" stroke={color} strokeWidth="1.3" vectorEffect="non-scaling-stroke" />
      </svg>
    </span>
  );
}
export function HistoryTable({
  program,
  onProgram,
  onMenu,
  span,
  view,
  trend,
  at,
  live,
  latest,
  mode,
  setMode,
  alive,
  onOpen,
  clearable,
  onClear,
}: {
  program: string | null;
  onProgram: (name: string | null) => void;
  onMenu: (event: React.MouseEvent, items: MenuEntry[]) => void;
  span: Span;
  view: Span;
  trend: ProcessKey;
  at: number;
  live: boolean;
  latest: boolean;
  mode: "moment" | "range";
  setMode: (m: "moment" | "range") => void;
  alive: Process[];
  onOpen: (pid: number) => void;
  clearable: boolean;
  onClear: () => void;
}) {
  const { recording } = useShared();
  const [body, port] = useRowWindow();
  const [query, setQuery] = useState(""),
    [sort, setSort] = useState("cpu"),
    [desc, setDesc] = useState(true);
  const [shownKeys, setShownKeys] = useState<Record<"moment" | "range", string[] | null>>({ moment: null, range: null }),
    [showTrend, setShowTrend] = useState(true),
    [open, setOpen] = useState<string[]>([]),
    [folded, setFolded] = useState<string[]>([]);
  const all = mode === "range" ? RANGE : MOMENT;
  const keys = shownKeys[mode] ?? all.filter((c) => c.shown).map((c) => c.key);
  const shown = all.filter((c) => keys.includes(c.key));
  const labels = mode === "range" ? rangeLabels : momentLabels;
  const sortKey = sort === "name" || shown.some((c) => c.key === sort) ? sort : "cpu";
  const [start, end] = span;
  const live_ = latest && mode === "moment";
  const seconds = Math.max(1, end - start);
  const rows = useMemo(() => {
    const pool = live_ ? alive : processes;
    // A range's samples on whole multiples of their spacing, so that a window
    // moving a second at a time reads the frames it read before (`frameAt`).
    const spacing = Math.max(1, Math.round((end - start) / 15));
    const samples =
      mode === "range" ? Array.from({ length: 16 }, (_, i) => Math.round((start + ((end - start) * i) / 15) / spacing) * spacing) : [at];
    const recorded = samples.filter((t) => !asleep(t));
    if (!recorded.length) return [] as Row[];
    const duration = (seconds * recorded.length) / samples.length;
    const out: Row[] = pool.map((p) => ({
      p,
      values: { cpu: 0, cpuMax: 0, mem: 0, disk: 0, diskRead: 0, diskWrite: 0, io: 0, net: 0, gpu: 0, gpuMem: 0, power: 0, seen: duration, threads: p.threads, handles: p.handles },
      trend: [],
    }));
    for (const t of recorded) {
      const data = live_ ? readings(t, pool) : frameAt(t);
      for (const row of out) {
        const r = data.get(row.p.pid)!,
          v = row.values,
          k = 1 / recorded.length;
        v.cpu += r.cpu * k;
        v.cpuMax = Math.max(v.cpuMax, r.cpu);
        v.mem = Math.max(v.mem, r.mem);
        v.gpuMem = Math.max(v.gpuMem, r.gpu * 0.018 * 1024 ** 3);
        v.diskRead += r.read * k;
        v.diskWrite += r.write * k;
        v.io += r.io * k;
        v.net += r.net * k;
        v.gpu += r.gpu * k;
        v.power = Math.max(v.power, r.cpu * 0.2 + r.gpu * 0.04 + Math.min(1.5, (r.read + r.write) / (512 * 1024 ** 2)));
      }
    }
    for (const { values: v } of out) {
      if (mode === "range") for (const key of ["net", "io", "diskRead", "diskWrite"]) v[key] *= duration;
      v.disk = v.diskRead + v.diskWrite;
    }
    // Trends: the window shown in moment mode, the range in range mode.
    const [a, b] = mode === "range" ? [start, end] : view;
    // Each point the mean of its stretch of the window, as the app's trend points
    // are: stretches on whole multiples of their length, read from the frames kept.
    const stretch = Math.max(1, Math.round((b - a) / 32)),
      first = Math.floor(a / stretch);
    const frames = Array.from({ length: 32 }, (_, i) =>
      [0.2, 0.5, 0.8].map((k) => Math.round((first + i + k) * stretch)).map((t) => (asleep(t) ? null : frameAt(t))),
    );
    for (const row of out)
      row.trend = frames.map((f) => {
        if (!f.every(Boolean)) return NaN;
        let n = 0;
        for (const frame of f) {
          const r = frame!.get(row.p.pid);
          if (r) n += processValue(r, trend);
        }
        return n / f.length;
      });
    return out;
  }, [mode, mode === "range" ? start : at, mode === "range" ? end : at, view[0], view[1], live_, alive, trend]);
  const additive = (c: Column) => c.additive;
  const aggregate = (list: Row[]) =>
    Object.fromEntries(
      shown.map((c) => [c.key, additive(c) ? list.reduce((s, r) => s + r.values[c.key], 0) : Math.max(...list.map((r) => r.values[c.key]))]),
    );
  const groupsByName = new Map<string, Row[]>();
  for (const row of rows) {
    if (!row.p.exe.toLowerCase().includes(query.trim().toLowerCase())) continue;
    const g = groupsByName.get(row.p.exe);
    if (g) g.push(row);
    else groupsByName.set(row.p.exe, [row]);
  }
  const sorted = [...groupsByName]
    .map(([name, list]) => ({
      name,
      rows: list,
      trend: list[0].trend.map((_, i) => list.reduce((n, r) => n + (Number.isFinite(r.trend[i]) ? r.trend[i] : 0), 0)),
      values: aggregate(list),
    }))
    .sort((a, b) => (sortKey === "name" ? a.name.localeCompare(b.name) : a.values[sortKey] - b.values[sortKey]) * (desc ? -1 : 1));
  const changeSort = (key: string) => {
    setDesc(key === sortKey ? !desc : key !== "name");
    setSort(key);
  };
  const display = (c: Column, v: number) => {
    switch (c.kind) {
      case "percent":
        return percent(v);
      case "bytes":
        return memory(v);
      case "rate":
        return v < 1 ? "0" : byteRate(v);
      case "total":
        return v < 512 ? "0" : memory(v);
      case "power":
        return POWER[[0.1, 0.3, 0.8, 2].filter((n) => v >= n).length];
      case "count":
        return thousands(v);
      default:
        return durationLabel(v);
    }
  };
  const cells = (data: Record<string, number>, heated = true) =>
    shown.map((c) => {
      const v = data[c.key];
      const intensity = heated && c.heat ? heat(c.heat, c.kind === "total" ? v / seconds : v) : 0;
      return (
        <span
          key={c.key}
          className="a-td is-end"
          style={intensity ? { background: "rgb(var(--a-heat) / " + intensity + ")" } : undefined}
        >
          {display(c, v)}
        </span>
      );
    });
  const canOpen = (pid: number) => {
    const p = alive.find((proc) => proc.pid === pid);
    if (!p) return false;
    const time = p.started.split(" ")[1]?.split(":").map(Number);
    return !!time && at >= time[0] * 3600 + time[1] * 60 + time[2] - (21 * 3600 + 36 * 60);
  };
  const [selected, setSelected] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const rowMenu = (event: React.MouseEvent, name: string, pids: number[]) => {
    event.preventDefault();
    const single = pids.length === 1 ? pids[0] : null;
    onMenu(event, [
      {
        label: program === name ? "不在图表上显示" : "在图表上显示曲线",
        checked: program === name,
        run: () => onProgram(program === name ? null : name),
      },
      { label: "转到进程", disabled: single === null || !canOpen(single), run: () => single !== null && onOpen(single) },
      { label: "复制名称与 PID", run: () => void navigator.clipboard?.writeText(`${name}（PID ${pids.join(", ")}）`) },
    ]);
  };
  const columnMenu = (button: HTMLElement) => {
    const r = button.getBoundingClientRect();
    onMenu({ clientX: r.left, clientY: r.bottom + 4 } as React.MouseEvent, [
      { label: mode === "range" ? "“区间”显示的列" : "“此刻”显示的列", disabled: true },
      ...all.map((c) => ({
        label: labels[c.key],
        checked: keys.includes(c.key),
        stay: true,
        run: () =>
          setShownKeys((s) => {
            const list = s[mode] ?? all.filter((o) => o.shown).map((o) => o.key);
            const next = list.includes(c.key) ? list.filter((k) => k !== c.key) : [...list, c.key];
            return { ...s, [mode]: next.length ? next : null };
          }),
      })),
      "-",
      { label: "趋势迷你曲线", checked: showTrend, stay: true, run: () => setShowTrend((v) => !v) },
      {
        label: "恢复默认列",
        run: () => {
          setShownKeys((s) => ({ ...s, [mode]: null }));
          setShowTrend(true);
        },
      },
    ]);
  };
  // Whose processes these are; filled after translating, as the two messages read alike.
  const title =
    mode === "range"
      ? fill("{range} 的进程", { range: rangeLabel(start, end, end - start < 3600) })
      : live_
        ? null
        : fill("{time} 的进程", { time: momentLabel(at) });
  const off = fill("历史记录已关闭。<link>打开设置</link>", {}).split(/<\/?link>/);
  const toggle = (name: string) => setOpen((old) => (old.includes(name) ? old.filter((k) => k !== name) : [...old, name]));
  const cols = "minmax(200px,1fr) " + (showTrend ? "96px " : "") + shown.map((c) => c.width + "px").join(" ");
  const groupLines = (g: (typeof sorted)[number]): Line[] => {
    const expandable = g.rows.length > 1,
      expanded = expandable && open.includes(g.name);
    const single = g.rows.length === 1 ? g.rows[0].p : null;
    let instances = g.rows;
    if (expanded && sortKey !== "name") instances = [...g.rows].sort((a, b) => b.values[sortKey] - a.values[sortKey]);
    const rest = instances.slice(4);
    const lines: Line[] = [
      {
        draw: () => (
          <div
            key={"g:" + g.name}
            className={"a-tr" + (selected === g.name ? " is-selected" : "")}
            onClick={() => setSelected(g.name)}
            onContextMenu={(e) => rowMenu(e, g.name, g.rows.map((r) => r.p.pid))}
            onDoubleClick={() => {
              if (expandable) toggle(g.name);
              else if (single && canOpen(single.pid)) onOpen(single.pid);
              else if (single) setToast(`进程 ${g.name}（PID ${single.pid}）的记录实例已结束，或无法确认其身份。`);
            }}
          >
            <div className="a-td h-name">
              {expandable ? (
                <button className="h-fold" aria-expanded={expanded} onClick={() => toggle(g.name)}>
                  <Icon name="chevronRight" size={13} />
                </button>
              ) : (
                <span className="h-fold" />
              )}
              <ProcessIcon glyph={g.rows[0].p.glyph} />
              <span className="h-label">{g.name}</span>
              <small>{expandable ? "×" + g.rows.length : `PID ${single!.pid}`}</small>
              {company(g.rows[0].p) && <small className="h-publisher">{company(g.rows[0].p)}</small>}
            </div>
            {showTrend && <Sparkline values={g.trend} color={trendColors[trend]} />}
            {cells(g.values)}
          </div>
        ),
      },
    ];
    if (!expanded) return lines;
    for (const r of instances.slice(0, 4))
      lines.push({
        draw: () => (
          <div
            className="a-tr h-instance"
            key={"i:" + r.p.pid}
            onContextMenu={(e) => rowMenu(e, g.name, [r.p.pid])}
            onDoubleClick={() => canOpen(r.p.pid) && onOpen(r.p.pid)}
          >
            <span className="a-td h-name">{`PID ${r.p.pid}`}</span>
            {showTrend && <Sparkline values={r.trend} color={trendColors[trend]} />}
            {cells(r.values)}
          </div>
        ),
      });
    if (rest.length > 0)
      lines.push({
        draw: () => (
          <div className="a-tr h-instance h-rest" key={"r:" + g.name}>
            <span className="a-td h-name">{`其余 ${rest.length} 个进程`}</span>
            {showTrend && <span className="a-td" />}
            {cells(aggregate(rest))}
          </div>
        ),
      });
    return lines;
  };
  const lines: Line[] =
    sortKey === "name"
      ? sections.flatMap(([key, name, tone]) => {
          const members = sorted.filter((g) => g.rows[0].p.group === key);
          if (!members.length) return [];
          const opened = !!query.trim() || !folded.includes(key);
          const head: Line = {
            draw: () => (
              <div
                key={"s:" + key}
                className="a-tr h-section"
                style={{ "--tone": tone } as CSSProperties}
                onClick={() => !query.trim() && setFolded((f) => (f.includes(key) ? f.filter((k) => k !== key) : [...f, key]))}
              >
                <div className="a-td h-name">
                  <span className="h-fold" data-off={!!query.trim() || undefined}>
                    <Icon name={opened ? "chevronDown" : "chevronRight"} size={13} />
                  </span>
                  <i />
                  <b>{name}</b>
                  <em>{members.length}</em>
                </div>
                {showTrend && <span className="a-td" />}
                {cells(aggregate(members.flatMap((g) => g.rows)), false)}
              </div>
            ),
          };
          return [head, ...(opened ? members.flatMap(groupLines) : [])];
        })
      : sorted.flatMap(groupLines);
  // Within the rows there are: a search or a fold can shorten them before the
  // body has scrolled back to where they end.
  const first = Math.max(0, Math.min(port.first, lines.length - port.count + 1)),
    from = Math.max(0, first - SPARE),
    to = Math.min(lines.length, first + port.count + SPARE);
  return (
    <div className="a-busy-card">
      <div className="a-busy-head">
        {title === null ? <b>最新的进程</b> : <b translate="no">{title}</b>}
        {clearable && (
          <button className="h-clear" title="清除游标（Esc）" onClick={onClear}>
            <Icon name="dismiss" size={13} />
          </button>
        )}
        <Segments
          label="历史进程"
          value={mode}
          onChange={setMode}
          items={[
            { value: "moment", label: "此刻" },
            { value: "range", label: "区间" },
          ]}
        />
        <input className="a-input" placeholder="搜索进程" value={query} onChange={(e) => setQuery(e.target.value)} />
        <span onClick={(e) => columnMenu(e.currentTarget)}>
          <Button icon="columns" small>
            列
          </Button>
        </span>
      </div>
      <div className="a-table" style={{ "--cols": cols } as CSSProperties}>
        <div className="a-thead" role="row">
          <button className="a-th h-name-head" aria-sort={sortKey === "name" ? (desc ? "descending" : "ascending") : "none"} onClick={() => changeSort("name")}>
            {"名称" + (sortKey === "name" ? (desc ? " ↓" : " ↑") : "")}
          </button>
          {showTrend && (
            <span className="a-th" title="所选范围内的变化，读数与最上方的轨道一致">{`趋势 · ${trendNames[trend]}`}</span>
          )}
          {shown.map((c) => (
            <button
              className="a-th is-end"
              aria-sort={sortKey === c.key ? (desc ? "descending" : "ascending") : "none"}
              key={c.key}
              onClick={() => changeSort(c.key)}
            >
              {labels[c.key] + (sortKey === c.key ? (desc ? " ↓" : " ↑") : "")}
            </button>
          ))}
        </div>
        <div className="a-tbody a-scroll" ref={body}>
          {!sorted.length && <p className="a-empty">{mode === "range" ? "此范围内暂无进程记录" : "该时刻无记录"}</p>}
          {from > 0 && <div style={{ height: from * ROW }} />}
          {lines.slice(from, to).map((line) => line.draw())}
          {to < lines.length && <div style={{ height: (lines.length - to) * ROW }} />}
        </div>
      </div>
      <footer className="a-busy-note">
        <span>
          最早的记录 <b translate="no">{`${monthDay(-30 * 86400 + 9000)} ${clock(-30 * 86400 + 9000, false)}`}</b>
        </span>
        {recording.enabled ? (
          <span>通常每 5 秒记录一次，后台采样较慢时按实际间隔记录。最近 10 分钟保留每次采样的读数。</span>
        ) : (
          <span translate="no">
            {off[0]}
            <button className="h-link" onClick={() => store.set({ request: { page: "settings", at: Date.now() } })}>
              {off[1]}
            </button>
            {off[2]}
          </span>
        )}
      </footer>
      {toast && (
        <div className="h-table-toast" onAnimationEnd={() => setToast(null)}>
          {toast}
        </div>
      )}
    </div>
  );
}
