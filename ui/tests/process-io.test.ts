import assert from "node:assert/strict";
import { test } from "node:test";
import { compareDiskRate, sumDiskIo } from "../src/lib/process-io.ts";

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
