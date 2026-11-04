"use client";
import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from "react";

export interface Series {
  values: number[];
  color: string;
  dashed?: boolean;
  fill?: boolean;
  /** A compact chart's secondary series: solid, at 55% (dashes break up at preview sizes). */
  faint?: boolean;
}
export type Motion = "smooth" | "step";

/** When the latest sample arrived: the window sets it, and a curve that mounts between samples starts its scroll there. */
export const sampleClock = { at: 0 };

// Samples drawn beyond each edge but never shown. The tangents at the ends of
// the data (and the limiter that adjusts them) change as samples arrive and
// leave; the app likewise keeps a scroll margin on both sides of its window.
const MARGIN = 2;

// Monotone cubic interpolation (Fritsch–Carlson), as the app joins adjacent readings.
function tangents(ys: number[]) {
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
function pathOf(ys: number[], m: number[]) {
  const f = (v: number) => +v.toFixed(2);
  let path = `M0 ${f(ys[0])}`;
  for (let i = 0; i < ys.length - 1; i++)
    path += `C${f(i + 1 / 3)} ${f(ys[i] + m[i] / 3)} ${f(i + 2 / 3)} ${f(ys[i + 1] - m[i + 1] / 3)} ${i + 1} ${f(ys[i + 1])}`;
  return path;
}
// The curve's height at x: its control points sit at thirds, so x is the Bézier parameter.
function heightAt(ys: number[], m: number[], x: number) {
  const i = Math.max(0, Math.min(ys.length - 2, Math.floor(x)));
  const t = Math.max(0, Math.min(1, x - i)),
    u = 1 - t;
  const a = ys[i],
    b = ys[i] + m[i] / 3,
    c = ys[i + 1] - m[i + 1] / 3,
    d = ys[i + 1];
  return u * u * u * a + 3 * u * u * t * b + 3 * u * t * t * c + t * t * t * d;
}
// Seconds as the app's readout shows them: tenths below ten.
const ago = (seconds: number) => (seconds < 10 ? String(Math.round(seconds * 10) / 10) : String(Math.round(seconds)));

/** A performance chart's hover readout: series names and how values read. */
export interface Readout {
  names?: string[];
  format: (value: number) => string;
}
interface Props {
  series: Series[];
  max: number;
  tick: number;
  interval: number;
  motion?: Motion;
  running?: boolean;
  className?: string;
  label?: string;
  readout?: Readout;
}

/*
 * The track holds one more interval than the frame shows, and the hidden
 * margins beyond that. Each new sample re-draws the path one step to the left
 * and the compositor carries the track back across that step over the refresh
 * interval, so the line moves at the display's frame rate without per-frame script.
 */
export default function Curve({
  series,
  max,
  tick,
  interval,
  motion = "smooth",
  running = true,
  className,
  label,
  readout,
}: Props) {
  const id = useId().replace(/:/g, "");
  const track = useRef<HTMLDivElement>(null);
  const box = useRef<HTMLDivElement>(null);
  const clip = useRef<HTMLDivElement>(null);
  // The demo is drawn scaled, so a curve's box seldom starts or ends on a whole
  // screen pixel. Its clip keeps to the screen pixels wholly inside the box:
  // the curve never shares a pixel with the divider or border beside it (which
  // it would paint over in some frames and not in others as it scrolls), and it
  // ends on a crisp column, as in the app. Each sample re-renders the curve, so
  // a move that changes no size is caught within one interval.
  const snap = useCallback(() => {
    const outer = box.current,
      inner = clip.current;
    if (!outer || !inner || !outer.offsetWidth) return;
    const r = outer.getBoundingClientRect(),
      ratio = devicePixelRatio,
      k = r.width / outer.offsetWidth;
    const left = r.left * ratio,
      right = r.right * ratio;
    // A hair inside the whole pixel: an edge a rounding error past it is
    // rounded out to the neighbouring pixel.
    const inset = (px: number) => ((px + 0.05) / ratio / k).toFixed(3) + "px";
    inner.style.left = inset(Math.ceil(left - 0.001) - left);
    inner.style.right = inset(right - Math.floor(right + 0.001));
  }, []);
  useLayoutEffect(snap);
  useEffect(() => {
    const outer = box.current;
    if (!outer) return;
    const observer = new ResizeObserver(snap);
    observer.observe(outer);
    addEventListener("resize", snap);
    return () => {
      observer.disconnect();
      removeEventListener("resize", snap);
    };
  }, [snap]);
  const count = series[0]?.values.length ?? 2;
  const margin = count >= 2 * MARGIN + 4 ? MARGIN : 0;
  const drawn = Math.max(2, count - 1);
  const shown = drawn - 2 * margin - 1;
  const at = (phase: number) => `translateX(-${((margin + phase) / drawn) * 100}%)`;
  const y = (v: number) => 100 - (Math.max(0, Math.min(max, v)) / max) * 100;
  const lines = useMemo(
    () =>
      series.map((s) => {
        const ys = s.values.map(y);
        const m = ys.length > 1 ? tangents(ys) : [0];
        return { ys, m, path: ys.length > 1 ? pathOf(ys, m) : "" };
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [series, max],
  );
  useLayoutEffect(() => {
    const el = track.current;
    if (!el) return;
    for (const a of el.getAnimations()) a.cancel();
    if (motion !== "smooth" || !running) return;
    if (matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const animation = el.animate([{ transform: at(0) }, { transform: at(1) }], {
      duration: interval,
      easing: "linear",
      fill: "forwards",
    });
    // A curve shown between samples joins the scroll where the others are.
    animation.currentTime = Math.min(interval, Math.max(0, performance.now() - sampleClock.at));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tick, motion, running, interval, drawn, margin]);

  // The readout: a line at the pointer, a dot on each curve that rides it every
  // frame, and text that reads the curve ten times a second.
  const [hover, setHover] = useState<number | null>(null);
  const [, setRead] = useState(0);
  const dots = useRef<(HTMLElement | null)[]>([]);
  const phase = () => {
    const timing = track.current?.getAnimations()[0]?.effect?.getComputedTiming();
    return typeof timing?.progress === "number" ? timing.progress : 1;
  };
  const xAt = (fraction: number) => margin + phase() + fraction * shown;
  useEffect(() => {
    if (hover === null || !readout) return;
    let frame = 0;
    const ride = () => {
      const x = xAt(hover);
      lines.forEach((line, i) => {
        const dot = dots.current[i];
        if (dot) dot.style.top = heightAt(line.ys, line.m, x) + "%";
      });
      frame = requestAnimationFrame(ride);
    };
    ride();
    const text = setInterval(() => setRead((n) => n + 1), 100);
    return () => {
      cancelAnimationFrame(frame);
      clearInterval(text);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hover, lines, readout]);
  const x = hover === null ? 0 : xAt(hover);
  const seconds = hover === null ? 0 : ((1 - hover) * shown * interval) / 1000;

  return (
    <div
      ref={box}
      className={"curve " + (readout ? "is-readout " : "") + (className ?? "")}
      role={label ? "img" : undefined}
      aria-label={label}
      onPointerMove={
        readout
          ? (event) => {
              const box = event.currentTarget.getBoundingClientRect();
              setHover(Math.max(0, Math.min(1, (event.clientX - box.left) / box.width)));
            }
          : undefined
      }
      onPointerLeave={readout ? () => setHover(null) : undefined}
    >
      <div className="curve-clip" ref={clip}>
        <div className="curve-track" ref={track} style={{ width: `${(drawn / shown) * 100}%`, transform: at(1) }}>
          <svg viewBox={`0 0 ${drawn} 100`} preserveAspectRatio="none" aria-hidden="true">
            <defs>
              {series.map((s, i) =>
                s.fill === false ? null : (
                  <linearGradient key={i} id={`${id}-${i}`} gradientUnits="userSpaceOnUse" x1="0" x2="0" y1="0" y2="100">
                    <stop offset="0" stopColor={s.color} stopOpacity="0.3" />
                    <stop offset="1" stopColor={s.color} stopOpacity="0" />
                  </linearGradient>
                ),
              )}
            </defs>
            {series.map((s, i) => (
              <g key={i}>
                {s.fill !== false && <path d={`${lines[i].path}L${drawn} 100L0 100Z`} fill={`url(#${id}-${i})`} />}
                <path
                  d={lines[i].path}
                  fill="none"
                  stroke={s.color}
                  strokeWidth="1.5"
                  strokeDasharray={s.dashed ? "4 3" : undefined}
                  strokeOpacity={s.faint ? 0.55 : undefined}
                  vectorEffect="non-scaling-stroke"
                  strokeLinejoin="round"
                />
              </g>
            ))}
          </svg>
        </div>
      </div>
      {readout && hover !== null && (
        <div className="curve-readout" style={{ "--x": hover } as CSSProperties} aria-hidden="true">
          <i className="curve-cursor" />
          {lines.map((line, i) => (
            <i
              key={i}
              className="curve-dot"
              ref={(el) => {
                dots.current[i] = el;
              }}
              style={{ top: heightAt(line.ys, line.m, x) + "%", background: series[i].color }}
            />
          ))}
          <div className={"curve-tip" + (hover > 0.5 ? " is-flipped" : "")}>
            <span className="curve-age">{ago(seconds) === "0" ? "最新" : `${ago(seconds)} 秒前`}</span>
            {lines.map((line, i) => (
              <span className="curve-row" key={i}>
                <i className={"curve-swatch" + (series[i].dashed ? " is-dashed" : "")} style={{ color: series[i].color }} />
                {readout.names?.[i] && <span>{readout.names[i]}</span>}
                <b>{readout.format(((100 - heightAt(line.ys, line.m, x)) / 100) * max)}</b>
              </span>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
