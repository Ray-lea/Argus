/** @jsxImportSource @/components/replica/i18n */
"use client";
import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { translate } from "./i18n";

export interface Series {
  values: number[];
  color: string;
  dashed?: boolean;
  fill?: boolean;
  /** A compact chart's secondary series: solid, at 55% (dashes break up at preview sizes). */
  faint?: boolean;
}
export type Motion = "smooth" | "step";

/**
 * When the latest sample's scroll began (a curve that mounts between samples
 * joins it there), or that the curves rest at the end of their step until
 * the first sample after a pause.
 */
export interface SampleClock {
  at: number;
  rest: boolean;
}
/** The window's clock: the window advances it, its charts follow it. */
export const sampleClock: SampleClock = { at: 0, rest: true };

// Samples drawn beyond each edge but never shown. The tangents at the ends of
// the data (and the limiter that adjusts them) change as samples arrive and
// leave; the app likewise keeps a scroll margin on both sides of its window.
const MARGIN = 2;
// How far past the next sample the scroll runs on, in samples. A sample whose
// render commits late (a page of many charts takes a while) then finds the
// curve still moving, instead of stopped at the end of its step and jumping on.
// It runs into the hidden margin, whose last tangent may still change.
const OVERRUN = 0.25;
/**
 * A sample has arrived. Its scroll begins one interval after the last one's,
 * however late the timer fired: began at the firing, the curves would step
 * back by the lateness. A tenth of the lateness is taken in, so the clock
 * keeps up with a timer that runs slow; a sample later than the overrun
 * covers carries on from where the scroll stopped.
 */
export function advanceClock(clock: SampleClock, interval: number) {
  const now = performance.now();
  if (clock.rest) clock.at = now;
  else {
    const late = now - (clock.at + interval);
    clock.at = Math.max(clock.at + interval + Math.max(0, late) * 0.1, now - OVERRUN * interval);
  }
  clock.rest = false;
}
/** How far into its step a curve on this clock is, in ms. */
const elapsed = (clock: SampleClock, interval: number) =>
  Math.min(interval * (1 + OVERRUN), clock.rest ? interval : Math.max(0, performance.now() - clock.at));
// The dash pattern of a dashed series (4 on, 3 off), in pixels.
const DASH = [4, 3];

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
// The drawn length of the first `count` segments, at sx and sy pixels per unit.
function lengthTo(ys: number[], m: number[], count: number, sx: number, sy: number) {
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
  /** The clock of this curve's samples, if not the window's (sampleClock). */
  clock?: SampleClock;
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
  clock = sampleClock,
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
    const animation = el.animate([{ transform: at(0) }, { transform: at(1 + OVERRUN) }], {
      duration: interval * (1 + OVERRUN),
      easing: "linear",
      fill: "forwards",
    });
    // A curve shown between samples joins the scroll where the others are;
    // until the first sample after a pause they rest at the end of the step.
    animation.currentTime = elapsed(clock, interval);
    if (clock.rest) animation.pause();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tick, motion, running, interval, drawn, margin]);
  // How far the scroll has carried the track, in samples.
  const phase = () => {
    const time = track.current?.getAnimations()[0]?.currentTime;
    return typeof time === "number" ? Math.min(1 + OVERRUN, time / interval) : 1;
  };

  // Dashes keep their place on the curve as it scrolls (the app keeps a dash
  // phase per series): each sample re-draws the path from one sample later,
  // so its pattern starts that much further along.
  const dashed = useRef<(SVGPathElement | null)[]>([]);
  const dash = useRef<{ tick: number; lines: typeof lines; offsets: number[] } | null>(null);
  useLayoutEffect(() => {
    if (!series.some((s) => s.dashed)) return;
    const outer = box.current;
    const last = dash.current;
    const offsets = lines.map((_, i) => last?.offsets[i] ?? 0);
    if (outer && last && last.lines !== lines && tick === last.tick + 1) {
      // Pixels per unit, as the stroke is drawn: in the curve's own box, before the demo's scale.
      const sx = outer.offsetWidth / shown,
        sy = outer.offsetHeight / 100;
      const period = DASH[0] + DASH[1];
      lines.forEach((line, i) => {
        const before = last.lines[i];
        if (!before) return;
        // Measured to the first sample shown, past the ends whose tangents differ.
        const moved = lengthTo(before.ys, before.m, margin + 1, sx, sy) - lengthTo(line.ys, line.m, margin, sx, sy);
        offsets[i] = (((offsets[i] + moved) % period) + period) % period;
      });
    }
    dash.current = { tick, lines, offsets };
    offsets.forEach((offset, i) => dashed.current[i]?.setAttribute("stroke-dashoffset", offset.toFixed(2)));
  }, [lines, tick, series, margin, shown]);

  // The readout: a line at the pointer, a dot on each curve that rides it every
  // frame, and text that reads the curve ten times a second, and at once when
  // the pointer moves or a sample arrives. It is placed outside React's
  // rendering: in the event that moves the pointer, on each frame, and after a
  // sample's commit has restarted the scroll. A render reads the scroll before
  // that restart, and would draw the dots a step off for a frame.
  const [hovering, setHovering] = useState(false);
  const pointer = useRef(0);
  const readoutBox = useRef<HTMLDivElement>(null);
  const tip = useRef<HTMLDivElement>(null);
  const age = useRef<HTMLSpanElement>(null);
  const dots = useRef<(HTMLElement | null)[]>([]);
  const values = useRef<(HTMLElement | null)[]>([]);
  const lastRead = useRef(0);
  const place = useRef<(read: boolean) => void>(() => {});
  useLayoutEffect(() => {
    place.current = (read) => {
      const el = readoutBox.current;
      if (!el || !readout) return;
      const fraction = pointer.current;
      // While the readout shows, this script places the scroll too, on the same
      // frame as the dots: on the compositor it would run ahead of them in any
      // frame the page is busy, and the dots would wobble on the line.
      const scroll = track.current?.getAnimations()[0];
      if (scroll) {
        if (scroll.playState === "running") scroll.pause();
        scroll.currentTime = elapsed(clock, interval);
      }
      const x = margin + phase() + fraction * shown;
      el.style.setProperty("--x", String(fraction));
      tip.current?.classList.toggle("is-flipped", fraction > 0.5);
      const heights = lines.map((line) => heightAt(line.ys, line.m, x));
      // Moved on their own layer by a fraction of a pixel at a time: placed by
      // `top`, a dot would be drawn on whole pixels and wobble against the line.
      const box = el.offsetHeight;
      heights.forEach((h, i) => {
        const dot = dots.current[i];
        if (dot) dot.style.transform = `translateY(${((h / 100) * box).toFixed(2)}px)`;
      });
      const now = performance.now();
      if (!read && now - lastRead.current < 100) return;
      lastRead.current = now;
      const seconds = ago(((1 - fraction) * shown * interval) / 1000);
      if (age.current) age.current.textContent = translate(seconds === "0" ? "最新" : `${seconds} 秒前`);
      heights.forEach((h, i) => {
        const value = values.current[i];
        if (value) value.textContent = translate(readout.format(((100 - h) / 100) * max));
      });
    };
    place.current(true);
  });
  useEffect(() => {
    if (!hovering) return;
    let frame = requestAnimationFrame(function ride() {
      place.current(false);
      frame = requestAnimationFrame(ride);
    });
    return () => {
      cancelAnimationFrame(frame);
      // The compositor takes the scroll back from where it is now.
      const scroll = track.current?.getAnimations()[0];
      if (scroll?.playState === "paused" && !clock.rest) {
        scroll.currentTime = elapsed(clock, interval);
        scroll.play();
      }
    };
  }, [hovering, interval, clock]);

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
              pointer.current = Math.max(0, Math.min(1, (event.clientX - box.left) / box.width));
              if (hovering) place.current(true);
              else setHovering(true);
            }
          : undefined
      }
      onPointerLeave={readout ? () => setHovering(false) : undefined}
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
                  ref={(el) => {
                    dashed.current[i] = s.dashed ? el : null;
                  }}
                  d={lines[i].path}
                  fill="none"
                  stroke={s.color}
                  strokeWidth="1.5"
                  strokeDasharray={s.dashed ? DASH.join(" ") : undefined}
                  strokeOpacity={s.faint ? 0.55 : undefined}
                  vectorEffect="non-scaling-stroke"
                  strokeLinejoin="round"
                />
              </g>
            ))}
          </svg>
        </div>
      </div>
      {readout && hovering && (
        <div className="curve-readout" ref={readoutBox} aria-hidden="true">
          <i className="curve-cursor" />
          {lines.map((_, i) => (
            <i
              key={i}
              className="curve-dot"
              ref={(el) => {
                dots.current[i] = el;
              }}
              style={{ background: series[i].color }}
            />
          ))}
          <div className="curve-tip" ref={tip}>
            <span className="curve-age" ref={age} />
            {lines.map((_, i) => (
              <span className="curve-row" key={i}>
                <i className={"curve-swatch" + (series[i].dashed ? " is-dashed" : "")} style={{ color: series[i].color }} />
                {readout.names?.[i] && <span>{readout.names[i]}</span>}
                <b
                  ref={(el) => {
                    values.current[i] = el;
                  }}
                />
              </span>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
