/** @jsxImportSource @/components/replica/i18n */
"use client";
import { useMemo, useEffect, useRef, useState, type PointerEvent } from "react";
import { bytes, processes, rate, readings, type Reading } from "./model";
import { byMetric, formatMetric, type Metric } from "./metrics";
import HistoryRaster from "./HistoryRaster";
import { readingTraces } from "./historySeries";
import { useShared } from "./store";
import { asleep, stamp, type Span } from "./historyTime";
export type Lane = { id: number; key: string; overlay?: string };
export function pathFor(values: number[], top: number) {
  if (values.length === 1) return "M0 " + (100 - (98 * values[0]) / top) + "H1000";
  const y = (v: number) => (100 - (98 * v) / top).toFixed(2);
  return values
    .map((v, i) => (i ? "H" + ((1000 * i) / (values.length - 1)).toFixed(2) + "V" + y(v) : "M0 " + y(v)))
    .join("");
}
export function Plot({
  lane,
  program,
  span,
  anchor,
  grid,
  animate,
  pace,
  times,
  at,
  onPick,
  onHoverTime,
  onClear,
  selection,
  onDrag,
  onZoom,
  onLive,
  onReset,
}: {
  lane: Lane;
  program: string | null;
  span: Span;
  anchor: number | null;
  grid: number[];
  animate: boolean;
  pace: string;
  times: number[];
  at: number | null;
  onPick: (n: number) => void;
  onHoverTime: (t: number | null) => void;
  onClear: () => void;
  selection: Span | null;
  onDrag: (a: number, b: number) => void;
  onZoom: (x: number, delta: number, pan: boolean) => void;
  onLive: () => void;
  onReset: () => void;
}) {
  const { historyBand } = useShared();
  const el = useRef<HTMLDivElement>(null);
  const down = useRef<number | null>(null);
  const [hover, setHover] = useState<number | null>(null);
  const lines = useMemo(
    () =>
      [byMetric[lane.key], ...(lane.overlay ? [byMetric[lane.overlay]] : [])].flatMap((m, i) =>
        readingTraces(m, times, historyBand).map((line) => ({ ...line, m, fill: i ? 0 : line.fill })),
      ),
    [lane, times, historyBand],
  );
  const plotLines = useMemo(() => {
    const metric = ({ cpu: "cpu", mem: "mem", gpu: "gpu", net: "net", disk: "io" } as Record<string, keyof Reading>)[
      lane.key
    ];
    if (!program || !metric) return lines;
    const ids = processes.filter((p) => p.exe === program).map((p) => p.pid);
    const values = times.map((t) => {
      if (asleep(t)) return NaN;
      const frame = readings(t, processes);
      return ids.reduce((sum, id) => {
        const r = frame.get(id);
        return sum + (r ? (lane.key === "disk" ? r.read + r.write : r[metric]) : 0);
      }, 0);
    });
    return [...lines, { ...lines[0], values, low: undefined, high: undefined, dash: true, fill: 0, opacity: 0.9 }];
  }, [program, lane.key, lines, times]);
  const drift = () => (anchor === null ? 0 : Math.min(2, Math.max(0, (performance.now() - anchor) / 1000)));
  useEffect(() => {
    const node = el.current;
    const wheel = (e: WheelEvent) => {
      e.preventDefault();
      const b = node!.getBoundingClientRect();
      onZoom((e.clientX - b.left) / b.width, e.deltaY, e.shiftKey);
    };
    node?.addEventListener("wheel", wheel, { passive: false });
    return () => node?.removeEventListener("wheel", wheel);
  }, [onZoom]);
  const fraction = (e: PointerEvent<HTMLDivElement>) => {
    const b = e.currentTarget.getBoundingClientRect();
    return Math.max(0, Math.min(1, (e.clientX - b.left) / b.width));
  };
  const index = (x: number) =>
    Math.max(
      0,
      Math.min(
        times.length - 1,
        Math.floor((span[0] + drift() + x * (span[1] - span[0]) - times[0]) / (times[1] - times[0])),
      ),
    );
  const mark = hover ?? (at === null ? null : (at - span[0]) / (span[1] - span[0]));
  const i = mark === null ? -1 : index(mark);
  const hovered = hover === null ? null : (times[i] ?? null);
  useEffect(() => {
    if (hover !== null) onHoverTime(hovered);
  }, [hover, hovered, onHoverTime]);
  const type = ({ cpu: "cpu", mem: "mem", gpu: "gpu", net: "net", disk: "io" } as Record<string, keyof Reading>)[
    lane.key
  ];
  const top = useMemo(
    () =>
      i < 0 || i >= times.length || asleep(times[i]) || !type
        ? []
        : [...readings(times[i], processes)].sort((a, b) => b[1][type] - a[1][type]).slice(0, 5),
    [i, times, type],
  );
  return (
    <div
      className="h-plot-wrap"
      style={{ "--right-axis": lane.overlay ? lines.at(-1)!.m.color : "var(--a-ink-3)" } as React.CSSProperties}
    >
      {[lines[0], lines.at(-1)!].map((line, side) => (
        <div key={side} className={"h-y-axis side-" + side}>
          {[1, 0.8, 0.6, 0.4, 0.2, 0].map((n) => (
            <span key={n}>{formatMetric(line.m.unit, line.min + (line.max - line.min) * n)}</span>
          ))}
        </div>
      ))}
      <div
        className="a-history-plot"
        ref={el}
        role="slider"
        tabIndex={0}
        aria-label={byMetric[lane.key].name}
        aria-valuemin={times[0]}
        aria-valuemax={times.at(-1)}
        aria-valuenow={at ?? times.at(-1)}
        onPointerDown={(e) => {
          if (e.button === 0) {
            down.current = fraction(e);
            e.currentTarget.setPointerCapture(e.pointerId);
          }
        }}
        onPointerMove={(e) => setHover(fraction(e))}
        onPointerLeave={() => {
          setHover(null);
          onHoverTime(null);
        }}
        onPointerCancel={() => {
          down.current = null;
        }}
        onPointerUp={(e) => {
          if (down.current === null) return;
          const x = fraction(e),
            a = times[index(down.current)],
            b = times[index(x)];
          down.current = null;
          if (a === b && times[1] - times[0] >= 10) onDrag(b, b + times[1] - times[0]);
          else if (a === b) onPick(b);
          else onDrag(Math.min(a, b), Math.max(a, b));
        }}
        onClick={(e) => {
          if (!e.isTrusted) {
            const box = e.currentTarget.getBoundingClientRect();
            const x = Math.max(0, Math.min(1, (e.clientX - box.left) / box.width));
            onPick(times[index(x)]);
          }
        }}
        onDoubleClick={onReset}
        onContextMenu={(e) => {
          e.preventDefault();
          onLive();
        }}
        onKeyDown={(e) => {
          const step = times[1] - times[0],
            pos = at ?? times.at(-1)!;
          if (e.key === "End") onLive();
          else if (e.key === "Home") onPick(Math.ceil(span[0]));
          else if (e.key === "Escape") onClear();
          else if (e.key === "ArrowLeft" || e.key === "ArrowRight")
            onPick(
              Math.max(
                times[0],
                Math.min(times.at(-1)!, pos + (e.key === "ArrowLeft" ? -1 : 1) * step * (e.shiftKey ? 10 : 1)),
              ),
            );
          else if (e.key === "+" || e.key === "-") onZoom(0.5, e.key === "+" ? -1 : 1, false);
          else if (e.key === "0") onReset();
          else return;
          e.preventDefault();
        }}
      >
        <HistoryRaster
          times={times}
          span={span}
          traces={plotLines}
          grid={grid}
          anchor={anchor}
          layerKey={lane.key + "/" + (lane.overlay ?? "")}
          motion={animate}
          pace={pace}
        />
        {selection && (
          <div
            className="h-time-selection"
            style={{
              left: (100 * (selection[0] - span[0])) / (span[1] - span[0]) + "%",
              width: (100 * (selection[1] - selection[0])) / (span[1] - span[0]) + "%",
            }}
          />
        )}
        {i >= 0 && i < times.length && (
          <>
            <i className="a-history-cursor" style={{ left: mark! * 100 + "%" }} />
            {!asleep(times[i]) &&
              lines.map((line, j) => (
                <i
                  key={j}
                  className="h-point"
                  style={{
                    left: mark! * 100 + "%",
                    top: 100 - (98 * (line.values[i] - line.min)) / (line.max - line.min) + "%",
                    background: line.m.color,
                  }}
                />
              ))}
            {hover !== null && (
              <div
                className="h-tip"
                style={{ [hover > 0.62 ? "right" : "left"]: (hover > 0.62 ? 1 - hover : hover) * 100 + "%" }}
              >
                <b>{stamp(times[i])}</b>
                {asleep(times[i]) ? (
                  <span>睡眠</span>
                ) : (
                  lines.map((l, j) => (
                    <div key={j}>
                      {l.m.name} {formatMetric(l.m.unit, l.values[i])}
                    </div>
                  ))
                )}
                {top.map(([pid, r]) => (
                  <div key={pid}>
                    {processes.find((p) => p.pid === pid)?.exe}{" "}
                    {type === "mem"
                      ? bytes(r.mem)
                      : type === "io" || type === "net"
                        ? rate(r[type])
                        : r[type].toFixed(1) + "%"}
                  </div>
                ))}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}

export function Navigator({
  span,
  extent,
  times,
  anchor,
  animate,
  pace,
  metric,
  change,
  reset,
}: {
  span: Span;
  extent: Span;
  times: number[];
  anchor: number | null;
  animate: boolean;
  pace: string;
  metric: Metric;
  change: (s: Span) => void;
  reset: () => void;
}) {
  const [a, b] = extent,
    length = b - a;
  const traces = useMemo(() => readingTraces(metric, times, false, true), [metric, times]);
  const pos = (t: number) => Math.max(0, Math.min(100, ((t - a) / length) * 100));
  const drag = (e: PointerEvent<HTMLDivElement>) => {
    const el = e.currentTarget,
      box = el.getBoundingClientRect(),
      x = e.clientX;
    el.setPointerCapture(e.pointerId);
    const edge = (e.target as HTMLElement).dataset.edge;
    const t = a + ((e.clientX - box.left) / box.width) * length,
      w = span[1] - span[0];
    const origin = !edge && (t < span[0] || t > span[1]) ? Math.max(a, Math.min(b - w, t - w / 2)) : span[0];
    if (origin !== span[0]) change([origin, origin + w]);
    const move = (event: globalThis.PointerEvent) => {
      const d = Math.round(((event.clientX - x) / box.width) * length),
        min = Math.min(length, 10);
      if (edge === "left") change([Math.max(a, Math.min(span[1] - min, span[0] + d)), span[1]]);
      else if (edge === "right") change([span[0], Math.min(b, Math.max(span[0] + min, span[1] + d))]);
      else {
        const start = Math.max(a, Math.min(b - (span[1] - span[0]), origin + d));
        change([start, start + span[1] - span[0]]);
      }
    };
    const done = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("lostpointercapture", done);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("lostpointercapture", done);
  };
  return (
    <div className="h-mini-nav" onPointerDown={drag} onDoubleClick={reset}>
      <div className="h-nav-plot">
        <HistoryRaster
          times={times}
          span={extent}
          traces={traces}
          anchor={anchor}
          layerKey={metric.id}
          motion={animate}
          pace={pace}
        />
      </div>
      <div className="h-nav-window" style={{ left: pos(span[0]) + "%", width: pos(span[1]) - pos(span[0]) + "%" }}>
        <i data-edge="left" />
        <i data-edge="right" />
      </div>
    </div>
  );
}
