import { asleep, type Span } from "./historyTime";
import { formatMetric, thousands, type Metric } from "./metrics";

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
  const top = value > 0 && Number.isFinite(value) ? value : percent ? 10 : 1;
  if (percent) return [10, 20, 25, 40, 50, 75, 100].find((v) => top * 1.06 <= v) ?? 100;
  const scale = 10 ** Math.floor(Math.log10(top));
  return [1, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10].map((v) => v * scale).find((v) => top * 1.06 <= v) ?? 10 * scale;
}
const KB = 1024,
  MB = KB * 1024,
  GB = MB * 1024;
// The size of the unit an axis value is labelled in (readings.rs `label_unit`).
function labelUnit(m: Metric, v: number) {
  if (m.unit === "rate") return v >= GB ? GB : v >= MB ? MB : KB;
  if (m.unit === "bytes") return v >= GB ? GB : MB;
  return 1;
}
const roundTop = (m: Metric, v: number) => {
  const unit = labelUnit(m, v);
  return niceTop(v / unit, false) * unit;
};
// What an axis spans for the values shown (readings.rs `axis_range`).
export function axisRange(m: Metric, high: number, low: number): [number, number] {
  high = Number.isFinite(high) ? high : 0;
  switch (m.axis) {
    case "percent":
      return [0, Math.max(10, niceTop(high, true))];
    case "total":
      return [0, m.total ?? roundTop(m, high)];
    case "temp": {
      const top = Math.min(110, Math.max(60, Math.ceil((Math.max(high, 50) + 4) / 10) * 10));
      return [Math.max(0, Math.min(30, Number.isFinite(low) ? low : 30)), top];
    }
    case "signed": {
      const far = Math.max(Math.abs(high), Math.abs(Number.isFinite(low) ? low : 0), 1);
      const top = roundTop(m, far);
      return [-top, top];
    }
    case "count": {
      const base = Math.floor((Number.isFinite(low) ? low : 0) / 50) * 50;
      return [base, base + niceTop(Math.max(high - base, 50), false)];
    }
    case "fixed":
      return m.fixed ?? [0, 100];
    default:
      return [0, roundTop(m, high)];
  }
}
// How many rows divide an axis into round steps: 25 in five, 40 in four, 75 in three (series.rs `rows_for`).
export function rowsFor(m: Metric, [low, high]: Span, height: number) {
  const scaled = Math.round(((high - low) / labelUnit(m, Math.max(Math.abs(high), Math.abs(low)))) * 1000) / 1000;
  const whole = (step: number) => Math.abs(step - Math.round(step)) < 1e-6 || (step < 1 && Math.abs(step * 10 - Math.round(step * 10)) < 1e-6);
  if (height < 50) return 1;
  if (height < 100) return whole(scaled / 2) ? 2 : 1;
  return [4, 5, 3].find((rows) => whole(scaled / rows)) ?? 4;
}
// An axis label: as short as the axis allows (readings.rs `axis_label`).
export function axisLabel(m: Metric, value: number, top: number) {
  const unit = labelUnit(m, Math.max(Math.abs(top), Math.abs(value)));
  const scaled = value / unit;
  const n = Math.abs(scaled - Math.round(scaled)) < 0.05 ? String(Math.round(scaled)) : scaled.toFixed(1);
  switch (m.unit) {
    case "%":
      return n + "%";
    case "rate":
      return value === 0 ? "0" : n + (unit >= GB ? " GB/s" : unit >= MB ? " MB/s" : " KB/s");
    case "bytes":
      return value === 0 ? "0" : n + (unit >= GB ? " GB" : " MB");
    case "°C":
      return n + "°C";
    case "GHz":
      return n + " GHz";
    case "W":
    case "±W":
      return n + " W";
    case "V":
      return value.toFixed(2) + " V";
    case "RPM":
      return n;
    case "ms":
      return n + " ms";
    case "":
      return thousands(value);
    default:
      return formatMetric(m.unit, value);
  }
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
// A reading's lines over `times`: each point the mean of its bucket, with the
// lowest and highest samples for the band; on the axis the app gives it, or
// for the navigator's overview, from zero.
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
        const value = get(t + Math.floor((i * step) / n));
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
  const banded = band && step >= 10 && !metric.second && !overview;
  const finite = (list: number[]) => list.filter(Number.isFinite);
  const tops = finite(parts.flatMap((p) => (banded ? p.high : p.values))),
    bottoms = finite(parts.flatMap((p) => (banded ? p.low : p.values)));
  let [min, max] = axisRange(metric, tops.length ? Math.max(...tops) : 0, bottoms.length ? Math.min(...bottoms) : NaN);
  if (overview) {
    min = Math.min(0, min);
    if (metric.axis === "percent") max = Math.max(10, niceTop(tops.length ? Math.max(...tops) : 0, true));
  }
  return parts.map((p, i) => ({
    ...p,
    min,
    max,
    color: metric.color,
    opacity: overview ? 0.75 : i ? 0.5 : 1,
    fill: i ? 0 : overview ? 0.35 : 0.22,
    low: banded ? p.low : undefined,
    high: banded ? p.high : undefined,
  }));
}
