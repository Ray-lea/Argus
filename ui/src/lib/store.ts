// Application state: the live snapshot, rolling history and view preferences.

import { batch, createMemo, createRoot, createSignal } from "solid-js";
import { appendHistory, appendSampleTime } from "./chart-history";
import { t } from "./i18n";
import { api } from "./ipc";
import { holdsOrder, keepOrder, type OrderHolds, pointerAloneHoldsOrder } from "./order-hold";
import { persisted } from "./prefs";
import { applicationKey, applicationMembers, canGroup } from "./process-identity";
import { type ProcessSection, withSection } from "./process-sections";
import { type PauseState, refreshPause } from "./refresh-pause";
import {
  type BuildOptions,
  buildRows,
  type DisplayRow,
  procKey,
  type SectionRow,
  type SortState,
  sectionOf,
  type TableRow,
  type ViewMode,
} from "./rows";
import { cpuTemperature } from "./sensors";
import type { AppInfo, ProcessRow, ServiceInfo, Snapshot } from "./types";
import { confirmRequest, menu, toast } from "./ui";

export { HISTORY } from "./chart-history";

// ---- Live data ---------------------------------------------------------------

export const [snapshot, setSnapshot] = createSignal<Snapshot | null>(null);
export const [appInfo, setAppInfo] = createSignal<AppInfo | null>(null);

interface ProcHistory {
  cpu: number[];
  mem: number[];
}
const procHistory = new Map<string, ProcHistory>();
/** The same per app (group key): totals of whichever processes ran at the time. */
const groupHistory = new Map<string, ProcHistory>();
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

/** Rolling history of an app: its processes' CPU and memory added up. */
export function groupHistoryOf(key: string): ProcHistory | undefined {
  historyTick();
  return groupHistory.get(key);
}

const pushSample = (h: ProcHistory, cpu: number, mem: number, keep: number) => {
  appendHistory(h.cpu, cpu, keep);
  appendHistory(h.mem, mem, keep);
};

const EMPTY: number[] = [];

/**
 * Rolling history of a device metric, e.g. `cpu`, `core.0.3`, `disk.0.read`,
 * `nic.<id>.rx`, `gpu.<id>.e2`. Reactive: re-reads on every snapshot.
 */
export function series(key: string): readonly number[] {
  historyTick();
  return seriesHistory.get(key) ?? EMPTY;
}

/** Records every device metric the performance view can chart. */
function recordSeries(s: Snapshot, keep: number) {
  const seen = new Set<string>();
  const put = (key: string, v: number) => {
    seen.add(key);
    let list = seriesHistory.get(key);
    if (!list) {
      list = [];
      seriesHistory.set(key, list);
    }
    appendHistory(list, v, keep);
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
  for (const [i, v] of sys.cores.entries()) {
    const thread = hw.cpu.threads[i];
    if (thread) putMaybe(`core.${thread.group}.${thread.processor}`, v);
  }
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

  putMaybe("cpu.temp", cpuTemperature(hw));
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
    const keep = appendSampleTime(times, performance.now(), intervalMs());
    setSnapshot(s);
    recordSeries(s, keep);

    const live = new Set<string>();
    const apps = new Map<string, { cpu: number; mem: number }>();
    for (const p of s.processes) {
      const k = procKey(p);
      live.add(k);
      let h = procHistory.get(k);
      if (!h) {
        h = { cpu: [], mem: [] };
        procHistory.set(k, h);
        if (!first) firstSeen.set(k, seq);
      }
      pushSample(h, p.cpu, p.privateWs, keep);
      if (!canGroup(p)) continue;
      const g = applicationKey(p);
      const sum = apps.get(g);
      if (sum) {
        sum.cpu += p.cpu;
        sum.mem += p.privateWs;
      } else apps.set(g, { cpu: p.cpu, mem: p.privateWs });
    }
    for (const k of procHistory.keys()) if (!live.has(k)) procHistory.delete(k);
    for (const [g, sum] of apps) {
      let h = groupHistory.get(g);
      if (!h) {
        h = { cpu: [], mem: [] };
        groupHistory.set(g, h);
      }
      pushSample(h, sum.cpu, sum.mem, keep);
    }
    for (const g of groupHistory.keys()) if (!apps.has(g)) groupHistory.delete(g);
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
/**
 * The processes page's details panel, shown by default. Kept under a new name
 * since the panel became resident (it used to show only with a row selected):
 * hiding it then does not hide it now.
 */
export const [detailsOpen, setDetailsOpen] = persisted<boolean>("detailsPanel", true);
/** Process categories folded under their headings (sorted by name). */
export const [collapsedSections, setCollapsedSections] = persisted<ProcessSection[]>("collapsedSections", []);

/** Folds or unfolds a category heading. */
export function toggleSection(row: SectionRow, fold = !row.collapsed) {
  if (row.toggleable) setCollapsedSections(withSection(collapsedSections(), row.section, fold));
}
const [pauseState, setPauseState] = createSignal<PauseState>({
  manual: false,
  control: false,
  paused: false,
});
const [effectivePaused, setEffectivePaused] = createSignal(false);
export const paused = effectivePaused;
export const manualPaused = () => pauseState().manual;
export const controlPaused = () => pauseState().control;
let pauseStarted = 0;
/** Also anchors charts mounted after pausing, such as when switching devices. */
export const pausedSince = () => (paused() ? pauseStarted : null);
const pause = refreshPause(
  (state) => {
    if (state.paused && !paused()) pauseStarted = performance.now();
    batch(() => {
      setPauseState(state);
      setEffectivePaused(state.paused);
    });
  },
  api.setPaused,
  (error) => toast("error", t("shell.pauseError", { error: String(error) })),
);
export const setControlPaused = pause.setControl;
export const togglePause = pause.toggleManual;
/** A rule the settings page should scroll to and highlight once (set from the process menu). */
export const [focusRule, setFocusRule] = createSignal<string | null>(null);
export const [query, setQuery] = createSignal("");

/** Where the settings page goes when it opens: a section, and a setting there to point out. */
export const [settingsTarget, setSettingsTarget] = createSignal<{ section: string; item?: string } | null>(
  null,
);

/** Opens the settings page at a section, pointing out one setting (`setting-<item>`) if given. */
export function goToSetting(section: string, item?: string) {
  setSettingsTarget({ section, item });
  setPage("settings");
}

/** The refresh intervals on offer (toolbar, settings, onboarding). */
export const INTERVALS = [500, 1000, 2000, 5000];

/** Samples every `ms` from now on, also after a restart. */
export function changeInterval(ms: number) {
  setIntervalMs(ms);
  void api.setInterval(ms);
}

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
  a.collapsed === b.collapsed &&
  a.collapsedSections === b.collapsedSections;

const derived = createRoot(() => {
  // While the user is aiming at a row (pointer over the list, menu or dialog
  // open), new data updates values in place but never reorders rows.
  const holds = (): OrderHolds => ({
    pointer: pointerInTable(),
    menu: menu() !== null,
    dialog: confirmRequest() !== null,
  });
  const holdOrder = createMemo(() => holdsOrder(holds()));

  let previous: { rows: TableRow[]; opts: BuildOptions } | null = null;
  const view = createMemo<{ rows: TableRow[]; held: boolean }>(() => {
    if (page() !== "processes") {
      previous = null;
      return { rows: [], held: false };
    }
    const s = snapshot();
    if (!s) return { rows: [], held: false };
    const opts: BuildOptions = {
      mode: viewMode(),
      sort: sort(),
      query: query(),
      expanded: expanded(),
      collapsed: collapsed(),
      collapsedSections: collapsedSections(),
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
    // A category heading selected with the keyboard is no process.
    if (!key || !s || key.startsWith("s:")) return null;
    if (key.startsWith("g:")) {
      const members = applicationMembers(s.processes, key);
      const proc = members.reduce<ProcessRow | null>(
        (a, b) => (!a || b.createTime < a.createTime ? b : a),
        null,
      );
      return { key, proc, members, isGroup: true };
    }
    const proc = s.processes.find((p) => procKey(p) === key) ?? null;
    return { key, proc, members: proc ? [proc] : [], isGroup: false };
  });

  return { rows, selection, orderHeldByPointer: () => view().held && pointerAloneHoldsOrder(holds()) };
});

export const rows = derived.rows;
export const selection = derived.selection;
/**
 * True while the pointer over the list, and nothing else, holds back a
 * re-sort (see holdOrder): what the status bar tells.
 */
export const orderHeldByPointer = derived.orderHeldByPointer;

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
      const key = applicationKey(proc);
      if (applicationMembers(s.processes, key).length > 1) {
        setExpanded((set) => new Set(set).add(key));
      }
    } else if (viewMode() === "tree") {
      const byPid = new Map(s.processes.map((p) => [p.pid, p]));
      const ancestors = new Set<string>();
      for (let p = byPid.get(proc.ppid); p && !ancestors.has(procKey(p)); p = byPid.get(p.ppid)) {
        ancestors.add(procKey(p));
      }
      setCollapsed((set) => new Set([...set].filter((k) => !ancestors.has(k))));
    }
    // Unfold the category heading it is under.
    if (collapsedSections().length) {
      const unfolded = buildRows(s.processes, {
        mode: viewMode(),
        sort: sort(),
        query: "",
        expanded: expanded(),
        collapsed: collapsed(),
        collapsedSections: [],
      });
      const section = sectionOf(unfolded, procKey(proc)) ?? sectionOf(unfolded, applicationKey(proc));
      if (section) setCollapsedSections(withSection(collapsedSections(), section, false));
    }
    setSelectedKey(procKey(proc));
    setDetailsOpen(true);
    setRevealTick((n) => n + 1);
  });
}
