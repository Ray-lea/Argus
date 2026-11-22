/** @jsxImportSource @/components/replica/i18n */
"use client";
import { useMemo, useState, type CSSProperties } from "react";
import { store, useShared } from "./store";
import { Button, Segments } from "./widgets";
import { Icon, ProcessIcon } from "./icons";
import { bytes, rate, processes, readings, heat, percent } from "./model";
import { asleep, stamp, type Span } from "./historyTime";
import { byMetric } from "./metrics";
import type { MenuEntry } from "./Pages";
import { pathFor } from "./HistoryPlot";
export function WatchPanel({ tick, alive }: { tick: number; alive: typeof processes }) {
  const { watch } = useShared();
  if (!watch) return null;
  const p = processes.find((p) => p.pid === watch.pid)!;
  const instances = watch.program ? processes.filter((proc) => proc.path === p.path) : [p];
  return (
    <div className="h-watch">
      <b>进程观察</b>
      <strong>{p.exe}</strong>
      {instances.map((proc) => {
        const until = watch.stop ?? tick,
          stopped = !alive.some((v) => v.pid === proc.pid);
        const values = Array.from(
          { length: Math.min(60, Math.max(1, until - watch.start + 1)) },
          (_, i) => readings(Math.max(watch.start, until - 59) + i, [proc]).get(proc.pid)!.mem,
        );
        return (
          <span key={proc.pid}>
            PID {proc.pid} <span>{stopped ? "此运行实例已结束" : watch.stop !== null ? "已停止记录" : "正在记录"}</span>{" "}
            <span>专用工作集</span> {bytes(values.at(-1)!)}{" "}
            <svg width="75" height="18" viewBox="0 0 1000 100" preserveAspectRatio="none">
              <path
                d={pathFor(values, Math.max(...values) * 1.02)}
                fill="none"
                stroke="var(--a-mem)"
                strokeWidth="1.3"
                vectorEffect="non-scaling-stroke"
              />
            </svg>
          </span>
        );
      })}
      {watch.stop === null && <Button onClick={() => store.set({ watch: { ...watch, stop: tick } })}>停止记录</Button>}
      <Button onClick={() => store.set({ watch: null })}>删除记录</Button>
    </div>
  );
}
const allFields = [
  "cpu",
  "cpuMax",
  "mem",
  "disk",
  "diskRead",
  "diskWrite",
  "io",
  "net",
  "gpu",
  "gpuMem",
  "power",
  "seen",
  "threads",
  "handles",
] as const;
type Field = (typeof allFields)[number];
const momentLabels: Record<Field, string> = {
  cpu: "CPU",
  cpuMax: "CPU 最高",
  mem: "内存",
  disk: "磁盘",
  diskRead: "磁盘读取",
  diskWrite: "磁盘写入",
  io: "I/O",
  net: "网络",
  gpu: "GPU",
  gpuMem: "专用 GPU 内存",
  power: "电源使用情况",
  seen: "记录时长",
  threads: "线程",
  handles: "句柄",
};
const rangeLabels: Record<Field, string> = {
  ...momentLabels,
  cpu: "CPU 平均",
  mem: "内存最高",
  disk: "磁盘读写量",
  diskRead: "磁盘读取量",
  diskWrite: "磁盘写入量",
  io: "I/O 量",
  net: "网络收发量",
  gpu: "GPU 平均",
  gpuMem: "专用 GPU 内存最高",
};
const optionalFields = ["diskRead", "diskWrite", "io", "gpuMem", "threads", "handles"];
export function HistoryTable({
  program,
  onProgram,
  onMenu,
  span,
  trend,
  at,
  live,
  mode,
  setMode,
  alive,
  onOpen,
}: {
  program: string | null;
  onProgram: (name: string | null) => void;
  onMenu: (event: React.MouseEvent, items: MenuEntry[]) => void;
  span: Span;
  trend: string;
  at: number;
  live: boolean;
  mode: "moment" | "range";
  setMode: (m: "moment" | "range") => void;
  alive: typeof processes;
  onOpen: (pid: number) => void;
}) {
  const [query, setQuery] = useState(""),
    [sort, setSort] = useState("cpu"),
    [desc, setDesc] = useState(true),
    [cols, setCols] = useState(false);
  const [hidden, setHidden] = useState({ moment: optionalFields, range: optionalFields }),
    [open, setOpen] = useState<string[]>([]);
  const fields = allFields.filter((f) => mode === "range" || (f !== "seen" && f !== "cpuMax"));
  const labels = mode === "range" ? rangeLabels : momentLabels,
    visible = fields.filter((f) => !hidden[mode].includes(f));
  const [start, end] = span;
  const list = useMemo(() => {
    const pool = live && mode === "moment" ? alive : processes;
    const samples =
      mode === "range" ? Array.from({ length: 16 }, (_, i) => Math.round(start + ((end - start) * i) / 15)) : [at];
    const recorded = samples.filter((t) => !asleep(t));
    if (!recorded.length) return [];
    const duration = ((end - start) * recorded.length) / samples.length;
    const rows = pool.map((p) => ({
      p,
      cpu: 0,
      cpuMax: 0,
      mem: 0,
      disk: 0,
      diskRead: 0,
      diskWrite: 0,
      io: 0,
      net: 0,
      gpu: 0,
      gpuMem: 0,
      power: 0,
      seen: duration,
      threads: p.threads,
      handles: p.handles,
      trend: [] as number[],
    }));
    for (const t of recorded) {
      const data = readings(t, pool);
      for (const row of rows) {
        const r = data.get(row.p.pid)!;
        const scale = 1 / recorded.length;
        row.cpu += r.cpu * scale;
        row.cpuMax = Math.max(row.cpuMax, r.cpu);
        row.mem = Math.max(row.mem, r.mem);
        row.gpuMem = Math.max(row.gpuMem, r.gpu * 0.018 * 1024 ** 3);
        row.diskRead += r.read * scale;
        row.diskWrite += r.write * scale;
        row.io += r.io * scale;
        row.net += r.net * scale;
        row.gpu += r.gpu * scale;
        row.power = Math.max(
          row.power,
          r.cpu * 0.2 + r.gpu * 0.04 + Math.min(1.5, (r.read + r.write) / (512 * 1024 ** 2)),
        );
        row.trend.push(
          trend === "disk"
            ? r.read + r.write
            : trend === "mem"
              ? r.mem
              : trend === "gpu"
                ? r.gpu
                : trend === "net"
                  ? r.net
                  : r.cpu,
        );
      }
    }
    for (const row of rows) {
      if (mode === "range") {
        row.net *= duration;
        row.io *= duration;
        row.diskRead *= duration;
        row.diskWrite *= duration;
      }
      row.disk = row.diskRead + row.diskWrite;
    }
    if (mode === "moment") {
      const target = ["mem", "gpu", "net"].includes(trend) ? (trend as "mem" | "gpu" | "net") : "cpu";
      const history = Array.from({ length: 18 }, (_, i) => readings(at - 17 + i, pool));
      for (const row of rows)
        row.trend = history.map((frame) => {
          const r = frame.get(row.p.pid)!;
          return trend === "disk" ? r.read + r.write : r[target];
        });
    }
    return rows;
  }, [mode, mode === "range" ? start : at, mode === "range" ? end : at, live, alive, trend]);
  const canOpen = (pid: number) => {
    const p = alive.find((proc) => proc.pid === pid);
    if (!p) return false;
    const clock = p.started.split(" ")[1]?.split(":").map(Number);
    return !!clock && at >= clock[0] * 3600 + clock[1] * 60 + clock[2] - (21 * 3600 + 36 * 60);
  };
  const [selected, setSelected] = useState<string | null>(null);
  const rowMenu = (event: React.MouseEvent, name: string, pids: number[]) => {
    event.preventDefault();
    const single = pids.length === 1 ? pids[0] : null;
    onMenu(event, [
      {
        label: program === name ? "不在图表上显示" : "在图表上显示曲线",
        checked: program === name,
        run: () => onProgram(program === name ? null : name),
      },
      {
        label: "转到进程",
        disabled: single === null || !canOpen(single),
        run: () => single !== null && onOpen(single),
      },
      { label: "复制名称与 PID", run: () => void navigator.clipboard?.writeText(name + " · " + pids.join(", ")) },
    ]);
  };
  const groups = new Map<string, typeof list>();
  for (const row of list) {
    if (!row.p.exe.toLowerCase().includes(query.toLowerCase())) continue;
    const group = groups.get(row.p.exe);
    if (group) group.push(row);
    else groups.set(row.p.exe, [row]);
  }
  const sorted = [...groups]
    .map(([name, rows]) => ({
      name,
      rows,
      trend: rows[0].trend.map((_, i) => rows.reduce((n, r) => n + r.trend[i], 0)),
      values: Object.fromEntries(
        fields.map((f) => [
          f,
          f === "power" || (mode === "range" && ["cpuMax", "mem", "gpuMem", "seen", "threads", "handles"].includes(f))
            ? Math.max(...rows.map((r) => r[f]))
            : rows.reduce((s, r) => s + r[f], 0),
        ]),
      ),
    }))
    .sort(
      (a, b) => (sort === "name" ? a.name.localeCompare(b.name) : a.values[sort] - b.values[sort]) * (desc ? -1 : 1),
    );
  const changeSort = (f: string) => {
    setDesc(f === sort ? !desc : f !== "name");
    setSort(f);
  };
  const display = (f: Field, v: number) => {
    if (f === "power") return ["非常低", "低", "中", "高", "非常高"][[0.1, 0.3, 0.8, 2].filter((n) => v >= n).length];
    if (f === "threads" || f === "handles") return Math.round(v).toLocaleString();
    if (f === "seen")
      return v >= 86400
        ? `${Math.floor(v / 86400)} 天 ${Math.floor((v % 86400) / 3600)} 小时`
        : new Date(v * 1000).toISOString().slice(11, 19);
    if (f === "mem" || f === "gpuMem") return bytes(v);
    if (f === "gpu" || f === "cpu" || f === "cpuMax") return percent(v);
    return mode === "range" ? bytes(v) : rate(v);
  };
  const [showTrend, setShowTrend] = useState(true);
  const trendShown = showTrend;
  const sizes: Record<Field, number> = {
    cpu: 64,
    cpuMax: 82,
    mem: 84,
    disk: 84,
    diskRead: 84,
    diskWrite: 84,
    io: 80,
    net: 84,
    gpu: 60,
    gpuMem: 84,
    power: 108,
    seen: 96,
    threads: 64,
    handles: 72,
  };
  const cells = (data: Record<Field, number>) =>
    visible.map((f) => {
      const kind =
        f === "cpu" || f === "gpu" || f === "cpuMax"
          ? "percent"
          : f === "mem" || f === "gpuMem"
            ? "memory"
            : f === "io" || f === "net" || f.startsWith("disk")
              ? "rate"
              : null;
      const intensity = kind ? heat(kind, data[f]) : 0;
      return (
        <span
          key={f}
          className="a-td is-end"
          style={intensity ? { background: "rgb(var(--a-heat) / " + intensity + ")" } : undefined}
        >
          {display(f, data[f])}
        </span>
      );
    });
  const graph = (values: number[]) => (
    <span className="a-td">
      <svg width="80" height="19" viewBox="0 0 1000 100" preserveAspectRatio="none">
        <path
          d={pathFor(values, Math.max(1, ...values)) + "V100H0Z"}
          fill={byMetric[trend]?.color ?? "var(--a-cpu)"}
          opacity=".18"
        />
        <path
          d={pathFor(values, Math.max(1, ...values))}
          fill="none"
          stroke={byMetric[trend]?.color ?? "var(--a-cpu)"}
          strokeWidth="1.3"
          vectorEffect="non-scaling-stroke"
        />
      </svg>
    </span>
  );
  const toggle = (name: string) =>
    setOpen((old) => (old.includes(name) ? old.filter((k) => k !== name) : [...old, name]));
  return (
    <div className="a-busy-card">
      <div className="a-busy-head">
        <b>{mode === "range" ? stamp(start) + " – " + stamp(end) : live ? "最新的进程" : stamp(at)}</b>
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
        <Button icon="columns" small pressed={cols} onClick={() => setCols(!cols)}>
          列
        </Button>
      </div>
      {cols && (
        <div className="h-column-options">
          {
            <label>
              <input type="checkbox" checked={showTrend} onChange={(e) => setShowTrend(e.target.checked)} />
              趋势迷你曲线
            </label>
          }
          {fields.map((f) => (
            <label key={f}>
              <input
                type="checkbox"
                checked={visible.includes(f)}
                onChange={() =>
                  setHidden((h) => ({
                    ...h,
                    [mode]: h[mode].includes(f) ? h[mode].filter((k) => k !== f) : [...h[mode], f],
                  }))
                }
              />
              {labels[f]}
            </label>
          ))}
        </div>
      )}
      <div
        className="a-table"
        style={
          {
            "--cols":
              "minmax(152px,1fr) " + (trendShown ? "88px " : "") + visible.map((f) => sizes[f] + "px").join(" "),
          } as CSSProperties
        }
      >
        <div className="a-thead" role="row">
          <button className="a-th" onClick={() => changeSort("name")}>
            名称
          </button>
          {trendShown && <span className="a-th">{`趋势 · ${byMetric[trend]?.name ?? "CPU"}`}</span>}
          {visible.map((f) => (
            <button
              className="a-th is-end"
              aria-sort={sort === f ? (desc ? "descending" : "ascending") : "none"}
              key={f}
              onClick={() => changeSort(f)}
            >
              {labels[f]} {sort === f ? (desc ? "↓" : "↑") : ""}
            </button>
          ))}
        </div>
        <div className="a-tbody a-scroll">
          {!list.length && <p className="a-empty">该时刻无记录</p>}
          {(sort === "name" ? ["app", "background", "windows"] : [""]).map((category) => (
            <div key={category}>
              {category && (
                <button className="a-group h-group" onClick={() => toggle(category)}>
                  {
                    ({ app: "应用", background: "后台进程", windows: "Windows 进程" } as Record<string, string>)[
                      category
                    ]
                  }
                </button>
              )}
              {(!category || query || !open.includes(category)) &&
                sorted
                  .filter((g) => !category || g.rows[0].p.group === category)
                  .map((g) => (
                    <div key={g.name}>
                      <div
                        className={"a-tr" + (selected === g.name ? " is-selected" : "")}
                        onClick={() => setSelected(g.name)}
                        onContextMenu={(e) =>
                          rowMenu(
                            e,
                            g.name,
                            g.rows.map((r) => r.p.pid),
                          )
                        }
                        onDoubleClick={() =>
                          g.rows.length === 1 && canOpen(g.rows[0].p.pid) ? onOpen(g.rows[0].p.pid) : toggle(g.name)
                        }
                      >
                        <div className="a-td">
                          {g.rows.length === 1 && <span className="a-fold-gap" />}
                          {g.rows.length > 1 && (
                            <button
                              className="a-fold"
                              aria-expanded={open.includes(g.name)}
                              onClick={() => toggle(g.name)}
                            >
                              <Icon name="chevronRight" size={12} />
                            </button>
                          )}
                          <ProcessIcon glyph={g.rows[0].p.glyph} />
                          {g.name} <small>{g.rows.length > 1 ? "×" + g.rows.length : "PID " + g.rows[0].p.pid}</small>
                        </div>
                        {trendShown && graph(g.trend)}
                        {cells(g.values as Record<Field, number>)}
                      </div>
                      {open.includes(g.name) &&
                        g.rows.map((r) => (
                          <div
                            className={"a-tr" + (selected === String(r.p.pid) ? " is-selected" : "")}
                            key={r.p.pid}
                            onClick={() => setSelected(String(r.p.pid))}
                            onContextMenu={(e) => rowMenu(e, g.name, [r.p.pid])}
                            onDoubleClick={() => canOpen(r.p.pid) && onOpen(r.p.pid)}
                          >
                            <span className="a-td"> PID {r.p.pid}</span>
                            {trendShown && graph(r.trend)}
                            {cells(r)}
                          </div>
                        ))}
                    </div>
                  ))}
            </div>
          ))}
        </div>
      </div>
      <footer className="a-busy-note">
        <span>
          最早的记录 <b>{stamp(-30 * 86400, true)}</b>
        </span>
        <span>通常每 5 秒记录一次，后台采样较慢时按实际间隔记录。最近 10 分钟保留每次采样的读数。</span>
      </footer>
    </div>
  );
}
