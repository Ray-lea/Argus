// The GPU view's engine charts as the app arranges them
// (pages/performance/engines.rs): which engines are charted, and where each
// chart goes in the grid.

/** What the visitor chose: busy engines (smart), or so many in an order. */
export interface EngineChoice {
  smart: boolean;
  /** How many charts; none for one per engine. */
  count: number | null;
  /** The engines in the order arranged. */
  order: string[];
}
/** Every engine in display order: the arranged ones first, then the rest as the GPU lists them. */
function arrange(engines: string[], choice: EngineChoice | null) {
  const first: string[] = [];
  for (const engine of choice?.order ?? []) if (engines.includes(engine) && !first.includes(engine)) first.push(engine);
  return [...first, ...engines.filter((e) => !first.includes(e))];
}
const chartCount = (engines: string[], choice: EngineChoice | null) =>
  choice?.count != null ? Math.min(Math.max(choice.count, 1), Math.max(engines.length, 1)) : engines.length;
export const isSmart = (choice: EngineChoice | null) => !choice || choice.smart;
/** The engines charted, in order: under smart display the busy ones, or the first engine. */
export function shownEngines(engines: string[], choice: EngineChoice | null, busy: (engine: string) => boolean) {
  const order = arrange(engines, choice);
  if (isSmart(choice)) {
    const active = order.filter(busy);
    return active.length ? active : order.slice(0, 1);
  }
  return order.slice(0, chartCount(engines, choice));
}
export const withSmart = (engines: string[], choice: EngineChoice | null): EngineChoice => ({
  smart: true,
  count: choice?.count ?? null,
  order: arrange(engines, choice),
});
/** What smart display shows, as a choice of that many charts: a manual change starts from the screen. */
function fromScreen(engines: string[], choice: EngineChoice | null, shown: string[]): EngineChoice | null {
  if (!isSmart(choice)) return choice;
  const first = shown.filter((e) => engines.includes(e));
  const rest = arrange(engines, choice).filter((e) => !first.includes(e));
  return { smart: false, count: first.length !== engines.length ? first.length : null, order: [...first, ...rest] };
}
export function withCount(engines: string[], choice: EngineChoice | null, count: number, shown: string[]): EngineChoice {
  const all = Math.max(engines.length, 1);
  const n = Math.min(Math.max(count, 1), all);
  return { smart: false, count: n !== all ? n : null, order: arrange(engines, fromScreen(engines, choice, shown)) };
}
/** Charts `engine` in place `slot`: one charted elsewhere trades places; one not
 * charted takes the place, and the one it replaces moves just past the charted ones. */
export function withEngine(engines: string[], current: EngineChoice | null, slot: number, engine: string, onScreen: string[]): EngineChoice {
  const choice = fromScreen(engines, current, onScreen);
  const order = arrange(engines, choice);
  const count = choice?.count ?? null;
  const from = order.indexOf(engine);
  if (from < 0 || slot >= order.length || from === slot) return { smart: false, count, order };
  const shown = chartCount(engines, choice);
  if (from < shown) [order[slot], order[from]] = [order[from], order[slot]];
  else {
    const replaced = order[slot];
    order[slot] = engine;
    const duplicate = order.findIndex((e, i) => i > slot && e === engine);
    if (duplicate >= 0) order.splice(duplicate, 1);
    order.splice(Math.min(shown, order.length), 0, replaced);
  }
  return { smart: false, count, order };
}

/** Where a chart goes: its first track and how many it spans (grid lines from 1). */
export interface Placement {
  column: number;
  columns: number;
  row: number;
  rows: number;
}
export interface Plan {
  columns: number;
  rows: number;
  cells: Placement[];
  /** Rows of charts stacked, for the grid's least height. */
  stack: number;
}
interface Shape {
  columns: number;
  rows: number;
  first: [number, number];
}
/** Width to height a small chart reads best at. */
const CELL_ASPECT = 1.5;
/** How much a grid that gives the first chart the spare units counts against one that needs none. */
const SPARE_WEIGHT = 0.8;
const SPARE_MAX = 3;
/** Focus arrangements per count: columns, rows, the first chart's columns and rows, for a wide area. */
const FOCUS: number[][][] = [
  [[1, 1, 1, 1]],
  [[2, 1, 1, 1]],
  [[3, 2, 2, 2]],
  [[4, 3, 3, 3]],
  [[4, 2, 2, 2]],
  [[3, 3, 2, 2]],
  [[4, 3, 3, 2], [5, 2, 2, 2]],
  [[4, 4, 3, 3], [5, 3, 4, 2]],
  [[5, 4, 4, 3], [4, 3, 2, 2], [6, 2, 2, 2]],
  [[5, 5, 4, 4], [6, 3, 3, 3], [5, 3, 3, 2]],
  [[6, 5, 5, 4], [4, 4, 3, 2], [7, 2, 2, 2]],
  [[5, 4, 3, 3], [5, 3, 2, 2]],
  [[4, 4, 2, 2], [7, 3, 3, 3]],
  [[5, 5, 4, 3], [7, 3, 4, 2]],
  [[6, 5, 4, 4], [5, 4, 3, 2], [6, 3, 2, 2]],
  [[6, 4, 3, 3], [7, 3, 3, 2]],
];
const turned = (s: Shape): Shape => ({ columns: s.rows, rows: s.columns, first: [s.first[1], s.first[0]] });
/** The first chart at the top left, every other one unit, row by row. */
function place(shape: Shape, count: number): Plan {
  const cells: Placement[] = [{ column: 1, columns: shape.first[0], row: 1, rows: shape.first[1] }];
  for (let row = 1; row <= shape.rows; row++)
    for (let column = 1; column <= shape.columns; column++) {
      if (row <= shape.first[1] && column <= shape.first[0]) continue;
      if (cells.length < count) cells.push({ column, columns: 1, row, rows: 1 });
    }
  return { columns: shape.columns, rows: shape.rows, cells, stack: shape.rows };
}
/** One unit's readable size in the area, and whether it is as large as a cell may be. */
function unit(shape: Shape, width: number, height: number, cell: { width: number; height: number }, gap: number): [boolean, number] {
  const w = (width - (shape.columns - 1) * gap) / shape.columns;
  const h = (height - (shape.rows - 1) * gap) / shape.rows;
  const fits = w >= cell.width - 0.5 && h >= cell.height - 0.5;
  // Below the minimum the grid grows and the view scrolls: counted at half.
  return [fits, Math.min(w / CELL_ASPECT, h) * (fits ? 1 : 0.5)];
}
function gridShapes(count: number): [Shape, number][] {
  const shapes: [Shape, number][] = [];
  for (let rows = 1; rows <= count; rows++) {
    const columns = Math.ceil(count / rows);
    const spare = columns * rows - count;
    if (spare > SPARE_MAX || (rows > 1 && Math.ceil(count / (rows - 1)) === columns)) continue;
    const units = spare + 1;
    let first: [number, number] | null = null;
    for (let w = units; w >= 1; w--) {
      if (units % w) continue;
      const h = units / w;
      if (w > columns || h > rows || h > w) continue;
      if (!first || Math.abs(w - h) < Math.abs(first[0] - first[1])) first = [w, h];
    }
    if (first) shapes.push([{ columns, rows, first }, spare]);
  }
  return shapes;
}
export function planGrid(layout: string, count: number, width: number, height: number, cell: { width: number; height: number }, gap: number): Plan {
  const n = Math.max(count, 1);
  if (width <= 0 || n === 1) return place({ columns: 1, rows: 1, first: [1, 1] }, n);
  if (layout === "rows") return place({ columns: 1, rows: n, first: [1, 1] }, n);
  const presets = layout === "focus" ? FOCUS[n - 1] : undefined;
  if (presets) {
    let best: [Shape, number] | null = null;
    for (const [columns, rows, fw, fh] of presets) {
      const wide: Shape = { columns, rows, first: [fw, fh] };
      const options = [wide, turned(wide)].map((shape) => [shape, unit(shape, width, height, cell, gap)] as const);
      const fitting = options.filter(([, [fits]]) => fits).sort((a, b) => b[1][1] - a[1][1]);
      if (fitting.length) return place(fitting[0][0], n);
      for (const [shape, [, size]] of options) if (!best || size > best[1]) best = [shape, size];
    }
    if (best) return place(best[0], n);
  }
  let best: [Shape, number] | null = null;
  for (const [shape, spare] of gridShapes(n)) {
    const score = unit(shape, width, height, cell, gap)[1] * (spare > 0 ? SPARE_WEIGHT : 1);
    if (!best || score > best[1]) best = [shape, score];
  }
  return place(best?.[0] ?? { columns: 1, rows: n, first: [1, 1] }, n);
}
