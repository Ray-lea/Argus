import assert from "node:assert/strict";
import { test } from "node:test";
import { type CoreGridSize, coreGridColumns } from "../src/lib/core-grid.ts";

const grid: CoreGridSize = {
  count: 20,
  width: 1116,
  height: 256,
  cellWidth: 150,
  cellHeight: 80,
  headerHeight: 22,
  gap: 6,
};

test("processor columns still adapt to both width and height", () => {
  assert.equal(coreGridColumns(grid), 7);
  assert.equal(coreGridColumns({ ...grid, height: 500 }), 5);
  assert.equal(coreGridColumns({ ...grid, height: 500 }, 7), 5);
  assert.equal(coreGridColumns({ ...grid, height: 380 }, 5), 7);
});

test("minor height changes near a crossover preserve the current layout", () => {
  for (const previous of [5, 7]) {
    let columns = previous;
    for (const height of [445, 447, 444.75, 447.25, 445]) {
      columns = coreGridColumns({ ...grid, height }, columns);
      assert.equal(columns, previous);
    }
  }
});

test("filters favor full rows without empty columns", () => {
  assert.equal(coreGridColumns({ ...grid, count: 12 }), 6);
  assert.equal(coreGridColumns({ ...grid, count: 8 }), 4);
  assert.equal(coreGridColumns({ ...grid, count: 1 }), 1);
  assert.equal(coreGridColumns({ ...grid, count: 0 }), 1);
});

test("minimum sizes override stability and short grids can scroll", () => {
  assert.equal(coreGridColumns({ ...grid, width: 1086 }, 7), 7);
  assert.ok(coreGridColumns({ ...grid, width: 1085.9 }, 7) <= 6);
  assert.equal(coreGridColumns({ ...grid, width: 149 }, 7), 1);
  assert.equal(coreGridColumns({ ...grid, count: 256, height: 20 }), 7);
});
