/** @jsxImportSource @/components/replica/i18n */
"use client";
import { useMemo, useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type PointerEvent } from "react";
import { createPortal } from "react-dom";
import { memory, processes, readings, type Reading } from "./model";
import { byMetric, byteRate, formatMetric, percentText, type Metric } from "./metrics";
import HistoryRaster from "./HistoryRaster";
import { axisLabel, readingTraces, rowsFor, type Trace } from "./historySeries";
import { useShared } from "./store";
import { asleep, edgeLabel, momentLabel, rangeLabel, type Span } from "./historyTime";
import { Icon, ProcessIcon } from "./icons";
import { localRect, scaleOf } from "./widgets";
export type Lane = { id: number; key: string; overlay?: string };
export function pathFor(values: number[], top: number) {
  if (values.length === 1) return "M0 " + (100 - (98 * values[0]) / top) + "H1000";
  const y = (v: number) => (100 - (98 * v) / top).toFixed(2);
  return values
    .map((v, i) => (i ? "H" + ((1000 * i) / (values.length - 1)).toFixed(2) + "V" + y(v) : "M0 " + y(v)))
    .join("");
}
// A process's reading of a kind, as the table and the tooltip rank by it (table.rs `process_value`).
export const processValue = (r: Reading, key: Metric["process"]) =>
  key === "mem" ? r.mem : key === "disk" ? r.read + r.write : key === "net" ? r.net : key === "gpu" ? r.gpu : r.cpu;
export const processText = (key: Metric["process"], v: number) =>
  key === "mem" ? memory(v) : key === "disk" || key === "net" ? byteRate(v) : percentText(v) + "%";
// The highest sample inside the bucket starting at `t`.
export function peakAt(m: Metric, t: number, step: number) {
  let peak = -Infinity;
  for (let i = 0; i < Math.min(12, step); i++) peak = Math.max(peak, m.read(t + Math.floor((i * step) / Math.min(12, step))));
  return peak;
}
// The processes busiest by `key` over the bucket at `t`, summed by name.
function busiest(key: Metric["process"], t: number, step: number) {
  const n = Math.min(4, step),
    sums = new Map<string, number>();
  for (let k = 0; k < n; k++)
    for (const [pid, r] of readings(t + Math.floor((k * step) / n), processes)) {
      const p = processes.find((p) => p.pid === pid)!;
      sums.set(p.exe, (sums.get(p.exe) ?? 0) + processValue(r, key) / n);
    }
  return [...sums].filter(([, v]) => v > 0).sort((a, b) => b[1] - a[1]).slice(0, 5);
}
type Marks = { value: number; at: number }[];
const marks = ([low, high]: Span, rows: number): Marks =>
  Array.from({ length: rows + 1 }, (_, i) => ({ value: high - ((high - low) * i) / rows, at: i / rows }));
function AxisLabels({ m, rows, axis, color, side }: { m: Metric; rows: number; axis: Span; color?: string; side: number }) {
  return (
    <div className={"h-y-axis side-" + side} style={color ? ({ "--right-axis": color } as CSSProperties) : undefined}>
      {marks(axis, rows).map((mark) => (
        <span key={mark.at} style={{ top: mark.at * 100 + "%", marginTop: -8 - 8 * Math.min(1, Math.max(0, (mark.at - 0.94) / 0.06)) }}>
          {axisLabel(m, mark.value, axis[1])}
        </span>
      ))}
    </div>
  );
}
export function Plot({
  lane,
  first,
  program,
  span,
  anchor,
  grid,
  animate,
  pace,
  times,
  pinned,
  dragging,
  onPick,
  onHoverTime,
  onClear,
  selection,
  onDrag,
  onZoom,
  onLive,
  onReset,
  onZoomSelection,
}: {
  lane: Lane;
  first: boolean;
  program: string | null;
  span: Span;
  anchor: number | null;
  grid: number[];
  animate: boolean;
  pace: string;
  times: number[];
  pinned: number | null;
  dragging: boolean;
  onPick: (n: number) => void;
  onHoverTime: (t: number | null) => void;
  onClear: () => void;
  selection: Span | null;
  onDrag: (a: number, b: number) => void;
  onZoom: (x: number, delta: number, pan: boolean, keys?: boolean) => void;
  onLive: () => void;
  onReset: () => void;
  onZoomSelection: () => void;
}) {
  const { historyBand } = useShared();
  const el = useRef<HTMLDivElement>(null);
  const down = useRef<number | null>(null);
  const [hover, setHover] = useState<{ x: number; client: [number, number] } | null>(null);
  const [box, setBox] = useState({ w: 1000, h: 200 });
  useLayoutEffect(() => {
    const node = el.current!;
    const measure = () => setBox((b) => (b.w === node.offsetWidth && b.h === node.offsetHeight ? b : { w: node.offsetWidth, h: node.offsetHeight }));
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(node);
    return () => ro.disconnect();
  }, []);
  const primary = byMetric[lane.key],
    overlay = lane.overlay ? byMetric[lane.overlay] : null;
  const shared = !!overlay && overlay.unit === primary.unit;
  const step = times[1] - times[0];
  const { lines, left, right } = useMemo(() => {
    const own = readingTraces(primary, times, historyBand);
    const second = overlay ? readingTraces(overlay, times, false).map((t) => ({ ...t, fill: 0 })) : [];
    let left: Span = [own[0].min, own[0].max],
      right: Span | null = second.length ? [second[0].min, second[0].max] : null;
    if (shared && right) left = right = [Math.min(left[0], right[0]), Math.max(left[1], right[1])];
    const put = (list: Trace[], axis: Span) => list.map((t) => ({ ...t, min: axis[0], max: axis[1] }));
    return {
      lines: [...put(own, left).map((t) => ({ ...t, m: primary })), ...(right ? put(second, right).map((t) => ({ ...t, m: overlay! })) : [])],
      left,
      right,
    };
  }, [primary, overlay, shared, times, historyBand]);
  const rows = rowsFor(primary, left, box.h);
  const plotLines = useMemo(() => {
    const key = primary.process;
    if (!program || !key) return lines;
    const ids = processes.filter((p) => p.exe === program).map((p) => p.pid);
    const values = times.map((t) => {
      if (asleep(t)) return NaN;
      const frame = readings(t, processes);
      return ids.reduce((sum, id) => sum + (frame.has(id) ? processValue(frame.get(id)!, key) : 0), 0);
    });
    return [
      ...lines,
      { ...lines[0], values, low: undefined, high: undefined, dash: true, fill: 0, opacity: 0.6, color: "var(--a-ink)" },
    ];
  }, [program, primary, lines, times]);
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
    Math.max(0, Math.min(times.length - 1, Math.floor((span[0] + drift() + x * (span[1] - span[0]) - times[0]) / step)));
  const x = hover?.x ?? null;
  const i = x === null ? -1 : index(x);
  const hovered = x === null ? null : (times[i] ?? null);
  useEffect(() => {
    if (hover !== null) onHoverTime(hovered);
  }, [hover, hovered, onHoverTime]);
  const at = (t: number) => ((t - span[0]) / (span[1] - span[0])) * 100;
  // Gaps the line breaks for, named where there is room.
  const gaps = useMemo(() => {
    const out: Span[] = [];
    for (const t of times) {
      if (!asleep(t)) continue;
      const last = out.at(-1);
      if (last && last[1] === t) last[1] = t + step;
      else out.push([t, t + step]);
    }
    return out;
  }, [times, step]);
  const chip = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const c = chip.current;
    if (!c || !selection) return;
    const mid = ((at(selection[0]) + at(selection[1])) / 200) * box.w,
      w = c.offsetWidth;
    c.style.left = Math.min(Math.max(4, mid - w / 2), Math.max(4, box.w - w - 4)) + "px";
  });
  return (
    <div className="h-plot-wrap">
      <AxisLabels m={primary} rows={rows} axis={left} side={0} />
      <div
        className="a-history-plot"
        ref={el}
        role="slider"
        tabIndex={0}
        aria-label={primary.long}
        aria-valuemin={times[0]}
        aria-valuemax={times.at(-1)}
        aria-valuenow={pinned ?? times.at(-1)}
        onPointerDown={(e) => {
          if (e.button === 0) {
            down.current = fraction(e);
            e.currentTarget.setPointerCapture(e.pointerId);
          }
        }}
        onPointerMove={(e) => setHover({ x: fraction(e), client: [e.clientX, e.clientY] })}
        onPointerLeave={() => {
          setHover(null);
          onHoverTime(null);
        }}
        onPointerCancel={() => {
          down.current = null;
        }}
        onPointerUp={(e) => {
          if (down.current === null) return;
          const a = times[index(down.current)],
            b = times[index(fraction(e))];
          down.current = null;
          if (a === b && step > 5) onDrag(b, b + step);
          else if (a === b) onPick(b);
          else onDrag(Math.min(a, b), Math.max(a, b));
        }}
        onClick={(e) => {
          if (!e.isTrusted) {
            const box = e.currentTarget.getBoundingClientRect();
            onPick(times[index(Math.max(0, Math.min(1, (e.clientX - box.left) / box.width)))]);
          }
        }}
        onDoubleClick={onReset}
        onContextMenu={(e) => {
          e.preventDefault();
          onLive();
        }}
        onKeyDown={(e) => {
          const pos = pinned ?? selection?.[0] ?? times.at(-1)!;
          const move = (by: number) => {
            const t = Math.max(times[0], Math.min(times.at(-1)!, pos + by * step));
            if (step > 5) onDrag(t, t + step);
            else onPick(t);
          };
          if (e.key === "End") onLive();
          else if (e.key === "Home") onPick(times.find((t) => t >= span[0]) ?? times[0]);
          else if (e.key === "Escape") onClear();
          else if (e.key === "ArrowLeft" || e.key === "ArrowRight") move((e.key === "ArrowLeft" ? -1 : 1) * (e.shiftKey ? 10 : 1));
          else if (e.key === "PageUp" || e.key === "PageDown") move(e.key === "PageUp" ? -10 : 10);
          else if (e.key === "+" || e.key === "=" || e.key === "-")
            onZoom(pinned === null ? 0.5 : (pinned - span[0]) / (span[1] - span[0]), e.key === "-" ? 1 : -1, false, true);
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
          rows={rows}
          anchor={anchor}
          layerKey={lane.key + "/" + (lane.overlay ?? "")}
          motion={animate}
          pace={pace}
        />
        {gaps.map(([a, b]) =>
          ((Math.min(100, at(b)) - Math.max(0, at(a))) * box.w) / 100 > 58 ? (
            <span key={a} className="h-gap-label" style={{ left: Math.max(0, at(a)) + "%", right: 100 - Math.min(100, at(b)) + "%" }}>
              睡眠
            </span>
          ) : null,
        )}
        {selection && (
          <div
            className="h-time-selection"
            style={{ left: at(selection[0]) + "%", width: at(selection[1]) - at(selection[0]) + "%" }}
          />
        )}
        {first && selection && !dragging && (
          <div className="h-zoom-chip" ref={chip} onPointerDown={(e) => e.stopPropagation()} onPointerUp={(e) => e.stopPropagation()}>
            <button onClick={onZoomSelection}>
              <Icon name="interfaceScale" size={13} />
              放大到所选
            </button>
            <button title="清除游标（Esc）" onClick={onClear}>
              <Icon name="dismiss" size={13} />
            </button>
          </div>
        )}
        {pinned !== null && at(pinned) >= 0 && at(pinned) <= 100 && (
          <>
            <i className="h-pin-line" style={{ left: at(pinned) + "%" }} />
            <button
              className="h-pin"
              style={{ left: at(pinned) + "%" }}
              title="清除游标（Esc）"
              onPointerDown={(e) => e.stopPropagation()}
              onPointerUp={(e) => e.stopPropagation()}
              onClick={onClear}
            >
              <Icon name="mapPin" size={12} />
              <Icon name="dismiss" size={12} />
            </button>
          </>
        )}
        {x !== null && i >= 0 && i < times.length && (
          <>
            <i className="a-history-cursor" style={{ left: x * 100 + "%" }} />
            {!asleep(times[i]) &&
              lines.map((line, j) => (
                <i
                  key={j}
                  className="h-point"
                  style={{
                    left: x * 100 + "%",
                    top: 100 - (100 * (line.values[i] - line.min)) / (line.max - line.min) + "%",
                    background: line.m.color,
                    opacity: line.opacity < 1 ? 0.6 : 1,
                  }}
                />
              ))}
          </>
        )}
      </div>
      {overlay && !shared && right ? (
        <AxisLabels m={overlay} rows={rows} axis={right} color={overlay.color} side={1} />
      ) : (
        <AxisLabels m={primary} rows={rows} axis={left} side={1} />
      )}
      {hover && hovered !== null && !dragging && el.current && (
        <Tip at={hovered} step={step} client={hover.client} host={el.current} metrics={[primary, ...(overlay ? [overlay] : [])]} />
      )}
    </div>
  );
}
// The card by the pointer: when, the lane's readings then, and what made them (tip.rs).
function Tip({ at: t, step, client, host, metrics }: { at: number; step: number; client: [number, number]; host: HTMLElement; metrics: Metric[] }) {
  const frame = host.closest<HTMLElement>(".a-window");
  const card = useRef<HTMLDivElement>(null);
  const k = scaleOf(host),
    win = frame ? localRect(frame) : { left: 0, top: 0, width: 1600, height: 900 };
  const px = client[0] / k - win.left,
    py = client[1] / k - win.top;
  const flip = px + 14 + 300 > win.width - 8;
  useLayoutEffect(() => {
    const c = card.current;
    if (c) c.style.top = Math.max(8, Math.min(py + 16, win.height - 8 - c.offsetHeight)) + "px";
  });
  if (!frame) return null;
  const main = metrics[0];
  const sleeping = asleep(t);
  const head =
    step <= 5 ? [momentLabel(t), step < 5 ? "每秒读数" : "5 秒记录"] : [rangeLabel(t, t + step, step < 60), "平均值"];
  const mean = (get: (t: number) => number) => {
    const n = Math.min(12, step);
    let sum = 0;
    for (let k = 0; k < n; k++) sum += get(t + Math.floor((k * step) / n));
    return sum / n;
  };
  const list = main.process ? busiest(main.process, t, step) : null;
  const programs = main.id === "processes" ? [...new Set(processes.map((p) => p.exe))] : null;
  const counted = programs
    ?.map((exe) => [exe, processes.filter((p) => p.exe === exe).length] as const)
    .sort((a, b) => b[1] - a[1]);
  return createPortal(
    <div className="h-tip" ref={card} style={flip ? { right: win.width - (px - 14) } : { left: px + 14 }}>
      {sleeping ? (
        <>
          <div className="h-tip-head">
            <b translate="no">{momentLabel(t, false)}</b>
          </div>
          <span className="h-tip-gap">无记录：睡眠</span>
        </>
      ) : (
        <>
          <div className="h-tip-head">
            <b translate="no">{head[0]}</b>
            <small>{head[1]}</small>
          </div>
          {metrics.map((m) => {
            const a = mean(m.read),
              b = m.second ? mean(m.second) : null;
            return (
              <div key={m.id}>
                <div className="h-tip-reading">
                  <i style={{ background: m.color }} />
                  <span>{m.long}</span>
                  <b>{formatMetric(m.unit, a + (b ?? 0))}</b>
                </div>
                {b !== null && m.parts && (
                  <small className="h-tip-sub">{`${m.parts[0]} ${formatMetric(m.unit, a)} · ${m.parts[1]} ${formatMetric(m.unit, b)}`}</small>
                )}
                {step > 5 && m.unit === "%" && !m.second && (
                  <small className="h-tip-sub">{`最高 ${formatMetric(m.unit, peakAt(m, t, step))}`}</small>
                )}
              </div>
            );
          })}
          {list && (
            <div className="h-tip-list">
              {Array.from({ length: 5 }, (_, n) => {
                const row = list[n];
                const p = row && processes.find((p) => p.exe === row[0]);
                return row && p ? (
                  <div key={n}>
                    <ProcessIcon glyph={p.glyph} />
                    <span>{row[0]}</span>
                    <small>{processText(main.process, row[1])}</small>
                  </div>
                ) : (
                  <div key={n}>
                    <i />
                    {" "}
                  </div>
                );
              })}
            </div>
          )}
          {counted && (
            <div className="h-tip-list">
              {counted.slice(0, 5).map(([exe, count]) => (
                <div key={exe}>
                  <ProcessIcon glyph={processes.find((p) => p.exe === exe)!.glyph} />
                  <span>{exe}</span>
                  <small>{count}</small>
                </div>
              ))}
              {counted.length > 5 && (
                <div className="h-tip-rest">
                  <i />
                  <span>{`其余 ${counted.length - 5} 个程序`}</span>
                  <small>{counted.slice(5).reduce((n, [, c]) => n + c, 0)}</small>
                </div>
              )}
            </div>
          )}
        </>
      )}
      <small className="h-tip-hint">单击可固定 · 右键单击回到最新 · Esc 清除</small>
    </div>,
    frame,
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
  now,
  following,
  change,
  reset,
  onLatest,
}: {
  span: Span;
  extent: Span;
  times: number[];
  anchor: number | null;
  animate: boolean;
  pace: string;
  metric: Metric;
  now: number;
  following: boolean;
  change: (s: Span) => void;
  reset: () => void;
  onLatest: () => void;
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
    <div className="h-track" data-paused={!following || undefined}>
      <small className="h-nav-start" translate="no">{edgeLabel(a, length, now)}</small>
      <div className="h-mini-nav" onPointerDown={drag} onDoubleClick={reset}>
        <div className="h-nav-plot">
          <HistoryRaster times={times} span={extent} traces={traces} anchor={anchor} layerKey={metric.id} motion={animate} pace={pace} />
        </div>
        <i className="h-nav-shade" style={{ left: 0, width: pos(span[0]) + "%" }} />
        <i className="h-nav-shade" style={{ left: pos(span[1]) + "%", right: 0 }} />
        <div className="h-nav-window" style={{ left: pos(span[0]) + "%", width: pos(span[1]) - pos(span[0]) + "%" }}>
          <i data-edge="left" />
          <i data-edge="right" />
        </div>
      </div>
      {following ? <small className="h-nav-end">现在</small> : <small className="h-nav-end" translate="no">{edgeLabel(b, length, now)}</small>}
      {!following && (
        <button className="h-track-latest" title="继续跟随最新读数（右键单击图表或按 End）" onClick={onLatest}>
          <Icon name="arrowRight" size={13} />
        </button>
      )}
    </div>
  );
}
