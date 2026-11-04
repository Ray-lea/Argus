// The curve's shape, shared by the chart (Curve.tsx) and the worker that draws
// it (curve.worker.ts): monotone cubic interpolation (Fritsch–Carlson), as the
// app joins adjacent readings.

export function tangents(ys: number[]) {
  const n = ys.length;
  const d = ys.slice(1).map((y, i) => y - ys[i]);
  const m = ys.map((_, i) =>
    i === 0
      ? d[0]
      : i === n - 1
        ? d[n - 2]
        : d[i - 1] * d[i] <= 0
          ? 0
          : (d[i - 1] + d[i]) / 2,
  );
  for (let i = 0; i < n - 1; i++) {
    if (d[i] === 0) {
      m[i] = m[i + 1] = 0;
      continue;
    }
    const a = m[i] / d[i],
      b = m[i + 1] / d[i],
      s = a * a + b * b;
    if (s > 9) {
      const t = 3 / Math.sqrt(s);
      m[i] = t * a * d[i];
      m[i + 1] = t * b * d[i];
    }
  }
  return m;
}

export function pathOf(ys: number[], m: number[]) {
  const f = (v: number) => +v.toFixed(2);
  let path = `M0 ${f(ys[0])}`;
  for (let i = 0; i < ys.length - 1; i++)
    path += `C${f(i + 1 / 3)} ${f(ys[i] + m[i] / 3)} ${f(i + 2 / 3)} ${f(ys[i + 1] - m[i + 1] / 3)} ${i + 1} ${f(ys[i + 1])}`;
  return path;
}

// The curve's height at x: its control points sit at thirds, so x is the Bézier parameter.
export function heightAt(ys: number[], m: number[], x: number) {
  const i = Math.max(0, Math.min(ys.length - 2, Math.floor(x)));
  const t = Math.max(0, Math.min(1, x - i)),
    u = 1 - t;
  const a = ys[i],
    b = ys[i] + m[i] / 3,
    c = ys[i + 1] - m[i + 1] / 3,
    d = ys[i + 1];
  return u * u * u * a + 3 * u * u * t * b + 3 * u * t * t * c + t * t * t * d;
}

// The drawn length of the first `count` segments, at sx and sy pixels per unit.
export function lengthTo(ys: number[], m: number[], count: number, sx: number, sy: number) {
  let length = 0;
  for (let i = 0; i < Math.min(count, ys.length - 1); i++) {
    let px = i * sx,
      py = ys[i] * sy;
    for (let k = 1; k <= 24; k++) {
      const t = k / 24;
      const x = (i + t) * sx,
        y = heightAt(ys, m, i + t) * sy;
      length += Math.hypot(x - px, y - py);
      px = x;
      py = y;
    }
  }
  return length;
}

/** Samples drawn beyond each edge but never shown (see Curve.tsx). */
export const MARGIN = 2;
/**
 * How far past the next sample a curve scrolls on while that sample is late,
 * in samples. It runs into the hidden margin; past one sample the shape there
 * may still change by more than a fraction of a pixel when the sample comes.
 */
export const OVERRUN = 1;
/** The dash pattern of a dashed series (4 on, 3 off), in CSS pixels. */
export const DASH = [4, 3] as const;

/** One chart as the worker draws it. Times are epoch milliseconds. */
export interface Frame {
  series: { ys: number[]; color: number[]; fill: boolean; faint: boolean; dashed: boolean }[];
  /** Data units: `drawn` samples wide, 100 high. */
  drawn: number;
  shown: number;
  margin: number;
  interval: number;
  /** When the latest sample's scroll began; null while the curve rests at the end of its step. */
  at: number | null;
  tick: number;
  /** Colour of the readout dots' ring. */
  ring: number[];
}
/** Where the chart's box lies on the canvas, in device pixels. */
export interface Geometry {
  width: number;
  height: number;
  /** The box, from the canvas's top left: it may start at a fraction of a pixel. */
  left: number;
  top: number;
  boxWidth: number;
  boxHeight: number;
  /** The clip: the box's own edges. */
  clipLeft: number;
  clipRight: number;
  /** Device pixels per CSS pixel of the demo (its scale times the display's). */
  scale: number;
}
export type Message =
  | { kind: "add"; id: number; canvas: OffscreenCanvas }
  | { kind: "geometry"; id: number; geometry: Geometry }
  | { kind: "frame"; id: number; frame: Frame }
  | { kind: "pointer"; id: number; at: number | null }
  | { kind: "visible"; id: number; visible: boolean }
  | { kind: "remove"; id: number };
