// Turns a flat process snapshot into the rows the table renders, for each view mode.

import type { ProcessRow } from "./types";

export type ViewMode = "apps" | "tree" | "list";
export type SortKey = "name" | "pid" | "cpu" | "mem" | "io" | "threads" | "handles";
export interface SortState {
  key: SortKey;
  desc: boolean;
}

export interface DisplayRow {
  key: string;
  kind: "process" | "group";
  depth: number;
  /** The process itself, or a group's earliest-started member. */
  proc: ProcessRow;
  label: string;
  /** Secondary text shown dimmed after the label. */
  detail: string | null;
  count: number;
  cpu: number;
  mem: number;
  io: number;
  threads: number;
  handles: number;
  expandable: boolean;
  expanded: boolean;
  suspended: boolean;
}

export interface BuildOptions {
  mode: ViewMode;
  sort: SortState;
  query: string;
  /** Expanded groups in "apps" mode (collapsed by default). */
  expanded: ReadonlySet<string>;
  /** Collapsed nodes in "tree" mode (expanded by default). */
  collapsed: ReadonlySet<string>;
  /** Services running in each process, shown in place of the generic description. */
  hosted?: ReadonlyMap<number, readonly string[]>;
}

export const procKey = (p: ProcessRow) => `${p.pid}:${p.createTime}`;
export const groupKey = (name: string) => `g:${name.toLowerCase()}`;
export const ioTotal = (p: ProcessRow) => p.ioRead + p.ioWrite + p.ioOther;

function procRow(p: ProcessRow, depth: number, label: string, detail: string | null): DisplayRow {
  return {
    key: procKey(p),
    kind: "process",
    depth,
    proc: p,
    label,
    detail,
    count: 1,
    cpu: p.cpu,
    mem: p.privateWs,
    io: ioTotal(p),
    threads: p.threads,
    handles: p.handles,
    expandable: false,
    expanded: false,
    suspended: p.suspended,
  };
}

const collator = new Intl.Collator("zh-CN", { numeric: true, sensitivity: "base" });

// Sort on displayed precision, so rows whose values only differ in invisible
// digits do not trade places on every refresh.
const quantize: Record<"cpu" | "mem" | "io" | "threads" | "handles", (r: DisplayRow) => number> = {
  cpu: (r) => Math.round(r.cpu * 10),
  mem: (r) => Math.round(r.mem / 104_858),
  io: (r) => Math.round(r.io / 1024),
  threads: (r) => r.threads,
  handles: (r) => r.handles,
};

function comparator(sort: SortState): (a: DisplayRow, b: DisplayRow) => number {
  const dir = sort.desc ? -1 : 1;
  const byName = (a: DisplayRow, b: DisplayRow) =>
    collator.compare(a.label, b.label) || a.proc.pid - b.proc.pid;
  switch (sort.key) {
    case "name":
      return (a, b) => dir * byName(a, b);
    case "pid":
      return (a, b) => dir * (a.proc.pid - b.proc.pid);
    default: {
      const q = quantize[sort.key];
      return (a, b) => dir * (q(a) - q(b)) || byName(a, b);
    }
  }
}

function matcher(
  query: string,
  hosted?: ReadonlyMap<number, readonly string[]>,
): ((p: ProcessRow) => boolean) | null {
  const q = query.trim().toLowerCase();
  if (!q) return null;
  const numeric = /^\d+$/.test(q);
  return (p) =>
    (numeric && String(p.pid).startsWith(q)) ||
    p.name.toLowerCase().includes(q) ||
    (p.description?.toLowerCase().includes(q) ?? false) ||
    (p.company?.toLowerCase().includes(q) ?? false) ||
    (p.path?.toLowerCase().includes(q) ?? false) ||
    (hosted?.get(p.pid)?.some((s) => s.toLowerCase().includes(q)) ?? false);
}

/** "DNS Client、DHCP Client 等 5 个服务" */
export function summarizeServices(names: readonly string[]): string {
  if (names.length <= 2) return names.join("、");
  return `${names.slice(0, 2).join("、")} 等 ${names.length} 个服务`;
}

export function buildRows(procs: readonly ProcessRow[], opts: BuildOptions): DisplayRow[] {
  const cmp = comparator(opts.sort);
  const match = matcher(opts.query, opts.hosted);
  let rows: DisplayRow[];
  switch (opts.mode) {
    case "list":
      rows = procs
        .filter((p) => !match || match(p))
        .map((p) => procRow(p, 0, p.name, p.description))
        .sort(cmp);
      break;
    case "tree":
      rows = buildTree(procs, cmp, match, opts.collapsed);
      break;
    case "apps":
      rows = buildApps(procs, cmp, match, opts.query.trim().toLowerCase(), opts.expanded);
      break;
  }
  // A service host's description ("Host Process for Windows Services") says
  // nothing; the services it runs do. App rows keep their exe name instead.
  if (opts.hosted?.size) {
    for (const row of rows) {
      const names = row.kind === "process" ? opts.hosted.get(row.proc.pid) : undefined;
      if (names && !(opts.mode === "apps" && row.depth === 0)) row.detail = summarizeServices(names);
    }
  }
  return rows;
}

function buildTree(
  procs: readonly ProcessRow[],
  cmp: (a: DisplayRow, b: DisplayRow) => number,
  match: ((p: ProcessRow) => boolean) | null,
  collapsed: ReadonlySet<string>,
): DisplayRow[] {
  const byPid = new Map(procs.map((p) => [p.pid, p]));
  const children = new Map<string, ProcessRow[]>();
  const roots: ProcessRow[] = [];
  for (const p of procs) {
    const parent = byPid.get(p.ppid);
    // A parent must predate its child; otherwise the parent PID was reused.
    if (parent && parent.pid !== p.pid && parent.createTime <= p.createTime) {
      const k = procKey(parent);
      const list = children.get(k);
      if (list) list.push(p);
      else children.set(k, [p]);
    } else {
      roots.push(p);
    }
  }

  // While searching, keep ancestors of matches so results stay in context.
  const visibleCache = new Map<string, boolean>();
  const visible = (p: ProcessRow): boolean => {
    if (!match) return true;
    const k = procKey(p);
    let v = visibleCache.get(k);
    if (v === undefined) {
      visibleCache.set(k, false);
      v = match(p) || (children.get(k) ?? []).some(visible);
      visibleCache.set(k, v);
    }
    return v;
  };

  const out: DisplayRow[] = [];
  const seen = new Set<string>();
  const walk = (list: ProcessRow[], depth: number) => {
    const level = list.filter(visible).map((p) => {
      const row = procRow(p, depth, p.name, p.description);
      row.expandable = (children.get(row.key)?.length ?? 0) > 0;
      row.expanded = row.expandable && (match !== null || !collapsed.has(row.key));
      return row;
    });
    level.sort(cmp);
    for (const row of level) {
      if (seen.has(row.key)) continue;
      seen.add(row.key);
      out.push(row);
      if (row.expanded) walk(children.get(row.key) ?? [], depth + 1);
    }
  };
  walk(roots, 0);
  return out;
}

function buildApps(
  procs: readonly ProcessRow[],
  cmp: (a: DisplayRow, b: DisplayRow) => number,
  match: ((p: ProcessRow) => boolean) | null,
  query: string,
  expanded: ReadonlySet<string>,
): DisplayRow[] {
  const groups = new Map<string, ProcessRow[]>();
  for (const p of procs) {
    const k = p.name.toLowerCase();
    const list = groups.get(k);
    if (list) list.push(p);
    else groups.set(k, [p]);
  }

  const top: { row: DisplayRow; members: ProcessRow[] }[] = [];
  for (const [name, members] of groups) {
    const rep = members.reduce((a, b) => (b.createTime < a.createTime ? b : a));
    const label = rep.description ?? rep.name;
    const detail = rep.description ? rep.name : null;
    let shown = members;
    if (match && !label.toLowerCase().includes(query) && !name.includes(query)) {
      shown = members.filter(match);
      if (shown.length === 0) continue;
    }
    if (members.length === 1) {
      top.push({ row: procRow(rep, 0, label, detail), members: [] });
      continue;
    }
    const key = groupKey(name);
    const row: DisplayRow = {
      key,
      kind: "group",
      depth: 0,
      proc: rep,
      label,
      detail,
      count: members.length,
      cpu: 0,
      mem: 0,
      io: 0,
      threads: 0,
      handles: 0,
      expandable: true,
      expanded: expanded.has(key) || shown.length < members.length,
      suspended: members.every((p) => p.suspended),
    };
    for (const p of members) {
      row.cpu += p.cpu;
      row.mem += p.privateWs;
      row.io += ioTotal(p);
      row.threads += p.threads;
      row.handles += p.handles;
    }
    top.push({ row, members: shown });
  }

  top.sort((a, b) => cmp(a.row, b.row));
  const out: DisplayRow[] = [];
  for (const { row, members } of top) {
    out.push(row);
    if (row.kind === "group" && row.expanded) {
      out.push(...members.map((p) => procRow(p, 1, p.name, null)).sort(cmp));
    }
  }
  return out;
}
