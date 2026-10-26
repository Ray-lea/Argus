import { aggregateGpu } from "./gpu-usage.ts";
import type { GpuEngineUsage } from "./types.ts";

export type ProcessSection = "apps" | "background" | "windows";

const SECTIONS: readonly ProcessSection[] = ["apps", "background", "windows"];

/** Expanded members follow their top-level row, even when a child owns no window. */
export function processSections<T extends { depth: number; app: boolean; windows: boolean }>(
  rows: readonly T[],
) {
  const empty = () => ({ rows: [] as T[], count: 0 });
  const groups = { apps: empty(), background: empty(), windows: empty() };
  let group = groups.background;
  for (const row of rows) {
    if (row.depth === 0) {
      group = groups[row.app ? "apps" : row.windows ? "windows" : "background"];
      group.count++;
    }
    group.rows.push(row);
  }
  return SECTIONS.map((section) => ({ section, ...groups[section] })).filter((g) => g.count > 0);
}

/** What a section's heading shows in the resource columns. */
export interface SectionTotals {
  cpu: number;
  mem: number;
  io: number;
  /** Null when no row in the section has a reading. */
  diskRead: number | null;
  diskWrite: number | null;
  /** The busiest physical engine after aggregating its contributions. */
  gpu: number;
  net: number;
  threads: number;
  handles: number;
}

interface Measured {
  depth: number;
  cpu: number;
  mem: number;
  io: number;
  diskRead: number | null;
  diskWrite: number | null;
  gpu: number;
  gpuEngine?: string | null;
  gpuEngines?: GpuEngineUsage[];
  netSent: number;
  netReceived: number;
  threads: number;
  handles: number;
}

/** A section's totals: its top-level rows only, since a group's already include its members. */
export function sectionTotals(rows: readonly Measured[]): SectionTotals {
  const totals: SectionTotals = {
    cpu: 0,
    mem: 0,
    io: 0,
    diskRead: null,
    diskWrite: null,
    gpu: 0,
    net: 0,
    threads: 0,
    handles: 0,
  };
  for (const row of rows) {
    if (row.depth !== 0) continue;
    totals.cpu += row.cpu;
    totals.mem += row.mem;
    totals.io += row.io;
    if (row.diskRead != null) totals.diskRead = (totals.diskRead ?? 0) + row.diskRead;
    if (row.diskWrite != null) totals.diskWrite = (totals.diskWrite ?? 0) + row.diskWrite;
    totals.net += row.netSent + row.netReceived;
    totals.threads += row.threads;
    totals.handles += row.handles;
  }
  totals.gpu = aggregateGpu(rows.filter((row) => row.depth === 0)).gpu;
  return totals;
}

/**
 * Whether a section shows folded: only when collapsed outside a search, so
 * every match of a search shows.
 */
export const folded = (collapsed: readonly ProcessSection[], section: ProcessSection, searching: boolean) =>
  !searching && collapsed.includes(section);

/** `collapsed` with `section` folded or unfolded, in the sections' own order. */
export const withSection = (collapsed: readonly ProcessSection[], section: ProcessSection, fold: boolean) =>
  SECTIONS.filter((s) => (s === section ? fold : collapsed.includes(s)));
