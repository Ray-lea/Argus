import assert from "node:assert/strict";
import { test } from "node:test";
import { parseSteps, pickFrames, runSeconds, unmet } from "../lib/ui-shots.mjs";

test("steps are checked before the window starts", () => {
  assert.throws(() => parseSteps({}), /array/);
  assert.throws(() => parseSteps([{ click: "x" }]), /unknown "click"/);
  assert.throws(() => parseSteps([{ expect: { a: 1 } }]), /expect needs inspect/);
  assert.throws(() => parseSteps([{ crop: { from: "later", name: "c", region: [0, 0, 1, 1] } }]), /earlier capture/);
  assert.throws(() => parseSteps([{ probe: "stop" }]), /other than stop/);
  assert.throws(() => parseSteps([{ capture: "a" }, { crop: { from: "a", name: "c", region: [0, 0, 1.5, 1] } }]), /whole pixels/);
  const [inspect, , sheet] = parseSteps([
    { inspect: "menu", expect: { "state.open": true } },
    { record: { name: "drag", ms: 800 } },
    { sheet: { name: "s", from: "drag", region: [0, 0, 10, 10] } },
  ]);
  assert.equal(inspect.label, "step-1");
  assert.deepEqual([sheet.sheet.columns, sheet.sheet.scale, sheet.sheet.every, sheet.sheet.limit], [1, 1, 1, 40]);
});

test("expectations read dotted paths and compare whole values", () => {
  const event = { state: { focus: { in: "submenu", item: { label: "CPU", checked: true } }, items: ["a", "b"] } };
  assert.deepEqual(unmet(event, { "state.focus.in": "submenu", "state.items.1": "b", "state.focus.item": { label: "CPU", checked: true } }), []);
  assert.deepEqual(unmet(event, { "state.focus.in": "menu", "state.missing": 1 }), [
    'state.focus.in: expected "menu", got "submenu"',
    "state.missing: expected 1, got undefined",
  ]);
});

test("a sheet keeps the last frame and spreads a long recording", () => {
  assert.deepEqual(pickFrames(10, 4, 40), [0, 4, 8, 9]);
  assert.deepEqual(pickFrames(0, 4, 40), []);
  const spread = pickFrames(200, 1, 5);
  assert.deepEqual(spread, [0, 50, 100, 149, 199]);
});

test("a drag spells out hover, press, even moves and release", () => {
  const steps = parseSteps([{ drag: { at: "header:cpu", dx: 100, steps: 4, interval: 20 }, record: { name: "d", ms: 500 }, wait: 900 }]);
  assert.deepEqual(
    steps.map((step) => step.control.pointer),
    [
      { at: "header:cpu", event: "hover" },
      { event: "down", dx: 0, dy: 0 },
      { event: "move", dx: 25, dy: 0 },
      { event: "move", dx: 50, dy: 0 },
      { event: "move", dx: 75, dy: 0 },
      { event: "move", dx: 100, dy: 0 },
      { event: "up", dx: 100, dy: 0 },
    ],
  );
  assert.equal(steps[1].record.name, "d");
  assert.equal(steps.at(-1).wait, 900);
  assert.throws(() => parseSteps([{ drag: { at: "x" }, capture: "c" }]), /combines only/);
});

test("the run lasts as long as its waits and recordings", () => {
  assert.equal(runSeconds(parseSteps([{ wait: 2000 }, { record: { name: "r", ms: 1500 } }])), 65);
});
