// Where the history charts leave gaps: Argus was not running there, as
// opposed to a slower background refresh interval that only spaces frames out.

/**
 * The widest spacing of frames Argus records while running: the slowest
 * minimized or tray refresh interval (30 s) plus how late a sample can land
 * (`MAX_INTERVAL_MS` in disk_process.rs makes the same allowance).
 */
export const MAX_CADENCE = 35_000;

/**
 * Whether each pair of neighbouring points is joined by the line, one entry
 * per pair (`joined[i]` for `ts[i]` and `ts[i + 1]`). Points up to 2.5 steps
 * apart always are. Past that, the spacing around the pair counts: the wider
 * of the neighbouring spacings, up to `MAX_CADENCE`, so a stretch recorded
 * every 10 or 30 s reads as a sparser line, while a longer stop of Argus
 * still leaves a gap.
 */
export function joins(ts: readonly number[], step: number): boolean[] {
  const joined: boolean[] = [];
  for (let i = 0; i + 1 < ts.length; i++) {
    const before = i > 0 ? ts[i] - ts[i - 1] : 0;
    const after = i + 2 < ts.length ? ts[i + 2] - ts[i + 1] : 0;
    const cadence = Math.min(MAX_CADENCE, Math.max(before, after));
    joined.push(ts[i + 1] - ts[i] <= Math.max(step, cadence) * 2.5);
  }
  return joined;
}

/** Index of the point closest to `at` (points in time order); -1 when there are none. */
export function nearest(ts: readonly number[], at: number): number {
  if (ts.length === 0) return -1;
  let lo = 0;
  let hi = ts.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (ts[mid] < at) lo = mid + 1;
    else hi = mid;
  }
  return lo > 0 && at - ts[lo - 1] < ts[lo] - at ? lo - 1 : lo;
}

/**
 * The point a reading at `at` shows: the nearest one when `at` lies on the
 * line between two joined points or within 1.5 steps of a point, none in a gap.
 */
export function pointAt(
  ts: readonly number[],
  joined: readonly boolean[],
  step: number,
  at: number,
): number | undefined {
  const i = nearest(ts, at);
  if (i < 0) return undefined;
  if (Math.abs(ts[i] - at) <= step * 1.5) return i;
  // The pair `at` falls between.
  const pair = at < ts[i] ? i - 1 : i;
  return joined[pair] ? i : undefined;
}
