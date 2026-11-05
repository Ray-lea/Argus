// Steps for scripts/ui-shots.mjs, checked before anything starts.
import assert from "node:assert/strict";
import { isDeepStrictEqual } from "node:util";

/** Inspections: the test control that asks, and the event that answers. */
export const INSPECTIONS = {
  columns: ["inspectColumns", "ui-process-columns"],
  menu: ["inspectMenu", "ui-menu"],
  layout: ["inspectLayout", "ui-layout"],
  vitals: ["inspectVitals", "ui-vitals"],
};

const KINDS = ["note", "control", "probe", "record", "wait", "inspect", "expect", "capture", "crop", "sheet"];
const NAME = /^[\w.-]+$/;

/**
 * A pointer drag spelled out: hover over `at`, press, move by `dx`, `dy` in
 * `steps` even steps `interval` ms apart, release. A `record` on the drag
 * starts with the press. Presses act only with a fixture.
 */
function expandDrag({ drag, ...rest }, where) {
  assert(drag && typeof drag.at === "string", `${where}: drag needs at`);
  const { at, dx = 0, dy = 0, steps = 20, interval = 30 } = drag;
  assert(Number.isInteger(steps) && steps > 0, `${where}: drag steps must be a whole number`);
  const pointer = (event, fraction) => ({ pointer: { event, dx: dx * fraction, dy: dy * fraction } });
  return [
    { control: { pointer: { at, event: "hover" } }, wait: 150 },
    { control: pointer("down", 0), wait: 80, ...(rest.record ? { record: rest.record } : {}) },
    ...Array.from({ length: steps }, (_, i) => ({ control: pointer("move", (i + 1) / steps), wait: interval })),
    { control: pointer("up", 1), wait: rest.wait ?? 400 },
  ];
}

const region = (value, where) => {
  assert(
    Array.isArray(value) && value.length === 4 && value.every((n) => Number.isInteger(n) && n >= 0),
    `${where}: region must be [x, y, width, height] in whole pixels`,
  );
  return value;
};

/**
 * Checks every step and fills in defaults. A step may combine kinds; they run
 * in the order of `KINDS` (control, then probe, record, wait, inspect, …).
 */
export function parseSteps(value) {
  assert(Array.isArray(value), "steps must be an array");
  const captures = new Set();
  const recordings = new Set();
  const expanded = value.flatMap((raw, index) => {
    if (!raw || typeof raw !== "object" || !("drag" in raw)) return [raw];
    const extra = Object.keys(raw).filter((key) => !["drag", "record", "wait", "note"].includes(key));
    assert(extra.length === 0, `step ${index + 1}: a drag combines only with record, wait and note`);
    return expandDrag(raw, `step ${index + 1}`).map((step, i) => (i === 0 && raw.note ? { note: raw.note, ...step } : step));
  });
  return expanded.map((raw, index) => {
    const where = `step ${index + 1}`;
    assert(raw && typeof raw === "object" && !Array.isArray(raw), `${where} must be an object`);
    for (const key of Object.keys(raw)) assert(KINDS.includes(key) || key === "label", `${where}: unknown "${key}"`);
    if ("label" in raw) assert("inspect" in raw && NAME.test(raw.label), `${where}: label names an inspection`);
    const step = { ...raw };
    if ("control" in step) assert(step.control && typeof step.control === "object", `${where}: control must be an object`);
    if ("probe" in step) assert(typeof step.probe === "string" && !/^(stop|record)\b/.test(step.probe), `${where}: probe takes a command other than stop or record`);
    if ("wait" in step) assert(Number.isFinite(step.wait) && step.wait >= 0, `${where}: wait is milliseconds`);
    if ("inspect" in step) {
      assert(step.inspect in INSPECTIONS, `${where}: inspect one of ${Object.keys(INSPECTIONS).join(", ")}`);
      step.label ??= `step-${index + 1}`;
    }
    if ("expect" in step) {
      assert("inspect" in step, `${where}: expect needs inspect`);
      assert(step.expect && typeof step.expect === "object", `${where}: expect maps paths to values`);
    }
    if ("capture" in step) {
      assert(NAME.test(step.capture), `${where}: capture names a file`);
      captures.add(step.capture);
    }
    if ("record" in step) {
      const record = step.record;
      assert(record && NAME.test(record.name ?? ""), `${where}: record needs a name`);
      assert(Number.isInteger(record.ms) && record.ms > 0, `${where}: record needs ms`);
      if (record.crop) region(record.crop, where);
      recordings.add(record.name);
    }
    if ("crop" in step) {
      const crop = step.crop;
      assert(captures.has(crop?.from), `${where}: crop takes an earlier capture`);
      assert(NAME.test(crop.name ?? ""), `${where}: crop needs a name`);
      region(crop.region, where);
      crop.scale ??= 1;
    }
    if ("sheet" in step) {
      const sheet = step.sheet;
      assert(NAME.test(sheet?.name ?? ""), `${where}: sheet needs a name`);
      const from = sheet.from;
      assert(
        (typeof from === "string" && recordings.has(from)) || (Array.isArray(from) && from.length > 0 && from.every((name) => captures.has(name))),
        `${where}: sheet takes an earlier recording, or earlier captures`,
      );
      region(sheet.region, where);
      sheet.columns ??= 1;
      sheet.scale ??= 1;
      sheet.every ??= 1;
      sheet.limit ??= 40;
    }
    return step;
  });
}

/** The value at a dotted path; numeric segments index arrays. */
export function valueAt(object, path) {
  return path.split(".").reduce((value, key) => value?.[key], object);
}

/** What an inspected event fails of its expectations, as messages. */
export function unmet(event, expect) {
  return Object.entries(expect)
    .filter(([path, want]) => !isDeepStrictEqual(valueAt(event, path), want))
    .map(([path, want]) => `${path}: expected ${JSON.stringify(want)}, got ${JSON.stringify(valueAt(event, path))}`);
}

/** Which of `count` frames a sheet shows: every `every`th, the last always, at most `limit`. */
export function pickFrames(count, every, limit) {
  const picked = [];
  for (let i = 0; i < count; i += Math.max(1, every)) picked.push(i);
  if (count > 0 && picked.at(-1) !== count - 1) picked.push(count - 1);
  if (picked.length <= limit) return picked;
  // Too many: spread `limit` of them evenly, keeping the first and last.
  return Array.from({ length: limit }, (_, i) => picked[Math.round((i * (picked.length - 1)) / (limit - 1))]);
}

/** How long the probe lets the test run: every wait and recording, plus room to start and stop. */
export function runSeconds(steps) {
  const ms = steps.reduce((sum, step) => sum + (step.wait ?? 0) + (step.record?.ms ?? 0) + 500, 0);
  return Math.ceil(ms / 1000) + 60;
}
