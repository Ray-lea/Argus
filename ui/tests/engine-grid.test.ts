// How the GPU view lays out its engine charts.

import assert from "node:assert/strict";
import { test } from "node:test";
import { evenRest, fills, type GridPlan, planGrid, samePlan } from "../src/lib/engine-grid.ts";

const CELL = { width: 150, height: 76 };
const GAP = 6;
/** A wide area like the window at its usual size, and a tall, narrow one. */
const WIDE = [1040, 400] as const;
const TALL = [420, 760] as const;
/** The grid, and the first chart's block. */
const shape = (plan: GridPlan) =>
  `${plan.columns}×${plan.rows} first ${plan.cells[0].columns}×${plan.cells[0].rows}`;
/** Each chart as "column,row" (from 1). */
const spots = (plan: GridPlan) => plan.cells.map((c) => `${c.column},${c.row}`);

test("focus follows the preset for each count: the first chart a block, every other one unit", () => {
  const focus = (n: number) => planGrid("focus", n, ...WIDE, CELL, GAP);
  assert.equal(shape(focus(2)), "2×1 first 1×1");
  // Three: the first two units square, two stacked beside it.
  assert.equal(shape(focus(3)), "3×2 first 2×2");
  assert.deepEqual(spots(focus(3)), ["1,1", "3,1", "3,2"]);
  // Four: three square, three stacked beside it.
  assert.equal(shape(focus(4)), "4×3 first 3×3");
  // Five: two square, four beside it in two columns.
  assert.equal(shape(focus(5)), "4×2 first 2×2");
  assert.deepEqual(spots(focus(5)), ["1,1", "3,1", "4,1", "3,2", "4,2"]);
  // Six: two square in an L of five.
  assert.equal(shape(focus(6)), "3×3 first 2×2");
  assert.deepEqual(spots(focus(6)), ["1,1", "3,1", "3,2", "1,3", "2,3", "3,3"]);
  // Eight: three square in an L of seven.
  assert.equal(shape(focus(8)), "4×4 first 3×3");
});

test("in a tall area focus turns the arrangement on its side: the first chart on top", () => {
  const three = planGrid("focus", 3, ...TALL, CELL, GAP);
  assert.equal(shape(three), "2×3 first 2×2");
  assert.deepEqual(spots(three), ["1,1", "1,3", "2,3"]);
  assert.equal(shape(planGrid("focus", 4, ...TALL, CELL, GAP)), "3×4 first 3×3");
});

test("focus falls back to a more compact preset where the first would make cells too small", () => {
  // Eight in a short, wide area: four rows of units would be too low.
  assert.equal(shape(planGrid("focus", 8, 1040, 260, CELL, GAP)), "5×3 first 4×2");
});

test("every layout fills its grid, and every chart but the first is one unit", () => {
  for (const layout of ["grid", "focus", "rows"] as const) {
    for (let n = 1; n <= 20; n++) {
      for (const [w, h] of [WIDE, TALL, [904, 234], [1285, 560], [524, 240]] as const) {
        const plan = planGrid(layout, n, w, h, CELL, GAP);
        assert.equal(plan.cells.length, n, `${layout} ${n} in ${w}×${h}: count`);
        assert.ok(fills(plan), `${layout} ${n} in ${w}×${h}: fills`);
        assert.ok(evenRest(plan), `${layout} ${n} in ${w}×${h}: equal`);
      }
    }
  }
});

test("a grid of a count that splits into rows and columns has every chart the same size", () => {
  const grid = (n: number) => planGrid("grid", n, ...WIDE, CELL, GAP);
  assert.equal(shape(grid(2)), "2×1 first 1×1");
  assert.equal(shape(grid(3)), "3×1 first 1×1");
  assert.equal(shape(grid(4)), "2×2 first 1×1");
  assert.equal(shape(grid(6)), "3×2 first 1×1");
  assert.equal(shape(grid(8)), "4×2 first 1×1");
  assert.equal(shape(grid(9)), "3×3 first 1×1");
  assert.equal(shape(grid(12)), "4×3 first 1×1");
  assert.equal(shape(grid(15)), "5×3 first 1×1");
  assert.equal(shape(grid(16)), "4×4 first 1×1");
});

test("a count that does not split: one row where it reads well, else the first chart takes the spare unit", () => {
  // Five in a short, wide area: one row of five.
  assert.equal(shape(planGrid("grid", 5, 904, 234, CELL, GAP)), "5×1 first 1×1");
  // Five in a taller one: three by two, the first two units wide.
  const five = planGrid("grid", 5, ...WIDE, CELL, GAP);
  assert.equal(shape(five), "3×2 first 2×1");
  assert.deepEqual(spots(five), ["1,1", "3,1", "1,2", "2,2", "3,2"]);
  // Seven: four by two, the first two units wide.
  assert.equal(shape(planGrid("grid", 7, ...WIDE, CELL, GAP)), "4×2 first 2×1");
});

test("rows put every engine in a row of its own", () => {
  assert.equal(shape(planGrid("rows", 4, ...WIDE, CELL, GAP)), "1×4 first 1×1");
});

test("plans compare by value", () => {
  assert.ok(samePlan(planGrid("focus", 6, ...WIDE, CELL, GAP), planGrid("focus", 6, ...WIDE, CELL, GAP)));
  assert.ok(!samePlan(planGrid("grid", 4, ...WIDE, CELL, GAP), planGrid("focus", 4, ...WIDE, CELL, GAP)));
});
