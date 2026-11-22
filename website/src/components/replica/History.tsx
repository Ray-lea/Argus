/** @jsxImportSource @/components/replica/i18n */
"use client";
import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import { localRect } from "./widgets";
import { Icon } from "./icons";
import { store, useShared } from "./store";
import { Button, Select } from "./widgets";
import { translate } from "./i18n";
import { sampleAt, historyRanges, type HistoryRange, type Process } from "./model";
import { byMetric, metrics, formatMetric, tabMetrics } from "./metrics";
import { glide } from "./HistoryRaster";
import { sampleTimes } from "./historySeries";
import motionConfig from "../../../../resources/motion.json";
import { DAY, stamp, type Span } from "./historyTime";
import { Navigator, Plot, type Lane } from "./HistoryPlot";
import { HistoryTable, WatchPanel } from "./HistoryTable";
import RangeDialog from "./HistoryRangeDialog";
import type { MenuEntry } from "./Pages";
type Choice = "add" | "primary" | "overlay";
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
export default function History({
  range,
  tick,
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
}: HistoryProps) {
  const page = useRef<HTMLDivElement>(null);
  const [ratio, setRatio] = useState(0.6);
  const [chooserAnchor, setAnchor] = useState({ x: 0, y: 0 });
  const [extra, setExtra] = useState(false);
  const [choiceIndex, setChoiceIndex] = useState(0);
  const { watch, recording } = useShared();
  const [followSpan, setFollowSpan] = useState<number | null>(null);
  const [custom, setCustom] = useState<Span | null>(null),
    [picker, setPicker] = useState(false);
  const [view, setView] = useState<Span | null>(null),
    [selection, setSelection] = useState<Span | null>(null);
  const [lanes, setLanes] = useState<Lane[]>(() => store.get().lanes),
    [active, setActive] = useState(() => store.get().lanes[0].id);
  const seq = useRef(Math.max(...lanes.map((l) => l.id)));
  useEffect(() => {
    if (store.get().lanes !== lanes) store.set({ lanes });
  }, [lanes]);
  const [chooser, setChooser] = useState<Choice | null>(null),
    [query, setQuery] = useState(""),
    [group, setGroup] = useState("");
  const [drawProgram, setDrawProgram] = useState<string | null>(null);
  const [pinned, setPinned] = useState<{ lane: number; time: number } | null>(null);
  const [hoverTime, setHoverTime] = useState<number | null>(null);
  const clearCursor = () => {
    setPinned(null);
    setSelection(null);
    setMode("moment");
  };
  const [mode, setMode] = useState<"moment" | "range">("moment");
  const length = Math.min(historyRanges.find((r) => r.value === range)!.hours * 3600, (recording.days || 30) * DAY);
  const duration = view ? view[1] - view[0] : (followSpan ?? (custom ? custom[1] - custom[0] : length));
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
  const span: Span = view ?? (followSpan !== null ? [end - followSpan, end] : custom) ?? [end - length, end];
  const [start, stop] = span;
  const anchor = flow ? (stop + lag) * 1000 : null;
  const instant = () => (anchor === null ? 0 : Math.min(2, Math.max(0, (performance.now() - anchor) / 1000)));
  const [width, setWidth] = useState(1200);
  useEffect(() => {
    const parent = page.current!;
    const resize = () =>
      setWidth(
        Math.max(100, parent.querySelector<HTMLElement>(".a-history-plot")?.offsetWidth ?? parent.clientWidth - 120),
      );
    resize();
    const obs = new ResizeObserver(resize);
    obs.observe(parent);
    return () => obs.disconnect();
  }, []);
  const times = useMemo(() => sampleTimes([start, stop], width), [start, stop, width]);
  const [fixedExtent, setFixedExtent] = useState<Span | null>(null);
  const extent: Span = fixedExtent ?? custom ?? [end - (followSpan ?? length), end];
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
  useEffect(() => {
    if (!axis.current) return;
    const el = axis.current,
      pps = el.clientWidth / (stop - start),
      lagged = instant();
    glide(el, -lagged * pps, anchor === null ? 0 : -2 * pps, anchor === null ? 0 : (2 - lagged) * 1000, pace);
  }, [start, stop, anchor, width, pace]);
  const at = hoverTime ?? pinned?.time ?? (cursor === null ? latest : stop);
  const summary = (metric: (typeof tabMetrics)[number]) => {
    const t = Math.floor(at),
      sample = sampleAt(t);
    return formatMetric(metric.unit, metric.read(sample) + (metric.second?.(sample) ?? 0));
  };

  const reset = () => {
    clearCursor();
    if (cursor === null) {
      setView(null);
      return;
    }
    setView(extent);
    onCursor(extent[1]);
  };
  const live = () => {
    setFixedExtent(null);
    setFollowSpan(null);
    setPinned(null);
    setCustom(null);
    setView(null);
    setSelection(null);
    onCursor(null);
  };
  const choose = (kind: Choice, id: number, button: HTMLElement, more = false) => {
    const frame = page.current!.closest<HTMLElement>(".a-window")!,
      box = localRect(button),
      windowBox = localRect(frame);
    setAnchor({
      x: Math.min(frame.clientWidth - 304, Math.max(8, box.left - windowBox.left)),
      y: Math.max(8, Math.min(box.bottom - windowBox.top + 4, frame.clientHeight - 435)),
    });
    setExtra(more);
    setChoiceIndex(0);

    setActive(id);
    setChooser(kind);
    setQuery("");
    setGroup("");
  };
  const place = (key: string, how: Choice, id = active) => {
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
  };
  const zoom = (x: number, delta: number, pan: boolean) => {
    const phase = instant(),
      a = extent[0] + phase,
      b = extent[1] + phase,
      whole = b - a,
      shown = stop - start;
    const desired = pan
      ? shown
      : Math.min(whole, Math.max(Math.min(600, Math.max(30, whole / 5)), shown * (delta > 0 ? 1.4 : 0.7)));
    const from = Math.min(
      b - desired,
      Math.max(a, start + phase + (pan ? Math.sign(delta) * shown * 0.1 : x * (shown - desired))),
    );
    setPinned(null);
    setFixedExtent([a, b]);
    setView([from, from + desired]);
    onCursor(from + x * desired);
    setSelection(null);
  };
  const filter = metrics
    .filter((m) => !["uptime", "memoryPercent"].includes(m.id) && (!extra || !tabMetrics.includes(m)))
    .filter(
      (m) =>
        (!group || m.group === group) &&
        [translate(m.name), m.device ?? "", translate(m.group)].join(" ").toLowerCase().includes(query.toLowerCase()),
    );
  const choices = historyRanges.filter((r) => ["5m", "3h", "1d", "3d", range].includes(r.value));
  useEffect(() => {
    if (cursor === null) {
      setPinned(null);
      setView(null);
      setFixedExtent(null);
      setCustom(null);
      setSelection(null);
    }
  }, [cursor]);
  return (
    <div className="a-page a-history" ref={page} style={{ "--chart-share": ratio * 100 + "%" } as CSSProperties}>
      {watch && <WatchPanel tick={tick} alive={alive} />}
      <div className="h-tabs">
        {tabMetrics.map((m) => (
          <div
            key={m.id}
            draggable
            onDragStart={(e) => e.dataTransfer.setData("argus-reading", m.id)}
            style={{ "--c": m.color } as CSSProperties}
            data-current={lanes.some((l) => l.key === m.id) || undefined}
          >
            <button onClick={() => place(m.id, "primary")}>
              {m.id === "battery" ? "电池" : m.name} <b>{summary(m)}</b>
            </button>
            <button title="在新轨道中显示" onClick={() => place(m.id, "add")}>
              +
            </button>
          </div>
        ))}
        <span className="h-tools">
          <button onClick={(e) => choose("primary", active, e.currentTarget, true)}>
            更多读数 <Icon name="chevronDown" size={12} />
          </button>
          <button onClick={(e) => choose("add", active, e.currentTarget)}>
            + <span>轨道</span>
          </button>
        </span>
      </div>
      <section className="a-history-card">
        <div className="h-lanes a-scroll">
          {lanes.map((l) => (
            <section
              key={l.id}
              className="h-lane"
              style={{ "--c": byMetric[l.key].color } as CSSProperties}
              data-multi={lanes.length > 1 || undefined}
              onPointerDown={() => setActive(l.id)}
              onDragOver={(e) => e.preventDefault()}
              onDrop={(e) => {
                e.preventDefault();
                const k = e.dataTransfer.getData("argus-reading");
                if (byMetric[k]) place(k, "overlay", l.id);
                const id = Number(e.dataTransfer.getData("argus-lane"));
                if (id && id !== l.id)
                  setLanes((prev) => {
                    const next = prev.filter((i) => i.id !== id);
                    next.splice(
                      next.findIndex((i) => i.id === l.id),
                      0,
                      prev.find((i) => i.id === id)!,
                    );
                    return next;
                  });
              }}
            >
              <header>
                <span
                  draggable
                  title="拖动可调整顺序"
                  onDragStart={(e) => e.dataTransfer.setData("argus-lane", String(l.id))}
                >
                  ⠿
                </span>
                <button className="h-primary" onClick={(e) => choose("primary", l.id, e.currentTarget)}>
                  <i />
                  {(
                    {
                      cpu: "CPU 使用率",
                      mem: "内存使用量",
                      disk: "磁盘读写",
                      net: "网络收发",
                      gpu: "GPU 使用率",
                    } as Record<string, string>
                  )[l.key] ?? byMetric[l.key].name}
                  <Icon name="chevronDown" size={12} />
                </button>
                <b>{summary(byMetric[l.key])}</b>
                <div
                  className="h-overlay"
                  style={
                    { "--overlay": l.overlay ? byMetric[l.overlay].color : "var(--a-stroke-strong)" } as CSSProperties
                  }
                >
                  <button onClick={(e) => choose("overlay", l.id, e.currentTarget)}>
                    {l.overlay ? (
                      <>
                        <i />
                        {byMetric[l.overlay].name} <b>{summary(byMetric[l.overlay])}</b>{" "}
                        <small>{byMetric[l.key].unit === byMetric[l.overlay].unit ? "共用左轴" : "右轴"}</small>
                      </>
                    ) : (
                      "叠加"
                    )}
                  </button>
                  {l.overlay && (
                    <button aria-label="取消叠加" onClick={() => place("", "overlay", l.id)}>
                      <Icon name="dismiss" size={11} />
                    </button>
                  )}
                </div>
                {lanes.length > 1 && (
                  <button
                    aria-label="移除此轨道"
                    onClick={() => {
                      const rest = lanes.filter((o) => o.id !== l.id);
                      setLanes(rest);
                      setActive(rest[0].id);
                    }}
                  >
                    ×
                  </button>
                )}
              </header>
              <Plot
                lane={l}
                program={drawProgram}
                span={span}
                anchor={anchor}
                grid={grid}
                animate={animate}
                pace={pace}
                times={times}
                at={pinned?.lane === l.id ? pinned.time : null}
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
              />
            </section>
          ))}
        </div>
        <div className="h-axis">
          <div ref={axis}>
            {grid.map((t) => (
              <span key={t} style={{ left: ((t - start) / (stop - start)) * 100 + "%" }}>
                {stamp(t, stop - start > DAY)}
              </span>
            ))}
          </div>
        </div>
        <div className="h-navigator">
          <Select
            reselect
            label="时间范围"
            value={custom || followSpan !== null ? "custom" : range}
            options={[...choices, { value: "custom", label: "自定义…" }]}
            onChange={(v) => {
              if (v === "custom") setPicker(true);
              else {
                onRange(v);
                live();
              }
            }}
          />
          <small className="h-nav-start">{stamp(start)}</small>
          <Navigator
            span={span}
            extent={extent}
            times={trackTimes}
            anchor={anchor}
            animate={animate}
            pace={pace}
            metric={byMetric[lanes.find((l) => l.id === active)?.key ?? "cpu"]}
            change={(next) => {
              setPinned(null);
              setFixedExtent(extent);
              setView(next);
              onCursor(next[1]);
              setSelection(null);
            }}
            reset={reset}
          />
          <small className="h-nav-end">现在</small>
          {selection && <Button onClick={() => setView(selection)}>放大到所选</Button>}
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
            setRatio((v) => Math.max(0.25, Math.min(0.8, v + (e.key === "ArrowDown" ? 0.03 : -0.03))));
          }
        }}
        onPointerDown={(e) => {
          const grip = e.currentTarget,
            box = page.current!.getBoundingClientRect(),
            start = e.clientY;
          grip.setPointerCapture(e.pointerId);
          const move = (p: PointerEvent) =>
            setRatio(Math.max(0.25, Math.min(0.8, ratio + (p.clientY - start) / box.height)));
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
      <HistoryTable
        program={drawProgram}
        onProgram={setDrawProgram}
        onMenu={onMenu}
        trend={lanes[0].key}
        span={selection ?? span}
        at={pinned?.time ?? stop}
        live={cursor === null}
        mode={mode}
        setMode={setMode}
        alive={alive}
        onOpen={onOpen}
      />
      {chooser &&
        page.current &&
        createPortal(
          <div
            className="h-chooser-layer"
            onPointerDown={(e) => e.target === e.currentTarget && setChooser(null)}
            onKeyDown={(e) => {
              if (e.key === "Escape") setChooser(null);
            }}
          >
            <div
              style={{ left: chooserAnchor.x, top: chooserAnchor.y }}
              className="a-dialog h-chooser"
              role="dialog"
              aria-label="搜索读数"
            >
              <div className="a-dialog-body">
                <h3>
                  {chooser === "add"
                    ? "添加轨道"
                    : chooser === "primary"
                      ? "在当前轨道显示"
                      : "叠加第二个读数（右侧纵轴）"}
                </h3>
                <input
                  className="a-input"
                  autoFocus
                  placeholder="搜索读数"
                  value={query}
                  onChange={(e) => {
                    setChoiceIndex(0);
                    setQuery(e.target.value);
                  }}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && filter[choiceIndex]) place(filter[choiceIndex].id, chooser);
                    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                      e.preventDefault();
                      const index = Math.min(
                        filter.length - 1,
                        Math.max(0, choiceIndex + (e.key === "ArrowUp" ? -1 : 1)),
                      );
                      setChoiceIndex(index);
                      e.currentTarget
                        .closest(".h-chooser")
                        ?.querySelectorAll(".h-choice")
                        [index]?.scrollIntoView({ block: "nearest" });
                    }
                  }}
                />
                <div className="h-choices">
                  <aside>
                    {["", ...new Set(metrics.map((m) => m.group))].map((g) => (
                      <Button
                        key={g}
                        pressed={group === g}
                        onClick={() => {
                          setGroup(g);
                          setChoiceIndex(0);
                        }}
                      >
                        {g || "全部"}{" "}
                        <small>
                          {
                            metrics.filter(
                              (m) =>
                                (!g || m.group === g) &&
                                !["uptime", "memoryPercent"].includes(m.id) &&
                                (!extra || !tabMetrics.includes(m)),
                            ).length
                          }
                        </small>
                      </Button>
                    ))}
                  </aside>
                  <div className="a-scroll">
                    {chooser === "overlay" && <Button onClick={() => place("", "overlay")}>不叠加</Button>}
                    {filter.map((m, index) => (
                      <div key={m.id}>
                        {(index === 0 ||
                          filter[index - 1].group !== m.group ||
                          filter[index - 1].device !== m.device) && (
                          <small className="h-device">{m.device ?? m.group}</small>
                        )}
                        <div
                          className="h-choice"
                          data-highlight={index === choiceIndex || undefined}
                          onPointerEnter={() => setChoiceIndex(index)}
                        >
                          <button onClick={() => place(m.id, chooser)}>
                            <i style={{ background: m.color }} />
                            <b>{m.name}</b>
                          </button>
                          <Button title="在新轨道中显示" onClick={() => place(m.id, "add")}>
                            +
                          </Button>
                        </div>
                      </div>
                    ))}
                    {!filter.length && <p>没有匹配的读数</p>}
                  </div>
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
            close={() => setPicker(false)}
            apply={(s, follow) => {
              setFixedExtent(null);
              setFollowSpan(follow ? s[1] - s[0] : null);
              setCustom(follow ? null : s);
              setView(follow ? null : s);
              onCursor(follow ? null : s[1]);
              setPicker(false);
              setSelection(null);
            }}
          />,
          page.current.closest(".a-window")!,
        )}
    </div>
  );
}
