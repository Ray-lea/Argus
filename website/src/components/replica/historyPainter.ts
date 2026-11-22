import type { Trace } from "./historySeries";
export interface RasterRequest {
  id: number;
  revision: number;
  width: number;
  height: number;
  ratio: number;
  traces: Trace[];
  grid?: { ticks: number[]; rows: number; color: string };
  gapColor: string;
}
// Round each step at its corners, preserving the dwell time of each sample.
// The native TimePlot caps the radius at 2.5 logical pixels.
function stepPath(
  values: number[],
  first: number,
  end: number,
  x: (n: number) => number,
  y: (v: number) => number,
  radius: number,
) {
  const path = new Path2D();
  path.moveTo(x(first), y(values[first]));
  for (let i = first + 1; i < end; i++) {
    const old = y(values[i - 1]),
      next = y(values[i]),
      at = x(i),
      r = Math.min(radius, (at - x(i - 1)) / 2, Math.abs(next - old) / 2),
      sign = Math.sign(next - old);
    path.lineTo(at - r, old);
    path.quadraticCurveTo(at, old, at, old + r * sign);
    path.lineTo(at, next - r * sign);
    path.quadraticCurveTo(at, next, at + r, next);
  }
  path.lineTo(x(end), y(values[end - 1]));
  return path;
}
export function paint(ctx: OffscreenCanvasRenderingContext2D | CanvasRenderingContext2D, r: RasterRequest) {
  const { width, height, ratio } = r;
  ctx.clearRect(0, 0, width, height);
  if (r.grid) {
    ctx.strokeStyle = r.grid.color;
    ctx.lineWidth = ratio;
    ctx.beginPath();
    for (const n of r.grid.ticks) {
      ctx.moveTo(n * width, 0.5);
      ctx.lineTo(n * width, height);
    }
    for (let i = 0; i <= r.grid.rows; i++) {
      const y = 0.5 + ((height - 1) * i) / r.grid.rows;
      ctx.moveTo(0, y);
      ctx.lineTo(width, y);
    }
    ctx.stroke();
  }
  r.traces.forEach((s, index) => {
    const x = (i: number) => (width * i) / (s.values.length - 1);
    const y = (v: number) => height - ((height - 1) * (v - s.min)) / (s.max - s.min);
    for (let i = 0; i < s.values.length; ) {
      const start = i;
      if (!Number.isFinite(s.values[i])) {
        while (i < s.values.length && !Number.isFinite(s.values[i])) i++;
        if (index === 0) {
          ctx.save();
          ctx.beginPath();
          ctx.rect(x(start), 0, x(i) - x(start), height);
          ctx.clip();
          ctx.beginPath();
          for (let at = x(start) - height; at < x(i); at += 7 * ratio) {
            ctx.moveTo(at, height);
            ctx.lineTo(at + height, 0);
          }
          ctx.strokeStyle = r.gapColor;
          ctx.stroke();
          ctx.restore();
        }
        continue;
      }
      while (i < s.values.length && Number.isFinite(s.values[i])) i++;
      const line = stepPath(s.values, start, i, x, y, 2.5 * ratio);
      if (s.fill) {
        const area = new Path2D(line);
        area.lineTo(x(i), height);
        area.lineTo(x(start), height);
        area.closePath();
        const fill = ctx.createLinearGradient(0, 0, 0, height);
        fill.addColorStop(0, s.color);
        fill.addColorStop(1, "transparent");
        ctx.globalAlpha = s.fill;
        ctx.fillStyle = fill;
        ctx.fill(area);
      }
      if (s.high && s.low) {
        ctx.fillStyle = s.color;
        ctx.globalAlpha = 0.13;
        for (let j = start; j < i; j++)
          ctx.fillRect(x(j), y(s.high[j]), x(j + 1) - x(j), Math.max(0, y(s.low[j]) - y(s.high[j])));
      }
      ctx.globalAlpha = s.opacity;
      ctx.strokeStyle = s.color;
      ctx.setLineDash(s.dash ? [4 * ratio, 3 * ratio] : []);
      ctx.lineWidth = 1.5 * ratio;
      ctx.lineJoin = "round";
      ctx.stroke(line);
      ctx.setLineDash([]);
      ctx.globalAlpha = 1;
    }
  });
}
