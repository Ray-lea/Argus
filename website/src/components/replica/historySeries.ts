import { sampleAt } from "./model";
import { asleep, type Span } from "./historyTime";
import type { Metric } from "./metrics";

// history/series.rs uses whole, origin-aligned buckets shared by the lanes and the navigator.
const buckets = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200, 10800, 21600];
export function sampleTimes([from, to]: Span, width: number) {
  const target = (to - from) / Math.max(60, width / 2.6);
  const step = buckets.find((v) => v >= target) ?? 21600;
  const first = Math.floor(from / step) * step - step,
    last = Math.ceil(to / step) * step + 2 * step;
  return Array.from({ length: Math.round((last - first) / step) + 1 }, (_, i) => first + i * step);
}
export function niceTop(value: number, percent: boolean) {
  const top = value > 0 ? value : percent ? 10 : 1;
  if (percent) return [10, 20, 25, 40, 50, 75, 100].find((v) => v >= top * 1.06) ?? 100;
  const scale = 10 ** Math.floor(Math.log10(top));
  return [1, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10].map((v) => v * scale).find((v) => v >= top * 1.06) ?? 10 * scale;
}
export type Trace = {
  dash?: boolean;
  values: number[];
  min: number;
  max: number;
  color: string;
  fill: number;
  opacity: number;
  low?: number[];
  high?: number[];
};
export function readingTraces(metric: Metric, times: number[], band: boolean, overview = false): Trace[] {
  const step = times[1] - times[0];
  const getters = metric.second && !overview ? [metric.read, metric.second] : [metric.read];
  const parts = getters.map((get) => {
    const low: number[] = [],
      high: number[] = [];
    const values = times.map((t) => {
      if (asleep(t)) {
        low.push(NaN);
        high.push(NaN);
        return NaN;
      }
      const n = Math.min(12, step);
      let sum = 0,
        min = Infinity,
        max = -Infinity;
      for (let i = 0; i < n; i++) {
        const value = get(sampleAt(t + Math.floor((i * step) / n)));
        sum += value;
        min = Math.min(min, value);
        max = Math.max(max, value);
      }
      low.push(min);
      high.push(max);
      return sum / n;
    });
    return { values, low, high };
  });
  const envelope = parts.flatMap((p) => (band && !overview ? p.high : p.values)).filter(Number.isFinite);
  const peak = Math.max(0, ...envelope);
  const min =
    metric.unit === "°C" && !overview && envelope.length
      ? Math.max(0, Math.floor((Math.min(...envelope) - 3) / 10) * 10)
      : 0;
  const max =
    metric.unit === "°C" ? Math.max(min + 10, Math.ceil((peak + 3) / 10) * 10) : niceTop(peak, metric.unit === "%");
  return parts.map((p, i) => ({
    ...p,
    min,
    max,
    color: metric.color,
    opacity: overview ? 0.75 : i ? 0.5 : 1,
    fill: i ? 0 : overview ? 0.35 : 0.23,
    low: band && step >= 10 && !metric.second && !overview ? p.low : undefined,
    high: band && step >= 10 && !metric.second && !overview ? p.high : undefined,
  }));
}
