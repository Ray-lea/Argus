import type { ProcessRow } from "./types.ts";

type DiskIo = Pick<ProcessRow, "diskRead" | "diskWrite">;

/** Adds up the processes measured over the last interval; one that just
 * started may be missing, and the total is unknown only when all are. */
export function sumDiskIo(processes: readonly DiskIo[]): DiskIo {
  let diskRead: number | null = null;
  let diskWrite: number | null = null;
  for (const p of processes) {
    if (p.diskRead != null) diskRead = (diskRead ?? 0) + p.diskRead;
    if (p.diskWrite != null) diskWrite = (diskWrite ?? 0) + p.diskWrite;
  }
  return { diskRead, diskWrite };
}

/** Missing intervals sort after measured values in either direction. */
export function compareDiskRate(a: number | null, b: number | null, desc: boolean): number {
  if (a == null || b == null) return a == null ? (b == null ? 0 : 1) : -1;
  return (desc ? -1 : 1) * (Math.round(a / 1024) - Math.round(b / 1024));
}
