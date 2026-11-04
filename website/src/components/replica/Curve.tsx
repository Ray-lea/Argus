/** @jsxImportSource @/components/replica/i18n */
"use client";
import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { DASH, MARGIN, OVERRUN, heightAt, pathOf, tangents, type Frame, type Geometry, type Message } from "./curve-math";
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
// Seconds as the app's readout shows them: tenths below ten.
const ago = (seconds: number) => (seconds < 10 ? String(Math.round(seconds * 10) / 10) : String(Math.round(seconds)));

/*
 * The curves are drawn by a worker (curve.worker.ts), each on a canvas laid
 * on whole device pixels. One that scrolls is drawn again on every display
 * frame where its clock puts it, as the app draws its charts. A texture moved
 * by the compositor instead would show the line's antialiasing and the clip
 * at its ends shifting from frame to frame, and its scroll, restarted for
 * every sample, could start a frame late and step back.
 */
let painter: Worker | null | undefined;
const charts = new Map<number, { repaint: () => void; drawn: () => void }>();
const given = new WeakMap<HTMLCanvasElement, number>();
const leaving = new Map<number, ReturnType<typeof setTimeout>>();
let ids = 0;
function getPainter() {
  if (painter !== undefined) return painter;
  painter = null;
  try {
    if (typeof OffscreenCanvas === "undefined" || !("transferControlToOffscreen" in HTMLCanvasElement.prototype)) return null;
    const worker = new Worker(new URL("./curve.worker.ts", import.meta.url), { type: "module" });
    worker.onmessage = ({ data }) => {
      if (data?.kind === "drawn") charts.get(data.id)?.drawn();
    };
    worker.onerror = () => {
      painter = null;
    };
    painter = worker;
    // The site's theme changes the curves' colours without a render.
    new MutationObserver(() => charts.forEach((c) => c.repaint())).observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class", "style", "data-theme"],
    });
  } catch {
    painter = null;
  }
  return painter;
}
const post = (message: Message, transfer: Transferable[] = []) => painter?.postMessage(message, transfer);
// A CSS colour as [r, g, b, a], read once per colour.
const colors = new Map<string, number[]>();
let probe: CanvasRenderingContext2D | null = null;
function rgba(css: string) {
  const known = colors.get(css);
  if (known) return known;
  probe ??= document.createElement("canvas").getContext("2d");
  if (!probe) return [0, 0, 0, 1];
  probe.fillStyle = "#000";
  probe.fillStyle = css;
  const value = String(probe.fillStyle);
  const hex = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(value);
  const numbers = (value.match(/[\d.]+/g) ?? []).map(Number);
  const parts = hex
    ? [parseInt(hex[1], 16), parseInt(hex[2], 16), parseInt(hex[3], 16), 1]
    : value.startsWith("color(")
      ? [...numbers.slice(0, 3).map((n) => Math.round(n * 255)), numbers[3] ?? 1]
      : numbers.concat(1).slice(0, 4);
  colors.set(css, parts);
  return parts;
}

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
  const box = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const ring = useRef<HTMLElement>(null);
  const strokes = useRef<(SVGPathElement | null)[]>([]);
  const chart = useRef<number | null>(null);
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
  // The curve scrolls unless it is held, stepped or motion is reduced. Held
  // while it scrolls, it stays where it is (the worker keeps it there too).
  const still = () => clock.rest || !running || motion !== "smooth" || matchMedia("(prefers-reduced-motion: reduce)").matches;
  const held = useRef<number | null>(null),
    moved = useRef(false);
  const phase = () => (still() ? (held.current ?? 1) : elapsed(clock, interval) / interval);

  // The demo is drawn scaled, so a curve's box seldom starts or ends on a
  // whole screen pixel. The canvas lies on whole device pixels around the box,
  // with room for the readout's dots beyond its edges, and the curve is cut
  // exactly at the box's edges: the column the edge crosses shows the curve
  // in the part inside the box, next to the border in the rest (cut at the
  // first whole pixel inside, a column of background showed between them).
  // Each sample re-renders the curve, so a move that changes no size is caught
  // within one interval.
  const geometry = useRef("");
  const snap = useCallback(() => {
    const outer = box.current,
      surface = canvas.current;
    if (!outer || !surface || !outer.offsetWidth || chart.current === null) return;
    const r = outer.getBoundingClientRect(),
      ratio = devicePixelRatio,
      k = r.width / outer.offsetWidth;
    const left = r.left * ratio,
      right = r.right * ratio,
      top = r.top * ratio,
      bottom = r.bottom * ratio;
    const scale = k * ratio,
      room = Math.ceil(7 * scale);
    const x = Math.floor(left) - room,
      y = Math.floor(top) - room;
    const g: Geometry = {
      width: Math.ceil(right) + room - x,
      height: Math.ceil(bottom) + room - y,
      left: left - x,
      top: top - y,
      boxWidth: right - left,
      boxHeight: bottom - top,
      clipLeft: left - x,
      clipRight: right - x,
      scale,
    };
    const key = JSON.stringify(g);
    if (key === geometry.current) return;
    geometry.current = key;
    const css = (px: number) => (px / scale).toFixed(4) + "px";
    Object.assign(surface.style, { left: css(x - left), top: css(y - top), width: css(g.width), height: css(g.height) });
    post({ kind: "geometry", id: chart.current, geometry: g });
  }, []);
  // The latest data, its clock and colours, sent together.
  const send = useRef(() => {});
  useLayoutEffect(() => {
    send.current = () => {
      const moving = !still();
      if (moving) held.current = null;
      else held.current ??= moved.current ? elapsed(clock, interval) / interval : 1;
      moved.current = moving;
      if (chart.current === null) return;
      const frame: Frame = {
        series: series.map((s, i) => ({
          ys: lines[i].ys,
          color: rgba(getComputedStyle(strokes.current[i] ?? document.body).stroke),
          fill: s.fill !== false,
          faint: !!s.faint,
          dashed: !!s.dashed,
        })),
        drawn,
        shown,
        margin,
        interval,
        at: still() ? null : performance.timeOrigin + clock.at,
        tick,
        ring: rgba(ring.current ? getComputedStyle(ring.current).color : "#000"),
      };
      post({ kind: "frame", id: chart.current, frame });
    };
  });
  useLayoutEffect(() => {
    snap();
    send.current();
  });
  useEffect(() => {
    const outer = box.current,
      surface = canvas.current;
    if (!outer || !surface) return;
    const worker = getPainter();
    let number: number | undefined;
    if (worker) {
      number = given.get(surface);
      if (number === undefined) {
        number = ++ids;
        given.set(surface, number);
        const offscreen = surface.transferControlToOffscreen();
        post({ kind: "add", id: number, canvas: offscreen }, [offscreen]);
      } else clearTimeout(leaving.get(number));
      chart.current = number;
      geometry.current = "";
      charts.set(number, {
        repaint: () => send.current(),
        // The first frame on the canvas: the server's drawing gives way to it.
        drawn: () => outer.setAttribute("data-live", ""),
      });
      snap();
      send.current();
    }
    const observer = new ResizeObserver(snap);
    observer.observe(outer);
    addEventListener("resize", snap);
    // So does the demo's own theme or accent, on its window.
    const restyle = new MutationObserver(() => send.current());
    const app = outer.closest(".argus-app");
    if (app) restyle.observe(app, { attributes: true, attributeFilter: ["class", "style", "data-mode"] });
    const sight = new IntersectionObserver(([entry]) => {
      if (number !== undefined) post({ kind: "visible", id: number, visible: entry.isIntersecting });
    });
    sight.observe(outer);
    return () => {
      observer.disconnect();
      restyle.disconnect();
      sight.disconnect();
      removeEventListener("resize", snap);
      if (number === undefined) return;
      const gone = number;
      charts.delete(gone);
      chart.current = null;
      // Re-mounted at once (React's strict mode), the canvas stays with the worker.
      leaving.set(
        gone,
        setTimeout(() => post({ kind: "remove", id: gone })),
      );
    };
  }, [snap]);

  // The readout: a line at the pointer, a dot on each curve and text that
  // reads the curve ten times a second, and at once when the pointer moves or
  // a sample arrives. The worker draws the dots with the curve; the text reads
  // the same clock.
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
      const x = margin + phase() + fraction * shown;
      el.style.setProperty("--x", String(fraction));
      tip.current?.classList.toggle("is-flipped", fraction > 0.5);
      const heights = lines.map((line) => heightAt(line.ys, line.m, x));
      // Without the worker, the dots are placed here, on their own layer.
      const height = el.offsetHeight;
      heights.forEach((h, i) => {
        const dot = dots.current[i];
        if (dot) dot.style.transform = `translateY(${((h / 100) * height).toFixed(2)}px)`;
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
    return () => cancelAnimationFrame(frame);
  }, [hovering]);
  const point = (at: number | null) => chart.current !== null && post({ kind: "pointer", id: chart.current, at });

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
              point(pointer.current);
              if (hovering) place.current(true);
              else setHovering(true);
            }
          : undefined
      }
      onPointerLeave={
        readout
          ? () => {
              point(null);
              setHovering(false);
            }
          : undefined
      }
    >
      {/* The server's drawing, and the curve where no worker can draw it. */}
      <div className="curve-clip">
        <div className="curve-track" style={{ width: `${(drawn / shown) * 100}%`, transform: at(1) }}>
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
                    strokes.current[i] = el;
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
      <canvas className="curve-canvas" ref={canvas} aria-hidden="true" />
      <i className="curve-ring" ref={ring} aria-hidden="true" />
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
