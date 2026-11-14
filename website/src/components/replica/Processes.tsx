/** @jsxImportSource @/components/replica/i18n */
"use client";
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
} from "react";
import {
  alignsEnd,
  cellOf,
  NAME_MIN,
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
  bytes,
  clock,
  groupLabel,
  machine,
  noise,
  percent,
  rate,
  rateCeiling,
  sampleAt,
  samples,
  services,
  sessionOf,
  thousands,
  totalMemory,
  userName,
  type Glyph,
  type Group,
  type Process,
  type Reading,
} from "./model";
import { scaleOf, slideIn, useIsoLayoutEffect } from "./widgets";

export type View = "apps" | "tree" | "list";
/** A process column's key (Columns.tsx). */
export type SortKey = string;
export type Device = "cpu" | "memory" | "disk" | "wifi" | "gpu" | "battery";

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
      const expanded = searching || !o.closed.has(key);
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
          row.expanded = row.expandable && (o.treeOpen.has(row.key) || !!o.query.trim());
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
      row.expanded = row.expandable && (o.open.has(row.key) || shown.length < members.length);
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
      const expanded = !o.closed.has(key) || !!o.query.trim();
      out.push({
        key,
        kind: "group",
        depth: 0,
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
    const points = Array.from({ length: samples }, (_, i) => sampleAt(tick - samples + 1 + i));
    return {
      cpu: points.map((p) => p.cpu),
      kernel: points.map((p) => p.kernel),
      mem: points.map((p) => p.mem),
      disk: points.map((p) => p.read + p.write),
      read: points.map((p) => p.read),
      write: points.map((p) => p.write),
      active: points.map((p) => p.active),
      rx: points.map((p) => p.rx),
      tx: points.map((p) => p.tx),
      net: points.map((p) => p.rx + p.tx),
      gpu: points.map((p) => p.gpu),
    };
  }, [tick]);
}
export const cpuTemperature = (t: number) => 62 + sampleAt(t).cpu * 0.32 + noise(31, t) * 2;
export const cpuPower = (t: number) => 28 + sampleAt(t).cpu * 0.82 + noise(32, t) * 4;
export const gpuTemperature = (t: number) => 50 + noise(7, t) * 4;
export const gpuPower = (t: number) => 18 + sampleAt(t).gpu * 0.2;
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
  const [diskValue, diskUnit] = s.read + s.write < 1 ? ["0", "KB/s"] : split(rate(s.read + s.write));
  const [netValue, netUnit] = s.rx + s.tx < 1 ? ["0", "KB/s"] : split(rate(s.rx + s.tx));
  const cards: {
    id: Device;
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
    },
    {
      id: "disk",
      name: "磁盘",
      color: "var(--a-io)",
      values: h.disk,
      max: diskMax,
      scale: rate(diskMax),
      figure: diskValue,
      unit: diskUnit,
      readings: [`活动 ${Math.round(s.active)}%`],
    },
    {
      id: "wifi",
      name: "网络",
      color: "var(--a-net)",
      values: h.net,
      max: netMax,
      scale: rate(netMax),
      figure: netValue,
      unit: netUnit,
      readings: [`收 ${rate(s.rx)} 发 ${rate(s.tx)}`],
    },
    {
      id: "gpu",
      name: "GPU",
      color: "var(--a-gpu)",
      values: h.gpu,
      max: 100,
      scale: "100%",
      figure: String(Math.round(s.gpu)),
      unit: "%",
      readings: [`${Math.round(gpuTemperature(tick))} °C`, `${Math.round(gpuPower(tick))} W`],
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
            onClick={() => onOpen(card.id)}
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
            <span className="a-figure">
              <b>{card.figure}</b>
              <small>{card.unit}</small>
            </span>
            <span className={"a-vital-readings" + (card.readings.length > 1 ? " is-split" : "")}>
              {card.readings.map((r) => (
                <span key={r.slice(-3)}>{r}</span>
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
interface TableProps {
  tick: number;
  rows: Row[];
  total: number;
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
  const others = t.columns.slice(1);
  const ctx = useMemo<CellContext>(
    () => ({ byPid: new Map(t.list.map((p) => [p.pid, p])), tick: t.tick, efficiency: t.efficiency, priority: t.priority }),
    [t.list, t.tick, t.efficiency, t.priority],
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
  const { widths, fill } = useColumnWidths(t.columns, t.sizing, t.text, t.rows, ctx, room);
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
  const reveal = (key: string) =>
    body.current?.querySelector(`[data-key="${CSS.escape(key)}"]`)?.scrollIntoView({ block: "nearest" });
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
  const sortMark = (key: SortKey) =>
    t.sort === key ? <Icon name={t.descending ? "chevronDown" : "chevronUp"} size={12} /> : null;
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
              {t.filtered && <Icon name="filter" size={13} />}
              {t.total} 个进程
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
              title={c.title}
              aria-sort={ariaSort(c.key)}
              data-tour={"col-" + c.key}
              data-col={c.key}
              onPointerDown={(event) => drag.press(event, c.key)}
              onClick={() => !drag.dragged() && t.onSort(c.key)}
            >
              <span className="a-th-total">
                <TotalFor column={c.key} tick={t.tick} list={t.list} />
              </span>
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
            <p className="a-empty">没有匹配的进程</p>
          ))}
        {t.rows.map((row) =>
          row.kind === "user" ? (
            <div
              key={row.key}
              role="row"
              data-group={row.key}
              aria-expanded={row.expanded}
              className={"a-tr a-user" + (row.expanded ? "" : " is-folded")}
              onClick={() => t.onToggle(row)}
              onContextMenu={(event) => {
                event.preventDefault();
                t.onHeadingMenu(event, row);
              }}
            >
              <span role="gridcell" className="a-td a-td-name a-col-name">
                <span className="a-fold" aria-expanded={row.expanded}>
                  <Icon name="chevronRight" size={12} />
                </span>
                <Icon name={row.system ? "computer" : "user"} />
                <span className="a-user-name" data-i18n="user">
                  {row.label}
                </span>
                <span className="a-user-detail">{row.detail}</span>
              </span>
              {others.map((c) => (
                <span role="gridcell" key={c.key} className={`a-td a-col-${c.key}` + align(c)} style={{ width: width(c) }}>
                  {c.aggregate ? cellOf(c, row, ctx).text : ""}
                </span>
              ))}
            </div>
          ) : row.kind === "group" ? (
            <div
              key={row.key}
              role="row"
              data-group={row.key}
              aria-expanded={row.expanded}
              className={"a-tr a-group" + (row.expanded ? "" : " is-folded") + (row.nested ? " is-nested" : "")}
              onClick={() => t.onToggle(row)}
              onContextMenu={(event) => {
                event.preventDefault();
                t.onHeadingMenu(event, row);
              }}
            >
              <span role="gridcell" className="a-td a-td-name a-col-name">
                <span className="a-fold" aria-expanded={row.expanded}>
                  <Icon name="chevronRight" size={12} />
                </span>
                <span className="a-group-name">{row.label}</span>
                <span className="a-group-count">{row.count}</span>
                <span className="a-group-rule" />
              </span>
              {others.map((c) => (
                <span role="gridcell" key={c.key} className={`a-td a-col-${c.key}` + align(c)} style={{ width: width(c) }}>
                  {c.aggregate ? cellOf(c, row, ctx).text : ""}
                </span>
              ))}
            </div>
          ) : (
            <div
              key={row.key}
              data-key={row.key}
              role="row"
              aria-selected={t.selected === row.key}
              aria-expanded={row.expandable ? row.expanded : undefined}
              className={"a-tr" + (t.selected === row.key ? " is-selected" : "")}
              onClick={(event) => {
                t.onSelect(t.selected === row.key ? null : row.key);
                if (event.detail === 2 && row.expandable) t.onToggle(row);
              }}
              onContextMenu={(event) => {
                event.preventDefault();
                t.onSelect(row.key);
                t.onMenu(event, row);
              }}
            >
              <span
                role="gridcell"
                className="a-td a-td-name a-col-name"
                style={{ paddingLeft: 6 + (row.nested ? USER_INSET : 0) + row.depth * 20 }}
                title={row.label + "\n" + (row.exe ?? "")}
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
                {row.pids.some((pid) => t.efficiency.has(pid)) && (
                  <span title="效率模式" className="a-leaf">
                    <Icon name="leaf" size={12} />
                  </span>
                )}
                {row.suspended && <span className="a-tag">已暂停</span>}
              </span>
              {others.map((c) => {
                const cell = cellOf(c, row, ctx);
                // Long text ends in an ellipsis; hovering shows all of it.
                const long = c.format === "text" && !!cell.text;
                return (
                  <span
                    role="gridcell"
                    key={c.key}
                    className={
                      `a-td a-col-${c.key}` + align(c) + (cell.faint ? " is-faint" : "") + (cell.heat ? " is-heat" : "")
                    }
                    style={
                      cell.heat ? ({ width: width(c), "--h": cell.heat.toFixed(3) } as CSSProperties) : { width: width(c) }
                    }
                    title={long ? cell.text : undefined}
                  >
                    {long ? <span className="a-cell-text">{cell.text}</span> : cell.text}
                  </span>
                );
              })}
            </div>
          ),
        )}
      </div>
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
// threads and handles count the whole system, as the status bar does.
function TotalFor({ column, tick, list }: { column: string; tick: number; list: Process[] }) {
  const s = sampleAt(tick);
  switch (column) {
    case "cpu":
      return <>{Math.round(s.cpu)}%</>;
    case "mem":
      return <>{Math.round(s.mem)}%</>;
    case "diskRead":
      return <>{rate(s.read)}</>;
    case "diskWrite":
      return <>{rate(s.write)}</>;
    case "io":
      return <>{rate((s.read + s.write) * 1.12 + s.rx * 0.05)}</>;
    case "gpu":
      return <>{Math.round(s.gpu)}%</>;
    case "net":
      return <>{rate(s.rx + s.tx)}</>;
    case "threads":
      return <>{thousands(list.reduce((a, p) => a + p.threads, 1180))}</>;
    case "handles":
      return <>{thousands(list.reduce((a, p) => a + p.handles, 52400))}</>;
    default:
      return null;
  }
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
  onDeselect: () => void;
  onEnd: (row: Row, confirm: boolean, tree?: boolean) => void;
  onSelectPid: (pid: number) => void;
  onDevice: (device: Device) => void;
  onService: (name: string) => void;
  onCopy: (text: string, what: string) => void;
}
const cpuTime = (seconds: number) =>
  `${Math.floor(seconds / 3600)}:${String(Math.floor((seconds % 3600) / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`;
function Facts({ rows }: { rows: [string, ReactNode][] }) {
  return (
    <dl className="a-facts">
      {rows.map(([name, value]) => (
        <div key={name}>
          <dt>{name}</dt>
          <dd>{value}</dd>
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
export function Details(d: DetailsProps) {
  const row = d.row;
  const proc = row ? d.list.find((p) => p.pid === row.pids[0]) : undefined;
  // The pane, opened, moves in from the right.
  const pane = useRef<HTMLElement>(null);
  useIsoLayoutEffect(() => {
    if (d.entering) slideIn(pane.current, { x: 24 });
  }, [d.entering]);
  return (
    <aside className="a-details" ref={pane} aria-label="详细信息面板" data-tour="details">
      {row && proc ? (
        <ProcessDetails key={row.key} {...d} row={row} proc={proc} />
      ) : (
        <Overview key="overview" onDevice={d.onDevice} onCopy={d.onCopy} />
      )}
    </aside>
  );
}
function ProcessDetails(d: DetailsProps & { row: Row; proc: Process }) {
  const { row, proc } = d;
  const group = row.count > 1;
  const members = row.pids.map((pid) => d.list.find((p) => p.pid === pid)).filter(Boolean) as Process[];
  const parent = d.list.find((p) => p.pid === proc.ppid);
  const h = d.history(row.pids);
  const memMax = Math.max(...h.mem) * 1.25 || 1;
  const curve = { tick: d.tick, interval: d.interval, running: d.running, motion: d.motion };
  const hosted = services.filter((s) => s.pid && row.pids.includes(s.pid));
  const total = (key: "threads" | "handles") => thousands(members.reduce((a, p) => a + p[key], 0));
  const net = row.values.net;
  const version = versionInfo(proc);
  const resources: [string, ReactNode][] = [
    ["专用工作集", bytes(row.values.mem)],
    ["工作集", bytes(row.values.mem * 1.38)],
    ["提交大小", bytes(row.values.mem * 1.17)],
    ["磁盘读取", rate(row.values.read)],
    ["磁盘写入", rate(row.values.write)],
    ["GPU", row.values.gpu >= 0.05 ? `${percent(row.values.gpu)}%（3D）` : "0%"],
    ["网络", `发送 ${rate(net * 0.22)}，接收 ${rate(net * 0.78)}`],
    ["线程", total("threads")],
    ["句柄", total("handles")],
  ];
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
          </div>
          <button type="button" className="a-icon-btn" aria-label="取消选中" title="取消选中" onClick={d.onDeselect}>
            <Icon name="dismiss" size={14} />
          </button>
        </header>
        <div className="a-minis">
          <div className="a-mini">
            <span>CPU</span>
            <b>{percent(row.values.cpu)}%</b>
            <Curve series={[{ values: h.cpu, color: "var(--a-cpu)" }]} max={100} {...curve} />
          </div>
          <div className="a-mini">
            <span>内存</span>
            <b>{bytes(row.values.mem)}</b>
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
        {group ? (
          <>
            <h4>资源</h4>
            <Facts
              rows={[
                ...resources,
                ["CPU 时间", cpuTime(members.reduce((a, p) => a + p.cpuTime, 0))],
                ["最早启动", members.map((p) => p.started).sort()[0]],
              ]}
            />
            <h4>进程（{row.count}）</h4>
            <div className="a-pick-list">
              {members
                .map((p) => ({ p, r: d.history([p.pid]) }))
                .sort((a, b) => (b.r.cpu.at(-1) ?? 0) - (a.r.cpu.at(-1) ?? 0))
                .map(({ p, r }) => {
                  return (
                    <button
                      type="button"
                      key={p.pid}
                      title={`查看 PID ${p.pid} 的详细信息`}
                      onClick={() => d.onSelectPid(p.pid)}
                    >
                      <span className="is-main">PID {p.pid}</span>
                      <span className="is-num">{percent(r.cpu.at(-1) ?? 0)}%</span>
                      <span className="is-num is-wide">{bytes(r.mem.at(-1) ?? p.mem)}</span>
                    </button>
                  );
                })}
            </div>
            <h4>文件位置</h4>
            {[...new Set(members.map((p) => p.path))].map((path) => (
              <PathRow key={path} path={path} onCopy={d.onCopy} />
            ))}
          </>
        ) : (
          <>
            <h4>进程</h4>
            <Facts
              rows={[
                ["PID", proc.pid],
                [
                  "父进程",
                  parent ? (
                    <button type="button" className="a-link" onClick={() => d.onSelectPid(parent.pid)}>
                      {parent.exe}（{parent.pid}）
                    </button>
                  ) : (
                    `无（PID ${proc.ppid}）`
                  ),
                ],
                ["启动时间", proc.started],
                ["CPU 时间", cpuTime(proc.cpuTime)],
                ["用户", proc.user],
                ["完整性", proc.integrity],
                ["架构", "64 位 (x64)"],
                ["优先级", d.priority.get(proc.pid) ?? "正常"],
                ["效率模式", d.efficiency.has(proc.pid) ? "开启" : "关闭"],
                ["会话", proc.group === "windows" && proc.user.startsWith("NT") ? "0" : "1"],
              ]}
            />
            <h4>资源</h4>
            <Facts rows={resources} />
            <h4>文件位置</h4>
            <PathRow path={proc.path} onCopy={d.onCopy} />
            {proc.args && (
              <>
                <h4>命令行</h4>
                <p className="a-code">
                  "{proc.path}" {proc.args}
                </p>
              </>
            )}
          </>
        )}
        {version && (
          <>
            <h4>文件版本</h4>
            <Facts
              rows={[
                ["产品", version[0]],
                ["版本", version[1]],
              ]}
            />
          </>
        )}
      </div>
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
  { device: "disk", icon: "disk", color: "var(--a-io)", kind: "存储", name: machine.disk, detail: "1.8 TB · SSD (NVMe) · C:" },
  { device: "wifi", icon: "network", color: "var(--a-net)", kind: "Wi-Fi", name: machine.wifi, detail: "WLAN · 1.2 Gbps" },
  { device: "gpu", icon: "gpu", color: "var(--a-gpu)", kind: "显卡", name: machine.gpu, detail: "8.0 GB 专用显存" },
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
                <span>在列表中单击应用或进程，即可在此查看其详细信息；再次单击取消选中。</span>
              </p>
              <p>
                <Icon name="panel" size={14} />
                <span>
                  详细信息面板默认常驻，单击窗口右上角的
                  <span className="a-key" aria-label="详细信息面板按钮">
                    <Icon name="panel" size={12} />
                  </span>
                  可收起。
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
