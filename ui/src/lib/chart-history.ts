/** Nominal samples spanning a live chart's width. */
export const HISTORY = 60;

export const chartSpan = (interval: number): number => (HISTORY - 1) * interval;
export const chartLag = (interval: number): number => 2 * interval + 250;

/**
 * Keep the visible time window, not a fixed number of samples: refreshes on
 * demand and a faster background cadence can put extra samples in that window.
 * Three intervals cover the canvas's pixel alignment and redraw slack; two
 * earlier points preserve the spline's slope at the retained boundary.
 */
export function appendSampleTime(times: number[], now: number, interval: number): number {
  times.push(now);
  const before = now - chartSpan(interval) - chartLag(interval) - 3 * interval;
  let drop = 0;
  while (drop + 2 < times.length && times[drop + 2] < before) drop++;
  if (drop > 0) times.splice(0, drop);
  return times.length;
}

/** All series share the retained timestamps, including ones that started later. */
export function appendHistory(list: number[], value: number, keep: number): void {
  const drop = list.length + 1 - keep;
  if (drop > 0) list.splice(0, drop);
  list.push(value);
}
