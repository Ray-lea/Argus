// Draws the demo's curves (Curve.tsx), each on its own canvas, off the page's
// main thread, as the app draws its charts: each sample's curve is drawn once
// into an image at twice the screen's density (the app's chartRasterScale),
// with every sample on the same fraction of a pixel it had in the image
// before, so the line's antialiasing stays as it was drawn. On every display
// frame the image is scaled down where the sample clock puts it, by fractions
// of a pixel; each screen pixel then averages several of the image's, so the
// line keeps its weight between whole pixels (an image at the screen's own
// density, moved by half a pixel, showed its line soft and its edges stepped
// differently from frame to frame). The ends are cut on whole pixels, and the
// hover dots are drawn on the same frame.
// A new sample arrives with its clock in one message, so the curve and its
// scroll change together; where the new clock puts the curve a little off the
// old one (a late timer), the difference is spread over the next half second
// as a change of speed, so the curve never steps.
import { DASH, OVERRUN, heightAt, lengthTo, tangents, type Frame, type Geometry, type Message } from "./curve-math";

interface Chart {
  canvas: OffscreenCanvas;
  ctx: OffscreenCanvasRenderingContext2D;
  geometry?: Geometry;
  frame?: Frame;
  lines: { ys: number[]; m: number[] }[];
  /** Each dashed series' dash phase where its path starts, in device pixels. */
  dashes: number[];
  pointer: number | null;
  visible: boolean;
  /** What is left of a difference between the old clock and a new one, in samples, and since when. */
  offset: number;
  offsetAt: number;
  /** How long it takes to even out (longer for a curve held back further, so it never stops). */
  settle: number;
  /** Where a held curve stopped, in samples (the end of its step if it never moved). */
  held: number | null;
  /** The curve drawn for the current data, and where its sample 0 lies in it. */
  image: OffscreenCanvas | null;
  imageX: number;
  /** The data, colours or size changed: draw the image again. */
  stale: boolean;
  /** Draw on the next frame even if nothing moves. */
  dirty: boolean;
  shown: boolean;
}

const scope = self as unknown as {
  onmessage: ((event: MessageEvent<Message>) => void) | null;
  postMessage(message: unknown): void;
  requestAnimationFrame?: (callback: (time: number) => void) => number;
};
const charts = new Map<number, Chart>();
const origin = performance.timeOrigin;
const raf = scope.requestAnimationFrame
  ? (callback: (time: number) => void) => scope.requestAnimationFrame!(callback)
  : (callback: (time: number) => void) => setTimeout(() => callback(performance.now()), 16);
let scheduled = false;
const schedule = () => {
  if (scheduled) return;
  scheduled = true;
  raf(frame);
};

// How far the scroll has carried the curve at `now`, in samples.
const phaseAt = (f: Frame, now: number) => (f.at === null ? 1 : Math.min(1 + OVERRUN, Math.max(0, (now - f.at) / f.interval)));
// How long a difference between clocks takes to even out, at least.
const SETTLE = 500;
const settling = (c: Chart, now: number) => c.offset * Math.max(0, 1 - (now - c.offsetAt) / c.settle);
// Where the curve is drawn at `now`: its clock's phase, with any difference still evening out.
const placeAt = (c: Chart, f: Frame, now: number) =>
  f.at === null ? (c.held ?? 1) : Math.min(1 + OVERRUN, Math.max(-1, phaseAt(f, now) + settling(c, now)));
const moving = (c: Chart, now: number) =>
  !!c.frame && c.frame.at !== null && ((now - c.frame.at) / c.frame.interval < 1 + OVERRUN || settling(c, now) !== 0);
const rgba = (color: number[], alpha = 1) => `rgba(${color[0]},${color[1]},${color[2]},${(color[3] ?? 1) * alpha})`;

// Pixels of room around the image, so that averaging at its edges never runs out.
const PAD = 2;
// Image pixels per screen pixel.
const DENSITY = 2;
function paint(c: Chart) {
  const g = c.geometry,
    f = c.frame;
  if (!g || !f) return;
  const sx = g.boxWidth / f.shown,
    sy = g.boxHeight / 100;
  // Sample 0 is the sample at tick - drawn; each sample keeps its fraction of an image pixel.
  const first = (f.tick - f.drawn) * sx * DENSITY;
  const x0 = PAD + (first - Math.floor(first)) / DENSITY;
  const width = Math.ceil((x0 + f.drawn * sx + PAD) * DENSITY),
    height = g.height * DENSITY;
  if (!c.image) c.image = new OffscreenCanvas(width, height);
  if (c.image.width !== width) c.image.width = width;
  if (c.image.height !== height) c.image.height = height;
  const ctx = c.image.getContext("2d");
  if (!ctx) return;
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.clearRect(0, 0, width, height);
  // Drawn in screen pixels, at the image's density.
  ctx.setTransform(DENSITY, 0, 0, DENSITY, 0, 0);
  c.imageX = x0;
  const X = (x: number) => x0 + x * sx,
    Y = (y: number) => g.top + y * sy;
  const trace = (ys: number[], m: number[]) => {
    ctx.moveTo(X(0), Y(ys[0]));
    for (let i = 0; i < ys.length - 1; i++)
      ctx.bezierCurveTo(X(i + 1 / 3), Y(ys[i] + m[i] / 3), X(i + 2 / 3), Y(ys[i + 1] - m[i + 1] / 3), X(i + 1), Y(ys[i + 1]));
  };
  f.series.forEach((s, i) => {
    const line = c.lines[i];
    if (!line || line.ys.length < 2) return;
    if (s.fill) {
      ctx.beginPath();
      trace(line.ys, line.m);
      ctx.lineTo(X(f.drawn), Y(100));
      ctx.lineTo(X(0), Y(100));
      ctx.closePath();
      const gradient = ctx.createLinearGradient(0, Y(0), 0, Y(100));
      gradient.addColorStop(0, rgba(s.color, 0.3));
      gradient.addColorStop(1, rgba(s.color, 0));
      ctx.fillStyle = gradient;
      ctx.fill();
    }
    ctx.beginPath();
    trace(line.ys, line.m);
    ctx.lineWidth = 1.5 * g.scale;
    ctx.lineJoin = "round";
    ctx.strokeStyle = rgba(s.color, s.faint ? 0.55 : 1);
    if (s.dashed) {
      ctx.setLineDash(DASH.map((d) => d * g.scale));
      ctx.lineDashOffset = c.dashes[i] ?? 0;
    } else ctx.setLineDash([]);
    ctx.stroke();
  });
  c.stale = false;
}

function draw(c: Chart, now: number) {
  const g = c.geometry,
    f = c.frame;
  if (!g || !f) return;
  if (c.stale || !c.image) paint(c);
  if (!c.image) return;
  const { ctx } = c;
  ctx.clearRect(0, 0, g.width, g.height);
  const phase = placeAt(c, f, now);
  const sx = g.boxWidth / f.shown,
    sy = g.boxHeight / 100;
  const Y = (y: number) => g.top + y * sy;
  ctx.save();
  ctx.beginPath();
  ctx.rect(g.clipLeft, g.top, g.clipRight - g.clipLeft, g.boxHeight);
  ctx.clip();
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = "medium";
  ctx.drawImage(c.image, g.left - (f.margin + phase) * sx - c.imageX, 0, c.image.width / DENSITY, c.image.height / DENSITY);
  ctx.restore();
  // The readout's dots ride the curve under the pointer, on this same frame.
  if (c.pointer !== null) {
    const x = f.margin + phase + c.pointer * f.shown;
    const cx = g.left + c.pointer * g.boxWidth + 0.5 * g.scale;
    f.series.forEach((s, i) => {
      const line = c.lines[i];
      if (!line || line.ys.length < 2) return;
      const cy = Y(heightAt(line.ys, line.m, x));
      ctx.beginPath();
      ctx.arc(cx, cy, 6 * g.scale, 0, Math.PI * 2);
      ctx.fillStyle = rgba(f.ring);
      ctx.fill();
      ctx.beginPath();
      ctx.arc(cx, cy, 4 * g.scale, 0, Math.PI * 2);
      ctx.fillStyle = rgba(s.color);
      ctx.fill();
    });
  }
}

function frame(time: number) {
  scheduled = false;
  const now = origin + time;
  let again = false;
  for (const [id, c] of charts) {
    if (!c.geometry || !c.frame) continue;
    const move = c.visible && moving(c, now);
    if (c.dirty || move) {
      draw(c, now);
      c.dirty = false;
      if (!c.shown) {
        c.shown = true;
        scope.postMessage({ kind: "drawn", id });
      }
    }
    if (move) again = true;
  }
  if (again) schedule();
}

// A new sample re-draws the path from one sample later: its dashes start
// that much further along their pattern, so they keep their place on the curve.
function carryDashes(c: Chart, next: Frame, lines: Chart["lines"]) {
  const before = c.frame,
    g = c.geometry;
  if (!before || !g || next.tick !== before.tick + 1) return;
  const sx = g.boxWidth / next.shown,
    sy = g.boxHeight / 100,
    period = (DASH[0] + DASH[1]) * g.scale;
  next.series.forEach((s, i) => {
    const old = c.lines[i];
    if (!s.dashed || !old) return;
    // Measured to the first sample shown, past the ends whose tangents differ.
    const moved = lengthTo(old.ys, old.m, next.margin + 1, sx, sy) - lengthTo(lines[i].ys, lines[i].m, next.margin, sx, sy);
    c.dashes[i] = ((((c.dashes[i] ?? 0) + moved) % period) + period) % period;
  });
}

scope.onmessage = ({ data }) => {
  if (data.kind === "add") {
    const ctx = data.canvas.getContext("2d");
    if (!ctx) return;
    charts.set(data.id, {
      canvas: data.canvas,
      ctx,
      lines: [],
      dashes: [],
      image: null,
      imageX: 0,
      stale: true,
      offset: 0,
      offsetAt: 0,
      settle: SETTLE,
      held: null,
      pointer: null,
      visible: true,
      dirty: true,
      shown: false,
    });
    return;
  }
  const c = charts.get(data.id);
  if (!c) return;
  if (data.kind === "remove") charts.delete(data.id);
  else if (data.kind === "geometry") {
    c.geometry = data.geometry;
    c.stale = true;
    if (c.canvas.width !== data.geometry.width) c.canvas.width = data.geometry.width;
    if (c.canvas.height !== data.geometry.height) c.canvas.height = data.geometry.height;
  } else if (data.kind === "frame") {
    const lines = data.frame.series.map((s) => ({ ys: s.ys, m: s.ys.length > 1 ? tangents(s.ys) : [0] }));
    // The new path starts `shift` samples later; where the old clock had the
    // curve, the new one starts it, and the offset evens out from there.
    const now = origin + performance.now(),
      old = c.frame,
      shift = old ? data.frame.tick - old.tick : 0;
    // Held (sampling paused), the curve stays where it is, and carries on from there.
    if (data.frame.at === null) c.held = old && old.at !== null && shift === 0 ? placeAt(c, old, now) : (c.held ?? null);
    if (old && data.frame.at !== null && shift >= 0 && shift <= 2) {
      const gap = phaseAt(data.frame, now) + shift - placeAt(c, old, now);
      c.offset = Math.abs(gap) < 1.5 ? -gap : 0;
      c.offsetAt = now;
      // Held back, the curve keeps at least half its speed while it evens out.
      c.settle = Math.max(SETTLE, 2 * c.offset * data.frame.interval);
    } else c.offset = 0;
    if (data.frame.at !== null) c.held = null;
    carryDashes(c, data.frame, lines);
    c.frame = data.frame;
    c.lines = lines;
    c.stale = true;
  } else if (data.kind === "pointer") c.pointer = data.at;
  else if (data.kind === "visible") c.visible = data.visible;
  c.dirty = true;
  schedule();
};
