import assert from "node:assert/strict";
import { test } from "node:test";
import {
  appendHistory,
  appendSampleTime,
  chartLag,
  chartSpan,
  HISTORY,
  sampleTime,
} from "../src/lib/chart-history.ts";

test("queued snapshots keep their sampling intervals when delivered in a burst", () => {
  const epoch = 1_800_000_000_000;
  const times: number[] = [];
  const values: number[] = [];
  // Six background samples spanning 25 s arrive within 25 ms on restore.
  for (let i = 0; i < 6; i++) {
    const received = 40_000 + i * 5;
    const at = sampleTime(epoch + 10_000 + i * 5000, epoch + received, received);
    appendHistory(values, i, appendSampleTime(times, at, 1000));
  }
  assert.deepEqual(times, [10_000, 15_000, 20_000, 25_000, 30_000, 35_000]);
  assert.deepEqual(values, [0, 1, 2, 3, 4, 5]);
  const at = sampleTime(epoch + 41_000, epoch + 41_025, 41_025);
  appendSampleTime(times, at, 1000);
  assert.equal(times.at(-1), 41_000);
});

test("old queued data ages out of the visible window when fresh samples arrive", () => {
  const epoch = 1_800_000_000_000;
  const times: number[] = [];
  const values: number[] = [];
  for (let i = 0; i <= 120; i++) {
    const received = 600_000 + i * 2;
    const at = sampleTime(epoch + i * 5000, epoch + received, received);
    appendHistory(values, i, appendSampleTime(times, at, 1000));
  }
  assert.equal(times.at(-1), 600_000);
  assert.ok(times.length < 20);
  assert.ok(times[0] < 600_000 - chartSpan(1000) - chartLag(1000));
  assert.deepEqual(
    values.map((i) => i * 5000),
    times,
  );
});

test("sample age uses the current clock pair after a system clock adjustment", () => {
  const epoch = 1_800_000_000_000;
  for (const adjustment of [-3_600_000, 0, 3_600_000]) {
    assert.equal(sampleTime(epoch + adjustment + 5000, epoch + adjustment + 5025, 5025), 5000);
  }
});

test("regular sampling covers the delayed chart and keeps bounded history", () => {
  const times: number[] = [];
  for (let now = 0; now <= 300_000; now += 1000) appendSampleTime(times, now, 1000);
  const left = 300_000 - chartSpan(1000) - chartLag(1000);
  assert.ok(times[0] < left);
  assert.ok(times.filter((at) => at < left).length >= 2);
  assert.ok(times.length <= HISTORY + 8);
});

test("extra refreshes cannot evict a point still inside the visible window", () => {
  const times: number[] = [];
  const values: number[] = [];
  const push = (now: number) => appendHistory(values, now, appendSampleTime(times, now, 1000));
  for (let now = 0; now <= 99_000; now += 1000) push(now);
  for (let now = 99_100; now <= 100_200; now += 100) {
    push(now);
    assert.ok(40_000 > now - chartSpan(1000) - chartLag(1000));
    assert.ok(times.includes(40_000));
    assert.deepEqual(values, times);
  }
});

test("a faster background cadence still covers the foreground time window", () => {
  const times: number[] = [];
  for (let now = 0; now <= 300_000; now += 500) appendSampleTime(times, now, 1000);
  assert.ok(times[0] < 300_000 - chartSpan(1000) - chartLag(1000));
  assert.ok(times.length > HISTORY + 8);
  assert.ok(times.length < 140);
});

test("increasing the interval preserves existing data while the wider window fills", () => {
  const times: number[] = [];
  for (let now = 0; now <= 100_000; now += 500) appendSampleTime(times, now, 500);
  const oldest = times[0];
  for (let now = 101_000; now <= 110_000; now += 1000) {
    appendSampleTime(times, now, 1000);
    assert.equal(times[0], oldest);
  }
});

test("sparse samples, newly available series and missing values stay aligned", () => {
  const times: number[] = [];
  const full: number[] = [];
  const recent: number[] = [];
  for (const now of [...Array.from({ length: 100 }, (_, i) => i * 1000), 120_000, 150_000, 180_000]) {
    const keep = appendSampleTime(times, now, 1000);
    appendHistory(full, now, keep);
    if (now >= 95_000) appendHistory(recent, now === 120_000 ? Number.NaN : now, keep);
    assert.deepEqual(full, times);
    assert.deepEqual(
      recent,
      recent.length ? times.slice(-recent.length).map((at) => (at === 120_000 ? Number.NaN : at)) : [],
    );
  }
  assert.ok(times.length < 10);
  assert.ok(times.filter((at) => at < 180_000 - chartSpan(1000) - chartLag(1000)).length >= 2);
});
