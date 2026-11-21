import assert from "node:assert/strict";
import { test } from "node:test";
import { aboutOf, parseSteps, pickFrames, runOptions, runSeconds, unmet, unwanted, verdict } from "../lib/ui-shots.mjs";

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

test("a value that must have changed is compared with the one it must not be", () => {
  const event = { state: { scroll: { y: -120 } }, settingsTop: 0 };
  assert.deepEqual(unwanted(event, { "state.scroll.y": 0 }), []);
  assert.deepEqual(unwanted(event, { settingsTop: 0, "state.scroll.y": 0 }), ["settingsTop: must not be 0"]);
  assert.throws(() => parseSteps([{ expectNot: { a: 1 } }]), /expectNot needs inspect/);
  assert.equal(parseSteps([{ inspect: "layout", expectNot: { settingsTop: 0 } }])[0].label, "step-1");
});

test("a steps file may say what it is for", () => {
  assert.equal(aboutOf([{ wait: 1 }]), "");
  assert.equal(aboutOf({ steps: [] }), "");
  assert.equal(aboutOf({ about: "  Checks the header menu.  ", steps: [] }), "Checks the header menu.");
  assert.throws(() => aboutOf({ about: "", steps: [] }), /non-empty text/);
  assert.throws(() => aboutOf({ about: ["a"], steps: [] }), /non-empty text/);
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

test("a steps file states what its run needs", () => {
  assert.deepEqual(runOptions([{ wait: 1 }]), { live: false, env: {}, parity: {} });
  assert.deepEqual(runOptions({ steps: [] }), { live: false, env: {}, parity: {} });
  assert.deepEqual(runOptions({ run: { live: true, env: { ARGUS_TEST_BACKDROP_BUILD: "22000" } }, steps: [] }), { live: true, env: { ARGUS_TEST_BACKDROP_BUILD: "22000" }, parity: {} });
  assert.deepEqual(runOptions({ run: { parity: { ARGUS_TABLE_ROWS: "component" } }, steps: [] }).parity, { ARGUS_TABLE_ROWS: "component" });
  assert.throws(() => runOptions({ run: { parity: { TABLE_ROWS: "component" } } }), /run.parity: TABLE_ROWS must be an ARGUS_ switch/);
  assert.throws(() => runOptions({ run: { parity: ["ARGUS_X"] } }), /run.parity maps names to values/);
  assert.throws(() => runOptions({ run: { env: { PATH: "x" } } }), /ARGUS_ switch/);
  assert.throws(() => runOptions({ run: { env: { ARGUS_X: 1 } } }), /string value/);
  assert.throws(() => runOptions({ run: { release: true } }), /unknown "release"/);
  assert.throws(() => runOptions({ run: { live: "yes" } }), /true or false/);
});

test("a listed failure is expected, and its passing is not", () => {
  assert.deepEqual(verdict(true, undefined), { result: "passed", unexpected: false });
  assert.deepEqual(verdict(false, undefined), { result: "failed", unexpected: true });
  assert.deepEqual(verdict(false, "menu moved"), { result: "known failure", unexpected: false, known: "menu moved" });
  assert.deepEqual(verdict(true, "menu moved"), { result: "fixed", unexpected: true });
});

test("the run lasts as long as its waits and recordings", () => {
  assert.equal(runSeconds(parseSteps([{ wait: 2000 }, { record: { name: "r", ms: 1500 } }])), 65);
});
