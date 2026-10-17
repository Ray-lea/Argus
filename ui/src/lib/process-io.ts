import type { ProcessRow } from "./types.ts";

type DiskIo = Pick<ProcessRow, "diskRead" | "diskWrite">;

/** A group total is unknown while any member lacks a complete disk interval. */
export function sumDiskIo(processes: readonly DiskIo[]): DiskIo {
  if (!processes.length || processes.some((p) => p.diskRead == null || p.diskWrite == null))
    return { diskRead: null, diskWrite: null };
  return {
    diskRead: processes.reduce((sum, p) => sum + (p.diskRead ?? 0), 0),
    diskWrite: processes.reduce((sum, p) => sum + (p.diskWrite ?? 0), 0),
  };
}

/** Missing intervals sort after measured values in either direction. */
export function compareDiskRate(a: number | null, b: number | null, desc: boolean): number {
  if (a == null || b == null) return a == null ? (b == null ? 0 : 1) : -1;
  return (desc ? -1 : 1) * (Math.round(a / 1024) - Math.round(b / 1024));
}
