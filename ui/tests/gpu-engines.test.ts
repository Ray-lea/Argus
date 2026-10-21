// Which engines the GPU view charts: how many, which, in which order.

import assert from "node:assert/strict";
import { test } from "node:test";
import { arrange, chartCount, shownEngines, withCount, withEngine } from "../src/lib/gpu-engines.ts";

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
