// Live charts on <canvas>, scrolling continuously.
//
// Points sit on a time axis: the right edge shows "now minus two intervals",
// so the newest sample is always already drawn just past the edge and the
// trace drifts left at a constant speed, without stopping between samples.
// Each sample is drawn once into a canvas a little wider than the box; then
// the canvas moves left until the next sample:
//  - fluid / balanced: one linear compositor animation per chart, so moving
//    costs no main-thread work, only frames (every refresh, or ~60 a second);
//  - pixel: one shared timer moves every chart a whole device pixel at a time.
// A hover readout's cursor stays under the pointer while the curve flows past:
// its dots ride the curve (on the compositor too, while the canvas glides) and
// its text follows ten times a second.

import {
  createEffect,
  createMemo,
  createRoot,
  createSignal,
  Index,
  onCleanup,
  onMount,
  Show,
  untrack,
} from "solid-js";
import { t } from "../lib/i18n";
import { balancedFrameMs, type ChartMode, chartMode } from "../lib/motion";
import { HISTORY, intervalMs, paused, sampleSeq, sampleTimes } from "../lib/store";

export interface Series {
  data: () => readonly number[];
  /** CSS custom property holding the color, e.g. `--cpu`. */
  colorVar: string;
  /** Area fill opacity at the top of the gradient; 0 for a bare line. */
  fill?: number;
  dashed?: boolean;
  /** Name in the hover readout. */
  label?: string;
}

interface ChartProps {
  series: Series[];
  max: () => number;
  class?: string;
  /** Formats values for the hover readout; charts without it are decorative. */
  format?: (v: number) => string;
}

/** Headroom (CSS px) so a line at full scale is not clipped. */
const TOP_PAD = 3;
/** How often a hover tip's text follows a trace gliding under the cursor, ms. */
const TEXT_EVERY = 100;

/**
 * How far behind "now" the right edge runs. The spline's bend at a point
 * depends on the point after it, so a segment only takes its final shape once
 * the sample after its end has arrived. Two intervals (plus delivery slack)
 * keep the edge behind every segment that a new sample could still reshape;
 * with one, the newest visible stretch visibly re-bent on every sample.
 */
const lag = () => 2 * intervalMs() + 250;

/** The mode in effect right now: paused charts hold still with the newest sample at the edge. */
const activeMode = (): ChartMode => (paused() ? "off" : chartMode());

// ---- Shared timer (pixel mode, and the readouts on its charts) ---------------------

interface Ticked {
  /** Milliseconds until this chart next needs a tick; Infinity for never. */
  next(now: number): number;
  /** Device pixels per millisecond, for charts moving in pixel mode (else 0). */
  pixelSpeed(): number;
  tick(now: number): void;
}

const ticked = new Set<Ticked>();
let timer: ReturnType<typeof setTimeout> | undefined;
let wakeQueued = false;

function wake() {
  clearTimeout(timer);
  timer = undefined;
  if (ticked.size === 0 || document.visibilityState !== "visible") return;
  const now = performance.now();
  // Pixel-mode charts move on the fastest one's ticks, so slower ones share its frames.
  let lead: Ticked | null = null;
  let wait = Number.POSITIVE_INFINITY;
  for (const c of ticked) {
    c.tick(now);
    if (c.pixelSpeed() > (lead?.pixelSpeed() ?? 0)) lead = c;
  }
  for (const c of ticked) {
    if (c.pixelSpeed() === 0 || c === lead) wait = Math.min(wait, c.next(now));
  }
  if (Number.isFinite(wait)) timer = setTimeout(wake, Math.min(500, Math.max(8, wait)));
}

function requestWake() {
  if (wakeQueued) return;
  wakeQueued = true;
  queueMicrotask(() => {
    wakeQueued = false;
    wake();
  });
}

createRoot(() => {
  createEffect(() => {
    activeMode();
    requestWake();
  });
});
document.addEventListener("visibilitychange", requestWake);

// ---- Drawing ----------------------------------------------------------------------

export function withAlpha(hex: string, alpha: number): string {
  const h = hex.replace("#", "");
  const full = h.length === 3 ? [...h].map((c) => c + c).join("") : h;
  const n = Number.parseInt(full, 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}

interface Curve {
  path: Path2D;
  /** Length of each segment, measured on the same flattening `dashes` cuts. */
  lengths(): number[];
  /** The curve cut into dashes, starting `start` px into an on/off pattern. */
  dashes(start: number, on: number, period: number): Path2D;
}

/**
 * Slopes of the monotone cubic through points with uneven spacing (samples
 * taken while the window was hidden are further apart): smooth, and never
 * overshoots the data.
 */
function slopes(xs: number[], ys: number[]): Float64Array {
  const n = xs.length;
  const m = new Float64Array(n);
  if (n >= 3) {
    const hs = new Float64Array(n - 1);
    const d = new Float64Array(n - 1);
    for (let i = 0; i < n - 1; i++) {
      hs[i] = Math.max(1e-6, xs[i + 1] - xs[i]);
      d[i] = (ys[i + 1] - ys[i]) / hs[i];
    }
    m[0] = d[0];
    m[n - 1] = d[n - 2];
    for (let i = 1; i < n - 1; i++) {
      if (d[i - 1] * d[i] <= 0) continue;
      // Fritsch–Butland: weighted harmonic mean of the neighbouring slopes.
      const w1 = 2 * hs[i] + hs[i - 1];
      const w2 = hs[i] + 2 * hs[i - 1];
      m[i] = (w1 + w2) / (w1 / d[i - 1] + w2 / d[i]);
    }
  } else if (n === 2) {
    m[0] = m[1] = (ys[1] - ys[0]) / Math.max(1e-6, xs[1] - xs[0]);
  }
  return m;
}

/**
 * The same cubic as a function: y at `x`, or null outside the points. The
 * control points sit at thirds of each segment, so x runs linearly with the
 * Bezier parameter and the curve read here is exactly the one drawn.
 */
function interpolate(xs: number[], ys: number[], m: Float64Array) {
  return (x: number): number | null => {
    const n = xs.length;
    if (n === 0 || x < xs[0] || x > xs[n - 1]) return null;
    let lo = 0;
    let hi = n - 1;
    while (hi - lo > 1) {
      const mid = (lo + hi) >> 1;
      if (xs[mid] <= x) lo = mid;
      else hi = mid;
    }
    if (hi === lo) return ys[lo];
    const k = (xs[hi] - xs[lo]) / 3;
    const t = k > 0 ? (x - xs[lo]) / (3 * k) : 0;
    const u = 1 - t;
    const y1 = ys[lo] + m[lo] * k;
    const y2 = ys[hi] - m[hi] * k;
    return u * u * u * ys[lo] + 3 * u * u * t * y1 + 3 * u * t * t * y2 + t * t * t * ys[hi];
  };
}

/** The monotone cubic through the points (see `slopes`) as a path. */
function monotone(xs: number[], ys: number[], m = slopes(xs, ys)): Curve {
  const n = xs.length;
  const path = new Path2D();
  path.moveTo(xs[0], ys[0]);
  // Bezier control points of segment i: (x_i + k, y_i + m_i k), (x_i+1 - k, y_i+1 - m_i+1 k).
  const control = (i: number) => {
    const k = (xs[i + 1] - xs[i]) / 3;
    return [xs[i] + k, ys[i] + m[i] * k, xs[i + 1] - k, ys[i + 1] - m[i + 1] * k] as const;
  };
  for (let i = 0; i < n - 1; i++) {
    const [x1, y1, x2, y2] = control(i);
    path.bezierCurveTo(x1, y1, x2, y2, xs[i + 1], ys[i + 1]);
  }
  // Segments flattened to pieces of about a pixel. Dashes are cut from this
  // polyline rather than by the canvas, whose own length estimate differs by a
  // fraction of a pixel: phases measured here then repeat exactly on redraw.
  const flats: number[][] = [];
  const flat = (i: number) => {
    let pts = flats[i];
    if (pts) return pts;
    const [x1, y1, x2, y2] = control(i);
    const x0 = xs[i];
    const y0 = ys[i];
    const x3 = xs[i + 1];
    const y3 = ys[i + 1];
    const hull = Math.hypot(x1 - x0, y1 - y0) + Math.hypot(x2 - x1, y2 - y1) + Math.hypot(x3 - x2, y3 - y2);
    const pieces = Math.min(96, Math.max(4, Math.ceil(hull)));
    pts = [x0, y0];
    for (let k = 1; k <= pieces; k++) {
      const t = k / pieces;
      const u = 1 - t;
      pts.push(
        u * u * u * x0 + 3 * u * u * t * x1 + 3 * u * t * t * x2 + t * t * t * x3,
        u * u * u * y0 + 3 * u * u * t * y1 + 3 * u * t * t * y2 + t * t * t * y3,
      );
    }
    flats[i] = pts;
    return pts;
  };
  const lengths = () => {
    const out: number[] = [];
    for (let i = 0; i < n - 1; i++) {
      const pts = flat(i);
      let len = 0;
      for (let j = 2; j < pts.length; j += 2) len += Math.hypot(pts[j] - pts[j - 2], pts[j + 1] - pts[j - 1]);
      out.push(len);
    }
    return out;
  };
  const dashes = (start: number, on: number, period: number) => {
    const out = new Path2D();
    let phase = ((start % period) + period) % period;
    let down = false;
    for (let i = 0; i < n - 1; i++) {
      const pts = flat(i);
      for (let j = 2; j < pts.length; j += 2) {
        let ax = pts[j - 2];
        let ay = pts[j - 1];
        const bx = pts[j];
        const by = pts[j + 1];
        let left = Math.hypot(bx - ax, by - ay);
        while (left > 1e-9) {
          const pen = phase < on;
          // Distance until the pen lifts (or comes back down).
          const step = Math.min((pen ? on : period) - phase, left);
          const t = step / left;
          const cx = ax + (bx - ax) * t;
          const cy = ay + (by - ay) * t;
          if (pen) {
            if (!down) out.moveTo(ax, ay);
            out.lineTo(cx, cy);
          }
          phase += step;
          if (phase >= period) phase -= period;
          down = pen && phase < on && phase > 0;
          ax = cx;
          ay = cy;
          left -= step;
        }
      }
    }
    return out;
  };
  return { path, lengths, dashes };
}

const fmtAgo = (s: number) => (s < 10 ? `${Math.round(s * 10) / 10}` : `${Math.round(s)}`);

/**
 * What the hover tip shows. It changes only when its text does: the dots, which
 * ride the trace on every frame, are moved directly (see updateReadout).
 */
interface Readout {
  /** The pointer's position in the box, CSS px: the cursor stays there while the trace flows past. */
  x: number;
  /** Past the middle of the box: put the tip on the left. */
  flip: boolean;
  /** Age of the time under the cursor, as shown (fmtAgo). */
  ago: string;
  /** Each series where it crosses the cursor, with its value there. */
  points: { s: Series; v: number }[];
}

/** A drawn series as functions: height at a track x (device px), value at a time. */
interface Drawn {
  y: (x: number) => number | null;
  value: (t: number) => number | null;
}

export function Chart(props: ChartProps) {
  let host!: HTMLDivElement;
  let track!: HTMLDivElement;
  let canvas!: HTMLCanvasElement;
  let ctx: CanvasRenderingContext2D | null = null;
  // Device pixels.
  let w = 0;
  let h = 0;
  let dpr = 1;
  let margin = 0; // canvas width beyond the visible box, for drift between samples
  let shown = 0; // pixel mode: current leftward shift
  // Time axis: canvas x = (t - left) * speed; `anchor` is the time at the visible
  // right edge as drawn, `zeroAt` the clock time at which that drawing sits unshifted.
  let anchor: number | null = null;
  let zeroAt = 0;
  let left = 0;
  let speed = 0;
  let mode: ChartMode = "off";
  let glide: Animation | null = null;
  /** Balanced mode: the shared grid the glide steps on (see move). */
  let grid = { start: 0, step: 0, steps: 0 };
  let hoverX: number | null = null;
  /** The series as last drawn, for the readout. */
  let drawn: (Drawn | null)[] = [];
  /** The dots' compositor animations on a gliding trace (see glideDots). */
  let dotGlides: Animation[] = [];
  /** The readout's dots, one per series, and the tip's text as last shown. */
  const dots: (HTMLSpanElement | undefined)[] = [];
  let shownText = "";
  const colors = new Map<string, string>();
  /**
   * Where each sample sits in the dash pattern (px into it), by sample time, for
   * each dashed series. A redraw starts the pattern so that a sample already on
   * screen keeps its phase; the dashes then stay put instead of re-flowing from
   * the new leftmost point.
   */
  const dashPhases: Map<number, number>[] = [];
  /** Visible width, CSS px. */
  const [box, setBox] = createSignal({ w: 0 });
  const [readout, setReadout] = createSignal<Readout | null>(null);

  const color = (v: string) => {
    let c = colors.get(v);
    if (!c) {
      c = getComputedStyle(host).getPropertyValue(v).trim() || "#888888";
      colors.set(v, c);
    }
    return c;
  };

  /** Matches the canvas to the box; false when nothing changed. */
  const fit = () => {
    const ratio = window.devicePixelRatio || 1;
    const nw = Math.round(host.clientWidth * ratio);
    const nh = Math.round(host.clientHeight * ratio);
    if (nw === w && nh === h && ratio === dpr) return false;
    w = nw;
    h = nh;
    dpr = ratio;
    return true;
  };

  /** Leftward shift of the track at `now` as the compositor draws it, device pixels. */
  const shift = (now: number) => {
    if (anchor === null) return 0;
    if (mode === "pixel") return shown;
    if (mode === "fluid") return Math.min(margin, Math.max(0, now - zeroAt) * speed);
    if (mode === "balanced" && grid.step > 0) {
      const k = Math.min(grid.steps, Math.max(0, Math.floor((now - grid.start) / grid.step)));
      return (grid.start + k * grid.step - zeroAt) * speed;
    }
    return 0;
  };

  const paint = (now: number) => {
    const c = ctx;
    if (!c || w === 0 || h === 0) return;
    const interval = intervalMs();
    const span = (HISTORY - 1) * interval;
    const nextSpeed = w / span;
    const times = sampleTimes();
    const newest = times[times.length - 1];
    const nextMode = activeMode();

    // Where the right edge is now, moved back so that the canvas origin falls on
    // a whole device pixel of the time axis. Every redraw then rasterizes each
    // point and dash at the same sub-pixel phase; otherwise antialiased edges
    // (short dashes most visibly) change width once per sample. The motion
    // starts slightly ahead to make up the difference.
    const exact = nextMode === "off" ? (newest ?? now) : now - lag();
    mode = nextMode;
    speed = nextSpeed;
    left = Math.floor((exact - span) * speed) / speed;
    anchor = left + span;
    zeroAt = now - (exact - anchor);
    const width = w + (mode === "off" ? 0 : Math.ceil((lag() + interval) * speed));
    if (canvas.width !== width || canvas.height !== h) {
      canvas.width = width;
      canvas.height = h;
      canvas.style.width = `${width / dpr}px`;
      canvas.style.height = `${h / dpr}px`;
    }
    margin = width - w;
    setBox({ w: w / dpr });

    c.clearRect(0, 0, width, h);
    const top = Math.round(TOP_PAD * dpr);
    const rangeY = h - top;
    const max = Math.max(props.max(), 1e-9);
    drawn = props.series.map(() => null);
    props.series.forEach((s, si) => {
      const data = s.data();
      const xs: number[] = [];
      const ys: number[] = [];
      const ts: number[] = [];
      const vs: number[] = [];
      // Newest first, back to two points left of the edge, then reversed. Two,
      // because the bend of the segment crossing the edge depends on the point
      // before it too; with one, that segment re-bent whenever the oldest point
      // dropped off.
      let beyond = 0;
      for (let j = 0; j < data.length && j < times.length && beyond < 2; j++) {
        const x = (times[times.length - 1 - j] - left) * speed;
        const v = data[data.length - 1 - j];
        // NaN marks a sample the sensor missed; the line bridges it.
        if (Number.isFinite(v)) {
          xs.push(x);
          ys.push(top + (1 - Math.min(1, Math.max(0, v / max))) * rangeY);
          ts.push(times[times.length - 1 - j]);
          vs.push(v);
          if (x < 0) beyond++;
        }
      }
      if (xs.length === 0) return;
      xs.reverse();
      ys.reverse();
      ts.reverse();
      vs.reverse();
      const n = xs.length;
      const m = slopes(xs, ys);
      const curve = monotone(xs, ys, m);
      // The readout reads the curve as drawn, and the values along the same cubic.
      drawn[si] = { y: interpolate(xs, ys, m), value: interpolate(ts, vs, slopes(ts, vs)) };
      const line = curve.path;
      const col = color(s.colorVar);
      const fill = s.fill ?? 0.3;
      if (fill > 0 && n > 1) {
        const area = new Path2D(line);
        area.lineTo(xs[n - 1], h);
        area.lineTo(xs[0], h);
        area.closePath();
        const g = c.createLinearGradient(0, top, 0, h);
        g.addColorStop(0, withAlpha(col, fill));
        g.addColorStop(1, withAlpha(col, 0));
        c.fillStyle = g;
        c.fill(area);
      }
      c.strokeStyle = col;
      c.lineWidth = 1.5 * dpr;
      c.lineJoin = "round";
      if (s.dashed) {
        // Dashes run along the curve, so they keep their length on steep
        // spikes. To keep them from crawling, begin the pattern so that the
        // first sample drawn last time keeps its phase.
        const dash = 4 * dpr;
        const period = dash + 3 * dpr;
        const lengths = curve.lengths();
        const known = dashPhases[si];
        let start = 0;
        for (let i = 0, along = 0; i < n; along += lengths[i] ?? 0, i++) {
          const phase = known?.get(ts[i]);
          if (phase !== undefined) {
            start = phase - along;
            break;
          }
        }
        const phases = new Map<number, number>();
        for (let i = 0, along = start; i < n; along += lengths[i] ?? 0, i++) {
          phases.set(ts[i], ((along % period) + period) % period);
        }
        dashPhases[si] = phases;
        c.stroke(curve.dashes(start, dash, period));
      } else {
        c.stroke(line);
      }
    });
  };

  /** Starts the track moving from where the fresh drawing sits. */
  const move = () => {
    glide?.cancel();
    glide = null;
    shown = 0;
    grid = { start: 0, step: 0, steps: 0 };
    track.style.transform = "translateX(0)";
    if (mode !== "fluid" && mode !== "balanced") return;
    // At any instant the shift must equal (now - zeroAt) * speed: exactly the
    // time axis, so the next drawing lines up with this one.
    if (mode === "fluid") {
      glide = track.animate(
        [{ transform: "translateX(0)" }, { transform: `translateX(${-margin / dpr}px)` }],
        { duration: margin / speed, fill: "forwards", easing: "linear" },
      );
      // The document timeline shares performance.now()'s clock.
      glide.startTime = zeroAt;
    } else {
      // Balanced: hold each position for a whole step of a grid shared by every
      // chart (steps are multiples of the refresh period), so all of them change
      // on the same frames instead of each adding its own.
      const step = balancedFrameMs();
      const start = Math.floor(zeroAt / step) * step;
      const steps = Math.max(1, Math.ceil((zeroAt - start + margin / speed) / step));
      grid = { start, step, steps };
      const from = ((zeroAt - start) * speed) / dpr;
      const to = from - (steps * step * speed) / dpr;
      glide = track.animate([{ transform: `translateX(${from}px)` }, { transform: `translateX(${to}px)` }], {
        duration: steps * step,
        fill: "forwards",
        easing: `steps(${steps}, end)`,
      });
      glide.startTime = start;
    }
  };

  /** Pixel mode: shifts the track to where the right edge is at `now`, in whole device pixels. */
  const place = (now: number) => {
    if (mode !== "pixel" || anchor === null) return;
    const px = Math.min(margin, Math.floor(Math.max(0, now - lag() - anchor) * speed));
    if (px !== shown) {
      shown = px;
      track.style.transform = `translateX(${-px / dpr}px)`;
    }
  };

  /**
   * The readout at the pointer: the cursor stays under it and the trace flows
   * past, so it reads the drawn curve there (the same cubic, at the track's
   * shift for `now`) rather than jumping between samples.
   */
  const updateReadout = (now: number) => {
    const b = box();
    const format = props.format;
    if (hoverX === null || !format || anchor === null || speed === 0 || hoverX < 0 || hoverX > b.w) {
      hideReadout();
      return;
    }
    const x = hoverX * dpr + shift(now);
    const at = left + x / speed;
    // Per series: the height drawn under the cursor (CSS px) and the value there.
    const read = props.series.map((_, i) => {
      const y = drawn[i]?.y(x);
      const v = drawn[i]?.value(at);
      return y == null || v == null ? null : { y: y / dpr, v };
    });
    const points = props.series.flatMap((s, i) => {
      const r = read[i];
      return r ? [{ s, v: r.v }] : [];
    });
    if (points.length === 0) {
      hideReadout();
      return;
    }
    // Age of the time under the cursor. A moving trace keeps a fixed age at a
    // fixed point, counted from now; a still one (off, paused) moves a sample
    // at a time, so count from the newest sample. Counting a moving trace from
    // the newest sample would climb back a second at every sample.
    const times = sampleTimes();
    const reference = mode === "off" ? times[times.length - 1] : now;
    const ago = fmtAgo(Math.max(0, (reference - at) / 1000));
    const flip = hoverX > b.w / 2;
    const text = [hoverX, flip, b.w, ago, ...points.map((p) => format(p.v))].join("|");
    if (text !== shownText || !readout()) {
      shownText = text;
      setReadout({ x: hoverX, flip, ago, points });
    }
    for (const [i, r] of read.entries()) {
      const dot = dots[i];
      if (!dot) continue;
      dot.style.visibility = r ? "" : "hidden";
      if (r) dot.style.transform = `translate(${hoverX}px, ${r.y}px)`;
    }
  };

  const hideReadout = () => {
    shownText = "";
    stopDots();
    if (readout()) setReadout(null);
  };

  /**
   * Fluid and balanced: the dots ride the trace on the compositor, as the track
   * does, from `now` until the track stops (a new sample redraws both first).
   * The keyframes follow the drawn curve under the cursor, one per device pixel
   * of travel (fluid) or per step of the shared grid (balanced), on the track's
   * own timeline; the main thread then only updates the tip's text.
   */
  const glideDots = (now: number) => {
    stopDots();
    if (hoverX === null || !readout() || anchor === null || speed === 0) return;
    const times: number[] = [];
    const shifts: number[] = [];
    if (mode === "fluid") {
      const end = zeroAt + margin / speed;
      if (now >= end) return;
      let s = Math.max(0, (now - zeroAt) * speed);
      for (;;) {
        times.push(zeroAt + s / speed);
        shifts.push(s);
        if (s >= margin) break;
        s = Math.min(margin, Math.floor(s) + 1);
      }
    } else if (mode === "balanced" && grid.step > 0) {
      const first = Math.max(0, Math.floor((now - grid.start) / grid.step));
      for (let k = first; k <= grid.steps; k++) {
        times.push(grid.start + k * grid.step);
        shifts.push((grid.start + k * grid.step - zeroAt) * speed);
      }
    }
    if (times.length < 2) return;
    const start = times[0];
    const x = hoverX * dpr;
    for (const [i, d] of drawn.entries()) {
      const dot = dots[i];
      if (!dot || !d) continue;
      // As far as the curve goes. Near the right edge it ends at the newest
      // sample before the track would stop; the next sample redraws long before
      // the dot gets there. Giving up on the whole glide instead left the dot to
      // the text's 10 updates a second there, visibly lagging the trace.
      const ys: number[] = [];
      for (const s of shifts) {
        const y = d.y(x + s);
        if (y == null) break;
        ys.push(y);
      }
      // No curve under the cursor yet (data does not reach back that far): stay put.
      if (ys.length < 2) continue;
      const duration = times[ys.length - 1] - start;
      const frames: Keyframe[] = ys.map((y, k) => ({
        offset: (times[k] - start) / duration,
        transform: `translate(${hoverX}px, ${y / dpr}px)`,
        // Balanced: hold each position for its step, as the track does.
        easing: mode === "balanced" ? "steps(1, end)" : "linear",
      }));
      const glide = dot.animate(frames, { duration, fill: "forwards" });
      glide.startTime = start;
      dotGlides.push(glide);
    }
  };

  const stopDots = () => {
    for (const glide of dotGlides) glide.cancel();
    dotGlides = [];
  };

  const redraw = () => {
    const now = performance.now();
    paint(now);
    move();
    place(now);
    updateReadout(now);
    glideDots(now);
    requestWake();
  };

  const ticks: Ticked = {
    next(now) {
      // Fluid and balanced readouts: the text keeps up with the trace (the dots glide).
      const text =
        hoverX !== null && (mode === "fluid" || mode === "balanced") ? TEXT_EVERY : Number.POSITIVE_INFINITY;
      if (mode !== "pixel" || anchor === null || speed === 0) return text;
      const px = Math.max(0, now - lag() - anchor) * speed;
      if (px >= margin) return Number.POSITIVE_INFINITY;
      return (Math.floor(px) + 1 - px) / speed + 0.5;
    },
    pixelSpeed: () => (mode === "pixel" ? speed : 0),
    tick(now) {
      place(now);
      if (hoverX !== null) updateReadout(now);
    },
  };

  onMount(() => {
    ctx = canvas.getContext("2d");
    fit();
    const ro = new ResizeObserver(() => {
      if (fit()) untrack(redraw);
    });
    ro.observe(host);
    // Colors come from CSS and differ between light and dark.
    const scheme = matchMedia("(prefers-color-scheme: dark)");
    const recolor = () => {
      colors.clear();
      untrack(redraw);
    };
    scheme.addEventListener("change", recolor);
    // Coming back from the tray: catch up with the samples taken meanwhile.
    const onVisible = () => document.visibilityState === "visible" && untrack(redraw);
    document.addEventListener("visibilitychange", onVisible);
    ticked.add(ticks);
    onCleanup(() => {
      ro.disconnect();
      scheme.removeEventListener("change", recolor);
      document.removeEventListener("visibilitychange", onVisible);
      ticked.delete(ticks);
      glide?.cancel();
      stopDots();
    });

    createEffect(() => {
      sampleSeq();
      for (const s of props.series) s.data();
      props.max();
      activeMode();
      intervalMs();
      untrack(redraw);
    });
  });

  return (
    <div
      ref={host}
      class={`chart ${props.class ?? ""}`}
      aria-hidden="true"
      onPointerMove={(e) => {
        if (!props.format) return;
        const entering = hoverX === null;
        hoverX = e.clientX - host.getBoundingClientRect().left;
        const now = performance.now();
        updateReadout(now);
        glideDots(now);
        // Starts the text updates (see ticks.next).
        if (entering) requestWake();
      }}
      onPointerLeave={() => {
        hoverX = null;
        updateReadout(performance.now());
      }}
    >
      <div ref={track} class="chart-track">
        <canvas ref={canvas} />
      </div>
      <Show when={readout()}>
        {(r) => (
          <>
            <span class="chart-cursor" style={{ transform: `translateX(${r().x}px)` }} />
            {/* Placed by updateReadout on every frame the trace moves. */}
            <Index each={props.series}>
              {(s, i) => (
                <span
                  ref={(el) => {
                    dots[i] = el;
                  }}
                  class="chart-dot"
                  style={{ "--dot": `var(${s().colorVar})` }}
                />
              )}
            </Index>
            <div
              class="chart-tip"
              style={r().flip ? { right: `${box().w - r().x + 10}px` } : { left: `${r().x + 10}px` }}
            >
              <div class="chart-tip-time">
                {r().ago === "0"
                  ? t("performance.chart.latest")
                  : t("performance.chart.ago", { seconds: r().ago })}
              </div>
              <Index each={r().points}>
                {(p) => (
                  <div class="chart-tip-row">
                    <i classList={{ dashed: p().s.dashed }} style={{ "--dot": `var(${p().s.colorVar})` }} />
                    <span>{p().s.label}</span>
                    <b>{props.format?.(p().v)}</b>
                  </div>
                )}
              </Index>
            </div>
          </>
        )}
      </Show>
    </div>
  );
}

/** Single-series shorthand. */
export default function Trace(props: {
  data: () => readonly number[];
  max: () => number;
  colorVar: string;
  class?: string;
}) {
  return (
    <Chart class={props.class} max={props.max} series={[{ data: props.data, colorVar: props.colorVar }]} />
  );
}

/**
 * Smooths an auto-scaling ceiling: it rises at once (nothing is clipped) but
 * comes down over a few samples, so a spike leaving the window doesn't make
 * the whole chart jump. When `identity` changes (another process), it resets.
 */
export function easedMax(target: () => number, identity?: () => unknown): () => number {
  let last: unknown;
  return createMemo((prev: number | undefined): number => {
    const next = target();
    const id = identity?.();
    const reset = id !== last;
    last = id;
    return prev === undefined || reset || next >= prev ? next : prev + (next - prev) * 0.3;
  }, undefined);
}
