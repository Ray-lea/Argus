/** @jsxImportSource @/components/replica/i18n */
"use client";
import { memo, type ComponentProps, type CSSProperties, type ReactNode } from "react";
import Curve, { type Motion, type Readout, type Series } from "./Curve";
import { span } from "./model";

// Pieces the performance views share (Performance.tsx, CpuDetails.tsx).

export interface CurveProps {
  tick: number;
  interval: number;
  running: boolean;
  motion: Motion;
}
// How the hover readout writes each kind of value.
export const percentOf = (v: number) => Math.round(v) + "%";
/** Watts as the app writes them (charts.rs `watts`): 0.012, 7.4, 78. */
export const wattsText = (v: number) => (v > 0 && v < 1 ? v.toFixed(3) : v < 10 ? v.toFixed(1) : String(Math.round(v)));
export const seriesOf = (count: number, f: (t: number) => number, tick: number) =>
  Array.from({ length: count }, (_, i) => f(tick - count + 1 + i));
export function spanLabel(interval: number) {
  const seconds = Math.round((span * interval) / 1000);
  return seconds >= 120 ? `${Math.round(seconds / 60)} 分钟` : `${seconds} 秒`;
}
/** The least whole number at or above `x`, at least 1; above ten, the least multiple of ten (charts.rs `round_ceiling`). */
export function roundCeiling(x: number) {
  if (!(x > 1)) return 1;
  const whole = Math.ceil(x - 1e-9);
  return whole <= 10 ? whole : Math.ceil(x / 10 - 1e-9) * 10;
}
/** "54 – 61 °C": the lowest and highest readings, one value when they agree (telemetry.rs `range`). */
export function rangeText(low: string, high: string, unit: string) {
  return low === high ? `${low} ${unit}` : `${low} – ${high} ${unit}`;
}

/*
 * A curve drawn again only when what it shows changes: a sample, its data or
 * its scale. Each render of a curve measures its box and hands the worker a
 * new frame, so renders of the page for other reasons (a drag, a hover, a
 * menu) leave the curves as they are; a box that changes size measures
 * itself. `moving` renders it along with something that moves it without
 * resizing it (a dragged list row), so that it stays on whole pixels.
 */
const sameSeries = (a: Series[], b: Series[]) =>
  a === b ||
  (a.length === b.length &&
    a.every((s, i) => {
      const t = b[i];
      return (
        s.color === t.color &&
        s.dashed === t.dashed &&
        s.fill === t.fill &&
        s.faint === t.faint &&
        (s.values === t.values || (s.values.length === t.values.length && s.values.every((v, k) => v === t.values[k])))
      );
    }));
type SteadyProps = ComponentProps<typeof Curve> & { moving?: unknown };
export const SteadyCurve = memo(
  function SteadyCurve({ moving: _moving, ...props }: SteadyProps) {
    return <Curve {...props} />;
  },
  (a: SteadyProps, b: SteadyProps) =>
    a.moving === b.moving &&
    a.tick === b.tick &&
    a.max === b.max &&
    a.interval === b.interval &&
    a.running === b.running &&
    a.motion === b.motion &&
    a.label === b.label &&
    a.className === b.className &&
    a.clock === b.clock &&
    // A readout's format is a pure function of the value, remade with each render.
    !a.readout === !b.readout &&
    (a.readout?.names ?? []).join("\n") === (b.readout?.names ?? []).join("\n") &&
    sameSeries(a.series, b.series),
);

/*
 * A performance chart: its name and top above the frame, the span and zero
 * below it. In a card at most 640 px high the captions go inside the frame
 * (replica.css, `@container perf`); they follow the frame in the document so
 * that they are drawn over the curve and under its readout.
 */
export function Chart({
  title,
  top,
  series,
  max,
  curve,
  fill,
  color = "var(--a-cpu)",
  foot = true,
  height,
  min,
  readout = { format: percentOf },
}: {
  title?: ReactNode;
  top?: ReactNode;
  series: Series[];
  max: number;
  curve: CurveProps;
  fill?: boolean;
  color?: string;
  foot?: boolean;
  height?: number | string;
  /** The frame's least height before the view scrolls. */
  min?: number;
  readout?: Readout;
}) {
  return (
    <figure className={"a-chart" + (fill ? " is-fill" : "") + (title !== undefined ? " is-boxed" : "")}>
      <div className="a-chart-frame" style={{ "--c": color, height, minHeight: min } as CSSProperties}>
        <div className="a-grid" aria-hidden="true" />
        <SteadyCurve series={series} max={max} {...curve} label={typeof title === "string" ? title : undefined} readout={readout} />
      </div>
      {title !== undefined && (
        <div className="a-chart-caption is-top">
          <span>{title}</span>
          <span>{top}</span>
        </div>
      )}
      {foot && (
        <div className="a-chart-caption is-foot">
          <span>{spanLabel(curve.interval)}</span>
          <span>0</span>
        </div>
      )}
    </figure>
  );
}

/** A details card: its title, a one-line summary on the right (or a control), and its body. */
export function Card({
  title,
  note,
  summary,
  control,
  alert,
  className,
  children,
}: {
  title: string;
  note?: string;
  summary?: ReactNode;
  control?: ReactNode;
  alert?: boolean;
  className?: string;
  children: ReactNode;
}) {
  return (
    <section className={"a-card" + (alert ? " is-alert" : "") + (className ? " " + className : "")}>
      <div className={"a-card-head" + (control ? " has-control" : "")}>
        <b>
          {title}
          {note && <small className="a-card-note">{note}</small>}
        </b>
        {control ? <div className="a-card-control">{control}</div> : <span>{summary}</span>}
      </div>
      {children}
    </section>
  );
}

/** A reading: small label, larger value with its unit (telemetry.rs `sensor_reading`). */
export function Reading({ label, value, unit, title }: { label: string; value: string; unit?: string; title?: string }) {
  return (
    <div className="a-disk-reading" title={title}>
      <span>{label}</span>
      <b>
        {value}
        {unit && <small>{unit}</small>}
      </b>
    </div>
  );
}

/** A compact table: 12 px, right-aligned figures under a header in the secondary ink (telemetry.rs `sensor_table`). */
export function SensorTable({ head, rows }: { head: ReactNode[]; rows: ReactNode[][] }) {
  return (
    <div className="a-sensor-table">
      <div style={{ gridTemplateColumns: `repeat(${head.length}, minmax(min-content, 1fr))` }}>
        {head.map((cell, i) => (
          <span key={"h" + i} className={"is-head" + (i ? "" : " is-first")}>
            {cell}
          </span>
        ))}
        {rows.flatMap((row, r) =>
          row.map((cell, i) => (
            <span key={r + ":" + i} className={i ? undefined : "is-first"}>
              {cell}
            </span>
          )),
        )}
      </div>
    </div>
  );
}
/** A table's column heading with its unit beside it. */
export const HeadCell = ({ name, unit, title, context }: { name: string; unit?: string; title?: string; context?: string }) => (
  <span className="a-head-cell" title={title} data-i18n={context}>
    {name}
    {unit && <small>{unit}</small>}
  </span>
);
