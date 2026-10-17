import assert from "node:assert/strict";
import { test } from "node:test";
import { compareDiskRate, sumDiskIo } from "../src/lib/process-io.ts";

test("application disk totals exclude generic I/O and keep idle distinct from unavailable", () => {
  const members = [
    { diskRead: 1024, diskWrite: 2048, ioRead: 500_000, ioOther: 900_000 },
    { diskRead: 3072, diskWrite: 0, ioRead: 800_000, ioOther: 200_000 },
  ];
  assert.deepEqual(sumDiskIo(members), { diskRead: 4096, diskWrite: 2048 });
  assert.deepEqual(sumDiskIo([{ diskRead: 0, diskWrite: 0 }]), { diskRead: 0, diskWrite: 0 });
  assert.deepEqual(sumDiskIo([...members, { diskRead: null, diskWrite: null }]), {
    diskRead: null,
    diskWrite: null,
  });
  assert.deepEqual(sumDiskIo([]), { diskRead: null, diskWrite: null });
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
