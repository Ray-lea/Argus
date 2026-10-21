import assert from "node:assert/strict";
import { test } from "node:test";
import { folded, processSections, sectionTotals, withSection } from "../src/lib/process-sections.ts";

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

const measured = (depth: number, values: Partial<Parameters<typeof sectionTotals>[0][number]> = {}) => ({
  depth,
  cpu: 1,
  mem: 100,
  io: 10,
  diskRead: null,
  diskWrite: null,
  gpu: 40,
  netSent: 1,
  netReceived: 2,
  threads: 3,
  handles: 4,
  ...values,
});

test("a heading sums its top-level rows, whose groups already include their members", () => {
  const totals = sectionTotals([
    measured(0, { diskRead: 5 }),
    measured(1, { cpu: 50, diskRead: 99 }),
    measured(0),
    measured(0, { gpu: 30 }),
  ]);
  assert.deepEqual(totals, {
    cpu: 3,
    mem: 300,
    io: 30,
    diskRead: 5,
    diskWrite: null,
    gpu: 100,
    net: 9,
    threads: 9,
    handles: 12,
  });
});

test("folded headings stay folded until unfolded, except while searching", () => {
  let collapsed = withSection([], "windows", true);
  collapsed = withSection(collapsed, "apps", true);
  assert.deepEqual(collapsed, ["apps", "windows"]);
  assert.equal(folded(collapsed, "apps", false), true);
  assert.equal(folded(collapsed, "background", false), false);
  assert.equal(folded(collapsed, "apps", true), false);
  assert.deepEqual(withSection(collapsed, "apps", false), ["windows"]);
  assert.deepEqual(withSection(collapsed, "apps", true), ["apps", "windows"]);
});
