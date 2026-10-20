// History: what the machine and its busiest processes did, up to a week back.
// Each chart is drawn once per query (range change, resize, new data every
// 10 s to 5 min); pointing at the charts only moves the cursor elements.

import {
  createEffect,
  createMemo,
  createResource,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
} from "solid-js";
import { copyText } from "../lib/actions";
import { fmtCpu, fmtGB, fmtMem, fmtNet, fmtRate, netUnit } from "../lib/format";
import { joins, nearest, pointAt } from "../lib/history-gaps";
import { localized, rich, t } from "../lib/i18n";
import { api } from "../lib/ipc";
import { persisted } from "../lib/prefs";
import { revealProcess, snapshot } from "../lib/store";
import type { HistoryPoint, HistoryProcess, ProcessRow } from "../lib/types";
import { type MenuEntry, openMenu, toast } from "../lib/ui";
import DataTable, { type Column } from "./DataTable";
import Indicator from "./Indicator";
import { CopyIcon, GoToIcon } from "./icons";
import ProcIcon from "./ProcIcon";
import { openSettings } from "./Settings";
import { withAlpha } from "./Trace";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
/** One recorded frame (`FRAME_MS` in history.rs). */
const FRAME = 5_000;
const KB = 1024;
const MB = 1024 * KB;
/** Headroom (CSS px) so a line at full scale is not clipped. */
const TOP_PAD = 3;
/** Chart border (CSS px): the plot area starts inside it. */
const BORDER = 1;

const RANGES = [
  { id: "1h", count: 1, unit: "hours", ms: HOUR, tick: 10 * MINUTE },
  { id: "6h", count: 6, unit: "hours", ms: 6 * HOUR, tick: HOUR },
  { id: "24h", count: 24, unit: "hours", ms: DAY, tick: 3 * HOUR },
  { id: "7d", count: 7, unit: "days", ms: 7 * DAY, tick: DAY },
] as const;
type RangeId = (typeof RANGES)[number]["id"];

const [rangeId, setRangeId] = persisted<RangeId>("history.range", "1h");
const range = () => RANGES.find((r) => r.id === rangeId()) ?? RANGES[0];
/** A moment picked on the charts; null follows the newest data. */
const [pinned, setPinned] = createSignal<number | null>(null);
/** End of the window while looking into the past; null keeps it at the present. */
const [anchor, setAnchor] = createSignal<number | null>(null);
const [selected, setSelected] = createSignal<string | null>(null);
/** Bumped when the theme changes: chart colors come from CSS. */
const [scheme, setScheme] = createSignal(0);
matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => setScheme((n) => n + 1));

function chooseRange(id: RangeId) {
  const r = RANGES.find((x) => x.id === id) ?? RANGES[0];
  setRangeId(id);
  // Zooming in keeps the picked moment in the middle of the view.
  const p = pinned();
  const end = p === null ? null : p + r.ms / 2;
  setAnchor(end !== null && end < Date.now() ? end : null);
}

/** The toolbar offers a way back to the newest data. */
export const showsLatest = () => pinned() !== null || anchor() !== null;

function backToLatest() {
  setPinned(null);
  setAnchor(null);
}

export function HistoryControls() {
  return (
    <>
      <div class="segmented" role="group" aria-label={t("history.range.label")}>
        <Indicator selector="[aria-pressed='true']" track={rangeId} class="seg-thumb" />
        <For each={RANGES}>
          {(r) => (
            <button type="button" aria-pressed={rangeId() === r.id} onClick={() => chooseRange(r.id)}>
              {t(`history.range.${r.unit}`, { count: r.count })}
            </button>
          )}
        </For>
      </div>
      <Show when={showsLatest()}>
        <button type="button" class="btn" title={t("history.latest.title")} onClick={backToLatest}>
          {t("history.latest.label")}
        </button>
      </Show>
    </>
  );
}

// ---- Scales and formats -----------------------------------------------------------

/** The next 1, 2 or 5 times a power of ten at or above `v`. */
function niceCeil(v: number): number {
  const p = 10 ** Math.floor(Math.log10(v));
  const m = v / p;
  return (m <= 1 ? 1 : m <= 2 ? 2 : m <= 5 ? 5 : 10) * p;
}

interface Metric {
  resource: "cpu" | "memory" | "disk" | "network" | "gpu" | "battery";
  colorVar: string;
  value: (p: HistoryPoint) => number;
  /** Missing measurements leave a gap and have no readout or cursor dot. */
  available?: (p: HistoryPoint) => boolean;
  /** Drawn faintly above the value: CPU's highest one-second sample. */
  peak?: (p: HistoryPoint) => number;
  max: (points: HistoryPoint[]) => number;
  /** Labels the top of an automatic scale. */
  scale?: (max: number) => string;
  format: (p: HistoryPoint) => string;
  detail?: (p: HistoryPoint) => string;
}

/** A round byte rate at or above `v`, in the unit fmtRate shows it in. */
const niceRate = (v: number) => (v <= MB ? Math.min(MB, niceCeil(v / KB) * KB) : niceCeil(v / MB) * MB);

/** Scales are round numbers: "20 MB/s" rather than "20.0 MB/s". */
const round = (label: string) => label.replace(/\.0+ /, " ");

/** Watts below ten keep a decimal, as on the performance page. */
const fmtW = (v: number) => (v < 10 ? v.toFixed(1) : String(Math.round(v)));

const highest = (points: HistoryPoint[], value: (p: HistoryPoint) => number) =>
  points.reduce((m, p) => Math.max(m, value(p)), 0);

const METRICS: Metric[] = [
  {
    resource: "cpu",
    colorVar: "--cpu",
    value: (p) => p.cpu,
    peak: (p) => p.cpuPeak,
    max: () => 100,
    format: (p) => `${fmtCpu(p.cpu)}%`,
    detail: (p) => t("history.metric.peak", { value: fmtCpu(p.cpuPeak) }),
  },
  {
    resource: "memory",
    colorVar: "--mem",
    value: (p) => p.memUsed,
    max: (points) =>
      Math.max(
        1,
        highest(points, (p) => p.memTotal),
      ),
    format: (p) => `${fmtGB(p.memUsed)} GB`,
    detail: (p) => t("history.metric.total", { value: fmtGB(p.memTotal) }),
  },
  {
    resource: "disk",
    colorVar: "--io",
    value: (p) => p.disk,
    max: (points) =>
      niceRate(
        Math.max(
          MB,
          highest(points, (p) => p.disk),
        ),
      ),
    scale: (max) => round(fmtRate(max)),
    format: (p) => fmtRate(p.disk, false),
  },
  {
    resource: "network",
    colorVar: "--net",
    value: (p) => p.net,
    // Round in the unit shown; at least 1 Mbps or 100 KB/s.
    max: (points) => {
      const top = highest(points, (p) => p.net);
      return netUnit() === "bits"
        ? niceCeil(Math.max(125_000, top) * 8) / 8
        : niceRate(Math.max(100 * KB, top));
    },
    scale: (max) => round(fmtNet(max)),
    format: (p) => fmtNet(p.net),
  },
  {
    resource: "gpu",
    colorVar: "--gpu",
    value: (p) => p.gpu,
    max: () => 100,
    format: (p) => `${fmtCpu(p.gpu)}%`,
  },
  {
    resource: "battery",
    colorVar: "--bat",
    value: (p) => p.battery ?? 0,
    available: (p) => p.battery !== null,
    max: () => 100,
    format: (p) => `${Math.round(p.battery ?? 0)}%`,
    // The power flow ties the drain to what the other charts were doing.
    detail: (p) => {
      const r = p.batteryRate;
      if (r === null || r === 0) return "";
      return t(r > 0 ? "history.metric.charging" : "history.metric.discharging", {
        value: `${fmtW(Math.abs(r) / 1000)} W`,
      });
    },
  },
];

const timeFmt = localized(
  (l) => new Intl.DateTimeFormat(l, { hour: "2-digit", minute: "2-digit", hourCycle: "h23" }),
);
const secFmt = localized(
  (l) =>
    new Intl.DateTimeFormat(l, {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hourCycle: "h23",
    }),
);
// "short" months read "10月11日" in Chinese; "numeric" gives "10/11".
const dayFmt = localized((l) => new Intl.DateTimeFormat(l, { month: "short", day: "numeric" }));

const isToday = (ts: number) => new Date(ts).toDateString() === new Date().toDateString();
/** A moment, with its date unless it is today. */
const fmtMoment = (ts: number) =>
  isToday(ts) ? secFmt().format(ts) : `${dayFmt().format(ts)} ${secFmt().format(ts)}`;
/** The cursor's time, as precise as the points under it. */
const fmtCursor = (ts: number, span: number) =>
  span > DAY
    ? `${dayFmt().format(ts)} ${timeFmt().format(ts)}`
    : span > 6 * HOUR
      ? timeFmt().format(ts)
      : secFmt().format(ts);

/** Axis ticks on local clock boundaries (every so many minutes past midnight, or midnights). */
function ticks(from: number, to: number, step: number): number[] {
  const out: number[] = [];
  const d = new Date(from);
  const [y, m, day] = [d.getFullYear(), d.getMonth(), d.getDate()];
  for (let k = 0; ; k++) {
    const at = (
      step >= DAY ? new Date(y, m, day + k) : new Date(y, m, day, 0, (k * step) / MINUTE)
    ).getTime();
    if (at > to) break;
    if (at >= from) out.push(at);
  }
  return out;
}

const tickLabel = (ts: number) => {
  const d = new Date(ts);
  return d.getHours() === 0 && d.getMinutes() === 0 ? dayFmt().format(d) : timeFmt().format(d);
};

// ---- Charts -----------------------------------------------------------------------

interface View {
  from: number;
  to: number;
  /** Spacing of the query's buckets; much wider jumps are gaps (`joins`). */
  step: number;
  points: HistoryPoint[];
  /** The points' times. */
  ts: number[];
  /** Whether the line joins each point to the next (`joins`). */
  joined: boolean[];
}

function HistoryChart(props: {
  view: View;
  metric: Metric;
  ticks: number[];
  /** The point the readout shows, marked with a dot. */
  cursor: HistoryPoint | null;
}) {
  let box!: HTMLDivElement;
  let canvas!: HTMLCanvasElement;
  const [size, setSize] = createSignal({ w: 0, h: 0 });
  const max = createMemo(() => Math.max(1e-9, props.metric.max(props.view.points)));
  const y = (v: number, h: number) => TOP_PAD + (1 - Math.min(1, Math.max(0, v / max()))) * (h - TOP_PAD);
  const cursor = () => {
    const p = props.cursor;
    return p && (props.metric.available?.(p) ?? true) ? p : null;
  };

  onMount(() => {
    const ro = new ResizeObserver(() => setSize({ w: box.clientWidth, h: box.clientHeight }));
    ro.observe(box);
    onCleanup(() => ro.disconnect());
  });

  createEffect(() => {
    scheme();
    const { w, h } = size();
    const { view, metric, ticks } = props;
    const ctx = canvas.getContext("2d");
    if (!ctx || w === 0 || h === 0) return;
    // Device pixels throughout, so grid lines land on whole pixels.
    const dpr = window.devicePixelRatio || 1;
    const W = Math.round(w * dpr);
    const H = Math.round(h * dpr);
    if (canvas.width !== W || canvas.height !== H) {
      canvas.width = W;
      canvas.height = H;
    }
    ctx.clearRect(0, 0, W, H);
    const color = getComputedStyle(box).getPropertyValue(metric.colorVar).trim() || "#888888";
    const x = (ts: number) => ((ts - view.from) / (view.to - view.from)) * W;
    const yd = (v: number) => y(v, h) * dpr;

    // Quarters across, the time ticks down.
    const grid = new Path2D();
    for (const f of [0.25, 0.5, 0.75]) {
      const gy = Math.round(H * f) + 0.5;
      grid.moveTo(0, gy);
      grid.lineTo(W, gy);
    }
    for (const tick of ticks) {
      const gx = Math.round(x(tick)) + 0.5;
      grid.moveTo(gx, 0);
      grid.lineTo(gx, H);
    }
    ctx.strokeStyle = withAlpha(color, 0.13);
    ctx.lineWidth = 1;
    ctx.stroke(grid);

    // Runs of points without gaps.
    const runs: HistoryPoint[][] = [];
    let run: HistoryPoint[] | undefined;
    view.points.forEach((p, i) => {
      if (metric.available && !metric.available(p)) {
        run = undefined;
        return;
      }
      if (!run || !view.joined[i - 1]) {
        run = [];
        runs.push(run);
      }
      run.push(p);
    });
    const trace = (run: HistoryPoint[], value: (p: HistoryPoint) => number) => {
      const path = new Path2D();
      path.moveTo(x(run[0].ts), yd(value(run[0])));
      for (const p of run) path.lineTo(x(p.ts), yd(value(p)));
      return path;
    };
    const fill = ctx.createLinearGradient(0, TOP_PAD * dpr, 0, H);
    fill.addColorStop(0, withAlpha(color, 0.28));
    fill.addColorStop(1, withAlpha(color, 0.02));
    ctx.lineJoin = "round";
    for (const run of runs) {
      if (run.length === 1) {
        // A lone frame between gaps: a dot, where a line would have no length.
        ctx.fillStyle = color;
        ctx.beginPath();
        ctx.arc(x(run[0].ts), yd(metric.value(run[0])), 1.5 * dpr, 0, 2 * Math.PI);
        ctx.fill();
        continue;
      }
      const line = trace(run, metric.value);
      const area = new Path2D(line);
      area.lineTo(x(run[run.length - 1].ts), H);
      area.lineTo(x(run[0].ts), H);
      area.closePath();
      ctx.fillStyle = fill;
      ctx.fill(area);
      if (metric.peak) {
        ctx.strokeStyle = withAlpha(color, 0.45);
        ctx.lineWidth = dpr;
        ctx.stroke(trace(run, metric.peak));
      }
      ctx.strokeStyle = color;
      ctx.lineWidth = 1.5 * dpr;
      ctx.stroke(line);
    }
  });

  return (
    <div ref={box} class="history-chart" style={{ "--c": `var(${props.metric.colorVar})` }}>
      <canvas ref={canvas} />
      <Show when={props.metric.scale}>{(scale) => <span class="history-scale">{scale()(max())}</span>}</Show>
      <Show when={cursor()}>
        {(p) => (
          <span
            class="chart-dot"
            style={{
              "--dot": "var(--c)",
              transform: `translate(${((p().ts - props.view.from) / (props.view.to - props.view.from)) * size().w}px, ${y(props.metric.value(p()), size().h)}px)`,
            }}
          />
        )}
      </Show>
    </div>
  );
}

/** The cursor line across the charts, with its time on the axis. */
function CursorMark(props: {
  ts: number;
  x: number;
  width: number;
  span: number;
  pinned?: boolean;
  labelled: boolean;
}) {
  return (
    <>
      <span
        class="history-cursor"
        classList={{ pinned: props.pinned }}
        style={{ transform: `translateX(${Math.round(props.x)}px)` }}
      />
      <Show when={props.labelled}>
        {/* Centred on the line, sliding to stay inside the plot near either end. */}
        <span
          class="history-time"
          classList={{ pinned: props.pinned }}
          style={{
            transform: `translateX(${props.x}px) translateX(${(-props.x / Math.max(1, props.width)) * 100}%)`,
          }}
        >
          {fmtCursor(props.ts, props.span)}
        </span>
      </Show>
    </>
  );
}

// ---- Page -------------------------------------------------------------------------

export default function HistoryPage() {
  const [now, setNow] = createSignal(Date.now());
  const following = () => pinned() === null && anchor() === null;
  // Follow new frames while showing the present; coarser views need it less often.
  createEffect(() => {
    if (!following()) return;
    setNow(Date.now());
    const every = Math.min(5 * MINUTE, Math.max(10_000, range().ms / 360));
    const timer = setInterval(() => document.visibilityState === "visible" && setNow(Date.now()), every);
    onCleanup(() => clearInterval(timer));
  });

  let plot!: HTMLDivElement;
  const [plotW, setPlotW] = createSignal(0);
  // Width the query resolution follows: settles after a resize instead of re-querying throughout.
  const [queryW, setQueryW] = createSignal(0);
  onMount(() => {
    let settle: ReturnType<typeof setTimeout> | undefined;
    const ro = new ResizeObserver(() => {
      const w = plot.clientWidth;
      setPlotW(w);
      clearTimeout(settle);
      if (queryW() === 0) setQueryW(w);
      else settle = setTimeout(() => setQueryW(w), 300);
    });
    ro.observe(plot);
    onCleanup(() => {
      ro.disconnect();
      clearTimeout(settle);
    });
  });

  const query = createMemo(() => {
    const w = queryW();
    if (w === 0) return null;
    const span = range().ms;
    const to = anchor() ?? now();
    // About a point every two pixels, never finer than a frame.
    const buckets = Math.max(30, Math.min(Math.ceil(span / FRAME), Math.floor(w / 2)));
    return { from: to - span, to, buckets };
  });
  const [series] = createResource(query, async (q): Promise<View> => {
    const points = await api.historySeries(q.from, q.to, q.buckets).catch(() => []);
    const step = Math.max(FRAME, (q.to - q.from) / q.buckets);
    const ts = points.map((p) => p.ts);
    return { from: q.from, to: q.to, step, points, ts, joined: joins(ts, step) };
  });
  // Drawn with the window it was queried for, so a refresh never shows old points on a new axis.
  const view = (): View =>
    series.latest ?? { from: now() - range().ms, to: now(), step: FRAME, points: [], ts: [], joined: [] };
  const tickList = createMemo(() => ticks(view().from, view().to, range().tick));

  /** Battery charts appear only where battery data was recorded. */
  const metrics = createMemo(() => {
    const hasBattery = view().points.some((p) => p.battery !== null);
    return hasBattery ? METRICS : METRICS.filter((m) => m.resource !== "battery");
  });

  /** The point drawn at `ts`, unless `ts` falls in a gap. */
  const shownAt = (ts: number) => {
    const v = view();
    const i = pointAt(v.ts, v.joined, v.step, ts);
    return i === undefined ? undefined : v.points[i];
  };
  /** Where a cursor at `ts` goes across the plot, if it is in view. */
  const markAt = (ts: number | null) => {
    if (ts === null) return null;
    const v = view();
    const f = (ts - v.from) / (v.to - v.from);
    return f >= 0 && f <= 1 ? { ts, x: BORDER + f * (plotW() - 2 * BORDER) } : null;
  };

  const [hover, setHover] = createSignal<number | null>(null);
  /** Pointer position -> time, snapped to the point drawn there. */
  const timeAt = (e: MouseEvent) => {
    // Read per event (cheap: layout is clean between frames); it also stays right while the page slides in.
    const r = plot.getBoundingClientRect();
    const f = (e.clientX - r.left - BORDER) / (r.width - 2 * BORDER);
    if (!(f >= 0 && f <= 1)) return null;
    const v = view();
    const ts = v.from + f * (v.to - v.from);
    // Whole milliseconds: the host takes timestamps as integers.
    return shownAt(ts)?.ts ?? Math.round(ts);
  };
  /** What the labels show: the hovered moment, else the picked one, else the newest. */
  const readTs = () => hover() ?? pinned();
  const readout = () => {
    const ts = readTs();
    return ts === null ? view().points.at(-1) : shownAt(ts);
  };
  const readoutFor = (metric: Metric) => {
    const p = readout();
    return p && (metric.available?.(p) ?? true) ? p : undefined;
  };
  const cursor = () => {
    const ts = readTs();
    return (ts === null ? undefined : shownAt(ts)) ?? null;
  };

  const move = (by: number) => {
    const { points: pts, ts } = view();
    if (pts.length === 0) return;
    const at = nearest(ts, pinned() ?? pts[pts.length - 1].ts);
    setPinned(pts[Math.min(pts.length - 1, Math.max(0, at + by))].ts);
  };
  const onKey = (e: KeyboardEvent) => {
    const pts = view().points;
    const far = Math.max(1, Math.round(pts.length / 10));
    if (e.key === "ArrowLeft") move(e.shiftKey ? -far : -1);
    else if (e.key === "ArrowRight") move(e.shiftKey ? far : 1);
    else if (e.key === "PageUp") move(-far);
    else if (e.key === "PageDown") move(far);
    else if (e.key === "Home" && pts.length > 0) setPinned(pts[0].ts);
    else if (e.key === "End" && pts.length > 0) setPinned(pts[pts.length - 1].ts);
    else if (e.key === "Escape") backToLatest();
    else return;
    e.preventDefault();
  };

  // The processes table: the picked moment, else the newest frame.
  const momentTs = () => pinned() ?? (series.latest ? view().to : null);
  const [moment] = createResource(momentTs, (ts) => api.historyMoment(ts).catch(() => null));
  const [first] = createResource(() => api.historyRange().catch(() => null));
  const [settings] = createResource(() => api.settings().catch(() => null));
  const recordingOff = () => settings.latest?.history === false;

  const live = createMemo(() => {
    const byPid = new Map<number, ProcessRow>();
    const byName = new Map<string, ProcessRow>();
    for (const p of snapshot()?.processes ?? []) {
      byPid.set(p.pid, p);
      const name = p.name.toLowerCase();
      if (!byName.has(name)) byName.set(name, p);
    }
    return { byPid, byName };
  });
  const running = (p: HistoryProcess) => live().byPid.get(p.pid)?.name === p.name;
  const openLive = (p: HistoryProcess) => {
    if (running(p)) revealProcess(p.pid);
    else toast("error", t("history.exited", { name: p.name, pid: p.pid }));
  };
  const rowMenu = (p: HistoryProcess): MenuEntry[] => [
    {
      label: t("common.goToProcess"),
      icon: () => <GoToIcon />,
      disabled: !running(p),
      run: () => revealProcess(p.pid),
    },
    "separator",
    {
      label: t("common.copyName"),
      icon: () => <CopyIcon />,
      run: () => copyText(p.name, t("history.copied.name")),
    },
  ];

  // Per-process network needs administrator rights; without it the column would be all zeros.
  const hasNet = createMemo(() => moment.latest?.processes.some((p) => p.net > 0) ?? false);
  const columns = createMemo(() => {
    const cols: Column<HistoryProcess>[] = [
      {
        id: "name",
        label: t("common.name"),
        width: "minmax(200px, 1fr)",
        sortValue: (p) => p.name,
        cell: (p) => {
          // The icon of the same program if it is still running.
          const same = () => live().byName.get(p().name.toLowerCase());
          return (
            <>
              <ProcIcon proc={{ icon: same()?.icon ?? null, path: same()?.path ?? "" }} />
              <span class="label" title={same()?.description ?? undefined}>
                {p().name}
              </span>
            </>
          );
        },
      },
      {
        id: "pid",
        label: "PID",
        width: "72px",
        align: "end",
        sortValue: (p) => p.pid,
        cell: (p) => <span class="dim">{p().pid}</span>,
      },
      {
        id: "cpu",
        label: "CPU",
        width: "70px",
        align: "end",
        title: t("history.column.cpuTitle"),
        sortValue: (p) => p.cpu,
        cell: (p) => fmtCpu(p().cpu),
      },
      {
        id: "mem",
        label: t("common.resource.memory"),
        width: "96px",
        align: "end",
        title: t("history.column.memTitle"),
        sortValue: (p) => p.mem,
        cell: (p) => fmtMem(p().mem),
      },
      {
        id: "io",
        label: "I/O",
        width: "92px",
        align: "end",
        sortValue: (p) => p.io,
        cell: (p) => fmtRate(p().io),
      },
    ];
    if (hasNet()) {
      cols.push({
        id: "net",
        label: t("common.resource.network"),
        width: "92px",
        align: "end",
        sortValue: (p) => p.net,
        cell: (p) => fmtNet(p().net, true),
      });
    }
    cols.push({
      id: "gpu",
      label: "GPU",
      width: "70px",
      align: "end",
      sortValue: (p) => p.gpu,
      cell: (p) => fmtCpu(p().gpu),
    });
    return cols;
  });

  const empty = () => series.latest !== undefined && series.latest.points.length === 0;

  return (
    <div class="page history">
      <section class="history-graphs" aria-label={t("history.systemHistory")}>
        <div class="history-labels">
          <For each={metrics()}>
            {(m) => (
              <div class="history-label" style={{ "--c": `var(${m.colorVar})` }}>
                <span>{t(`common.resource.${m.resource}`)}</span>
                <Show
                  when={readoutFor(m)}
                  fallback={<b class="dim">{readTs() === null && !readout() ? "—" : t("history.noData")}</b>}
                >
                  {(p) => (
                    <>
                      <b>{m.format(p())}</b>
                      <Show when={m.detail}>{(detail) => <small>{detail()(p())}</small>}</Show>
                    </>
                  )}
                </Show>
              </div>
            )}
          </For>
        </div>

        <div ref={plot} class="history-plot">
          <For each={metrics()}>
            {(m) => <HistoryChart view={view()} metric={m} ticks={tickList()} cursor={cursor()} />}
          </For>
          <div class="history-axis" aria-hidden="true">
            <For each={tickList()}>
              {(tick) => {
                const f = () => (tick - view().from) / (view().to - view().from);
                return (
                  <Show when={f() > 0.03 && f() < 0.97}>
                    <span style={{ left: `${f() * 100}%` }}>{tickLabel(tick)}</span>
                  </Show>
                );
              }}
            </For>
          </div>
          <Show when={empty()}>
            <div class="history-empty">
              <span>{recordingOff() ? t("history.empty.off") : t("history.empty.none")}</span>
              <small>{recordingOff() ? t("history.empty.offHint") : t("history.empty.hint")}</small>
            </div>
          </Show>
          <div
            class="history-overlay"
            role="slider"
            tabIndex={0}
            aria-label={t("history.time")}
            aria-valuemin={view().from}
            aria-valuemax={view().to}
            aria-valuenow={pinned() ?? view().points.at(-1)?.ts ?? view().to}
            aria-valuetext={fmtMoment(pinned() ?? view().points.at(-1)?.ts ?? view().to)}
            onPointerMove={(e) => setHover(timeAt(e))}
            onPointerLeave={() => setHover(null)}
            onClick={(e) => {
              const ts = timeAt(e);
              if (ts !== null) setPinned(ts);
            }}
            onKeyDown={onKey}
          >
            {/* While hovering, the picked moment keeps its line but gives up its label. */}
            <Show when={markAt(pinned())}>
              {(m) => (
                <CursorMark
                  ts={m().ts}
                  x={m().x}
                  width={plotW()}
                  span={view().to - view().from}
                  pinned
                  labelled={hover() === null}
                />
              )}
            </Show>
            <Show when={markAt(hover())}>
              {(m) => (
                <CursorMark ts={m().ts} x={m().x} width={plotW()} span={view().to - view().from} labelled />
              )}
            </Show>
          </div>
        </div>
      </section>

      <section class="history-moment">
        <header class="history-moment-head">
          <h2>
            <Show when={moment.latest} fallback={t("history.busiest")}>
              {(m) => <>{t("history.busiestAt", { time: fmtMoment(m().system.ts) })}</>}
            </Show>
          </h2>
          <span>{pinned() === null ? t("history.hint.pick") : t("history.hint.move")}</span>
        </header>
        <DataTable
          id="history"
          label={t("history.tableLabel")}
          rows={moment.latest?.processes ?? []}
          columns={columns()}
          rowKey={(p) => `${p.name}:${p.pid}`}
          initialSort={{ id: "cpu", desc: true }}
          selected={selected()}
          onSelect={setSelected}
          onContextMenu={(p, e) => openMenu(e, rowMenu(p))}
          onActivate={openLive}
          empty={<span>{moment.loading ? "" : t("history.noRecordAt")}</span>}
        />
        <footer class="page-foot">
          <Show when={first.latest}>
            {(r) => (
              <span>
                {t("history.earliest")} <b>{fmtMoment(r()[0])}</b>
              </span>
            )}
          </Show>
          <Show when={recordingOff()} fallback={<span>{t("history.note")}</span>}>
            <span>
              {rich("history.off", {
                link: (text) => (
                  <button type="button" class="link" onClick={() => openSettings("data")}>
                    {text}
                  </button>
                ),
              })}
            </span>
          </Show>
        </footer>
      </section>
    </div>
  );
}
