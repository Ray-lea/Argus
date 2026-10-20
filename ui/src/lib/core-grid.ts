/** Available grid space and the minimum dimensions of a processor card. */
export interface CoreGridSize {
  count: number;
  width: number;
  height: number;
  cellWidth: number;
  cellHeight: number;
  headerHeight: number;
  gap: number;
}

/** Prefer readable plots and full rows; keep a near-equal layout while resizing. */
export function coreGridColumns(size: CoreGridSize, previous = 0): number {
  if (size.count <= 1 || !(size.width > 0)) return 1;
  const limit = Math.min(
    size.count,
    Math.max(1, Math.floor((size.width + size.gap) / (size.cellWidth + size.gap))),
  );
  const score = (columns: number) => {
    const rows = Math.ceil(size.count / columns);
    const width = (size.width - (columns - 1) * size.gap) / columns;
    const height = (size.height - (rows - 1) * size.gap) / rows;
    if (height < size.cellHeight - 0.5) return 0;
    const plot = Math.min(width / 1.8, height - size.headerHeight);
    const empty = 1 - size.count / (rows * columns);
    return plot * (1 - empty * 0.15);
  };
  let best = limit;
  let bestScore = 0;
  for (let columns = 1; columns <= limit; columns++) {
    const value = score(columns);
    if (value > bestScore) {
      best = columns;
      bestScore = value;
    }
  }
  if (previous > 0 && previous <= limit) {
    const currentScore = score(previous);
    // A small dead band prevents one-pixel drags from repeatedly moving cores.
    if (currentScore > 0 && bestScore <= currentScore * 1.08) return previous;
  }
  // When no arrangement fits vertically, minimize the height and allow scrolling.
  return best;
}
