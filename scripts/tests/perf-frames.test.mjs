import { test } from "node:test";
import assert from "node:assert/strict";
import { summarizeFrames } from "../perf-frames.mjs";

test("frame membership uses QPC time rather than receipt batches", () => {
  const events = [
    { event: "sample", atMs: 15, capture: { times100ns: [0, 100000, 200000] } },
    { event: "mark", atMs: 50, capture: { times100ns: [300000, 400000, 500000, 600000] } },
  ];
  const summary = summarizeFrames(events, 150000, 550000);
  assert.equal(summary.observedFrames, 4);
  assert.equal(summary.frameWindowMs, 30);
  assert.equal(summary.presentedFps, 100);
  assert.equal(summary.missedRefreshes, 0);
});

test("missing refreshes survive batch boundaries and duplicate timestamps do not add frames", () => {
  const events = [
    { capture: { times100ns: [100000, 200000, 300000] } },
    { capture: { times100ns: [300000, 500000, 600000] } },
  ];
  const summary = summarizeFrames(events, 100000, 600000);
  assert.equal(summary.observedFrames, 5);
  assert.equal(summary.presentedFps, 80);
  assert.equal(summary.missedRefreshes, 1);
  assert.equal(summary.frameMaxMs, 20);
});

test("frame summaries reject old probes and handle an empty hidden interval", () => {
  assert.throws(() => summarizeFrames([], undefined, undefined));
  assert.throws(() => summarizeFrames([], 10, 10));
  const summary = summarizeFrames([{ capture: { times100ns: [5, 35] } }], 10, 30);
  assert.equal(summary.presentedFps, 0);
  assert.equal(summary.observedFrames, 0);
});
