import type { PrivilegedServiceStatus, ProcessRow } from "./types.ts";

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

/** Some process has a disk reading: the source works, and a process without
 * one has started since the last refresh. */
export function hasDiskReading(processes: readonly DiskIo[]): boolean {
  return processes.some((p) => p.diskRead != null || p.diskWrite != null);
}

/** Why a process shows no disk reading, as a `processes.diskUnavailable` key. */
export type MissingDiskReason = "pending" | "enable" | "update" | "start" | "service" | "unavailable";

/** What disk readings depend on in the privileged service's status. */
export type DiskService = Pick<PrivilegedServiceStatus, "state" | "authorized" | "version"> & {
  /** The running service speaks another protocol: only updating it reconnects. */
  incompatible?: boolean;
};

/**
 * Names what a missing disk reading waits for. While any process has one, the
 * counters arrive (from Windows 11 24H2+ itself, the service or Argus as
 * administrator) and the missing ones only wait for the next refresh: the
 * service is no concern then. Without any, the readings would come from the
 * service, unless Argus runs as administrator and reads them itself.
 */
export function missingDiskReason(source: {
  /** Some process of the snapshot has a reading (`hasDiskReading`). */
  measured: boolean;
  elevated: boolean;
  /** Null until read, or when it cannot be. */
  service: DiskService | null;
  appVersion: string | null;
}): MissingDiskReason {
  const { service } = source;
  if (source.measured) return "pending";
  if (source.elevated || !service) return "unavailable";
  if (service.state === "notInstalled") return "enable";
  // Compared as the settings page does: the installer updates the service
  // along with Argus, except while the task manager replacement is on.
  const outdated = !!service.version && !!source.appVersion && service.version !== source.appVersion;
  if (service.incompatible || outdated) return "update";
  if (service.state === "stopped" && service.authorized) return "start";
  return "service";
}
