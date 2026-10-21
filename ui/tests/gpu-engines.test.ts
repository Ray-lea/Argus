// Which engines the GPU view charts: how many, which, in which order.

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  arrange,
  chartCount,
  isSmart,
  shownEngines,
  wasBusy,
  withCount,
  withEngine,
  withSmart,
} from "../src/lib/gpu-engines.ts";

const ENGINES = ["3D", "Copy", "Video Encode", "Video Decode", "Compute_0", "Security"];

test("until chosen, every engine is charted in the GPU's order", () => {
  assert.deepEqual(shownEngines(ENGINES), ENGINES);
  assert.equal(chartCount(ENGINES), 6);
  assert.deepEqual(shownEngines([]), []);
});

test("the count keeps the arranged order and stays within the engines", () => {
  const two = withCount(ENGINES, undefined, 2);
  assert.deepEqual(two, { count: 2, order: ENGINES });
  assert.deepEqual(shownEngines(ENGINES, two), ["3D", "Copy"]);
  // All of them is no count: engines the GPU adds later are charted too.
  assert.equal(withCount(ENGINES, two, 6).count, null);
  assert.equal(withCount(ENGINES, two, 99).count, null);
  assert.equal(withCount(ENGINES, two, 0).count, 1);
  // A stored count larger than what this GPU has.
  assert.equal(chartCount(["3D", "Copy"], { count: 4, order: [] }), 2);
});

test("an engine not charted takes the place, and the one it replaces comes next", () => {
  const two = withCount(ENGINES, undefined, 2);
  const chosen = withEngine(ENGINES, two, 0, "Video Decode");
  assert.deepEqual(shownEngines(ENGINES, chosen), ["Video Decode", "Copy"]);
  assert.deepEqual(chosen.order, ["Video Decode", "Copy", "3D", "Video Encode", "Compute_0", "Security"]);
  // Showing one more brings the replaced engine back.
  assert.deepEqual(shownEngines(ENGINES, withCount(ENGINES, chosen, 3)), ["Video Decode", "Copy", "3D"]);
});

test("an engine charted elsewhere trades places", () => {
  const two = withCount(ENGINES, undefined, 2);
  const swapped = withEngine(ENGINES, two, 0, "Copy");
  assert.deepEqual(shownEngines(ENGINES, swapped), ["Copy", "3D"]);
  // Choosing the engine already there changes nothing.
  assert.deepEqual(withEngine(ENGINES, two, 1, "Copy"), two);
  // An engine this GPU does not have changes nothing either.
  assert.deepEqual(shownEngines(ENGINES, withEngine(ENGINES, two, 0, "VR")), ["3D", "Copy"]);
});

test("a choice made on another driver keeps what still applies", () => {
  // Engines renamed or gone are skipped, new ones follow the arranged ones.
  const stored = { count: 3, order: ["Video Decode", "Legacy Overlay", "Video Decode", "Copy"] };
  assert.deepEqual(arrange(ENGINES, stored), [
    "Video Decode",
    "Copy",
    "3D",
    "Video Encode",
    "Compute_0",
    "Security",
  ]);
  assert.deepEqual(shownEngines(ENGINES, stored), ["Video Decode", "Copy", "3D"]);
});

// Smart display: 3D and Video Encode busy, the rest idle.
const BUSY = new Set(["3D", "Video Encode"]);
const busy = (e: string) => BUSY.has(e);

test("until chosen, smart display charts the busy engines in order", () => {
  assert.ok(isSmart(undefined));
  assert.deepEqual(shownEngines(ENGINES, undefined, busy), ["3D", "Video Encode"]);
  // Nothing busy: the first engine keeps the place.
  assert.deepEqual(
    shownEngines(ENGINES, undefined, () => false),
    ["3D"],
  );
  // In the order the user arranged.
  const arranged = { ...withCount(ENGINES, undefined, 6), smart: true };
  arranged.order = ["Video Encode", ...ENGINES.filter((e) => e !== "Video Encode")];
  assert.deepEqual(shownEngines(ENGINES, arranged, busy), ["Video Encode", "3D"]);
});

test("a count picked before smart display existed is kept", () => {
  const saved = { count: 2, order: ENGINES };
  assert.ok(!isSmart(saved));
  assert.deepEqual(shownEngines(ENGINES, saved, busy), ["3D", "Copy"]);
});

test("a busy engine is one that reached 1% within the span", () => {
  assert.ok(wasBusy([0, 0, 1.2, 0]));
  assert.ok(!wasBusy([0, 0.4, 0.9, Number.NaN]));
  assert.ok(!wasBusy([]));
});

test("picking a count under smart display starts from the charts on screen", () => {
  const two = withCount(ENGINES, undefined, 2, ["3D", "Video Encode"]);
  assert.ok(!isSmart(two));
  assert.deepEqual(shownEngines(ENGINES, two, busy), ["3D", "Video Encode"]);
  assert.deepEqual(shownEngines(ENGINES, withCount(ENGINES, two, 3), busy), ["3D", "Video Encode", "Copy"]);
});

test("choosing an engine under smart display picks the charts by hand from then on", () => {
  const chosen = withEngine(ENGINES, undefined, 1, "Copy", ["3D", "Video Encode"]);
  assert.ok(!isSmart(chosen));
  assert.deepEqual(shownEngines(ENGINES, chosen, busy), ["3D", "Copy"]);
});

test("back to smart display keeps the count and order for later", () => {
  const two = withCount(ENGINES, undefined, 2, ["3D", "Video Encode"]);
  const smart = withSmart(ENGINES, two);
  assert.ok(isSmart(smart));
  assert.deepEqual(shownEngines(ENGINES, smart, busy), ["3D", "Video Encode"]);
  assert.deepEqual(shownEngines(ENGINES, { ...smart, smart: false }, busy), ["3D", "Video Encode"]);
});
