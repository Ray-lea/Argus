/** @jsxImportSource @/components/replica/i18n */
"use client";
import { store, useShared } from "./store";
import { dateLocale } from "@/lib/languages";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type MouseEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
  type RefObject,
} from "react";
import { flushSync } from "react-dom";
import {
  alignsEnd,
  architecture,
  cellOf,
  columnDef,
  company,
  MISSING,
  NAME_MIN,
  POWER_LEVELS,
  powerEstimate,
  powerLevel,
  resourceLevel,
  trendEstimate,
  type ColumnPrefs,
  useColumnWidths,
  useHeaderDrag,
  useTableScroll,
  versionInfo,
  type CellContext,
  type ColumnDef,
} from "./Columns";
import Curve, { type Motion } from "./Curve";
import { language, translate } from "./i18n";
import { Icon, ProcessIcon, type UiIcon } from "./icons";
import {
  adapters,
  clock,
  cpuPower,
  cpuTemperature,
  disks,
  gpuPower,
  gpuTemperature,
  gpuEngine,
  gpus,
  groupLabel,
  machineTotals,
  machine,
  memory,
  noise,
  percent,
  rate,
  rateCeiling,
  rateLabel,
  sampleAt,
  samples,
  services,
  sessionOf,
  thousands,
  totalMemory,
  transferred,
  userName,
  type Glyph,
  type Group,
  type Process,
  type Reading,
} from "./model";
import { scaleOf, slideIn, useIsoLayoutEffect } from "./widgets";

/** The memory card's and the column's scope (processes.memoryScope). */
const MEMORY_SCOPE = "整机已用物理内存为总量减去可用内存，包括使用中和已修改的内存。";

export type View = "apps" | "tree" | "list";
/** A process column's key (Columns.tsx). */
export type SortKey = string;
export type Device = "cpu" | "memory" | "disk" | "disk1" | "wifi" | "wsl" | "gpu" | "gpu1" | "npu" | "battery";

export interface Row {
  key: string;
  /** A category heading, a user's heading (grouped by user), or a process or app. */
  kind: "group" | "user" | "unit";
  /** Inside a user's group: stands in from its heading. */
  nested?: boolean;
  /** A user heading that holds the sessions no one is signed in to. */
  system?: boolean;
  depth: number;
  label: string;
  /** The faint text after the name: the image name beside a friendly name. */
  detail?: string;
  exe?: string;
  count: number;
  pids: number[];
  glyph?: Glyph;
  group?: Group;
  expandable: boolean;
  expanded: boolean;
  values: Reading;
  suspended?: boolean;
}

type Metric = keyof Reading;
/** The columns that show a live reading as it is, by the reading they show. */
const readingOf: Record<string, Metric> = {
  cpu: "cpu",
  mem: "mem",
  diskRead: "read",
  diskWrite: "write",
  io: "io",
  gpu: "gpu",
  net: "net",
};
const zero: Reading = { cpu: 0, mem: 0, read: 0, write: 0, io: 0, gpu: 0, net: 0 };
function sum(pids: number[], now: Map<number, Reading>): Reading {
  const out = { ...zero };
  for (const pid of pids) {
    const r = now.get(pid);
    if (!r) continue;
    out.cpu += r.cpu;
    out.mem += r.mem;
    out.read += r.read;
    out.write += r.write;
    out.io += r.io;
    out.gpu = Math.max(out.gpu, r.gpu);
    out.net += r.net;
  }
  return out;
}
export const hostedServices = (pid: number) => services.filter((s) => s.pid === pid).map((s) => s.name);
function matches(proc: Process, query: string) {
  if (!query) return true;
  const q = query.trim().toLowerCase();
  if (!q) return true;
  if (/^\d+$/.test(q)) return String(proc.pid).startsWith(q);
  return (
    proc.name.toLowerCase().includes(q) ||
    translate(proc.name).toLowerCase().includes(q) ||
    proc.exe.toLowerCase().includes(q) ||
    proc.path.toLowerCase().includes(q) ||
    hostedServices(proc.pid).some((s) => s.toLowerCase().includes(q))
  );
}

interface BuildOptions {
  view: View;
  sort: SortKey;
  descending: boolean;
  query: string;
  list: Process[];
  now: Map<number, Reading>;
  sortReadings?: Map<number, Reading>;
  open: Set<string>;
  closed: Set<string>;
  /** The tree's branches shown open; the rest stay folded, as the app starts. */
  treeOpen: Set<string>;
  /** What a column other than the name, PID and the live readings sorts by. */
  sortValue?: (row: Row) => number | string;
  /** Grouped by user: the signed-in user's group first, then the system's. */
  users?: boolean;
  /** Prefixes the category headings' keys within a user's group. */
  scope?: string;
  /** Every group and branch open: the list the column widths measure (widths.rs `WidthRows`). */
  openAll?: boolean;
}
// Rows as the app builds them (frontend/model.rs): sorted by name, the list
// falls into the Task Manager categories; any other sort keeps one list.
// Grouped by user, each user's rows are built so under its own heading.
export function buildRows(o: BuildOptions): Row[] {
  if (o.users) {
    const out: Row[] = [];
    const searching = !!o.query.trim();
    for (const [id, title, system] of [
      ["1", userName, false],
      ["system", "系统", true],
    ] as const) {
      const part = o.list.filter((p) => (sessionOf(p) === 0) === system);
      const key = "u:" + id;
      const rows = buildRows({ ...o, list: part, users: false, scope: key + "/" });
      if (!rows.length) continue;
      // The processes its rows stand for: all of the user's, or a search's matches.
      const members = searching ? [...new Set(rows.flatMap((r) => (r.kind === "unit" ? r.pids : [])))] : part.map((p) => p.pid);
      const expanded = o.openAll || searching || !o.closed.has(key);
      out.push({
        key,
        kind: "user",
        system,
        depth: 0,
        label: title,
        detail: `${members.length} 个进程`,
        count: members.length,
        pids: members,
        expandable: true,
        expanded,
        values: sum(members, o.now),
      });
      if (expanded) out.push(...rows.map((row) => ({ ...row, nested: true })));
    }
    return out;
  }
  const byPid = new Map(o.list.map((p) => [p.pid, p]));
  // Names sort as they read in the page's language.
  const collator = new Intl.Collator(dateLocale(language()), { numeric: true });
  const name = (row: Row) => translate(row.label);
  const scale: Record<Metric, number> = {
    cpu: 10,
    gpu: 10,
    mem: 1 / 104858,
    read: 1 / 1024,
    write: 1 / 1024,
    io: 1 / 1024,
    net: 1 / 1024,
  };
  const compare = (a: Row, b: Row) => {
    if(o.sortReadings){a={...a,values:sum(a.pids,o.sortReadings)};b={...b,values:sum(b.pids,o.sortReadings)}}
    let c: number;
    if (o.sort === "name") c = collator.compare(name(a), name(b));
    else if (o.sort === "pid") c = (a.count === 1 ? a.pids[0] : 1e9) - (b.count === 1 ? b.pids[0] : 1e9);
    else if (readingOf[o.sort]) {
      const k = readingOf[o.sort];
      c = Math.round(a.values[k] * scale[k]) - Math.round(b.values[k] * scale[k]);
      if (c === 0) return collator.compare(name(a), name(b)) || a.key.localeCompare(b.key);
    } else {
      const va = o.sortValue?.(a) ?? 0,
        vb = o.sortValue?.(b) ?? 0;
      c = typeof va === "number" && typeof vb === "number" ? Math.sign(va - vb) : collator.compare(String(va), String(vb));
      if (c === 0) return collator.compare(name(a), name(b)) || a.key.localeCompare(b.key);
    }
    if (c === 0) c = a.key.localeCompare(b.key);
    return o.descending ? -c : c;
  };
  const unit = (key: string, pids: number[], depth: number, label: string, detail?: string): Row => {
    const first = byPid.get(pids[0])!;
    return {
      key,
      kind: "unit",
      depth,
      label,
      detail,
      exe: first.exe,
      count: pids.length,
      pids,
      glyph: first.glyph,
      group: first.group,
      expandable: false,
      expanded: false,
      values: sum(pids, o.now),
      suspended: pids.every((pid) => byPid.get(pid)?.suspended),
    };
  };
  const named = (p: Process, depth: number) =>
    unit("p:" + p.pid, [p.pid], depth, p.name, p.name !== p.exe ? p.exe : undefined);
  const out: Row[] = [];
  if (o.view === "tree") {
    const children = new Map<number, Process[]>();
    for (const proc of o.list) {
      const parent = byPid.has(proc.ppid) && proc.ppid !== proc.pid ? proc.ppid : -1;
      children.set(parent, [...(children.get(parent) ?? []), proc]);
    }
    // While searching, the ancestors of matches stay in place.
    const keep = new Set<number>();
    if (o.query.trim()) {
      for (const proc of o.list.filter((p) => matches(p, o.query))) {
        let at: Process | undefined = proc;
        while (at && !keep.has(at.pid)) {
          keep.add(at.pid);
          at = byPid.get(at.ppid);
        }
      }
    }
    const walk = (parent: number, depth: number) => {
      const rows = (children.get(parent) ?? [])
        .filter((proc) => !o.query.trim() || keep.has(proc.pid))
        .map((proc) => {
          const row = named(proc, depth);
          row.expandable = children.has(proc.pid);
          row.expanded = row.expandable && (o.openAll || o.treeOpen.has(row.key) || !!o.query.trim());
          return row;
        });
      for (const row of rows.sort(compare)) {
        out.push(row);
        if (row.expanded) walk(row.pids[0], depth + 1);
      }
    };
    walk(-1, 0);
    return out;
  }
  let units: Row[];
  if (o.view === "apps") {
    const byApp = new Map<string, Process[]>();
    for (const proc of o.list) {
      const key = proc.group + "/" + proc.exe + "/" + (proc.exe === "svchost.exe" ? proc.pid : "");
      byApp.set(key, [...(byApp.get(key) ?? []), proc]);
    }
    units = [];
    for (const members of byApp.values()) {
      const root = members.find((m) => !members.some((n) => n.pid === m.ppid)) ?? members[0];
      const q = o.query.trim().toLowerCase();
      // A search that names the app shows all of it; otherwise only the matches.
      const named = !q || root.name.toLowerCase().includes(q) || root.exe.toLowerCase().includes(q);
      const shown = named ? members : members.filter((m) => matches(m, o.query));
      if (!shown.length) continue;
      const pids = [root.pid, ...members.filter((m) => m !== root).map((m) => m.pid)];
      const row = unit("a:" + root.exe + ":" + root.pid, pids, 0, root.name, root.name !== root.exe ? root.exe : undefined);
      row.expandable = pids.length > 1;
      row.expanded = row.expandable && (o.openAll || o.open.has(row.key) || shown.length < members.length);
      (row as Row & { shown?: number[] }).shown = shown.map((m) => m.pid);
      units.push(row);
    }
  } else {
    units = o.list.filter((p) => matches(p, o.query)).map((p) => named(p, 0));
  }
  const pushUnit = (row: Row & { shown?: number[] }) => {
    out.push(row);
    if (!row.expanded) return;
    const members = (row.shown ?? row.pids)
      .map((pid) => unit("p:" + pid, [pid], 1, byPid.get(pid)!.exe))
      .sort(compare);
    out.push(...members);
  };
  if (o.sort === "name") {
    for (const group of ["app", "background", "windows"] as Group[]) {
      const members = units.filter((u) => u.group === group).sort(compare);
      if (!members.length) continue;
      const key = (o.scope ?? "") + "g:" + group;
      const expanded = o.openAll || !o.closed.has(key) || !!o.query.trim();
      out.push({
        key,
        kind: "group",
        depth: 0,
        group,
        label: groupLabel[group],
        count: members.length,
        pids: members.flatMap((m) => m.pids),
        expandable: true,
        expanded,
        values: sum(
          members.flatMap((m) => m.pids),
          o.now,
        ),
      });
      if (expanded) members.forEach(pushUnit);
    }
  } else units.sort(compare).forEach(pushUnit);
  return out;
}

/** The tree's branches: every process with a child in the list. */
export function treeParents(list: Process[]) {
  const pids = new Set(list.map((p) => p.pid));
  return new Set(list.filter((p) => p.ppid !== p.pid && pids.has(p.ppid)).map((p) => "p:" + p.ppid));
}

export function useSystemHistory(tick: number) {
  return useMemo(() => {
    const ticks = Array.from({ length: samples }, (_, i) => tick - samples + 1 + i);
    const points = ticks.map(sampleAt);
    return {
      cpu: points.map((p) => p.cpu),
      kernel: points.map((p) => p.kernel),
      mem: points.map((p) => p.mem),
      // The whole machine's disk and network rates, and its busiest GPU (vitals.rs totals);
      // the others are the first disk's and adapter's.
      disk: ticks.map((t) => machineTotals(t).disk),
      read: points.map((p) => p.read),
      write: points.map((p) => p.write),
      active: points.map((p) => p.active),
      rx: points.map((p) => p.rx),
      tx: points.map((p) => p.tx),
      net: ticks.map((t) => machineTotals(t).net),
      gpu: ticks.map((t) => machineTotals(t).gpu),
    };
  }, [tick]);
}
export { cpuPower, cpuTemperature, gpuPower, gpuTemperature };
const split = (text: string): [string, string] => {
  const at = text.indexOf(" ");
  return at < 0 ? [text, ""] : [text.slice(0, at), text.slice(at + 1)];
};

interface VitalProps {
  tick: number;
  interval: number;
  running: boolean;
  motion: Motion;
  onOpen: (device: Device) => void;
}
export function Vitals({ tick, interval, running, motion, onOpen }: VitalProps) {
  const s = sampleAt(tick);
  const h = useSystemHistory(tick);
  const curve = { tick, interval, running, motion };
  const used = (s.mem / 100) * totalMemory;
  const diskMax = rateCeiling(Math.max(...h.disk));
  const netMax = rateCeiling(Math.max(...h.net), true);
  const totals = machineTotals(tick);
  const [diskValue, diskUnit] = totals.disk < 1 ? ["0", "KB/s"] : split(rate(totals.disk));
  const [netValue, netUnit] = totals.net < 1 ? ["0", "KB/s"] : split(rate(totals.net));
  const gpuNow = totals.busiestGpu;
  const gpuWatts = gpuNow.power < 1 ? gpuNow.power.toFixed(3) : gpuNow.power < 10 ? gpuNow.power.toFixed(1) : String(Math.round(gpuNow.power));
  const cards: {
    id: Device;
    /** The device a click opens: the busiest disk or GPU, the first adapter. */
    open?: Device;
    name: string;
    color: string;
    values: number[];
    max: number;
    /** The top of the curve's scale, named in the headroom. */
    scale: string;
    figure: string;
    unit: string;
    /** Separate readings (CPU, GPU), or one line. */
    readings: string[];
    /** What each reading is, and the figure's own hint (vitals.rs). */
    tips: string[];
    figureTip?: string;
  }[] = [
    {
      id: "cpu",
      name: "CPU",
      color: "var(--a-cpu)",
      values: h.cpu,
      max: 100,
      scale: "100%",
      figure: String(Math.round(s.cpu)),
      unit: "%",
      readings: [
        `${clock("p", tick).toFixed(2)} GHz`,
        `${Math.round(cpuTemperature(tick))} °C`,
        `${Math.round(cpuPower(tick))} W`,
      ],
      tips: ["速度", "温度", "封装功耗"],
    },
    {
      id: "memory",
      name: "内存",
      color: "var(--a-mem)",
      values: h.mem,
      max: 100,
      scale: "32.0 GB",
      figure: (used / 1024 ** 3).toFixed(1),
      unit: "/ 32.0 GB",
      readings: [`${Math.round(s.mem)}%`],
      tips: [MEMORY_SCOPE],
      figureTip: MEMORY_SCOPE,
    },
    {
      id: "disk",
      name: "磁盘",
      color: "var(--a-io)",
      values: h.disk,
      max: diskMax,
      scale: rateLabel(diskMax),
      figure: diskValue,
      unit: diskUnit,
      readings: [`活动 ${Math.round(totals.busiestDisk.active)}%`],
      tips: [],
      open: totals.busiestDisk.id,
    },
    {
      id: "wifi",
      name: "网络",
      color: "var(--a-net)",
      values: h.net,
      max: netMax,
      scale: rateLabel(netMax),
      figure: netValue,
      unit: netUnit,
      readings: [`收 ${rate(totals.rx)} 发 ${rate(totals.tx)}`],
      tips: ["接收 / 发送"],
    },
    {
      id: "gpu",
      name: "GPU",
      color: "var(--a-gpu)",
      values: h.gpu,
      max: 100,
      scale: "100%",
      figure: String(Math.round(gpuNow.util)),
      unit: "%",
      readings: [`${Math.round(gpuNow.temperature)} °C`, `${gpuWatts} W`],
      tips: ["GPU 温度", "功耗"],
      figureTip: "Windows 引擎",
      open: gpuNow.id,
    },
  ];
  return (
    <div className="a-vitals" role="group" aria-label="系统概况">
      {cards.map((card) => {
        const open = `在“性能”中查看${card.name}`;
        return (
          <button
            key={card.id}
            type="button"
            className={"a-vital" + (card.id === "cpu" ? " is-cpu" : "")}
            style={{ "--c": card.color } as CSSProperties}
            onClick={() => onOpen(card.open ?? card.id)}
            aria-label={open}
            title={open}
          >
            {/* Under the curve, which keeps below them, as the app draws them. */}
            <span className="a-vital-scale" aria-hidden="true">
              {card.scale}
            </span>
            <span className="a-vital-plot">
              <Curve
                series={[{ values: card.values, color: card.color, fill: VITAL_FILL }]}
                max={card.max * VITAL_STRETCH}
                {...curve}
              />
            </span>
            <span className="a-vital-scrim" aria-hidden="true" />
            <span className="a-vital-tint" aria-hidden="true" />
            <span className="a-vital-name">{card.name}</span>
            <span className="a-figure" title={card.figureTip}>
              <b>{card.figure}</b>
              <small>{card.unit}</small>
            </span>
            <span className={"a-vital-readings" + (card.readings.length > 1 ? " is-split" : "")}>
              {card.readings.map((r, i) => (
                <span key={r.slice(-3)} title={card.tips[i]}>
                  {r}
                </span>
              ))}
            </span>
          </button>
        );
      })}
    </div>
  );
}
/** The overview curves' area fill at the top of the card, as the app's. */
const VITAL_FILL = 0.2;
/**
 * The curve's scale tops out 20 px under the card's top, at its line, over
 * a plot 118 px high: the chart's own top is raised to leave the headroom.
 */
const VITAL_STRETCH = 118 / 98;

/** How far a user's rows stand in from its heading (tables.rs `USER_INSET`). */
const USER_INSET = 20;
/** The rows put in the page around the view: past it by at least OVERSCAN, in steps of STEP. */
const WINDOW_OVERSCAN = 8;
const WINDOW_STEP = 16;
interface TableProps {
  tick: number;
  rows: Row[];
  /** Every row in the page, not only those near the view: an export reads them all. */
  whole?: boolean;
  /** The list with every group open and nothing searched, which the widths measure. */
  measured: Row[];
  total: number;
  /** Rows held out of the order the sort would give them (`Desktop::holds_order`). */
  heldOrder: boolean;
  hiddenCount: number;
  /** Processes the leak alerts mark; the header's mark lists them. */
  leaks: Set<number>;
  onLeak: (anchor: HTMLElement) => void;
  /** I/O and memory priority set from the menu (Columns.tsx `CellContext`). */
  resource: Record<string, string>;
  /** The search that left the list empty, and the button that clears it. */
  query: string;
  onClearSearch: () => void;
  /** The columns shown, in their order, the name first (Columns.tsx). */
  columns: ColumnDef[];
  sizing: Pick<ColumnPrefs, "autoWidth" | "fullWidth" | "widths">;
  /** A column's edge dragged to `width`, or double-clicked back to automatic (null). */
  onWidth: (key: string, width: number | null) => void;
  /** The font size setting against its 13 px default. */
  text: number;
  list: Process[];
  priority: Map<number, string>;
  sort: SortKey;
  descending: boolean;
  selected: string | null;
  filtered: boolean;
  /** What the empty list says while system processes are hidden, with the button that shows them. */
  hiding?: { run: () => void };
  efficiency: Set<number>;
  onSort: (key: SortKey) => void;
  onToggle: (row: Row) => void;
  onSelect: (key: string | null) => void;
  onMenu: (event: MouseEvent | KeyboardEvent, row: Row) => void;
  /** A user's heading: folding, and the user's account. */
  onHeadingMenu: (event: MouseEvent, row: Row) => void;
  onEnd: (row: Row, confirm: boolean) => void;
  onDetails: () => void;
  onHold: (hold: boolean) => void;
  /** The header's menu, for the column under the pointer. */
  onHeaderMenu: (event: MouseEvent, column: string | null) => void;
  /** A dragged header took another place: before `before`, or last. */
  onMoveColumn: (key: string, before: string | null) => void;
}
export function ProcessTable(t: TableProps) {
  const table = useRef<HTMLDivElement>(null);
  const head = useRef<HTMLDivElement>(null);
  const body = useRef<HTMLDivElement>(null);
  const deselectTimer=useRef<ReturnType<typeof setTimeout>|undefined>(undefined);
  useEffect(()=>()=>clearTimeout(deselectTimer.current),[t.selected]);
  const others = t.columns.slice(1);
  const groups = useMemo(() => groupsOf(t.rows), [t.rows]);
  const scroller = useGroupScroll(body, t.rows, groups);
  // Only the rows near the view are in the page, which keeps each refresh
  // cheap on a long list; spacers keep the others' height. The headings
  // (held at the top) and the selected row always are.
  const [span, setSpan] = useState<[number, number]>([0, 3 * WINDOW_STEP]);
  const follow = (scrolled: boolean) => {
    const b = body.current;
    if (!b) return;
    const rowH = b.querySelector<HTMLElement>(":scope > .a-tr")?.offsetHeight || 30;
    const first = Math.floor(b.scrollTop / rowH),
      last = Math.ceil((b.scrollTop + b.clientHeight) / rowH);
    const next: [number, number] = [
      Math.max(0, Math.floor((first - WINDOW_OVERSCAN) / WINDOW_STEP) * WINDOW_STEP),
      Math.ceil((last + WINDOW_OVERSCAN) / WINDOW_STEP) * WINDOW_STEP,
    ];
    if (next[0] === span[0] && next[1] === span[1]) return;
    // Scrolled past the rows in the page: they come in before the frame shows.
    if (scrolled && (first < span[0] || last > span[1])) flushSync(() => setSpan(next));
    else setSpan(next);
  };
  useIsoLayoutEffect(() => follow(false));
  const shown: { row: Row; index: number; gap: number }[] = [];
  t.rows.forEach((row, index) => {
    if (t.whole || row.kind !== "unit" || (index >= span[0] && index < span[1]) || row.key === t.selected)
      shown.push({ row, index, gap: 0 });
    else if (shown.at(-1)?.gap) shown[shown.length - 1].gap++;
    else shown.push({ row, index, gap: 1 });
  });
  const ctx = useMemo<CellContext>(
    () => ({
      byPid: new Map(t.list.map((p) => [p.pid, p])),
      tick: t.tick,
      efficiency: t.efficiency,
      priority: t.priority,
      resource: t.resource,
      leaks: t.leaks,
    }),
    [t.list, t.tick, t.efficiency, t.priority, t.resource, t.leaks],
  );
  const [room, setRoom] = useState(0);
  useIsoLayoutEffect(() => {
    const b = body.current;
    if (!b) return;
    const measure = () => setRoom(b.clientWidth);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(b);
    return () => observer.disconnect();
  }, []);
  const totals = Object.fromEntries(t.columns.map((c) => [c.key, totalText(c.key, t.tick, t.list, t.total, t.hiddenCount)]));
  const { widths, fill } = useColumnWidths(t.columns, t.sizing, t.text, t.measured, ctx, room, totals);
  const nameWidth = fill ? NAME_MIN : (widths.get("name") ?? NAME_MIN);
  const rowMin = others.reduce((a, c) => a + (widths.get(c.key) ?? c.width), nameWidth);
  // Dragging a column's edge: the column keeps the width; a double-click
  // returns it to automatic and marks it in the accent as the width settles.
  const [resizing, setResizing] = useState<string | null>(null);
  const [fitted, setFitted] = useState<{ key: string; id: number } | null>(null);
  const resize = (event: ReactPointerEvent<HTMLElement>, key: string) => {
    if (event.button !== 0) return;
    event.stopPropagation();
    event.preventDefault();
    const edge = event.currentTarget;
    edge.setPointerCapture(event.pointerId);
    const k = scaleOf(edge);
    const x0 = event.clientX;
    const w0 = key === "name" ? (table.current?.querySelector<HTMLElement>(".a-tbody .a-col-name")?.offsetWidth ?? nameWidth) : (widths.get(key) ?? 0);
    setResizing(key);
    const move = (e: PointerEvent) => t.onWidth(key, w0 + (e.clientX - x0) / k);
    const up = () => {
      edge.removeEventListener("pointermove", move);
      edge.removeEventListener("pointerup", up);
      edge.removeEventListener("pointercancel", up);
      setResizing(null);
    };
    edge.addEventListener("pointermove", move);
    edge.addEventListener("pointerup", up);
    edge.addEventListener("pointercancel", up);
  };
  const fit = (key: string) => {
    t.onWidth(key, null);
    setFitted({ key, id: Date.now() });
  };
  useEffect(() => {
    if (!fitted) return;
    const id = setTimeout(() => setFitted(null), 700);
    return () => clearTimeout(id);
  }, [fitted]);
  const edge = (key: string) => (
    <span
      className="a-col-edge"
      title="拖动调整宽度，双击自动调整"
      aria-hidden="true"
      onPointerDown={(event) => resize(event, key)}
      onClick={(event) => event.stopPropagation()}
      onDoubleClick={(event) => {
        event.stopPropagation();
        fit(key);
      }}
    />
  );
  useTableScroll(table, head, body);
  const drag = useHeaderDrag({ table, head, body, columns: t.columns, widths, onMove: t.onMoveColumn });
  const reveal = (key: string) => {
    const b = body.current;
    const line = b?.querySelector(`[data-key="${CSS.escape(key)}"]`);
    if (line) return line.scrollIntoView({ block: "nearest" });
    // A row not in the page lies beyond the view: scroll to where it is.
    const index = t.rows.findIndex((r) => r.key === key);
    if (!b || index < 0) return;
    const top = index * scroller.rowH;
    b.scrollTop = top < b.scrollTop ? top : top + scroller.rowH - b.clientHeight;
  };
  const keyed = (event: KeyboardEvent<HTMLDivElement>) => {
    const units = t.rows.filter((r) => r.kind === "unit");
    const at = units.findIndex((r) => r.key === t.selected);
    const row = at >= 0 ? units[at] : null;
    const move = (index: number) => {
      const next = units[Math.max(0, Math.min(units.length - 1, index))];
      if (!next) return;
      t.onSelect(next.key);
      reveal(next.key);
    };
    const page = Math.max(1, Math.floor((body.current?.clientHeight ?? 300) / 30) - 1);
    switch (event.key) {
      case "ArrowDown":
      case "ArrowUp":
        event.preventDefault();
        move(at < 0 ? 0 : at + (event.key === "ArrowDown" ? 1 : -1));
        return;
      case "PageDown":
      case "PageUp":
        event.preventDefault();
        move((at < 0 ? 0 : at) + (event.key === "PageDown" ? page : -page));
        return;
      case "Home":
      case "End":
        event.preventDefault();
        move(event.key === "Home" ? 0 : units.length - 1);
        return;
    }
    if (!row) return;
    if (event.key === "Delete" || (event.altKey && event.key.toLowerCase() === "e")) {
      event.preventDefault();
      t.onEnd(row, !event.shiftKey);
    } else if (event.key === "Enter") {
      event.preventDefault();
      t.onDetails();
    } else if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) {
      event.preventDefault();
      t.onMenu(event, row);
    } else if (event.key === "ArrowRight" && row.expandable) {
      event.preventDefault();
      if (!row.expanded) t.onToggle(row);
      else move(at + 1);
    } else if (event.key === "ArrowLeft") {
      event.preventDefault();
      if (row.expandable && row.expanded) t.onToggle(row);
      else if (row.depth > 0) {
        const index = t.rows.indexOf(row);
        const parent = t.rows.slice(0, index).reverse().find((r) => r.kind === "unit" && r.depth < row.depth);
        if (parent) {
          t.onSelect(parent.key);
          reveal(parent.key);
        }
      }
    }
  };
  // Rows held out of this order: the arrow gives way to a pause mark in the accent until they re-sort.
  const sortMark = (key: SortKey) =>
    t.sort !== key ? null : t.heldOrder ? (
      <span className="a-order-held" title="排序已暂停，读数照常刷新。移开指针或关闭菜单、对话框后恢复排序。">
        <Icon name="pause" size={12} />
      </span>
    ) : (
      <Icon name={t.descending ? "chevronDown" : "chevronUp"} size={12} />
    );
  const leakCount = t.list.filter((p) => t.leaks.has(p.pid)).length;
  const efficient = (row: Row) => row.pids.filter((pid) => t.efficiency.has(pid)).length;
  const leaking = (row: Row) => row.pids.some((pid) => t.leaks.has(pid));
  const ariaSort = (key: SortKey) => (t.sort === key ? (t.descending ? "descending" : "ascending") : "none");
  const width = (c: ColumnDef) => widths.get(c.key) ?? c.width;
  const align = (c: ColumnDef) => (alignsEnd(c) ? " is-end" : "");
  return (
    <div
      ref={table}
      className="a-table a-ptable"
      role="grid"
      aria-label="进程"
      aria-rowcount={t.rows.length + 1}
      aria-colcount={t.columns.length}
      data-moving={drag.lifted ? "" : undefined}
      data-fit={fill ? undefined : ""}
      data-resizing={resizing ?? undefined}
      style={{ "--a-row-min": rowMin + "px", "--a-name-w": nameWidth + "px" } as CSSProperties}
    >
      {drag.lifted && <style>{drag.style}</style>}
      {resizing && <style>{`.a-ptable .a-col-${resizing}{box-shadow:inset -2px 0 0 var(--a-accent)}`}</style>}
      {fitted && <style key={fitted.id}>{`.a-ptable .a-col-${fitted.key}{animation:a-fit-flash 700ms ease-out}`}</style>}
      <div
        className="a-thead"
        ref={head}
        onContextMenu={(event) => {
          event.preventDefault();
          t.onHeaderMenu(event, (event.target as Element).closest("[data-col]")?.getAttribute("data-col") ?? null);
        }}
      >
        <div className="a-thead-row" role="row">
          <button
            type="button"
            role="columnheader"
            className="a-th a-col-name"
            aria-sort={ariaSort("name")}
            title="应用名称或文件描述；次级文字显示可执行文件名。"
            data-tour="col-name"
            data-col="name"
            onClick={() => t.onSort("name")}
          >
            <span className="a-th-total">
              {t.filtered && (
                <span className="a-filter-mark" title="列表已按搜索内容筛选">
                  <Icon name="filter" size={13} />
                </span>
              )}
              <span className="a-th-count">{totals.name}</span>
              {/* Leak alerts: a mark after the count, which lists them; its press never sorts. */}
              {leakCount > 0 && (
                <span
                  role="button"
                  tabIndex={-1}
                  className="a-leak-mark"
                  data-anchor="leak-mark"
                  title={`${leakCount} 项可能的资源泄漏`}
                  aria-label={`${leakCount} 项可能的资源泄漏`}
                  onPointerDown={(event) => event.stopPropagation()}
                  onClick={(event) => {
                    event.stopPropagation();
                    t.onLeak(event.currentTarget);
                  }}
                >
                  <Icon name="warning" size={11} />
                  {leakCount}
                </span>
              )}
            </span>
            <span className="a-th-label">
              <span>名称</span>
              {sortMark("name")}
            </span>
            {edge("name")}
          </button>
          {others.map((c) => (
            <button
              key={c.key}
              type="button"
              role="columnheader"
              className={`a-th a-col-${c.key}` + align(c)}
              style={{ width: width(c) }}
              title={c.key === "mem" ? "百分比为整机物理内存占用率。进程行显示专用工作集，分组显示成员合计。" : c.title}
              aria-sort={ariaSort(c.key)}
              data-tour={"col-" + c.key}
              data-col={c.key}
              onPointerDown={(event) => drag.press(event, c.key)}
              onClick={() => !drag.dragged() && t.onSort(c.key)}
            >
              <span className="a-th-total">{totals[c.key]}</span>
              <span className="a-th-label">
                <span data-i18n="column">{c.label}</span>
                {sortMark(c.key)}
              </span>
              {edge(c.key)}
            </button>
          ))}
        </div>
      </div>
      <div
        className="a-tbody a-scroll"
        ref={body}
        onScroll={() => {
          follow(true);
          scroller.onScroll();
        }}
        tabIndex={0}
        role="rowgroup"
        onKeyDown={keyed}
        onPointerEnter={() => t.onHold(true)}
        onPointerLeave={() => t.onHold(false)}
      >
        {t.rows.length === 0 &&
          (t.hiding ? (
            <div className="a-empty">
              <span>当前筛选条件下没有可显示的项目。</span>
              <button type="button" className="a-btn" onClick={t.hiding.run}>
                显示全部并清除筛选
              </button>
            </div>
          ) : (
            <div className="a-empty">
              <span>{`没有与“${t.query.trim()}”匹配的进程`}</span>
              <button type="button" className="a-btn" onClick={t.onClearSearch}>
                清除搜索
              </button>
            </div>
          ))}
        {shown.map(({ row, index, gap }) =>
          gap ? (
            <div key={"gap:" + index} className="a-tgap" style={{ height: gap * scroller.rowH }} aria-hidden="true" />
          ) : row.kind !== "unit" ? (
            // A user's or a category's heading, in its group's colour (tables/groups.rs).
            <div
              key={row.key}
              role="row"
              aria-rowindex={index + 2}
              data-row={index}
              data-group={row.key}
              data-tone={groups.find((g) => g.heading === index)?.tone}
              aria-expanded={row.expanded}
              className={
                "a-tr " +
                (row.kind === "user" ? "a-user" : "a-group") +
                (row.expanded ? "" : " is-folded") +
                (row.nested ? " is-nested" : "") +
                (t.filtered ? " is-locked" : "")
              }
              // Every match shows while searching: a heading cannot fold.
              title={t.filtered ? "搜索时显示全部匹配项，清除搜索后可折叠" : undefined}
              onClick={() => !t.filtered && t.onToggle(row)}
              onContextMenu={(event) => {
                event.preventDefault();
                t.onHeadingMenu(event, row);
              }}
            >
              <span role="gridcell" className="a-td a-td-name a-col-name">
                <span className="a-fold" aria-expanded={row.expanded}>
                  <Icon name="chevronRight" size={12} />
                </span>
                {row.kind === "user" ? (
                  <>
                    <span className="a-user-mark">
                      <Icon name={row.system ? "computer" : "user"} size={12} />
                    </span>
                    <span className="a-user-name" data-i18n="user">
                      {row.label}
                    </span>
                    <span className="a-user-detail">{row.detail}</span>
                  </>
                ) : (
                  <>
                    <span className="a-group-mark" />
                    <span className="a-group-name">{row.label}</span>
                    <span className="a-group-count">{row.count}</span>
                    <span className="a-group-rule" />
                  </>
                )}
              </span>
              {others.map((c) => {
                const cell = cellOf(c, row, ctx);
                return (
                  <span
                    role="gridcell"
                    key={c.key}
                    className={`a-td a-col-${c.key}` + align(c)}
                    style={{ width: width(c) }}
                    title={c.key === "mem" ? "此分类的专用工作集合计。共享内存和系统内存另计。" : undefined}
                  >
                    {cell.text}
                  </span>
                );
              })}
            </div>
          ) : (
            <div
              key={row.key}
              data-key={row.key}
              role="row"
              aria-rowindex={index + 2}
              data-row={index}
              aria-selected={t.selected === row.key}
              aria-expanded={row.expandable ? row.expanded : undefined}
              className={"a-tr" + (t.selected === row.key ? " is-selected" : "")}
              onClick={(event) => {
                clearTimeout(deselectTimer.current);
                if(event.detail>=2&&row.expandable){t.onSelect(row.key);t.onToggle(row)}
                else if(t.selected===row.key&&row.expandable)deselectTimer.current=setTimeout(()=>t.onSelect(null),500);
                else t.onSelect(t.selected===row.key?null:row.key);
              }}
              onContextMenu={(event) => {
                event.preventDefault();
                clearTimeout(deselectTimer.current);t.onSelect(row.key);
                t.onMenu(event, row);
              }}
            >
              <span
                role="gridcell"
                className="a-td a-td-name a-col-name"
                style={{ paddingLeft: 6 + (row.nested ? USER_INSET : 0) + row.depth * 20 }}
                title={row.label + "\n" + (ctx.byPid.get(row.pids[0])?.path ?? row.exe ?? "")}
              >
                {row.expandable ? (
                  <button
                    type="button"
                    className="a-fold"
                    aria-label={row.expanded ? "折叠" : "展开"}
                    aria-expanded={row.expanded}
                    tabIndex={-1}
                    onClick={(event) => {
                      event.stopPropagation();
                      t.onToggle(row);
                    }}
                  >
                    <Icon name="chevronRight" size={12} />
                  </button>
                ) : (
                  <span className="a-fold-gap" />
                )}
                <ProcessIcon glyph={row.glyph ?? "app"} />
                <span
                  className={
                    "a-name" + (row.count > 1 ? " is-strong" : "") + (row.suspended ? " is-muted" : "")
                  }
                >
                  {row.label}
                </span>
                {row.count > 1 && <span className="a-count">{row.count}</span>}
                {row.detail && <span className="a-exe">{row.detail}</span>}
                {efficient(row) > 0 && (
                  <span
                    title={row.count > 1 ? `${efficient(row)} 个进程处于效率模式` : "效率模式"}
                    className="a-leaf"
                  >
                    <Icon name="leaf" size={12} />
                  </span>
                )}
                {row.suspended && <span className="a-tag">已暂停</span>}
                {/* A leak alert: the kind in amber, what it rests on under the pointer. */}
                {leaking(row) && (
                  <span className="a-tag is-warn" title={LEAK.describe}>
                    句柄泄漏
                  </span>
                )}
              </span>
              {others.map((c) => {
                const cell = cellOf(c, row, ctx);
                // Long text ends in an ellipsis; hovering shows all of it.
                const long = (c.format === "text" || c.key === "basePriority") && !!cell.text;
                // The count a handle leak alert is about, in its amber.
                const warn = c.key === "handles" && leaking(row);
                const hint = long
                  ? cell.text
                  : warn
                    ? LEAK.describe
                    : c.key === "gpu" && row.values.gpu >= 0.05
                      ? gpuEngine(row.pids)
                      : c.key === "net"
                        ? `发送 ${byteRate(row.values.net * 0.22)}，接收 ${byteRate(row.values.net * 0.78)}`
                        : undefined;
                return (
                  <span
                    role="gridcell"
                    key={c.key}
                    className={
                      `a-td a-col-${c.key}` +
                      align(c) +
                      (cell.faint ? " is-faint" : "") +
                      (cell.heat ? " is-heat" : "") +
                      (warn ? " is-warn" : "")
                    }
                    style={
                      cell.heat ? ({ width: width(c), "--h": cell.heat.toFixed(3) } as CSSProperties) : { width: width(c) }
                    }
                    data-i18n={c.key === "efficiency" ? "state" : c.format === "power" ? "power" : undefined}
                    title={hint}
                  >
                    {long ? <span className="a-cell-text">{cell.text}</span> : cell.text}
                  </span>
                );
              })}
            </div>
          ),
        )}
      </div>
      {scroller.track}

      {/* The raised strip under a dragged column, within the scrolled columns' lane. */}
      {drag.lifted && (
        <div
          className="a-drag-lane"
          style={{ left: drag.lifted.left, right: drag.lifted.right, bottom: drag.lifted.bottom }}
          aria-hidden="true"
        >
          <div className="a-drag-strip" style={{ width: drag.lifted.width }} />
        </div>
      )}
    </div>
  );
}

// Column totals come from the system sample, as in the app's header band;
// threads and handles count the whole system, as the status bar does. The
// name's counts every process, and with system processes hidden, how many
// of them the list leaves out (tables.rs `column_total`).
function totalText(column: string, tick: number, list: Process[], total: number, hidden: number) {
  const s = sampleAt(tick);
  const disk = (value: number) => (value < 1 ? "0 KB/s" : rate(value));
  switch (column) {
    case "name":
      return hidden ? `${total} 个进程 (${hidden} 个隐藏)` : `${total} 个进程`;
    case "cpu":
      return Math.round(s.cpu) + "%";
    case "mem":
      return Math.round(s.mem) + "%";
    case "diskRead":
      return disk(s.read);
    case "diskWrite":
      return disk(s.write);
    case "io":
      return rate((s.read + s.write) * 1.12 + s.rx * 0.05);
    case "gpu":
      return Math.round(s.gpu) + "%";
    case "net":
      return rate(s.rx + s.tx);
    case "threads":
      return thousands(list.reduce((a, p) => a + p.threads, 1180));
    case "handles":
      return thousands(list.reduce((a, p) => a + p.handles, 52400));
    default:
      return "";
  }
}
/** The column a right press landed on in a process row, for the menu's
 *  "复制“{column}”": its label and what the cell shows (tables.rs `context_column`). */
export function columnUnder(target: Element) {
  const row = target.closest<HTMLElement>('[role="row"]');
  const grid = row?.closest<HTMLElement>(".a-ptable");
  const cell = target.closest<HTMLElement>('[role="gridcell"]');
  if (!row || !grid || !cell || !row.contains(cell)) return null;
  const index = [...row.querySelectorAll(':scope > [role="gridcell"]')].indexOf(cell);
  const header = grid.querySelectorAll<HTMLElement>('[role="columnheader"]')[index];
  const label = header?.querySelector<HTMLElement>(".a-th-label")?.innerText.trim();
  if (!label) return null;
  return { label, value: (cell.querySelector<HTMLElement>(".a-name") ?? cell).innerText.trim() };
}
/** A rate as the details write it: an idle reading in kilobytes too (details.rs `byte_rate`). */
const byteRate = (value: number) => (value < 1 ? "0 KB/s" : rate(value));

// The demo's one leak finding (leaks.rs): a fictional utility whose handles keep rising.
export const LEAK_PID = 18340;
const LEAK = {
  low: 8126,
  handles: 38240,
  span: "30 分钟",
  describe: "30 分钟内句柄数持续上升，增加了 30,114 个，当前 38,240 个",
};

// ---------------------------------------------------------------------------
// The process list's groups (tables/groups.rs): a user's or a category's
// heading and the rows up to the next heading of its level or above; the
// headings held at the top while their group scrolls under them; a map of
// the groups beside the scrollbar, with the group's name by the thumb while
// the list scrolls.
interface ListGroup {
  heading: number;
  end: number;
  /** A category rather than a user's group. */
  category: boolean;
  tone: string;
}
function groupsOf(rows: Row[]): ListGroup[] {
  let users = 0;
  const heads = rows.flatMap((r, i) =>
    r.kind === "user"
      ? [{ i, category: false, tone: r.system ? "system" : "user" + ((users++ % 3) + 1) }]
      : r.kind === "group"
        ? [{ i, category: true, tone: r.group === "app" ? "apps" : (r.group ?? "background") }]
        : [],
  );
  return heads.map((h, n) => ({
    heading: h.i,
    category: h.category,
    tone: h.tone,
    // A category ends at any heading; a user's group at the next user's.
    end: heads.slice(n + 1).find((next) => h.category || !next.category)?.i ?? rows.length,
  }));
}
/** The headings held at the top of a list scrolled `scroll` down, and how far down each is. */
function held(groups: ListGroup[], count: number, scroll: number, rowH: number) {
  const bands: [number, number][] = [];
  let stack = 0;
  if (!count) return bands;
  for (const category of [false, true]) {
    const at = Math.min(count - 1, Math.floor((scroll + stack) / rowH));
    const g = groups.find((g) => g.category === category && g.heading <= at && at < g.end);
    if (!g || g.heading * rowH >= scroll + stack) continue;
    const y = Math.min(g.end * rowH - scroll - rowH, stack);
    if (y <= -rowH) continue;
    bands.push([g.heading, y]);
    stack = y + rowH;
  }
  return bands;
}
/** How long the group's name stays beside the scrollbar once scrolling stops. */
const NAME_HOLD = 1000;
function useGroupScroll(body: RefObject<HTMLDivElement | null>, rows: Row[], groups: ListGroup[]) {
  const pill = useRef<HTMLSpanElement>(null);
  const track = useRef<HTMLDivElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const last = useRef<number | null>(null);
  const covering = useRef(new Set<Element>());
  const [frame, setFrame] = useState({ top: 0, height: 0, total: 0, rowH: 30 });
  const rowHeight = (b: HTMLElement) => b.querySelector<HTMLElement>(":scope > .a-tr")?.offsetHeight || 30;
  const place = (scrolled: boolean) => {
    const b = body.current;
    if (!b) return;
    const rowH = rowHeight(b);
    const scroll = b.scrollTop,
      height = b.clientHeight,
      max = Math.max(0, b.scrollHeight - height);
    // Headings held at the top, the user's above the category's; the lowest ends them with a line.
    const bands = held(groups, rows.length, scroll, rowH);
    for (const g of groups) {
      const el = b.querySelector<HTMLElement>(`:scope > [data-group="${CSS.escape(rows[g.heading]?.key ?? "")}"]`);
      if (!el) continue;
      const band = bands.find(([at]) => at === g.heading);
      if (band) {
        el.dataset.held = "";
        el.style.top = band[1] + "px";
      } else if ("held" in el.dataset) {
        delete el.dataset.held;
        el.style.top = "";
      }
      el.toggleAttribute("data-lowest", !!band && band === bands.at(-1));
    }
    // The rows under the held headings: their names (sticky to the left)
    // would otherwise paint over the headings' edge.
    const covered = bands.length ? bands[bands.length - 1][1] + rowH : 0;
    const under = new Set<Element>();
    if (covered > 0) {
      const from = Math.floor(scroll / rowH),
        to = Math.floor((scroll + covered) / rowH);
      for (const el of Array.from(b.children) as HTMLElement[]) {
        const at = Number(el.dataset.row ?? -1);
        if (at >= from && at <= to && !("held" in el.dataset)) under.add(el);
      }
    }
    for (const el of covering.current) if (!under.has(el)) el.removeAttribute("data-under");
    for (const el of under) el.setAttribute("data-under", "");
    covering.current = under;
    // The group at the top: stronger on the track, named beside the thumb.
    const reach = held(groups, rows.length, scroll + 0.5, rowH).length * rowH;
    const row = Math.min(rows.length - 1, Math.floor((scroll + reach) / rowH));
    const group = [...groups].reverse().find((g) => g.heading <= row && row < g.end);
    track.current?.querySelectorAll<HTMLElement>("[data-heading]").forEach((mark) => {
      mark.toggleAttribute("data-current", Number(mark.dataset.heading) === group?.heading);
    });
    const p = pill.current;
    const moved = last.current !== null && Math.abs(last.current - scroll) > 0.5;
    last.current = scroll;
    if (!p || !group || !scrolled || !moved) return;
    const user = group.category ? groups.find((u) => !u.category && u.heading < group.heading && group.heading < u.end) : undefined;
    const title = translate(rows[group.heading].label, rows[group.heading].kind === "user" ? "user" : "");
    p.dataset.tone = group.tone;
    p.querySelector(".a-group-pill-name")!.textContent = user
      ? `${translate(rows[user.heading].label, "user")} · ${title}`
      : title;
    p.querySelector(".a-group-pill-place")!.textContent = `${Math.max(1, row - group.heading)} / ${group.end - group.heading - 1}`;
    // By the thumb, as the bar draws it, below the headings at the top.
    const thumb = Math.min(height, Math.max(17, Math.round((height / (height + max)) * height)));
    const middle = (scroll / Math.max(1, max)) * (height - thumb) + thumb / 2;
    p.style.top = Math.min(height - 24 - 4, Math.max(reach + 4, middle - 12)) + "px";
    p.dataset.shown = "";
    clearTimeout(timer.current);
    timer.current = setTimeout(() => delete p.dataset.shown, NAME_HOLD);
  };
  useEffect(() => () => clearTimeout(timer.current), []);
  // Where the list is, as it lays out anew (rows, folds, the window's size).
  const measure = () => {
    const b = body.current;
    if (!b) return;
    const next = { top: b.offsetTop, height: b.clientHeight, total: b.scrollHeight, rowH: rowHeight(b) };
    setFrame((f) => (f.top === next.top && f.height === next.height && f.total === next.total && f.rowH === next.rowH ? f : next));
    place(false);
  };
  const latest = useRef(measure);
  latest.current = measure;
  useIsoLayoutEffect(() => latest.current());
  useIsoLayoutEffect(() => {
    const b = body.current;
    if (!b) return;
    const observer = new ResizeObserver(() => latest.current());
    observer.observe(b);
    return () => observer.disconnect();
  }, []);
  const scrollable = frame.total - frame.height >= 0.5;
  // The innermost groups: a user's own when it has no categories.
  const leaves = groups.filter((g) => !groups.some((inner) => inner.heading > g.heading && inner.heading < g.end));
  const rowH = frame.rowH;
  return {
    rowH,
    onScroll: () => place(true),
    track: groups.length > 0 && (
      <div className="a-group-track" ref={track} style={{ top: frame.top, height: frame.height }} aria-hidden="true">
        {scrollable &&
          leaves.map((g) => {
            const top = ((g.heading * rowH) / frame.total) * frame.height;
            const bottom = ((g.end * rowH) / frame.total) * frame.height;
            return (
              <i
                key={rows[g.heading].key}
                data-heading={g.heading}
                data-tone={g.tone}
                style={{ top: top + 1, height: Math.max(2, bottom - top - 2) }}
              />
            );
          })}
        <span className="a-group-pill" ref={pill}>
          <i />
          <span className="a-group-pill-name" />
          <span className="a-group-pill-place" />
        </span>
      </div>
    ),
  };
}

interface DetailsProps {
  entering?: boolean;
  row: Row | null;
  list: Process[];
  tick: number;
  interval: number;
  running: boolean;
  motion: Motion;
  history: (pids: number[]) => { cpu: number[]; mem: number[] };
  efficiency: Set<number>;
  priority: Map<number, string>;
  resourcePriority: Record<string, string>;
  /** Each process's readings now. */
  now: Map<number, Reading>;
  /** Processes the leak alerts mark, and setting the finding aside. */
  leaks: Set<number>;
  onIgnoreLeak: () => void;
  onDeselect: () => void;
  onEnd: (row: Row, confirm: boolean, tree?: boolean) => void;
  onSelectPid: (pid: number) => void;
  onDevice: (device: Device) => void;
  onService: (name: string) => void;
  onCopy: (text: string, what: string) => void;
}
// ---------------------------------------------------------------------------
// What the pane lists about the selected row (details/sheet.rs), which "Copy
// details" also puts on the clipboard, so the copy never leaves out a fact.
interface Fact {
  name: string;
  value: string;
  /** Followed by a quiet part, which copies along. */
  quiet?: string;
  /** Quiet text, such as a reading Argus cannot take. */
  faint?: boolean;
  /** A link that selects this process. */
  link?: number;
  /** A hint on the value. */
  hint?: string;
  /** Where the value's words read differently (i18n `data-i18n`). */
  context?: string;
}
interface Sheet {
  process: Fact[];
  resources: Fact[];
  totals: Fact[];
  paths: string[];
  command?: string;
  version: Fact[];
}
export interface SheetContext extends CellContext {
  list: Process[];
  /** Each process's readings now. */
  now: Map<number, Reading>;
}
const factOf = (ctx: SheetContext, row: Row, key: string) => {
  const c = columnDef(key);
  return c ? cellOf(c, row, ctx).text : MISSING;
};
function detailsSheet(row: Row, proc: Process, ctx: SheetContext): Sheet {
  const group = row.count > 1;
  const members = row.pids.map((pid) => ctx.byPid.get(pid)).filter(Boolean) as Process[];
  const v = row.values;
  const cell = (key: string) => factOf(ctx, row, key);
  const resources: Fact[] = [
    { name: "专用工作集", value: memory(v.mem) },
    { name: "工作集", value: cell("workingSet") },
    // Peaks and address ranges are a single process's.
    ...(group ? [] : [{ name: "工作集峰值", value: cell("peakWorkingSet") }]),
    { name: "提交大小", value: cell("commit") },
    ...(group ? [] : [{ name: "虚拟地址空间", value: cell("virtualSize") }]),
    { name: "磁盘读取", value: byteRate(v.read), hint: "每秒磁盘读取字节数，不包含网络及其他设备 I/O" },
    { name: "磁盘写入", value: byteRate(v.write), hint: "每秒磁盘写入字节数，不包含网络及其他设备 I/O" },
    { name: "I/O", value: byteRate(v.io), hint: "读、写及其他 I/O 合计，包含文件、网络和设备访问" },
    // The share of the busiest engine, which it names.
    { name: "GPU", value: `${percent(Math.min(100, v.gpu))}%`, quiet: v.gpu >= 0.05 ? `（${gpuEngine(row.pids)}）` : undefined },
    { name: "专用 GPU 内存", value: cell("gpuDedicated") },
    { name: "共享 GPU 内存", value: cell("gpuShared") },
    { name: "网络", value: `发送 ${byteRate(v.net * 0.22)}，接收 ${byteRate(v.net * 0.78)}` },
    {
      name: "电源使用情况",
      value: `${POWER_LEVELS[powerLevel(powerEstimate(v))]} · 趋势：${POWER_LEVELS[powerLevel(members.reduce((a, p) => a + trendEstimate(p), 0))]}`,
      context: "power",
    },
    { name: "线程", value: thousands(members.reduce((a, p) => a + p.threads, 0)) },
    { name: "句柄", value: thousands(members.reduce((a, p) => a + p.handles, 0)) },
    ...(group ? [{ name: "最早启动", value: members.map((p) => p.started).sort()[0] ?? MISSING }] : []),
  ];
  const parent = ctx.list.find((p) => p.pid === proc.ppid);
  const level = (kind: "I/O 优先级" | "内存优先级") => resourceLevel(ctx, kind, proc) || MISSING;
  const process: Fact[] = group
    ? []
    : [
        { name: "PID", value: String(proc.pid) },
        parent
          ? { name: "父进程", value: `${parent.exe}（${parent.pid}）`, link: parent.pid }
          : { name: "父进程", value: `无（PID ${proc.ppid}）`, faint: true },
        { name: "启动时间", value: proc.started },
        { name: "用户", value: proc.user },
        { name: "完整性", value: proc.integrity },
        { name: "架构", value: architecture(proc) || MISSING },
        { name: "优先级", value: ctx.priority.get(proc.pid) ?? "正常" },
        { name: "I/O 优先级", value: level("I/O 优先级") },
        { name: "内存优先级", value: level("内存优先级") },
        { name: "效率模式", value: ctx.efficiency.has(proc.pid) ? "开启" : "关闭", context: "state" },
        { name: "已暂停", value: proc.suspended ? "是" : "否" },
        { name: "会话", value: String(sessionOf(proc)) },
      ];
  // Counts since the start, added up over an app group's members.
  const totals: Fact[] = [
    { name: "CPU 时间", value: cell("cpuTime") },
    { name: "用户 CPU 时间", value: cell("cpuUserTime") },
    { name: "内核 CPU 时间", value: cell("cpuKernelTime") },
    { name: "I/O 读取", value: cell("ioReadTotal") },
    { name: "I/O 写入", value: cell("ioWriteTotal") },
    { name: "其他 I/O", value: cell("ioOtherTotal") },
    { name: "缺页次数", value: cell("pageFaults") },
  ];
  const version = versionInfo(proc);
  return {
    process,
    resources,
    totals,
    paths: [...new Set(members.map((p) => p.path))],
    command: group || !proc.args ? undefined : `"${proc.path}" ${proc.args}`,
    version: version
      ? [
          { name: "产品", value: version[0] },
          { name: "版本", value: version[1] },
        ]
      : [],
  };
}
/** The row's details as text, as the pane lists them ("Copy details", Ctrl+Shift+C). */
export function detailsText(row: Row, ctx: SheetContext) {
  const proc = ctx.byPid.get(row.pids[0]);
  if (!proc) return row.label;
  const sheet = detailsSheet(row, proc, ctx);
  const group = row.count > 1;
  const line = (f: Fact) => translate(`${f.name}：${f.value}${f.quiet ?? ""}`, f.context);
  const lines = (facts: Fact[]) => facts.filter((f) => f.value !== MISSING).map(line);
  const hosted = services.filter((s) => s.pid && row.pids.includes(s.pid));
  const members = row.pids.map((pid) => ctx.byPid.get(pid)).filter(Boolean) as Process[];
  const sections: [string, string[]][] = [
    ["进程", lines(sheet.process)],
    ["资源", lines(sheet.resources)],
    ["累计", lines(sheet.totals)],
    [`运行的服务（${hosted.length}）`, hosted.map((s) => `${s.display} · ${s.name}`)],
    ...(group
      ? ([
          [
            `进程（${members.length}）`,
            [...members]
              .sort((a, b) => a.pid - b.pid)
              .map((p) => {
                const r = ctx.now.get(p.pid);
                return [`PID ${p.pid}`, r ? `${percent(r.cpu)}%` : "", r ? memory(r.mem) : ""].filter(Boolean).join(" · ");
              }),
          ],
        ] as [string, string[]][])
      : []),
    ["文件位置", sheet.paths],
    ["命令行", sheet.command ? [sheet.command] : []],
    ["文件版本", lines(sheet.version)],
  ];
  const head = [translate(row.label), [translate(group ? `${proc.exe} · ${row.count} 个进程` : proc.exe), company(proc)].filter(Boolean).join(" · ")];
  return [
    head.filter(Boolean).join("\n"),
    ...sections.filter(([, l]) => l.length).map(([name, l]) => [translate(`【${name}】`), ...l].join("\n")),
  ].join("\n\n");
}
function Facts({ facts, onSelectPid }: { facts: Fact[]; onSelectPid?: (pid: number) => void }) {
  return (
    <dl className="a-facts">
      {facts.map((f) => (
        <div key={f.name}>
          <dt>{f.name}</dt>
          <dd className={f.faint ? "is-faint" : undefined} title={f.hint} data-i18n={f.context}>
            {f.link !== undefined ? (
              <button type="button" className="a-link" onClick={() => onSelectPid?.(f.link!)}>
                {f.value}
              </button>
            ) : (
              f.value
            )}
            {f.quiet && <span className="is-faint">{f.quiet}</span>}
          </dd>
        </div>
      ))}
    </dl>
  );
}
function PathRow({ path, onCopy }: { path: string; onCopy: (text: string, what: string) => void }) {
  return (
    <div className="a-path">
      <p className="a-code">{path}</p>
      <div>
        <button type="button" className="a-icon-btn" title="打开文件位置" aria-label="打开文件位置">
          <Icon name="folder" size={14} />
        </button>
        <button type="button" className="a-icon-btn" title="复制路径" aria-label="复制路径" onClick={() => onCopy(path, "路径")}>
          <Icon name="copy" size={14} />
        </button>
      </div>
    </div>
  );
}
// A details card settles into place as it opens: up from 6 px below, fading in.
const settle = (el: HTMLDivElement | null) => slideIn(el, { y: 6, fade: true });
/** The pane's width: 348 px at first, dragged between 300 and 560 (details.rs). */
const DETAILS_WIDTH = 348;
const DETAILS_MIN = 300;
const DETAILS_MAX = 560;
let detailsWidth = DETAILS_WIDTH;
export function Details(d: DetailsProps) {
  const row = d.row;
  const proc = row ? d.list.find((p) => p.pid === row.pids[0]) : undefined;
  // The pane, opened, moves in from the right.
  const pane = useRef<HTMLElement>(null);
  useIsoLayoutEffect(() => {
    if (d.entering) slideIn(pane.current, { x: 24 });
  }, [d.entering]);
  // The handle on the pane's left edge: drag it, use the arrow keys, Home or
  // End, or double-click to go back to the default. Never more than 45% of
  // the window.
  const [width, setWidth] = useState(detailsWidth);
  const [dragging, setDragging] = useState(false);
  const resize = (next: number) => {
    detailsWidth = Math.round(Math.min(DETAILS_MAX, Math.max(DETAILS_MIN, next)));
    setWidth(detailsWidth);
  };
  const press = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || event.detail >= 2) return;
    const handle = event.currentTarget;
    handle.setPointerCapture(event.pointerId);
    const k = scaleOf(handle);
    const x0 = event.clientX,
      w0 = pane.current?.offsetWidth ?? width;
    setDragging(true);
    const move = (e: PointerEvent) => resize(w0 - (e.clientX - x0) / k);
    const up = () => {
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", up);
      handle.removeEventListener("pointercancel", up);
      setDragging(false);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up);
    handle.addEventListener("pointercancel", up);
  };
  return (
    <aside
      className="a-details"
      ref={pane}
      aria-label="详细信息面板"
      data-tour="details"
      style={{ width: `min(${width}px, 45cqw)` }}
    >
      <div
        className={"a-details-splitter" + (dragging ? " is-dragging" : "")}
        role="separator"
        aria-orientation="vertical"
        aria-label="调整详细信息面板宽度"
        aria-valuenow={width}
        aria-valuemin={DETAILS_MIN}
        aria-valuemax={DETAILS_MAX}
        tabIndex={0}
        onPointerDown={press}
        onDoubleClick={() => resize(DETAILS_WIDTH)}
        onKeyDown={(event) => {
          // The pane sits right of its handle: moving the handle left widens it.
          const next =
            event.key === "ArrowLeft"
              ? width + 16
              : event.key === "ArrowRight"
                ? width - 16
                : event.key === "Home"
                  ? DETAILS_MIN
                  : event.key === "End"
                    ? DETAILS_MAX
                    : null;
          if (next === null) return;
          event.preventDefault();
          event.stopPropagation();
          resize(next);
        }}
      />
      {row && proc ? (
        <ProcessDetails key={row.key} {...d} row={row} proc={proc} />
      ) : (
        <Overview key="overview" onDevice={d.onDevice} onCopy={d.onCopy} />
      )}
    </aside>
  );
}
/** Starts watching a process's runs and shows them on the History page (process_watch.rs). */
export function startWatch(pid: number, tick: number) {
  store.set({ watch: { pid, start: tick, stop: null }, request: { page: "history", at: Date.now() } });
}
function ProcessDetails(d: DetailsProps & { row: Row; proc: Process }) {
  const { row, proc } = d;
  const group = row.count > 1;
  const members = row.pids.map((pid) => d.list.find((p) => p.pid === pid)).filter(Boolean) as Process[];
  const h = d.history(row.pids);
  const memMax = Math.max(...h.mem) * 1.25 || 1;
  const curve = { tick: d.tick, interval: d.interval, running: d.running, motion: d.motion };
  const hosted = services.filter((s) => s.pid && row.pids.includes(s.pid));
  const ctx: SheetContext = {
    byPid: new Map(d.list.map((p) => [p.pid, p])),
    tick: d.tick,
    efficiency: d.efficiency,
    priority: d.priority,
    resource: d.resourcePriority,
    leaks: d.leaks,
    list: d.list,
    now: d.now,
  };
  const sheet = detailsSheet(row, proc, ctx);
  const vendor = company(proc);
  const leak = row.pids.some((pid) => d.leaks.has(pid));
  return (
    <div className="a-details-body" ref={settle}>
      <div className="a-details-scroll a-scroll">
        <header className="a-details-head">
          <span className="a-head-glyph">
            <ProcessIcon glyph={proc.glyph} size={32} />
          </span>
          <div className="a-details-title">
            <h3 title={row.label}>{row.label}</h3>
            <p>{group ? `${proc.exe} · ${row.count} 个进程` : proc.exe}</p>
            {vendor && <p>{vendor}</p>}
          </div>
          {/* Copy and deselect, with watching the process under them (details.rs). */}
          <div className="a-details-tools is-stack">
            <div>
              <button
                type="button"
                className="a-icon-btn"
                data-details-copy
                title="复制详细信息（Ctrl+Shift+C）"
                aria-label="复制详细信息（Ctrl+Shift+C）"
                onClick={() => d.onCopy(detailsText(row, ctx), "详细信息")}
              >
                <Icon name="copy" size={14} />
              </button>
              <button type="button" className="a-icon-btn" aria-label="取消选中" title="取消选中" onClick={d.onDeselect}>
                <Icon name="dismiss" size={14} />
              </button>
            </div>
            <button
              type="button"
              className="a-icon-btn"
              aria-label="观察进程"
              title="观察进程"
              onClick={() => startWatch((members[0] ?? proc).pid, d.tick)}
            >
              <Icon name="eye" size={14} />
            </button>
          </div>
        </header>
        {/* A leak alert comes first after the heading: it is why to look here (leaks.rs `leak_card`). */}
        {leak && <LeakCard onIgnore={d.onIgnoreLeak} />}
        <div className="a-minis">
          <div className="a-mini">
            <span>CPU</span>
            <b>{percent(row.values.cpu)}%</b>
            <Curve series={[{ values: h.cpu, color: "var(--a-cpu)" }]} max={100} {...curve} />
          </div>
          <div className="a-mini">
            <span>内存</span>
            <b>{memory(row.values.mem)}</b>
            <Curve series={[{ values: h.mem, color: "var(--a-mem)" }]} max={memMax} {...curve} />
          </div>
        </div>
        {hosted.length > 0 && (
          <>
            <h4>运行的服务（{hosted.length}）</h4>
            <div className="a-pick-list">
              {hosted.map((s) => (
                <button type="button" key={s.name} title={s.description} onClick={() => d.onService(s.name)}>
                  <span className="is-main">{s.display}</span>
                  <span className="is-side">{s.name}</span>
                </button>
              ))}
            </div>
          </>
        )}
        {!group && (
          <>
            <h4>进程</h4>
            <Facts facts={sheet.process} onSelectPid={d.onSelectPid} />
          </>
        )}
        <h4>资源</h4>
        <Facts facts={sheet.resources} />
        <h4>累计</h4>
        <Facts facts={sheet.totals} />
        {group && (
          <>
            <h4>进程（{row.count}）</h4>
            <div className="a-pick-list">
              {members
                .map((p) => ({ p, r: d.now.get(p.pid) }))
                .sort((a, b) => (b.r?.cpu ?? 0) - (a.r?.cpu ?? 0) || (b.r?.mem ?? 0) - (a.r?.mem ?? 0))
                .map(({ p, r }) => (
                  <button
                    type="button"
                    key={p.pid}
                    title={`查看 PID ${p.pid} 的详细信息`}
                    onClick={() => d.onSelectPid(p.pid)}
                  >
                    <span className="is-main">PID {p.pid}</span>
                    <span className="is-num">{percent(r?.cpu ?? 0)}%</span>
                    <span className="is-num is-wide">{memory(r?.mem ?? p.mem)}</span>
                  </button>
                ))}
            </div>
          </>
        )}
        {sheet.paths.length > 0 && <h4>文件位置</h4>}
        {sheet.paths.map((path) => (
          <PathRow key={path} path={path} onCopy={d.onCopy} />
        ))}
        {sheet.command && (
          <>
            <h4>命令行</h4>
            <p className="a-code">{sheet.command}</p>
          </>
        )}
        {sheet.version.length > 0 && (
          <>
            <h4>文件版本</h4>
            <Facts facts={sheet.version} />
          </>
        )}
      </div>
      <div className="a-details-actions">
        <footer className="a-details-foot">
          <button type="button" className="a-btn is-danger" onClick={() => d.onEnd(row, true)}>
            {group ? "全部结束" : "结束进程"}
          </button>
          {!group && (
            <button type="button" className="a-btn" onClick={() => d.onEnd(row, true, true)}>
              结束进程树
            </button>
          )}
        </footer>
      </div>
    </div>
  );
}

/** The details pane's notice for the process the alert marks: what was seen,
 *  how the count went, what can be done, and the way to stop the alert. */
function LeakCard({ onIgnore }: { onIgnore: () => void }) {
  // The handle count over the last half hour, rising from 8,126 to 38,240.
  const trend = Array.from({ length: 31 }, (_, i) => LEAK.low + (LEAK.handles - LEAK.low) * (i / 30) ** 1.15 + noise(LEAK_PID + i, 0) * 600 * (i ? 1 : 0));
  trend[30] = LEAK.handles;
  const low = Math.min(...trend),
    top = Math.max(...trend);
  const points = trend.map((v, i) => `${((i / 30) * 100).toFixed(2)},${(31 - ((v - low) / (top - low)) * 30).toFixed(2)}`).join(" ");
  return (
    <div className="a-leak-card" role="note">
      <div className="a-leak-card-head">
        <Icon name="warning" size={14} />
        <b>可能存在句柄泄漏</b>
        <button type="button" className="a-btn is-small is-ghost" onClick={onIgnore}>
          忽略此程序
        </button>
      </div>
      <p>{LEAK.describe}</p>
      <div className="a-leak-trend">
        <span>{`句柄数 · 近 ${LEAK.span}`}</span>
        <span>{`${thousands(LEAK.low)} → ${thousands(LEAK.handles)}`}</span>
      </div>
      <svg className="a-leak-spark" viewBox="0 0 100 32" preserveAspectRatio="none" aria-hidden="true">
        <polyline points={points} fill="none" stroke="currentColor" strokeWidth="1.5" vectorEffect="non-scaling-stroke" />
      </svg>
      <p className="a-leak-advice">结束或重新启动该进程即可释放这些句柄；若反复出现，可更新或卸载其所属软件。</p>
    </div>
  );
}

const parts: { device: Device; icon: UiIcon; color: string; kind: string; name: string; detail: string }[] = [
  {
    device: "cpu",
    icon: "chip",
    color: "var(--a-cpu)",
    kind: "处理器",
    name: machine.cpu,
    detail: "14 核（6 性能核 + 8 能效核） · 20 线程 · 基准 2.50 GHz",
  },
  { device: "memory", icon: "memory", color: "var(--a-mem)", kind: "内存", name: "32 GB DDR5", detail: "5600 MT/s · 已用插槽 2 / 2 · SODIMM" },
  // Every disk and GPU, the adapters but the virtual ones (details.rs overview).
  ...disks.map((d) => ({
    device: d.id,
    icon: "disk" as UiIcon,
    color: d.color,
    kind: "存储",
    name: d.model,
    detail: `${(d.size / 1024 ** 4).toFixed(1)} TB · ${d.kind} · ${d.volumes.join(" ")}`,
  })),
  ...adapters
    .filter((a) => a.kind !== "virtual")
    .map((a) => ({ device: a.id, icon: "network" as UiIcon, color: a.color, kind: "Wi-Fi", name: a.model, detail: `${a.name} · ${(a.link / 1e9).toFixed(1)} Gbps` })),
  ...gpus.map((g) => ({
    device: g.id,
    icon: "gpu" as UiIcon,
    color: g.color,
    kind: "显卡",
    name: g.model,
    detail: g.dedicated > 0 ? `${(g.dedicated / 1024 ** 3).toFixed(1)} GB 专用显存` : "使用共享内存",
  })),
  { device: "battery", icon: "battery", color: "var(--a-bat)", kind: "电池", name: "Contoso CT-4410", detail: "设计容量 80 Wh · 健康度 90%" },
];
function Overview({ onDevice, onCopy }: { onDevice: (device: Device) => void; onCopy: (text: string, what: string) => void }) {
  const [visible, setVisible] = useState(true);
  const [tipsOpen, setTipsOpen] = useState(true);
  const text = ["本机配置", ...parts.map((p) => `${p.kind}：${p.name}\n${p.detail}`)].join("\n\n");
  return (
    <div className="a-details-body" ref={settle}>
      <div className="a-details-scroll a-scroll">
        <header className="a-details-head is-center">
          <span className="a-head-glyph">
            <Icon name="computer" size={32} />
          </span>
          <div className="a-details-title">
            <h3>本机配置</h3>
          </div>
          <div className="a-details-tools">
            <button type="button" className="a-icon-btn" title="复制配置为文字" aria-label="复制配置为文字" onClick={() => onCopy(text, "本机配置")}>
              <Icon name="copy" />
            </button>
            <button
              type="button"
              className="a-icon-btn"
              title={visible ? "隐藏本机配置" : "显示本机配置"}
              aria-label={visible ? "隐藏本机配置" : "显示本机配置"}
              aria-pressed={!visible}
              onClick={() => setVisible(!visible)}
            >
              <Icon name={visible ? "eye" : "eyeOff"} />
            </button>
          </div>
        </header>
        <div className="a-tips">
          <button
            type="button"
            className="a-tips-head"
            aria-label={tipsOpen ? "收起操作提示" : "展开操作提示"}
            aria-expanded={tipsOpen}
            onClick={() => setTipsOpen(!tipsOpen)}
          >
            <Icon name="tip" size={14} />
            <span>操作提示</span>
            <Icon name={tipsOpen ? "chevronUp" : "chevronDown"} size={14} />
          </button>
          {tipsOpen && (
            <div className="a-tips-body">
              <p>
                <Icon name="pointer" size={14} />
                <span>单击应用或进程查看详细信息，再次单击取消选中。</span>
              </p>
              <p>
                <Icon name="panel" size={14} />
                <span>
                  单击页面栏的
                  <span className="a-key" aria-label="详细信息面板按钮">
                    <Icon name="panel" size={12} />
                  </span>
                  可收起面板。
                </span>
              </p>
            </div>
          )}
        </div>
        {visible && (
          <ul className="a-hardware">
            {parts.map((part) => (
              <li key={part.device}>
                <button type="button" onClick={() => onDevice(part.device)}>
                  <span className="a-chip" style={{ "--c": part.color } as CSSProperties}>
                    <Icon name={part.icon} />
                  </span>
                  <span className="a-hardware-text">
                    <small>{part.kind}</small>
                    <b>{part.name}</b>
                    <span>{part.detail}</span>
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
