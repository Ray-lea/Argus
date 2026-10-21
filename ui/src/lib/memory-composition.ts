import type { SystemStats } from "./types.ts";

/** Disjoint parts of physical memory; cached is an overlapping summary. */
export function memoryComposition(system: Pick<SystemStats, "memTotal" | "memUsed" | "memoryLists">) {
  const total = Math.max(0, system.memTotal);
  const lists = system.memoryLists;
  if (!lists) {
    return { used: Math.min(total, Math.max(0, system.memUsed)), modified: null, standby: null, free: null };
  }
  // Bound each remaining part, including while independent totals change.
  const free = Math.min(total, Math.max(0, lists.free));
  const standby = Math.min(total - free, Math.max(0, lists.standby));
  const modified = Math.min(total - free - standby, Math.max(0, lists.modified));
  return { used: total - free - standby - modified, modified, standby, free };
}
