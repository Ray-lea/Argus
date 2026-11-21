import assert from "node:assert/strict";
import { test } from "node:test";
import { assignments, compare, format, phaseFigures, sides } from "../lib/perf-ab.mjs";

// A perf-native result with one phase: `cycles` spent over ten seconds and `frames` presented.
function result(cycles, frames, extra = {}) {
  const mark = (tag, total) => ({ event: "mark", tag, processes: [{ cpuCycles: total * 0.75 }, { cpuCycles: total * 0.25 }] });
  return {
    events: [mark("0-scroll-begin", 1e9), mark("0-scroll-end", 1e9 + cycles), { event: "sample" }],
    summaries: [
      { tag: "0-scroll", durationMs: 10000, observedFrames: frames, presentedFps: frames / 10, systemBusyPercent: 30, cpuMsPerSecond: cycles / 3e7, uiThreadCpuMsPerSecond: null, gpuBusiestEnginePercent: 1.5, privateMiB: 180, frameP95Ms: 9, ...extra },
    ],
  };
}

test("figures come from the marks and the phase summary", () => {
  const [phase] = phaseFigures(result(12e9, 1200));
  assert.equal(phase.phase, "0-scroll");
  assert.equal(phase.busy, 30);
  assert.equal(phase.figures.mcyclesPerFrame, 10);
  assert.equal(phase.figures.mcyclesPerSecond, 1200);
  assert.equal(phase.figures.cpuMsPerSecond, 400);
  assert.equal(phase.figures.uiThreadCpuMsPerSecond, null);
  // Without capture there are no frames to count by.
  const [blind] = phaseFigures(result(12e9, null, { presentedFps: null, frameP95Ms: null }));
  assert.equal(blind.figures.mcyclesPerFrame, null);
  assert.equal(blind.figures.mcyclesPerSecond, 1200);
});

test("sides take turns and assignments split at the first equals sign", () => {
  assert.deepEqual([sides(0), sides(1), sides(2)], [["base", "new"], ["new", "base"], ["base", "new"]]);
  assert.deepEqual(assignments(["ARGUS_TABLE_ROWS=component", "A=b=c", "EMPTY="]), { ARGUS_TABLE_ROWS: "component", A: "b=c", EMPTY: "" });
  assert.throws(() => assignments(["NOVALUE"]), /NAME=VALUE/);
});

test("pairs are compared by the median of their changes", () => {
  const pair = (base, fresh, frames = [1200, 1200]) => ({ base: phaseFigures(result(base, frames[0])), new: phaseFigures(result(fresh, frames[1])) });
  const [phase] = compare([pair(12e9, 9e9), pair(10e9, 8e9), pair(12e9, 9.6e9)]);
  const row = (key) => phase.rows.find((entry) => entry.key === key);
  assert.equal(phase.pairs, 3);
  assert.equal(phase.unmatched, 0);
  assert.deepEqual(phase.busy, [30, 30]);
  const frame = row("mcyclesPerFrame");
  assert.equal(frame.base, 10);
  assert.equal(frame.new, 7.5);
  assert.ok(Math.abs(frame.change + 0.2) < 1e-9 && Math.abs(frame.least + 0.25) < 1e-9 && Math.abs(frame.most + 0.2) < 1e-9);
  assert.deepEqual([frame.lower, frame.higher, frame.pairs], [3, 0, 3]);
  assert.equal(row("uiThreadCpuMsPerSecond").pairs, 0);
  assert.deepEqual([row("privateMiB").change, row("privateMiB").lower, row("privateMiB").higher], [0, 0, 0]);

  const text = format([phase]).join("\n");
  assert.match(text, /0-scroll {2}\(3 pairs; system 30-30% busy\)/);
  assert.match(text, /Mcycles\/frame\s+10\.00\s+7\.50\s+-20\.0%\s+-25\.0% … -20\.0%\s+all lower/);
  assert.match(text, /UI thread ms\/s\s+-\s+-$/m);
  assert.match(text, /private MiB\s+180\.0\s+180\.0\s+\+0\.0%\s+\+0\.0% … \+0\.0%\s+equal/);
  // One pair has nothing to agree with.
  assert.match(format(compare([pair(12e9, 9e9)])).join("\n"), /Mcycles\/s\s+1200\s+900\s+-25\.0%\s+-25\.0% … -25\.0%\s+lower$/m);
});

test("a pair with unmatched frame rates is left out of per-frame figures only", () => {
  const pair = (base, fresh, frames) => ({ base: phaseFigures(result(base, frames[0])), new: phaseFigures(result(fresh, frames[1])) });
  const [phase] = compare([pair(12e9, 9e9, [1200, 1200]), pair(12e9, 9e9, [1200, 1500])]);
  assert.equal(phase.unmatched, 1);
  assert.equal(phase.rows.find((entry) => entry.key === "mcyclesPerFrame").pairs, 1);
  assert.equal(phase.rows.find((entry) => entry.key === "mcyclesPerSecond").pairs, 2);
  assert.match(format([phase]).join("\n"), /1 with frame rates more than 10% apart/);
  // A disagreement shows as such.
  const [mixed] = compare([pair(10e9, 9e9, [1200, 1200]), pair(10e9, 11e9, [1200, 1200])]);
  assert.match(format([mixed]).join("\n"), /Mcycles\/s\s+1000\s+1000\s+\+0\.0%\s+-10\.0% … \+10\.0%\s+1 lower, 1 higher/);
});
