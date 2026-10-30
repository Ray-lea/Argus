import assert from "node:assert/strict";

export function physicalPixelsMatch(logical, scale, physical) {
  return [logical, scale, physical].every((value) => Number.isFinite(value) && value > 0) &&
    Math.abs(logical * scale - physical) < 0.01;
}

// Receipt batches may straddle a measurement mark. Only the compositor's own
// timestamps determine membership; end markers also drain the final partial batch.
export function summarizeFrames(events, begin100ns, end100ns, captured = true) {
  assert(Number.isSafeInteger(begin100ns) && Number.isSafeInteger(end100ns) && end100ns > begin100ns,
    "probe must provide a valid QPC measurement interval");
  const frames = [...new Set(events.flatMap((event) => event.capture?.times100ns ?? []))]
    .filter((at) => at >= begin100ns && at <= end100ns)
    .sort((a, b) => a - b);
  const gaps = frames.slice(1).map((at, index) => (at - frames[index]) / 10000).sort((a, b) => a - b);
  const quantile = (q) => gaps[Math.floor((gaps.length - 1) * q)] ?? 0;
  const median = quantile(0.5);
  const nearMedian = gaps.filter((gap) => Math.abs(gap - median) <= median * 0.1);
  const cadence = nearMedian.length ? nearMedian.reduce((total, gap) => total + gap, 0) / nearMedian.length : 0;
  const frameWindowMs = frames.length > 1 ? (frames.at(-1) - frames[0]) / 10000 : 0;
  const summary = {
    observedFrames: frames.length,
    frameWindowMs,
    presentedFps: frameWindowMs > 0 ? (frames.length - 1) * 1000 / frameWindowMs : 0,
    cadenceMs: cadence,
    frameP50Ms: median,
    frameP95Ms: quantile(0.95),
    frameP99Ms: quantile(0.99),
    frameMaxMs: gaps.at(-1) ?? 0,
    missedRefreshes: cadence > 0 ? gaps.reduce((total, gap) => total + Math.max(0, Math.round(gap / cadence) - 1), 0) : 0,
    longFrames50Ms: gaps.filter((gap) => gap > 50).length,
  };
  return captured ? summary : Object.fromEntries(Object.keys(summary).map((key) => [key, null]));
}
