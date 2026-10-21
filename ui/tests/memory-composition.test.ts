import assert from "node:assert/strict";
import { test } from "node:test";
import { memoryComposition } from "../src/lib/memory-composition.ts";

test("modified memory occupies its own part rather than overlapping used or standby memory", () => {
  const parts = memoryComposition({
    memTotal: 100,
    memUsed: 35,
    memoryLists: { modified: 5, standby: 55, free: 10 },
  });
  assert.deepEqual(parts, { used: 30, modified: 5, standby: 55, free: 10 });
  assert.equal(parts.used + (parts.modified ?? 0) + (parts.standby ?? 0) + (parts.free ?? 0), 100);
});

test("missing memory-list readings stay unknown instead of appearing as zero or cached memory", () => {
  assert.deepEqual(memoryComposition({ memTotal: 100, memUsed: 35, memoryLists: null }), {
    used: 35,
    modified: null,
    standby: null,
    free: null,
  });
  assert.deepEqual(memoryComposition({ memTotal: 0, memUsed: 0, memoryLists: null }), {
    used: 0,
    modified: null,
    standby: null,
    free: null,
  });
});

test("memory composition stays within physical memory when independent readings cross", () => {
  for (const memoryLists of [
    { modified: 10, standby: 95, free: 5 },
    { modified: 200, standby: 0, free: 0 },
    { modified: 0, standby: 0, free: 200 },
    { modified: 0, standby: 0, free: 0 },
  ]) {
    const parts = memoryComposition({ memTotal: 100, memUsed: 20, memoryLists });
    assert.ok(Object.values(parts).every((value) => value !== null && value >= 0));
    assert.equal(parts.used + (parts.modified ?? 0) + (parts.standby ?? 0) + (parts.free ?? 0), 100);
  }
});
