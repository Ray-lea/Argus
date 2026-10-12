// Application state: the live snapshot, rolling history and view preferences.

import { batch, createMemo, createRoot, createSignal } from "solid-js";
import { t } from "./i18n";
import { api } from "./ipc";
import { persisted } from "./prefs";
import {
  type BuildOptions,
  buildRows,
  type DisplayRow,
  groupKey,
  procKey,
  type SortState,
  type TableRow,
  type ViewMode,
} from "./rows";
import type { AppInfo, ProcessRow, ServiceInfo, Snapshot } from "./types";
import { confirmRequest, menu, toast } from "./ui";

/** Samples spanning a chart's width. */
export const HISTORY = 60;
/**
 * Samples kept: more than shown, because charts run two intervals behind the
 * newest sample and draw two points past the left edge (see Trace.tsx).
 */
const KEEP = HISTORY + 8;

const pushCapped = (list: number[], v: number) => {
  if (list.length >= KEEP) list.shift();
  list.push(v);
};

// ---- Live data ---------------------------------------------------------------

export const [snapshot, setSnapshot] = createSignal<Snapshot | null>(null);
export const [appInfo, setAppInfo] = createSignal<AppInfo | null>(null);

interface ProcHistory {
  cpu: number[];
  mem: number[];
}
const procHistory = new Map<string, ProcHistory>();
const seriesHistory = new Map<string, number[]>();
const times: number[] = [];
const [historyTick, setHistoryTick] = createSignal(0);

/** Counts snapshots; charts redraw when it changes. */
export const sampleSeq = historyTick;

/** When each kept sample arrived (`performance.now()`, the charts' clock), oldest first. */
export function sampleTimes(): readonly number[] {
  historyTick();
  return times;
}

/** Processes Argus saw start, with the sample they first appeared in. */
const firstSeen = new Map<string, number>();
/** Samples a new process stays highlighted. */
const FRESH_SAMPLES = 3;

/** The process started within the last few samples (not merely present at launch). */
export function isFresh(key: string): boolean {
  const seq = historyTick();
  const at = firstSeen.get(key);
  return at !== undefined && seq - at < FRESH_SAMPLES;
}

/** Rolling history of one process (tracked for every live process). */
export function procHistoryOf(key: string): ProcHistory | undefined {
  historyTick();
  return procHistory.get(key);
}

const EMPTY: number[] = [];

/**
 * Rolling history of a device metric, e.g. `cpu`, `core.3`, `disk.0.read`,
 * `nic.<id>.rx`, `gpu.<id>.e2`. Reactive: re-reads on every snapshot.
 */
export function series(key: string): readonly number[] {
  historyTick();
  return seriesHistory.get(key) ?? EMPTY;
}

/** Records every device metric the performance view can chart. */
function recordSeries(s: Snapshot) {
  const seen = new Set<string>();
  const put = (key: string, v: number) => {
    seen.add(key);
    let list = seriesHistory.get(key);
    if (!list) {
      list = [];
      seriesHistory.set(key, list);
    }
    pushCapped(list, v);
  };
  // Sensors can miss a reading; record a gap (NaN) so the series stays aligned
  // with the sample times, but don't start a series for a sensor that never answered.
  const putMaybe = (key: string, v: number | null) => {
    if (v !== null) put(key, v);
    else if (seriesHistory.has(key)) put(key, Number.NaN);
  };
  const { system: sys, hardware: hw } = s;
  put("cpu", sys.cpu);
  put("cpu.kernel", sys.cpuKernel);
  for (const [i, v] of sys.cores.entries()) put(`core.${i}`, v);
  put("mem", sys.memUsed);
  put("commit", sys.commitUsed);

  let diskTotal = 0;
  for (const d of hw.disks) {
    const k = `disk.${d.info.index}`;
    put(`${k}.active`, d.active);
    put(`${k}.read`, d.read);
    put(`${k}.write`, d.write);
    diskTotal += d.read + d.write;
  }
  put("disk.total", diskTotal);

  let netTotal = 0;
  for (const n of hw.nics) {
    const k = `nic.${n.info.id}`;
    put(`${k}.rx`, n.rx);
    put(`${k}.tx`, n.tx);
    netTotal += n.rx + n.tx;
  }
  put("net.total", netTotal);

  let gpuMax = 0;
  for (const g of hw.gpus) {
    const k = `gpu.${g.info.id}`;
    put(`${k}.util`, g.util);
    for (const [i, v] of g.engines.entries()) put(`${k}.e${i}`, v);
    put(`${k}.ded`, g.dedicatedUsed);
    put(`${k}.shared`, g.sharedUsed);
    gpuMax = Math.max(gpuMax, g.util);
  }
  put("gpu.max", gpuMax);

  putMaybe("cpu.temp", hw.thermal.length ? Math.max(...hw.thermal.map((z) => z.celsius)) : null);
  putMaybe("cpu.pkgW", hw.power.package);
  putMaybe("cpu.coresW", hw.power.cores);
  for (const d of hw.disks) putMaybe(`disk.${d.info.index}.temp`, d.temperature);
  for (const g of hw.gpus) putMaybe(`gpu.${g.info.id}.clock`, g.clockMhz);
  for (const b of hw.batteries) {
    const k = `bat.${b.info.id}`;
    put(`${k}.pct`, b.percent);
    putMaybe(`${k}.w`, b.rate === null ? null : Math.abs(b.rate) / 1000);
  }

  // Devices that went away (unplugged disk, disconnected adapter).
  for (const k of seriesHistory.keys()) if (!seen.has(k)) seriesHistory.delete(k);
}

export function ingest(s: Snapshot) {
  batch(() => {
    const first = snapshot() === null;
    const seq = historyTick() + 1;
    setSnapshot(s);
    recordSeries(s);
    if (times.length >= KEEP) times.shift();
    times.push(performance.now());

    const live = new Set<string>();
    for (const p of s.processes) {
      const k = procKey(p);
      live.add(k);
      let h = procHistory.get(k);
      if (!h) {
        h = { cpu: [], mem: [] };
        procHistory.set(k, h);
        if (!first) firstSeen.set(k, seq);
      }
      if (h.cpu.length >= KEEP) {
        h.cpu.shift();
        h.mem.shift();
      }
      h.cpu.push(p.cpu);
      h.mem.push(p.privateWs);
    }
    for (const k of procHistory.keys()) if (!live.has(k)) procHistory.delete(k);
    for (const [k, at] of firstSeen) if (seq - at >= FRESH_SAMPLES || !live.has(k)) firstSeen.delete(k);
    setHistoryTick(seq);
  });
}

// ---- Pending actions -----------------------------------------------------------

/** Rows being ended: dimmed until the process is gone, so the click shows at once. */
export const [ending, setEnding] = createSignal<ReadonlySet<string>>(new Set());

export function markEnding(key: string, on: boolean) {
  setEnding((set) => {
    const next = new Set(set);
    if (on) next.add(key);
    else next.delete(key);
    return next;
  });
}

// ---- Preferences -------------------------------------------------------------

export type Page = "processes" | "performance" | "services" | "startup" | "network" | "history" | "settings";
export const [page, setPage] = persisted<Page>("page", "processes");
/** Device shown on the performance page: `cpu`, `memory`, `disk:<n>`, `nic:<id>`, `gpu:<id>`. */
export const [perfDevice, setPerfDevice] = persisted<string>("perfDevice", "cpu");
export const [cpuMode, setCpuMode] = persisted<"overall" | "logical">("cpuMode", "overall");
export const [viewMode, setViewMode] = persisted<ViewMode>("view", "apps");
export const [sort, setSort] = persisted<SortState>("sort", { key: "cpu", desc: true });
export const [intervalMs, setIntervalMs] = persisted<number>("interval", 1000);
export const [detailsOpen, setDetailsOpen] = persisted<boolean>("details", true);
export const [paused, setPaused] = createSignal(false);
/** A rule the settings page should scroll to and highlight once (set from the process menu). */
export const [focusRule, setFocusRule] = createSignal<string | null>(null);
export const [query, setQuery] = createSignal("");

// ---- Table state -------------------------------------------------------------

export const [selectedKey, setSelectedKey] = createSignal<string | null>(null);
/** The pointer is over the process list. */
export const [pointerInTable, setPointerInTable] = createSignal(false);
const [expanded, setExpanded] = createSignal<ReadonlySet<string>>(new Set());
const [collapsed, setCollapsed] = createSignal<ReadonlySet<string>>(new Set());

export function toggleRow(row: DisplayRow) {
  if (!row.expandable) return;
  const flip = (set: ReadonlySet<string>) => {
    const next = new Set(set);
    if (next.has(row.key)) next.delete(row.key);
    else next.add(row.key);
    return next;
  };
  if (viewMode() === "tree") setCollapsed(flip);
  else setExpanded(flip);
}

export interface Selection {
  key: string;
  /** Null once the selected process has exited. */
  proc: ProcessRow | null;
  members: ProcessRow[];
  isGroup: boolean;
}

// ---- Services --------------------------------------------------------------------
// Shared by the services page and the process list (svchost rows show what they host).

export const [services, setServices] = createSignal<ServiceInfo[]>([]);
let servicesLoading = false;

export async function refreshServices() {
  if (servicesLoading) return;
  servicesLoading = true;
  try {
    setServices(await api.services());
  } catch {
    // Keep the last list; the services page reports errors on its own actions.
  } finally {
    servicesLoading = false;
  }
}

/** Display names of the running services in each process. */
export const hostedServices = createRoot(() =>
  createMemo(() => {
    const map = new Map<number, string[]>();
    for (const s of services()) {
      if (s.pid === 0) continue;
      const list = map.get(s.pid);
      if (list) list.push(s.displayName);
      else map.set(s.pid, [s.displayName]);
    }
    return map as ReadonlyMap<number, readonly string[]>;
  }),
);

const sameView = (a: BuildOptions, b: BuildOptions) =>
  a.mode === b.mode &&
  a.sort.key === b.sort.key &&
  a.sort.desc === b.sort.desc &&
  a.query === b.query &&
  a.expanded === b.expanded &&
  a.collapsed === b.collapsed;

/**
 * Reorders fresh rows to follow the previous order. Rows that are new keep
 * their place right after the row that precedes them in the fresh list, which
 * keeps children inside their expanded parent.
 */
function keepOrder(fresh: TableRow[], previous: TableRow[]): TableRow[] {
  const rank = new Map(previous.map((r, i) => [r.key, i]));
  let last = -1;
  const ranked = fresh.map((row) => {
    last = rank.get(row.key) ?? last + 1e-3;
    return { row, rank: last };
  });
  ranked.sort((a, b) => a.rank - b.rank);
  return ranked.map((r) => r.row);
}

const derived = createRoot(() => {
  // While the user is aiming at a row (pointer over the list, menu or dialog
  // open), new data updates values in place but never reorders rows.
  const holdOrder = createMemo(() => pointerInTable() || menu() !== null || confirmRequest() !== null);

  let previous: { rows: TableRow[]; opts: BuildOptions } | null = null;
  const view = createMemo<{ rows: TableRow[]; held: boolean }>(() => {
    const s = snapshot();
    if (!s) return { rows: [], held: false };
    const opts: BuildOptions = {
      mode: viewMode(),
      sort: sort(),
      query: query(),
      expanded: expanded(),
      collapsed: collapsed(),
      hosted: hostedServices(),
    };
    const fresh = buildRows(s.processes, opts);
    let rows = fresh;
    if (holdOrder() && previous && sameView(previous.opts, opts)) {
      rows = keepOrder(fresh, previous.rows);
    }
    previous = { rows, opts };
    return { rows, held: rows !== fresh && rows.some((r, i) => r !== fresh[i]) };
  });
  const rows = () => view().rows;

  const selection = createMemo<Selection | null>(() => {
    const key = selectedKey();
    const s = snapshot();
    if (!key || !s) return null;
    if (key.startsWith("g:")) {
      const name = key.slice(2);
      const members = s.processes.filter((p) => p.name.toLowerCase() === name);
      const proc = members.reduce<ProcessRow | null>(
        (a, b) => (!a || b.createTime < a.createTime ? b : a),
        null,
      );
      return { key, proc, members, isGroup: true };
    }
    const proc = s.processes.find((p) => procKey(p) === key) ?? null;
    return { key, proc, members: proc ? [proc] : [], isGroup: false };
  });

  return { rows, selection, orderHeld: () => view().held };
});

export const rows = derived.rows;
export const selection = derived.selection;
/** True while a re-sort is being held back (see holdOrder). */
export const orderHeld = derived.orderHeld;

// ---- Navigation --------------------------------------------------------------------

/** Bumped to ask the process table to scroll the selected row into view. */
export const [revealTick, setRevealTick] = createSignal(0);

/** Opens the processes page on `pid`, expanding whatever hides it. */
export function revealProcess(pid: number) {
  const s = snapshot();
  const proc = s?.processes.find((p) => p.pid === pid);
  if (!s || !proc) {
    toast("error", t("processes.pidExited", { pid }));
    return;
  }
  batch(() => {
    setPage("processes");
    setQuery("");
    if (viewMode() === "apps") {
      const name = proc.name.toLowerCase();
      if (s.processes.filter((p) => p.name.toLowerCase() === name).length > 1) {
        setExpanded((set) => new Set(set).add(groupKey(proc.name)));
      }
    } else if (viewMode() === "tree") {
      const byPid = new Map(s.processes.map((p) => [p.pid, p]));
      const ancestors = new Set<string>();
      for (let p = byPid.get(proc.ppid); p && !ancestors.has(procKey(p)); p = byPid.get(p.ppid)) {
        ancestors.add(procKey(p));
      }
      setCollapsed((set) => new Set([...set].filter((k) => !ancestors.has(k))));
    }
    setSelectedKey(procKey(proc));
    setDetailsOpen(true);
    setRevealTick((n) => n + 1);
  });
}
