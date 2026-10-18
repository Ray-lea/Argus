import assert from "node:assert/strict";
import { test } from "node:test";
import {
  compareDiskRate,
  type DiskService,
  hasDiskReading,
  missingDiskReason,
  sumDiskIo,
} from "../src/lib/process-io.ts";

const MB = 1024 * 1024;
const starting = { diskRead: null, diskWrite: null };

test("application disk totals exclude generic I/O and keep idle distinct from unavailable", () => {
  const members = [
    { diskRead: 1024, diskWrite: 2048, ioRead: 500_000, ioOther: 900_000 },
    { diskRead: 3072, diskWrite: 0, ioRead: 800_000, ioOther: 200_000 },
  ];
  assert.deepEqual(sumDiskIo(members), { diskRead: 4096, diskWrite: 2048 });
  assert.deepEqual(sumDiskIo([{ diskRead: 0, diskWrite: 0 }]), { diskRead: 0, diskWrite: 0 });
  // Unknown only when nothing is measured: the first sample, or no source at all.
  assert.deepEqual(sumDiskIo([starting, starting]), { diskRead: null, diskWrite: null });
  assert.deepEqual(sumDiskIo([]), { diskRead: null, diskWrite: null });
});

test("a process without a disk interval yet leaves the total to the measured ones", () => {
  // The table header: 625 measured processes and one that has just started.
  const running = Array.from({ length: 625 }, (_, i) => ({
    diskRead: i < 10 ? 5 * MB : 0,
    diskWrite: i < 4 ? 2 * MB : 0,
  }));
  assert.deepEqual(sumDiskIo([...running, starting]), { diskRead: 50 * MB, diskWrite: 8 * MB });
});

test("disk sorting never promotes missing samples above real activity or measured idle", () => {
  const rates = [null, 0, 8192, 4096];
  assert.deepEqual(
    [...rates].sort((a, b) => compareDiskRate(a, b, true)),
    [8192, 4096, 0, null],
  );
  assert.deepEqual(
    [...rates].sort((a, b) => compareDiskRate(a, b, false)),
    [0, 4096, 8192, null],
  );
});

test("an app keeps its place in a disk sort while one of its processes starts", () => {
  const code = [
    { diskRead: 2 * MB, diskWrite: 0 },
    { diskRead: MB, diskWrite: 0 },
  ];
  const chrome = [...Array(10).fill({ diskRead: 5 * MB, diskWrite: MB }), starting];
  const groups = [
    { name: "Code", ...sumDiskIo(code) },
    { name: "Chrome", ...sumDiskIo(chrome) },
    { name: "Setup", ...sumDiskIo([starting]) },
  ];
  const order = (desc: boolean) =>
    [...groups].sort((a, b) => compareDiskRate(a.diskRead, b.diskRead, desc)).map((g) => g.name);
  assert.deepEqual(order(true), ["Chrome", "Code", "Setup"]);
  assert.deepEqual(order(false), ["Code", "Chrome", "Setup"]);
});

test("readings count as arriving when any process has one", () => {
  assert.equal(hasDiskReading([]), false);
  assert.equal(hasDiskReading([starting, starting]), false);
  assert.equal(hasDiskReading([starting, { diskRead: 0, diskWrite: 0 }]), true);
});

test("only a source without any readings gets advice about the privileged service", () => {
  const service = (state: DiskService["state"], extra: Partial<DiskService> = {}): DiskService => ({
    state,
    authorized: state !== "notInstalled",
    version: null,
    ...extra,
  });
  const connected = service("running", { version: "0.2.0" });
  const incompatible = service("running", { incompatible: true });
  const reason = (status: DiskService | null, source: { measured?: boolean; elevated?: boolean } = {}) =>
    missingDiskReason({ measured: false, elevated: false, service: status, appVersion: "0.2.0", ...source });

  // Windows 11 24H2+ (or a working service) measures the others: a process
  // without a reading has just started, whatever state the service is in.
  for (const status of [null, service("notInstalled"), incompatible, service("stopped"), connected])
    assert.equal(reason(status, { measured: true }), "pending");

  assert.equal(reason(service("notInstalled")), "enable");
  // A service of another protocol, or an older version that still answers.
  assert.equal(reason(incompatible), "update");
  assert.equal(reason(service("running", { version: "0.1.0" })), "update");
  assert.equal(reason(service("stopped")), "start");
  // Running without answering, starting, stopping, installed for another
  // account, or connected without readings (yet).
  for (const status of [
    service("running"),
    service("startPending"),
    service("stopPending"),
    service("unavailable"),
    service("stopped", { authorized: false }),
    service("running", { authorized: false }),
    connected,
  ])
    assert.equal(reason(status), "service");

  // As administrator Argus reads the counters itself, and an unknown status
  // names no remedy.
  assert.equal(reason(service("notInstalled"), { elevated: true }), "unavailable");
  assert.equal(reason(incompatible, { elevated: true }), "unavailable");
  assert.equal(reason(null), "unavailable");
});
