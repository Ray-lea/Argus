/** @jsxImportSource @/components/replica/i18n */
"use client";
import {
  createContext,
  memo,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { createPortal } from "react-dom";
import { localRect } from "./widgets";
import { Icon } from "./icons";
import { store, useShared } from "./store";
import { Select } from "./widgets";
import { translate } from "./i18n";
import { historyRanges, type HistoryRange, type Process } from "./model";
import { byMetric, metrics, formatMetric, groups, tabMetrics, widest, type Metric } from "./metrics";
import { glide } from "./HistoryRaster";
import { sampleTimes } from "./historySeries";
import motionConfig from "../../../../resources/motion.json";
import { DAY, durationLabel, tickLabel, type Span } from "./historyTime";
import { Navigator, Plot, peakAt, type Lane } from "./HistoryPlot";
import { HistoryTable, WatchPanel } from "./HistoryTable";
import RangeDialog from "./HistoryRangeDialog";
import type { MenuEntry } from "./Pages";
type Choice = "lane" | "more" | "add" | "overlay";
export type HistoryProps = {
  onMenu: (event: React.MouseEvent, items: MenuEntry[]) => void;
  range: HistoryRange;
  tick: number;
  interval: number;
  running: boolean;
  pace: string;
  smooth: boolean;
  animate: boolean;
  cursor: number | null;
  onCursor: (n: number | null) => void;
  onRange: (r: HistoryRange) => void;
  onOpen: (pid: number) => void;
  alive: Process[];
};
// The drop-down's ranges (navigator.rs QUICK); others read as "Custom…".
const quick: HistoryRange[] = ["5m", "3h", "1d", "3d"];
// A value in a slot as wide as the reading's widest, so that nothing moves as it changes.
export function Slot({ unit, children, className }: { unit: string; children: string; className?: string }) {
  return (
    <b className={"h-slot" + (className ? " " + className : "")}>
      <span>{children}</span>
      <span aria-hidden="true">{widest(unit)}</span>
    </b>
  );
}
// The replica re-renders on every sample, but the page follows its own clock
// (`now` below): only the watch card takes the replica's tick, through a
// context, so the charts and the table are not rendered a second time a second.
const Tick = createContext(0);
function LiveWatch({ alive }: { alive: Process[] }) {
  return <WatchPanel tick={useContext(Tick)} alive={alive} />;
}
export default function History(props: HistoryProps) {
  const { tick, onMenu, onOpen, ...rest } = props;
  const latest = useRef({ onMenu, onOpen });
  useLayoutEffect(() => {
    latest.current = { onMenu, onOpen };
  });
  const [calls] = useState(() => ({
    onMenu: (event: React.MouseEvent, items: MenuEntry[]) => latest.current.onMenu(event, items),
    onOpen: (pid: number) => latest.current.onOpen(pid),
  }));
  return (
    <Tick.Provider value={tick}>
      <HistoryPage {...rest} {...calls} />
    </Tick.Provider>
  );
}
const HistoryPage = memo(function HistoryPage({
  range,
  interval,
  running,
  pace,
  smooth,
  animate,
  cursor,
  onCursor,
  onRange,
  onOpen,
  alive,
  onMenu,
}: Omit<HistoryProps, "tick">) {
  const page = useRef<HTMLDivElement>(null);
  const [ratio, setRatio] = useState(0.6);
  const [chooserAnchor, setAnchor] = useState({ left: 0, top: 0, bottom: 0 });
  const [choiceIndex, setChoiceIndex] = useState<number | null>(null);
  const { watch, recording } = useShared();
  const [followSpan, setFollowSpan] = useState<number | null>(null);
  const [rangeName, setRangeName] = useState<string | null>(null);
  const [zoomSpan, setZoomSpan] = useState<number | null>(null);
  const [custom, setCustom] = useState<Span | null>(null),
    [picker, setPicker] = useState(false);
  const [view, setView] = useState<Span | null>(null),
    [selection, setSelection] = useState<Span | null>(null);
  const [lanes, setLanes] = useState<Lane[]>(() => store.get().lanes),
    [active, setActive] = useState(() => store.get().lanes[0].id);
  const [weights, setWeights] = useState<Record<number, number>>({});
  const seq = useRef(Math.max(...lanes.map((l) => l.id)));
  useEffect(() => {
    if (store.get().lanes !== lanes) store.set({ lanes });
  }, [lanes]);
  const [chooser, setChooser] = useState<{ kind: Choice; lane: number } | null>(null),
    [query, setQuery] = useState(""),
    [group, setGroup] = useState("");
  const [drawProgram, setDrawProgram] = useState<string | null>(null);
  const [pinned, setPinned] = useState<{ lane: number; time: number } | null>(null);
  const [hoverTime, setHoverTime] = useState<number | null>(null);
  const [dragging, setDragging] = useState(false);
  const [dropLane, setDropLane] = useState<number | null>(null);
  const [toast, setToast] = useState<{ text: string; at: number } | null>(null);
  const [mode, setMode] = useState<"moment" | "range">("moment");
  const clearCursor = () => {
    setPinned(null);
    setSelection(null);
    setMode("moment");
  };
  const say = (text: string) => setToast({ text, at: Date.now() });
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 1500);
    return () => clearTimeout(t);
  }, [toast]);
  const length = Math.min(historyRanges.find((r) => r.value === range)!.hours * 3600, (recording.days || 30) * DAY);
  const whole = followSpan ?? (custom ? custom[1] - custom[0] : length);
  const duration = view ? view[1] - view[0] : (cursor === null && zoomSpan) || whole;
  const flow =
    cursor === null &&
    running &&
    recording.enabled &&
    duration <= 900 &&
    smooth &&
    animate &&
    !matchMedia("(prefers-reduced-motion: reduce)").matches;
  const lag = (motionConfig.chartLagIntervals * interval + motionConfig.chartLagSlackMs) / 1000;
  const [now, setNow] = useState(() => Math.floor(performance.now() / 1000));
  const cadence = duration > 21600 ? 60 : 1;
  useEffect(() => {
    if (!running || !recording.enabled) return;
    let timer: ReturnType<typeof setTimeout>;
    const update = () => {
      const t = performance.now() / 1000;
      const step = Math.floor(t / cadence) * cadence;
      setNow(step);
      timer = setTimeout(update, Math.max(8, (step + cadence - t) * 1000));
    };
    update();
    return () => clearTimeout(timer);
  }, [running, recording.enabled, cadence]);
  const latest = recording.until ?? now;
  const end = cursor === null ? latest - (flow ? lag : 0) : (view?.[1] ?? cursor);
  const span: Span =
    view ??
    (cursor === null && zoomSpan ? [end - zoomSpan, end] : null) ??
    (followSpan !== null ? [end - followSpan, end] : custom) ?? [end - length, end];
  const [start, stop] = span;
  const anchor = flow ? (stop + lag) * 1000 : null;
  const instant = () => (anchor === null ? 0 : Math.min(2, Math.max(0, (performance.now() - anchor) / 1000)));
  const [width, setWidth] = useState(1200);
  useEffect(() => {
    const parent = page.current!;
    const resize = () =>
      setWidth(Math.max(100, parent.querySelector<HTMLElement>(".a-history-plot")?.offsetWidth ?? parent.clientWidth - 120));
    resize();
    const obs = new ResizeObserver(resize);
    obs.observe(parent);
    return () => obs.disconnect();
  }, []);
  const times = useMemo(() => sampleTimes([start, stop], width), [start, stop, width]);
  const [fixedExtent, setFixedExtent] = useState<Span | null>(null);
  const extent: Span = fixedExtent ?? custom ?? [end - whole, end];
  const trackTimes = useMemo(
    () => (extent[0] === start && extent[1] === stop ? times : sampleTimes(extent, width)),
    [extent[0], extent[1], times, start, stop, width],
  );
  const gridStep =
    [5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200, 10800, 21600, 43200, 86400, 172800, 604800].find(
      (step) => step >= ((stop - start) * 86) / width,
    ) ?? 604800;
  const grid = useMemo(() => {
    const first = Math.ceil((start - gridStep) / gridStep) * gridStep;
    return Array.from({ length: Math.ceil((stop - start) / gridStep) + 4 }, (_, i) => first + i * gridStep);
  }, [start, stop, gridStep]);
  const axis = useRef<HTMLDivElement>(null);
  // The axis's width as its observer reports it: read after each sample's render,
  // it would lay the whole page out again.
  const [axisWidth, setAxisWidth] = useState(0);
  useEffect(() => {
    const el = axis.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setAxisWidth(el.clientWidth));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  useEffect(() => {
    if (!axis.current) return;
    const el = axis.current,
      pps = axisWidth / (stop - start),
      lagged = instant();
    glide(el, -lagged * pps, anchor === null ? 0 : -2 * pps, anchor === null ? 0 : (2 - lagged) * 1000, pace);
  }, [start, stop, anchor, axisWidth, pace]);
  const at = hoverTime ?? pinned?.time ?? (cursor === null ? latest : stop);
  const step = times[1] - times[0];
  const summary = (m: Metric) => formatMetric(m.unit, m.read(Math.floor(at)) + (m.second?.(Math.floor(at)) ?? 0));
  const part = (m: Metric, second: boolean) => formatMetric(m.unit, (second ? m.second! : m.read)(Math.floor(at)));
  // Cursor changes from outside (the page bar's live button) and from the lanes.
  const was = useRef(cursor);
  const shown = useRef(stop - start);
  shown.current = stop - start;
  useEffect(() => {
    if (was.current === cursor) return;
    if (cursor === null) {
      setZoomSpan(shown.current < whole - 1 ? shown.current : null);
      setPinned(null);
      setView(null);
      setFixedExtent(null);
      if (custom) setFollowSpan(custom[1] - custom[0]);
      setCustom(null);
      setSelection(null);
      setMode("moment");
      say("已回到最新");
    } else if (was.current === null) say("已暂停自动更新");
    was.current = cursor;
  }, [cursor]);
  const reset = () => {
    if (cursor === null) {
      setZoomSpan(null);
      setView(null);
      return;
    }
    setView(extent);
    onCursor(extent[1]);
  };
  // A range chosen: the whole of it, following the clock.
  const choose = (follow: number | null, name: string | null = null) => {
    setFixedExtent(null);
    setFollowSpan(follow);
    setRangeName(name);
    setZoomSpan(null);
    setPinned(null);
    setCustom(null);
    setView(null);
    setSelection(null);
    setMode("moment");
    onCursor(null);
  };
  const live = () => onCursor(null);
  const openChooser = (kind: Choice, id: number, button: HTMLElement) => {
    const box = localRect(button);
    setAnchor({ left: box.left, top: box.top, bottom: box.bottom });
    setChoiceIndex(null);
    setActive(id);
    setChooser({ kind, lane: id });
    setQuery("");
    setGroup("");
  };
  const place = (key: string, how: "add" | "primary" | "overlay", id = active) => {
    if (how === "add") {
      const next = ++seq.current;
      setLanes((prev) => [...prev, { id: next, key }]);
      setActive(next);
    } else
      setLanes((prev) =>
        prev.map((l) =>
          l.id !== id
            ? l
            : how === "primary"
              ? { ...l, key, overlay: l.overlay === key ? undefined : l.overlay }
              : { ...l, overlay: key && key !== l.key ? key : undefined },
        ),
      );
    setChooser(null);
  };
  const point = (t: number) => {
    const sub = instant();
    setFixedExtent([extent[0] + sub, extent[1] + sub]);
    setView([start + sub, stop + sub]);
    onCursor(t);
    setPinned({ lane: active, time: t });
    setSelection(null);
    setMode("moment");
  };
  const zoom = (x: number, delta: number, pan: boolean, keys = false) => {
    const phase = instant(),
      a = extent[0] + phase,
      b = extent[1] + phase,
      whole = b - a,
      shown = stop - start;
    const desired = pan ? shown : Math.min(whole, Math.max(Math.min(600, Math.max(30, whole / 5)), shown * (delta > 0 ? 1.4 : 0.7)));
    // The keys zoom about the pin or the middle and keep following.
    if (keys && cursor === null) {
      setZoomSpan(desired < whole - 1 ? desired : null);
      return;
    }
    const from = Math.min(b - desired, Math.max(a, start + phase + (pan ? Math.sign(delta) * shown * 0.1 : x * (shown - desired))));
    if (!keys) setPinned(null);
    setFixedExtent([a, b]);
    setView([from, from + desired]);
    onCursor(keys && pinned ? pinned.time : from + x * desired);
    if (!keys) setSelection(null);
  };
  useEffect(() => {
    if (cursor === null) setPinned(null);
  }, [cursor]);
  // The reading chooser's lines: each group, its readings of no device, then each device's.
  const current = chooser && lanes.find((l) => l.id === chooser.lane);
  const settings = chooser && {
    title: { lane: "", more: "在当前轨道显示", add: "添加轨道", overlay: "叠加第二个读数（右侧纵轴）" }[chooser.kind],
    current: chooser.kind === "overlay" ? current?.overlay : chooser.kind === "add" ? undefined : current?.key,
    exclude:
      chooser.kind === "more"
        ? tabMetrics.map((m) => m.id)
        : chooser.kind === "add"
          ? lanes.map((l) => l.key)
          : chooser.kind === "overlay"
            ? [current?.key]
            : [],
    none: chooser.kind === "overlay" && !!current?.overlay,
    add: chooser.kind === "lane" || chooser.kind === "more",
  };
  const found = (m: Metric) =>
    [m.long, m.name, m.device ?? "", m.part ?? "", translate(m.long), translate(m.group)].join(" ").toLowerCase().includes(query.trim().toLowerCase());
  const offered = metrics.filter((m) => m.listed && !settings?.exclude.includes(m.id));
  type Line = { kind: "group" | "device"; text: string } | { kind: "none" } | { kind: "reading"; m: Metric; indent: boolean };
  const searching = !!query.trim();
  const lines: Line[] = [];
  if (settings?.none && !searching) lines.push({ kind: "none" });
  for (const g of groups) {
    if (!searching && group && group !== g) continue;
    const of = offered.filter((m) => m.group === g && (!searching || found(m)));
    if (!of.length) continue;
    if (searching || !group) lines.push({ kind: "group", text: g });
    for (const m of of.filter((m) => !m.device)) lines.push({ kind: "reading", m, indent: false });
    for (const device of [...new Set(of.filter((m) => m.device).map((m) => m.device!))]) {
      lines.push({ kind: "device", text: device });
      for (const m of of.filter((m) => m.device === device)) lines.push({ kind: "reading", m, indent: true });
    }
  }
  const choices = lines.filter((l) => l.kind === "none" || l.kind === "reading");
  const highlight =
    choiceIndex ??
    Math.max(
      0,
      choices.findIndex((l) => (l.kind === "none" ? !settings?.current : l.kind === "reading" && l.m.id === settings?.current)),
    );
  const pick = (line: Line | undefined, add = false) => {
    if (!line || !chooser) return;
    if (line.kind === "none") place("", "overlay", chooser.lane);
    else if (line.kind === "reading")
      place(line.m.id, add || chooser.kind === "add" ? "add" : chooser.kind === "overlay" ? "overlay" : "primary", chooser.lane);
  };
  const panel = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const el = panel.current,
      frame = page.current?.closest<HTMLElement>(".a-window");
    if (!el || !frame) return;
    const win = localRect(frame),
      w = el.offsetWidth,
      h = el.offsetHeight;
    const left = Math.max(8, Math.min(chooserAnchor.left - win.left, frame.clientWidth - w - 8));
    const below = chooserAnchor.bottom - win.top + 4 + h <= frame.clientHeight - 8 || chooserAnchor.top - win.top - 4 - h < 8;
    el.style.left = left + "px";
    el.style.top = (below ? chooserAnchor.bottom - win.top + 4 : chooserAnchor.top - win.top - 4 - h) + "px";
  });
  const choosable = quick.includes(range) && !custom && followSpan === null;
  const pillName =
    rangeName ??
    (custom
      ? "自定义"
      : followSpan !== null
        ? (historyRanges.find((r) => r.hours * 3600 === followSpan && r.value !== "all")?.label ?? durationLabel(followSpan))
        : historyRanges.find((r) => r.value === range)!.label);
  const multi = lanes.length > 1;
  const lift = (id: number) => (e: React.DragEvent) => {
    e.preventDefault();
    setDropLane(null);
    const k = e.dataTransfer.getData("argus-reading");
    if (byMetric[k]) place(k, "overlay", id);
    const moved = Number(e.dataTransfer.getData("argus-lane"));
    if (moved && moved !== id)
      setLanes((prev) => {
        const next = prev.filter((i) => i.id !== moved);
        next.splice(
          next.findIndex((i) => i.id === id),
          0,
          prev.find((i) => i.id === moved)!,
        );
        return next;
      });
  };
  return (
    <div className="a-page a-history" ref={page} style={{ "--chart-share": ratio * 100 + "%" } as CSSProperties}>
      {watch && <LiveWatch alive={alive} />}
      <div className="h-tabs">
        {tabMetrics.map((m) => {
          const shown = lanes.some((l) => l.key === m.id);
          return (
            <div
              key={m.id}
              draggable
              title="单击可在当前轨道显示；拖到图表上可叠加或新建轨道"
              onDragStart={(e) => e.dataTransfer.setData("argus-reading", m.id)}
              style={{ "--c": m.color } as CSSProperties}
              data-current={shown || undefined}
              data-ring={(multi && lanes.find((l) => l.id === active)?.key === m.id) || undefined}
              data-overlaid={lanes.some((l) => l.overlay === m.id) || undefined}
            >
              <button onClick={() => place(m.id, "primary")}>
                <i />
                {m.name}
                <Slot unit={m.unit}>{summary(m)}</Slot>
              </button>
              <button title="在新轨道中显示" onClick={() => place(m.id, "add")}>
                <Icon name="plus" size={12} />
              </button>
            </div>
          );
        })}
        <span className="h-tools">
          <button onClick={(e) => openChooser("more", active, e.currentTarget)}>
            更多读数 <Icon name="chevronDown" size={13} />
          </button>
          <button title="添加轨道" onClick={(e) => openChooser("add", active, e.currentTarget)}>
            <Icon name="plus" size={13} />
            轨道
          </button>
        </span>
      </div>
      <section className="a-history-card">
        <div className="h-lanes a-scroll">
          {lanes.map((l, n) => {
            const m = byMetric[l.key],
              o = l.overlay ? byMetric[l.overlay] : null;
            const peak = m.unit === "%" && !m.second && step > 5 ? formatMetric("%", peakAt(m, Math.floor((at - times[0]) / step) * step + times[0], step)) : null;
            return [
              n > 0 && (
                <div
                  key={"d" + l.id}
                  className="h-divider"
                  role="separator"
                  onPointerDown={(e) => {
                    const a = lanes[n - 1].id,
                      b = l.id,
                      wa = weights[a] ?? 1,
                      wb = weights[b] ?? 1,
                      y = e.clientY,
                      height = e.currentTarget.parentElement!.getBoundingClientRect().height / lanes.length;
                    const grip = e.currentTarget;
                    grip.setPointerCapture(e.pointerId);
                    const move = (p: globalThis.PointerEvent) => {
                      const d = ((p.clientY - y) / height) * ((wa + wb) / 2);
                      const na = Math.max(0.3, Math.min(wa + wb - 0.3, wa + d));
                      setWeights((w) => ({ ...w, [a]: na, [b]: wa + wb - na }));
                    };
                    const done = () => {
                      grip.removeEventListener("pointermove", move);
                      grip.removeEventListener("lostpointercapture", done);
                    };
                    grip.addEventListener("pointermove", move);
                    grip.addEventListener("lostpointercapture", done);
                  }}
                >
                  <i />
                </div>
              ),
              <section
                key={l.id}
                className="h-lane"
                style={{ "--c": m.color, flexGrow: weights[l.id] ?? 1 } as CSSProperties}
                data-multi={multi || undefined}
                data-active={(multi && active === l.id) || undefined}
                onPointerDown={() => setActive(l.id)}
                onDragOver={(e) => {
                  e.preventDefault();
                  if (e.dataTransfer.types.includes("argus-reading")) setDropLane(l.id);
                }}
                onDragLeave={(e) => {
                  if (!e.currentTarget.contains(e.relatedTarget as Node)) setDropLane(null);
                }}
                onDrop={lift(l.id)}
              >
                <header>
                  {multi && (
                    <span
                      className="h-grip"
                      draggable
                      title="拖动可调整顺序"
                      onDragStart={(e) => e.dataTransfer.setData("argus-lane", String(l.id))}
                    >
                      <Icon name="grip" size={14} />
                    </span>
                  )}
                  <button className="h-primary" onClick={(e) => openChooser("lane", l.id, e.currentTarget)}>
                    <i />
                    {m.long}
                    <Icon name="chevronDown" size={13} />
                  </button>
                  <Slot unit={m.unit}>{summary(m)}</Slot>
                  {m.parts &&
                    m.parts.map((name, k) => (
                      <span key={name} className="h-part" data-second={k === 1 || undefined}>
                        <i />
                        {name}
                        <Slot unit={m.unit}>{part(m, k === 1)}</Slot>
                      </span>
                    ))}
                  {!m.parts && m.unit === "%" && (
                    <small className="h-peak">
                      <span>{peak ? `最高 ${peak}` : ""}</span>
                      <span aria-hidden="true">{`最高 ${widest("%")}`}</span>
                    </small>
                  )}
                  {o ? (
                    <div className="h-overlay" style={{ "--overlay": o.color } as CSSProperties} title="更换叠加的读数">
                      <button onClick={(e) => openChooser("overlay", l.id, e.currentTarget)}>
                        <i />
                        {o.long}
                        <Slot unit={o.unit}>{summary(o)}</Slot>
                        <small>{m.unit === o.unit ? "共用左轴" : "右轴"}</small>
                      </button>
                      <button title="取消叠加" onClick={() => place("", "overlay", l.id)}>
                        <Icon name="dismiss" size={12} />
                      </button>
                    </div>
                  ) : (
                    <button
                      className="h-overlay-add"
                      title="叠加第二个读数，使用右侧纵轴"
                      onClick={(e) => openChooser("overlay", l.id, e.currentTarget)}
                    >
                      <Icon name="plus" size={12} />
                      叠加
                    </button>
                  )}
                  {drawProgram && m.process && (
                    <span className="h-curve-chip">
                      <i />
                      <span>{drawProgram}</span>
                      <button onClick={() => setDrawProgram(null)} aria-label="不在图表上显示">
                        <Icon name="dismiss" size={12} />
                      </button>
                    </span>
                  )}
                  {multi && (
                    <button
                      className="h-remove"
                      title="移除此轨道"
                      onClick={() => {
                        const rest = lanes.filter((o) => o.id !== l.id);
                        setLanes(rest);
                        setActive(rest[0].id);
                      }}
                    >
                      <Icon name="dismiss" size={14} />
                    </button>
                  )}
                </header>
                <Plot
                  lane={l}
                  first={n === 0}
                  program={drawProgram}
                  span={span}
                  anchor={anchor}
                  grid={grid}
                  animate={animate}
                  pace={pace}
                  times={times}
                  pinned={pinned && (pinned.lane === l.id || (n === 0 && !lanes.some((o) => o.id === pinned.lane))) ? pinned.time : null}
                  dragging={dragging}
                  onHoverTime={setHoverTime}
                  onClear={clearCursor}
                  selection={selection}
                  onPick={point}
                  onDrag={(a, b) => {
                    point(b);
                    setPinned(null);
                    setSelection([a, b]);
                    setMode("range");
                  }}
                  onZoom={zoom}
                  onLive={live}
                  onReset={reset}
                  onZoomSelection={() => {
                    if (!selection) return;
                    setView(selection);
                    setSelection(null);
                    setMode("range");
                  }}
                />
                {dropLane === l.id && (
                  <div className="h-drop">
                    <span>
                      <Icon name="layers" size={13} />
                      叠加到此轨道（右侧纵轴）
                    </span>
                  </div>
                )}
              </section>,
            ];
          })}
          {toast && (
            <div className="h-toast" key={toast.at}>
              <span>{toast.text}</span>
            </div>
          )}
        </div>
        <div className="h-axis">
          <div ref={axis}>
            {grid
              .filter((t) => {
                // Labels clear of the ends, as the app paints them.
                const x = ((t - start) / (stop - start)) * width;
                return x > 18 && x < width - 18;
              })
              .map((t) => (
                <span key={t} translate="no" style={{ left: ((t - start) / (stop - start)) * 100 + "%" }}>
                  {tickLabel(t, gridStep)}
                </span>
              ))}
          </div>
        </div>
        <div className="h-navigator">
          <Select
            reselect
            label="时间范围"
            value={choosable ? range : "custom"}
            displayLabel={pillName}
            options={[
              ...quick.map((value) => ({ value, label: historyRanges.find((r) => r.value === value)!.label })),
              { value: "custom", label: "自定义…" },
            ]}
            onChange={(v) => {
              if (v === "custom") setPicker(true);
              else {
                onRange(v as HistoryRange);
                choose(null);
              }
            }}
          />
          <Navigator
            span={span}
            extent={extent}
            times={trackTimes}
            anchor={anchor}
            animate={animate}
            pace={pace}
            metric={byMetric[lanes.find((l) => l.id === active)?.key ?? "cpu"]}
            now={latest}
            following={cursor === null}
            change={(next) => {
              setPinned(null);
              setFixedExtent(extent);
              setView(next);
              onCursor(next[1]);
              setSelection(null);
            }}
            reset={reset}
            onLatest={live}
          />
        </div>
      </section>
      <div
        className="h-split"
        role="separator"
        aria-orientation="horizontal"
        tabIndex={0}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown" || e.key === "ArrowUp") {
            e.preventDefault();
            setRatio((v) => Math.max(0.34, Math.min(0.82, v + (e.key === "ArrowDown" ? 0.03 : -0.03))));
          }
        }}
        onPointerDown={(e) => {
          const grip = e.currentTarget,
            box = page.current!.getBoundingClientRect(),
            start = e.clientY;
          grip.setPointerCapture(e.pointerId);
          setDragging(true);
          const move = (p: PointerEvent) => setRatio(Math.max(0.34, Math.min(0.82, ratio + (p.clientY - start) / box.height)));
          const done = () => {
            setDragging(false);
            grip.removeEventListener("pointermove", move);
            grip.removeEventListener("lostpointercapture", done);
          };
          grip.addEventListener("pointermove", move);
          grip.addEventListener("lostpointercapture", done);
        }}
      >
        <i />
      </div>
      <HistoryTable
        program={drawProgram}
        onProgram={setDrawProgram}
        onMenu={onMenu}
        trend={byMetric[lanes[0].key].process ?? "cpu"}
        span={selection ?? span}
        view={span}
        at={pinned?.time ?? stop}
        live={cursor === null}
        latest={cursor === null && !pinned}
        mode={mode}
        setMode={setMode}
        alive={alive}
        onOpen={onOpen}
        clearable={!!pinned || !!selection}
        onClear={clearCursor}
      />
      {chooser &&
        settings &&
        page.current &&
        createPortal(
          <div
            className="h-chooser-layer"
            onPointerDown={(e) => e.target === e.currentTarget && setChooser(null)}
            onKeyDown={(e) => {
              if (e.key === "Escape") setChooser(null);
            }}
          >
            <div ref={panel} className="a-dialog h-chooser" role="dialog" aria-label="搜索读数">
              <div className="h-chooser-head">
                {settings.title && <h3>{settings.title}</h3>}
                <input
                  className="a-input"
                  autoFocus
                  placeholder="搜索读数"
                  value={query}
                  onChange={(e) => {
                    setChoiceIndex(e.target.value.trim() ? 0 : null);
                    setQuery(e.target.value);
                  }}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") pick(choices[highlight]);
                    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                      e.preventDefault();
                      const index = Math.min(choices.length - 1, Math.max(0, highlight + (e.key === "ArrowUp" ? -1 : 1)));
                      setChoiceIndex(index);
                      e.currentTarget
                        .closest(".h-chooser")
                        ?.querySelectorAll(".h-choice")
                        [index]?.scrollIntoView({ block: "nearest" });
                    }
                  }}
                />
              </div>
              <div className="h-choices">
                <aside>
                  {["", ...groups.filter((g) => offered.some((m) => m.group === g))].map((g) => {
                    const count = offered.filter((m) => (!g || m.group === g) && (!searching || found(m))).length;
                    return (
                      <button
                        key={g}
                        aria-pressed={!searching && group === g}
                        data-empty={!count || undefined}
                        onClick={() => {
                          setQuery("");
                          setGroup(g);
                          setChoiceIndex(null);
                        }}
                      >
                        <span>{g || "全部"}</span>
                        <small>{count}</small>
                      </button>
                    );
                  })}
                </aside>
                <div className="a-scroll">
                  {!lines.length && <p>没有匹配的读数</p>}
                  {lines.map((line, n) => {
                    if (line.kind === "group")
                      return (
                        <small key={"g" + line.text} className="h-group-head" data-first={n === 0 || undefined}>
                          {line.text}
                        </small>
                      );
                    if (line.kind === "device")
                      return (
                        <b key={"d" + line.text} className="h-device">
                          <span>{line.text}</span>
                        </b>
                      );
                    const index = (choices as Line[]).indexOf(line);
                    const m = line.kind === "reading" ? line.m : null;
                    const checked = m ? m.id === settings.current : !settings.current;
                    return (
                      <div
                        key={m?.id ?? "none"}
                        className="h-choice"
                        data-indent={(line.kind === "reading" && line.indent) || undefined}
                        data-highlight={index === highlight || undefined}
                        onPointerEnter={() => setChoiceIndex(index)}
                        onClick={() => pick(line)}
                      >
                        <i style={m ? { background: m.color } : undefined} />
                        <span>{m ? (line.kind === "reading" && line.indent ? m.part : m.long) : "不叠加"}</span>
                        {m?.service && (
                          <span className="h-service" title="由特权服务提供">
                            <Icon name="shieldCheck" size={13} />
                          </span>
                        )}
                        <span className="h-check">{checked && <Icon name="check" size={14} />}</span>
                        {settings.add && m && (
                          <button
                            title="在新轨道中显示"
                            onClick={(e) => {
                              e.stopPropagation();
                              pick(line, true);
                            }}
                          >
                            <Icon name="plus" size={13} />
                          </button>
                        )}
                      </div>
                    );
                  })}
                </div>
              </div>
            </div>
          </div>,
          page.current.closest(".a-window")!,
        )}
      {picker &&
        page.current &&
        createPortal(
          <RangeDialog
            now={latest}
            span={span}
            following={cursor === null}
            current={rangeName ?? (custom ? null : followSpan !== null ? followSpan : length)}
            close={() => setPicker(false)}
            apply={(s, follow, name) => {
              setPicker(false);
              if (follow) {
                choose(s[1] - s[0], name ?? null);
                return;
              }
              choose(null);
              setRangeName(name ?? null);
              setCustom(s);
              setView(s);
              onCursor(s[1]);
            }}
          />,
          page.current.closest(".a-window")!,
        )}
    </div>
  );
});
