import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { MAX_MINUTES, clearHold, holdGate, readHold, writeHold } from "../lib/dev-hold.mjs";

test("a hold is in force until it expires or is cleared", () => {
  const dir = mkdtempSync(join(tmpdir(), "argus-hold-"));
  try {
    const path = join(dir, "native-dev", "hold.json");
    const now = 1_000_000;
    assert.equal(readHold(path, now), null);
    assert.deepEqual(writeHold(path, 20, "splitting history.rs", now), { until: now + 20 * 60000, reason: "splitting history.rs" });
    assert.deepEqual(readHold(path, now + 19 * 60000), { until: now + 20 * 60000, reason: "splitting history.rs" });
    assert.equal(readHold(path, now + 20 * 60000), null, "an expired hold is none");
    assert.equal(clearHold(path, now + 60000), true);
    assert.equal(clearHold(path, now + 60000), false);
    assert.equal(readHold(path, now), null);

    assert.throws(() => writeHold(path, MAX_MINUTES + 1, "", now), /up to/);
    assert.throws(() => writeHold(path, 0, "", now), /up to/);
    // A file that is not a hold, or one that claims to last beyond the limit, holds nothing.
    writeFileSync(path, "not json");
    assert.equal(readHold(path, now), null);
    writeFileSync(path, JSON.stringify({ until: now + (MAX_MINUTES + 5) * 60000 }));
    assert.equal(readHold(path, now), null);
    writeFileSync(path, JSON.stringify({ until: now + 60000, reason: 7 }));
    assert.deepEqual(readHold(path, now), { until: now + 60000, reason: "" });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("changes during a hold lead to one build after it", (context) => {
  context.mock.timers.enable({ apis: ["setInterval"] });
  let hold = null;
  const events = [];
  const gate = holdGate({ active: () => hold, build: () => events.push("build"), report: (state) => events.push(state ? "held" : "released"), poll: 1000 });

  gate.changed();
  assert.deepEqual(events, ["build"], "no hold: a change builds at once");

  hold = { until: 1, reason: "" };
  gate.changed();
  gate.changed();
  context.mock.timers.tick(3000);
  gate.changed();
  assert.deepEqual(events, ["build", "held"], "held: reported once, nothing built");

  hold = null;
  gate.changed();
  assert.deepEqual(events, ["build", "held"], "the waiting build covers a change right after the release");
  context.mock.timers.tick(1000);
  assert.deepEqual(events, ["build", "held", "released", "build"]);
  context.mock.timers.tick(5000);
  assert.equal(events.length, 4, "the waiting ended with the hold");

  gate.changed();
  assert.equal(events.at(-1), "build");
  hold = { until: 1, reason: "" };
  gate.changed();
  gate.stop();
  hold = null;
  context.mock.timers.tick(5000);
  assert.deepEqual(events.slice(4), ["build", "held"], "a stopped gate builds nothing more");
});
