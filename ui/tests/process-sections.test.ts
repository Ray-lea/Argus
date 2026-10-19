import assert from "node:assert/strict";
import { test } from "node:test";
import { processSections } from "../src/lib/process-sections.ts";

const row = (id: string, app = false, windows = false, depth = 0) => ({ id, app, windows, depth });

test("category order is apps, background, Windows without changing order inside a category", () => {
  const groups = processSections([row("os", false, true), row("b1"), row("a", true), row("b2")]);
  assert.deepEqual(
    groups.map((g) => g.section),
    ["apps", "background", "windows"],
  );
  assert.deepEqual(
    groups.map((g) => g.rows.map((r) => r.id)),
    [["a"], ["b1", "b2"], ["os"]],
  );
});

test("a Windows app with a taskbar window stays with applications", () => {
  const [group] = processSections([row("notepad", true, true)]);
  assert.equal(group.section, "apps");
});

test("expanded members stay with their parent and do not inflate category counts", () => {
  const groups = processSections([
    row("browser", true),
    row("renderer", false, false, 1),
    row("helper", false, false, 1),
    row("system-group", false, true),
    row("system-child", false, false, 1),
    row("bg"),
  ]);
  assert.deepEqual(
    groups.map((g) => g.count),
    [1, 1, 1],
  );
  assert.deepEqual(
    groups[0].rows.map((r) => r.id),
    ["browser", "renderer", "helper"],
  );
  assert.deepEqual(
    groups[2].rows.map((r) => r.id),
    ["system-group", "system-child"],
  );
});

test("filtered empty categories disappear, including a completely empty result", () => {
  assert.deepEqual(processSections([]), []);
  assert.deepEqual(
    processSections([row("os", false, true)]).map((g) => g.section),
    ["windows"],
  );
});
