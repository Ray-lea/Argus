// Tops of the charts without a fixed one (ui/src/lib/chart-scale.ts).

import assert from "node:assert/strict";
import { test } from "node:test";
import { rateCeiling, rateCeilingLabel, roundCeiling } from "../src/lib/chart-scale.ts";

const KB = 1024;
const MB = KB * 1024;
const GB = MB * 1024;

test("a top is the next whole number, or above ten the next multiple of ten", () => {
  const cases: [number, number][] = [
    [0, 1],
    [0.3, 1],
    [1, 1],
    [1.2, 2],
    [7.34, 8],
    [8, 8],
    [10, 10],
    [10.2, 20],
    [19.99, 20],
    [20, 20],
    [145.7, 150],
    [999, 1000],
    [1001, 1010],
    [30.000000000004, 30],
  ];
  for (const [x, top] of cases) assert.equal(roundCeiling(x), top, String(x));
  assert.equal(roundCeiling(Number.NaN), 1);
});

test("rates round in the unit their label shows", () => {
  assert.equal(rateCeiling(7.3 * MB, "bytes"), 8 * MB);
  assert.equal(rateCeiling(23 * MB, "bytes"), 30 * MB);
  assert.equal(rateCeiling(64 * KB, "bytes"), 70 * KB);
  assert.equal(rateCeiling(900 * KB, "bytes"), 900 * KB);
  assert.equal(rateCeiling(1.2 * GB, "bytes"), 2 * GB);
  // Megabits, as broadband is sold: 145.7 Mbps tops at 150.
  assert.equal(rateCeiling((145.7e6 / 8) | 0, "bits"), 150e6 / 8);
  assert.equal(rateCeiling(3e6 / 8, "bits"), 3e6 / 8);
});

test("rounding past a unit's end continues in the next unit", () => {
  // 1023.5 KB/s would round to 1030 KB/s, which fmtRate shows as MB/s.
  assert.equal(rateCeiling(1023.5 * KB, "bytes"), MB);
  assert.equal(rateCeiling(1020 * KB, "bytes"), 1020 * KB);
  assert.equal(rateCeiling(995e3 / 8, "bits"), 1e6 / 8);
});

test("labels are whole numbers in the unit of the top", () => {
  assert.equal(rateCeilingLabel(8 * MB, "bytes"), "8 MB/s");
  assert.equal(rateCeilingLabel(70 * KB, "bytes"), "70 KB/s");
  assert.equal(rateCeilingLabel(MB, "bytes"), "1 MB/s");
  assert.equal(rateCeilingLabel(2 * GB, "bytes"), "2 GB/s");
  assert.equal(rateCeilingLabel(150e6 / 8, "bits"), "150 Mbps");
  assert.equal(rateCeilingLabel(1e6 / 8, "bits"), "1 Mbps");
  assert.equal(rateCeilingLabel(530e3 / 8, "bits"), "530 Kbps");
});
