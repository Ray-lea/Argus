// How the GPU view lays out its engine charts, on a grid of equal units that
// they fill with no empty unit. Grid: every chart one unit, when the count
// splits into rows × columns that suit the area; otherwise the first chart
// takes the units left over. Focus: the first chart a block of units (square
// or nearly), every other chart one unit, in a preset arrangement for each
// count, turned on its side in a tall area. List: one row per engine. Pure;
// run by ui/tests directly.

export type EngineLayout = "grid" | "focus" | "rows";
export const ENGINE_LAYOUTS: EngineLayout[] = ["grid", "focus", "rows"];

/** The smallest a cell can be. */
export interface CellSize {
  width: number;
  height: number;
}

/** Where a chart goes: its first track and how many it spans, across and down (CSS grid lines, from 1). */
export interface Placement {
  column: number;
  columns: number;
  row: number;
  rows: number;
}

/**
 * What the grid gets: equal tracks across and down, each chart's place in
 * order, and the rows of units (each needs a cell's minimum height, which
 * sets the grid's).
 */
export interface GridPlan {
  columns: number;
  rows: number;
  cells: Placement[];
  stack: number;
}

/** A grid of units and the block the first chart takes at its top left; every other chart takes one unit. */
interface Shape {
  columns: number;
  rows: number;
  first: { columns: number; rows: number };
}

/** Width to height a small chart reads best at. */
const CELL_ASPECT = 1.5;
/** How much a grid that gives the first chart the spare units counts against one that needs none. */
const SPARE_WEIGHT = 0.8;
/** At most this many units left over in a grid, for the first chart to take. */
const SPARE_MAX = 3;

/**
 * Focus arrangements for each count, in order of preference, as [columns,
 * rows, first chart's columns, first chart's rows] for a wide area: the first
 * chart left (and up), the others in a column or two beside it, or around it
 * in an L. A tall area turns them on their side.
 */
const FOCUS: Record<number, [number, number, number, number][]> = {
  1: [[1, 1, 1, 1]],
  2: [[2, 1, 1, 1]],
  3: [[3, 2, 2, 2]],
  4: [[4, 3, 3, 3]],
  5: [[4, 2, 2, 2]],
  6: [[3, 3, 2, 2]],
  7: [
    [4, 3, 3, 2],
    [5, 2, 2, 2],
  ],
  8: [
    [4, 4, 3, 3],
    [5, 3, 4, 2],
  ],
  9: [
    [5, 4, 4, 3],
    [4, 3, 2, 2],
    [6, 2, 2, 2],
  ],
  10: [
    [5, 5, 4, 4],
    [6, 3, 3, 3],
    [5, 3, 3, 2],
  ],
  11: [
    [6, 5, 5, 4],
    [4, 4, 3, 2],
    [7, 2, 2, 2],
  ],
  12: [
    [5, 4, 3, 3],
    [5, 3, 2, 2],
  ],
  13: [
    [4, 4, 2, 2],
    [7, 3, 3, 3],
  ],
  14: [
    [5, 5, 4, 3],
    [7, 3, 4, 2],
  ],
  15: [
    [6, 5, 4, 4],
    [5, 4, 3, 2],
    [6, 3, 2, 2],
  ],
  16: [
    [6, 4, 3, 3],
    [7, 3, 3, 2],
  ],
};

const shapeOf = ([columns, rows, firstColumns, firstRows]: [number, number, number, number]): Shape => ({
  columns,
  rows,
  first: { columns: firstColumns, rows: firstRows },
});

/** The same arrangement turned on its side: the first chart on top, the others under and beside it. */
const turned = (s: Shape): Shape => ({
  columns: s.rows,
  rows: s.columns,
  first: { columns: s.first.rows, rows: s.first.columns },
});

/** The first chart at the top left, every other one unit, row by row. */
function place(shape: Shape, count: number): GridPlan {
  const cells: Placement[] = [{ column: 1, columns: shape.first.columns, row: 1, rows: shape.first.rows }];
  for (let row = 1; row <= shape.rows; row++) {
    for (let column = 1; column <= shape.columns; column++) {
      if (row <= shape.first.rows && column <= shape.first.columns) continue;
      if (cells.length < count) cells.push({ column, columns: 1, row, rows: 1 });
    }
  }
  return { columns: shape.columns, rows: shape.rows, cells, stack: shape.rows };
}

/** One unit's size in the area: its readable size at CELL_ASPECT, and whether it is as large as a cell may be. */
function unit(shape: Shape, width: number, height: number, cell: CellSize, gap: number) {
  const w = (width - (shape.columns - 1) * gap) / shape.columns;
  const h = (height - (shape.rows - 1) * gap) / shape.rows;
  const fits = w >= cell.width - 0.5 && h >= cell.height - 0.5;
  // Below the minimum the grid grows and the view scrolls: counted at half.
  return { fits, size: Math.min(w / CELL_ASPECT, h) * (fits ? 1 : 0.5) };
}

/**
 * Grid: every way of putting `count` charts in rows and columns with at most
 * SPARE_MAX units over, the first chart taking those (in as square a block as
 * fits, then as wide as can be).
 */
function gridShapes(count: number): { shape: Shape; spare: number }[] {
  const shapes: { shape: Shape; spare: number }[] = [];
  for (let rows = 1; rows <= count; rows++) {
    const columns = Math.ceil(count / rows);
    const spare = columns * rows - count;
    if (spare > SPARE_MAX || (rows > 1 && Math.ceil(count / (rows - 1)) === columns)) continue;
    const units = spare + 1;
    let first: Shape["first"] | null = null;
    for (let w = units; w >= 1; w--) {
      const h = units / w;
      if (!Number.isInteger(h) || w > columns || h > rows || h > w) continue;
      if (!first || Math.abs(w - h) < Math.abs(first.columns - first.rows)) first = { columns: w, rows: h };
    }
    if (first) shapes.push({ shape: { columns, rows, first }, spare });
  }
  return shapes;
}

/**
 * The plan for `count` charts in a `width` × `height` area. Grid: the
 * arrangement whose units read best, one needing no spare unit preferred
 * (SPARE_WEIGHT). Focus: the count's first preset whose units are as large as
 * a cell may be, wide or turned on its side (whichever reads better), else
 * the one that reads best; past the presets, as the grid. List: one row each.
 */
export function planGrid(
  layout: EngineLayout,
  count: number,
  width: number,
  height: number,
  cell: CellSize,
  gap: number,
): GridPlan {
  const n = Math.max(1, count);
  if (!(width > 0) || n === 1) return place({ columns: 1, rows: 1, first: { columns: 1, rows: 1 } }, n);
  if (layout === "rows") return place({ columns: 1, rows: n, first: { columns: 1, rows: 1 } }, n);
  const presets = layout === "focus" ? FOCUS[n] : undefined;
  if (presets) {
    let best: { shape: Shape; size: number } | null = null;
    for (const preset of presets) {
      const wide = shapeOf(preset);
      const options = [wide, turned(wide)].map((shape) => ({
        shape,
        ...unit(shape, width, height, cell, gap),
      }));
      const fitting = options.filter((o) => o.fits).sort((a, b) => b.size - a.size)[0];
      if (fitting) return place(fitting.shape, n);
      for (const o of options) if (!best || o.size > best.size) best = o;
    }
    if (best) return place(best.shape, n);
  }
  let best: { shape: Shape; score: number } | null = null;
  for (const { shape, spare } of gridShapes(n)) {
    const score = unit(shape, width, height, cell, gap).size * (spare ? SPARE_WEIGHT : 1);
    if (!best || score > best.score) best = { shape, score };
  }
  return place(best?.shape ?? { columns: 1, rows: n, first: { columns: 1, rows: 1 } }, n);
}

export const samePlan = (a: GridPlan, b: GridPlan) =>
  a.columns === b.columns &&
  a.rows === b.rows &&
  a.stack === b.stack &&
  a.cells.length === b.cells.length &&
  a.cells.every(
    (c, i) =>
      c.column === b.cells[i].column &&
      c.columns === b.cells[i].columns &&
      c.row === b.cells[i].row &&
      c.rows === b.cells[i].rows,
  );

/** No chart's area overlaps another's, and together they cover the grid: for tests. */
export function fills(plan: GridPlan): boolean {
  const seen = new Set<string>();
  for (const c of plan.cells) {
    for (let x = c.column; x < c.column + c.columns; x++) {
      for (let y = c.row; y < c.row + c.rows; y++) {
        const key = `${x},${y}`;
        if (seen.has(key) || x > plan.columns || y > plan.rows) return false;
        seen.add(key);
      }
    }
  }
  return seen.size === plan.columns * plan.rows;
}

/** Every chart but the first one unit: for tests. */
export const evenRest = (plan: GridPlan) => plan.cells.slice(1).every((c) => c.columns === 1 && c.rows === 1);
