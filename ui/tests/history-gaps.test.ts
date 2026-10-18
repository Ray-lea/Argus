// Gaps on the history charts: Argus not running, not a slower refresh interval.

import assert from "node:assert/strict";
import { test } from "node:test";
import { joins, MAX_CADENCE, nearest, pointAt } from "../src/lib/history-gaps.ts";

const SECOND = 1000;
/** The 1-hour view on a plot about 1000 px wide: 500 buckets of 7.2 s. */
const HOUR_STEP = 7.2 * SECOND;

/** Frame times from `from`, `count` of them `every` ms apart. */
const frames = (from: number, count: number, every: number) =>
  Array.from({ length: count }, (_, i) => from + i * every);

test("frames recorded every 5 seconds form one line; a stop of Argus is a gap", () => {
  const ts = [...frames(0, 100, 5 * SECOND), ...frames(600 * SECOND, 100, 5 * SECOND)];
  const joined = joins(ts, HOUR_STEP);
  assert.equal(joined.length, ts.length - 1);
  assert.deepEqual(
    joined.flatMap((j, i) => (j ? [] : [i])),
    [99],
  );
});

test("a 10 or 30 second background interval is a sparser line, not dots", () => {
  for (const every of [10 * SECOND, 30 * SECOND]) {
    // Foreground frames, then the background ones, then foreground again.
    const ts = [
      ...frames(0, 50, 5 * SECOND),
      ...frames(250 * SECOND, 60, every),
      ...frames(250 * SECOND + 60 * every, 50, 5 * SECOND),
    ];
    assert.ok(
      joins(ts, HOUR_STEP).every((j) => j),
      `every ${every} ms`,
    );
  }
  // Samples land a little late or early, and frames are whole 5-second slots.
  const jittered = [0, 30, 55, 90, 120, 145, 180].map((s) => s * SECOND);
  assert.ok(joins(jittered, HOUR_STEP).every((j) => j));
});

test("a lone frame between two stops stays a dot", () => {
  const ts = [...frames(0, 20, 5 * SECOND), 1000 * SECOND, ...frames(2000 * SECOND, 20, 5 * SECOND)];
  const joined = joins(ts, HOUR_STEP);
  assert.equal(joined[19], false);
  assert.equal(joined[20], false);
  // Even beside a sparse stretch, a stop much longer than the slowest interval is a gap.
  const sparse = [...frames(0, 20, 30 * SECOND), 20 * 30 * SECOND + 5 * MAX_CADENCE];
  assert.equal(joins(sparse, HOUR_STEP).at(-1), false);
});

test("wide buckets of the long ranges join as before", () => {
  // 7 days over 500 buckets: about 20 minutes each.
  const step = (7 * 24 * 3600 * SECOND) / 500;
  const ts = frames(0, 400, step);
  assert.ok(joins(ts, step).every((j) => j));
  ts.push(ts[ts.length - 1] + 3 * step);
  assert.equal(joins(ts, step).at(-1), false);
});

test("a reading between sparse frames shows the nearest; one in a gap shows none", () => {
  const ts = [...frames(0, 60, 30 * SECOND), ...frames(3600 * SECOND, 10, 5 * SECOND)];
  const joined = joins(ts, HOUR_STEP);
  // Everywhere along the 30-second stretch has a reading.
  for (let at = 0; at <= 59 * 30 * SECOND; at += 500) {
    const i = pointAt(ts, joined, HOUR_STEP, at);
    assert.ok(i !== undefined, `at ${at}`);
    assert.equal(i, nearest(ts, at));
  }
  // In the stop, only near its edges.
  assert.equal(pointAt(ts, joined, HOUR_STEP, 59 * 30 * SECOND + 10 * SECOND), 59);
  assert.equal(pointAt(ts, joined, HOUR_STEP, 2500 * SECOND), undefined);
  assert.equal(pointAt(ts, joined, HOUR_STEP, 3600 * SECOND - 10 * SECOND), 60);
  // Past either end, within one and a half steps.
  assert.equal(pointAt(ts, joined, HOUR_STEP, -10 * SECOND), 0);
  assert.equal(pointAt(ts, joined, HOUR_STEP, -20 * SECOND), undefined);
  assert.equal(pointAt([], [], HOUR_STEP, 0), undefined);
});

test("nearest picks the closer neighbour", () => {
  assert.equal(nearest([], 5), -1);
  assert.equal(nearest([10, 20, 30], 14), 0);
  assert.equal(nearest([10, 20, 30], 16), 1);
  assert.equal(nearest([10, 20, 30], 99), 2);
  assert.equal(nearest([10, 20, 30], -5), 0);
});
