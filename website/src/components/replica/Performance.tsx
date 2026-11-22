/** @jsxImportSource @/components/replica/i18n */
"use client";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import theme from "../../../../resources/theme.json";
import type { Motion } from "./Curve";
import CpuDetails, { logicalTemperatures } from "./CpuDetails";
import {graphSnapshot,animateGraphSwitch} from "./graphSwitch";
import DiskDetails from "./DiskDetails";
import { isSmart, planGrid, shownEngines, withCount, withEngine, withSmart, type EngineChoice } from "./engineGrid";
import type { MenuEntry } from "./Pages";
import { Chart, percentOf, seriesOf, SteadyCurve, wattsText } from "./perfParts";
import { cpuPower, cpuTemperature, useSystemHistory, type Device } from "./Processes";
import {
  adapterAt,
  adapters,
  batteryDischarge,
  batteryLevel,
  bytes,
  clock,
  coreLoad,
  diskAt,
  disks,
  gpuAt,
  gpus,
  logical,
  machine,
  noise,
  rate,
  rateCeiling,
  rateLabel,
  sampleAt,
  samples,
  thousands,
  totalMemory,
} from "./model";
import { Button, localRect, scaleOf, Segments, Select, slideIn, useIsoLayoutEffect } from "./widgets";

export { spanLabel } from "./perfParts";

interface Props {
  tick: number;
  interval: number;
  running: boolean;
  motion: Motion;
  device: Device;
  /** The pages chosen within the devices; the window keeps them. */
  pages: PerformancePages;
  onPages: (pages: Partial<PerformancePages>) => void;
  processCount: number;
  threads: number;
  handles: number;
  onDevice: (device: Device) => void;
  /** A context menu at the pointer (Replica's). */
  onMenu: (event: ReactMouseEvent, items: MenuEntry[]) => void;
}
// Which page of a device shows: the CPU's overview (as one curve or one per
// logical processor, all of them or one kind of core) or its details, and the
// disk's overview or its details. The window holds them, so that they outlast
// a visit to another page and the home page can ask for one of them.
export interface PerformancePages {
  cpuPage: "overview" | "details";
  graph: "overall" | "logical";
  filter: "all" | "p" | "e";
  diskPage: "overview" | "details";
  /** The device list's order, as dragged; and the devices taken out of it. */
  order: Device[];
  hidden: Device[];
  /** The CPU graphs draw the kernel time (right-click a CPU graph). */
  kernel: boolean;
  /** SMART raw values in hexadecimal (the attributes card's switch); decimal by default. */
  rawHex?: boolean;
}
/** The list's order as the app arranges it: CPU, memory, disks, network adapters, GPUs, NPUs, batteries. */
export const defaultDevices: Device[] = ["cpu", "memory", "disk", "disk1", "wifi", "wsl", "gpu", "gpu1", "npu", "battery"];
const GB = 1024 ** 3;
const MB = 1024 ** 2;
// How the hover readout writes each kind of value.
const gigabytes = (v: number) => v.toFixed(1) + " GB";
const watts = (v: number) => wattsText(v) + " W";
/** A rate in bits per second (charts.rs `bit_rate`): 9.62 Mbps, 103.6 Mbps. */
function bitRate(bytesPerSecond: number) {
  const bits = bytesPerSecond * 8;
  if (bits < 1) return "0 Kbps";
  if (bits < 1e6) return (bits / 1e3).toFixed(bits < 1e4 ? 1 : 0) + " Kbps";
  if (bits < 1e9) return (bits / 1e6).toFixed(bits < 1e7 ? 2 : 1) + " Mbps";
  return (bits / 1e9).toFixed(2) + " Gbps";
}

// The live figures and the specs beside or under them (performance.rs
// `stats_layout`): narrower columns only where they spare a row, several rows
// as even as they go; the specs go under the figures where the figures' share
// would hold fewer than three columns, spread over columns of their own.
const STAT_MIN = 120;
const STAT_TIGHT = 112;
const STAT_GAP_X = 20;
const STATS_GAP = 28;
const SIDE_COLUMNS = 3;
const SPECS_MIN = 280;
function statsLayout(width: number, cpu: boolean, text: number, figures: number, specs: number) {
  const fit = (w: number, min: number) => Math.max(1, Math.floor((w + STAT_GAP_X) / (min * text + STAT_GAP_X)));
  const share = cpu ? (width - STATS_GAP) / 2 : ((width - STATS_GAP) * 1.1) / 2.1;
  const stacked = fit(share, STAT_MIN) < SIDE_COLUMNS;
  const statsWidth = stacked ? width : share;
  const rows = Math.ceil(figures / fit(statsWidth, STAT_TIGHT));
  const columns = rows > 1 ? Math.ceil(figures / rows) : Math.max(fit(statsWidth, STAT_MIN), figures);
  const specColumns = stacked ? Math.min(fit(width, SPECS_MIN), Math.max(1, Math.ceil(specs / 2))) : 1;
  return { stacked, columns, specColumns };
}
/** A figure: label, value, unit, a note under it, and an explanation on hover. */
type Figure = [string, ReactNode, string?, string?, string?];
function Stats({ figures, specs, cpu }: { figures: Figure[]; specs: [string, ReactNode][]; cpu?: boolean }) {
  const box = useRef<HTMLDivElement>(null);
  // The card's content width (1600 × 900 window until measured) and the text scale.
  const [size, setSize] = useState({ width: 1264, text: 1 });
  useIsoLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    const measure = () => {
      const width = el.offsetWidth;
      const text = parseFloat(getComputedStyle(el).fontSize) / 13 || 1;
      if (width) setSize((s) => (s.width === width && s.text === text ? s : { width, text }));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  const layout = statsLayout(size.width, !!cpu, size.text, figures.length, specs.length);
  const list = (rows: [string, ReactNode][], key?: number) => (
    <dl className="a-specs" key={key}>
      {rows.map(([label, value]) => [<dt key={label + ":t"}>{label}</dt>, <dd key={label + ":d"}>{value}</dd>])}
    </dl>
  );
  // In reading order down each column, the columns as even as they go.
  const per = Math.ceil(specs.length / layout.specColumns);
  const columns = Array.from({ length: Math.ceil(specs.length / Math.max(1, per)) }, (_, i) => specs.slice(i * per, (i + 1) * per));
  return (
    <div ref={box} className={"a-stats" + (cpu ? " is-cpu" : "") + (layout.stacked ? " is-stacked" : "")}>
      <dl className="a-figures" style={{ gridTemplateColumns: `repeat(${layout.columns}, minmax(0, 1fr))` }}>
        {figures.map(([label, value, unit, note, hint]) => (
          <div key={label} title={hint}>
            <dt>{label}</dt>
            <dd>
              {value}
              {unit && <small>{unit}</small>}
            </dd>
            {note && <dd>{note}</dd>}
          </div>
        ))}
      </dl>
      {specs.length > 0 &&
        (columns.length > 1 ? (
          <div className="a-spec-columns" style={{ gridTemplateColumns: `repeat(${columns.length}, minmax(0, 1fr))` }}>
            {columns.map((rows, i) => list(rows, i))}
          </div>
        ) : (
          list(specs)
        ))}
    </div>
  );
}
function Head({ title, model, children }: { title: string; model?: string; children?: ReactNode }) {
  return (
    <header className="a-perf-head">
      <div className="a-perf-title">
        <h2>{title}</h2>
        {children}
      </div>
      {model && <span className="a-model">{model}</span>}
    </header>
  );
}
function Legend({ items }: { items: [string, string, string?][] }) {
  return (
    <div className="a-legend">
      {items.map(([label, color, kind]) => (
        <span key={label} className={kind ? "is-" + kind : undefined} style={{ "--c": color } as CSSProperties}>
          {label}
        </span>
      ))}
    </div>
  );
}
/*
 * The memory's composition (performance.rs `memory_composition`): each part
 * a share of the bar, in bytes. A new sample eases the parts to their new
 * shares over 600 ms, as a width transition would, but on the compositor:
 * each part is laid out at its new share at once and moved there from where
 * it showed, so that the frames between lay out and paint nothing.
 */
const EASE_MOVE = "cubic-bezier(0.33, 0, 0.1, 1)"; // --a-ease-move
function MemoryBar({ parts }: { parts: [string, number][] }) {
  const bar = useRef<HTMLDivElement>(null);
  // Each part's start and length (shares of the bar) it eases from and to.
  const shown = useRef(new Map<string, { from: number[]; to: number[]; run?: Animation }>());
  const shares = parts.map(([, bytes]) => Math.max(0, bytes) / totalMemory);
  useIsoLayoutEffect(() => {
    const el = bar.current;
    if (!el) return;
    const still = !!el.closest("[data-motion-off]") || matchMedia("(prefers-reduced-motion: reduce)").matches;
    let start = 0;
    parts.forEach(([name], i) => {
      const node = el.children[i] as HTMLElement | undefined;
      const to = [start, shares[i]];
      start += shares[i];
      const last = shown.current.get(name);
      // Where it shows now: on its way to the last share, or at it.
      let from = last?.to;
      if (last?.run) {
        const e = last.run.effect?.getComputedTiming().progress;
        if (e != null) from = last.from.map((v, k) => v + (last.to[k] - v) * e);
        last.run.cancel();
      }
      let run: Animation | undefined;
      if (node && from && !still && to[1] > 0 && (from[0] !== to[0] || from[1] !== to[1]))
        // Its edges move as a width transition moves them (transform-origin at its left).
        run = node.animate(
          [{ transform: `translateX(${((from[0] - to[0]) / to[1]) * 100}%) scaleX(${from[1] / to[1]})` }, { transform: "none" }],
          { duration: 600, easing: EASE_MOVE },
        );
      shown.current.set(name, { from: from ?? to, to, run });
    });
  }, [shares.join()]);
  return (
    <div className="a-membar" ref={bar} role="img" aria-label="内存组成">
      {parts.map(([name], i) => (
        <i key={name} className={name} style={{ width: shares[i] * 100 + "%" }} />
      ))}
    </div>
  );
}
const split = (text: string): [string, string] => {
  const at = text.lastIndexOf(" ");
  return at < 0 ? [text, ""] : [text.slice(0, at), text.slice(at + 1)];
};

// The logical processors' grid as the app arranges it (model.rs
// `core_grid_columns`): full rows of readable plots first; cards narrower
// than their caption leave out the frequency, then the whole caption, so that
// a small area takes more columns of plots of a readable shape. While
// resizing, the arrangement it has stays unless another is clearly better.
const { coreHeaderHeight: CORE_HEADER, corePlotMinWidth, corePlotMinHeight, coreGap: CORE_GAP } = theme.layout;
/** What a card's caption shows: number, frequency and load; number and load; none. */
type Caption = 0 | 1 | 2;
interface CoreGrid {
  count: number;
  width: number;
  height: number;
  /** The narrowest card with its whole caption, and with its caption short of the frequency. */
  full: number;
  short: number;
}
interface CoreArrangement {
  count: number;
  columns: number;
  rows: number;
  caption: Caption;
  rowMin: number;
}
const plotMin = (width: number) => Math.max(corePlotMinHeight, Math.min(width / 2.8, 64));
const cardWidth = (g: CoreGrid, columns: number) => (g.width - (columns - 1) * CORE_GAP) / columns;
function coreArrangement(g: CoreGrid, columns: number): CoreArrangement {
  columns = Math.max(1, columns);
  const rows = Math.max(1, Math.ceil(g.count / columns));
  const width = cardWidth(g, columns);
  const share = (g.height - (rows - 1) * CORE_GAP) / rows;
  const plot = plotMin(width);
  const caption: Caption = width >= g.short && share >= CORE_HEADER + plot ? (width >= g.full ? 0 : 1) : 2;
  return { count: g.count, columns, rows, caption, rowMin: (caption === 2 ? 0 : CORE_HEADER) + plot };
}
function coreScore(g: CoreGrid, a: CoreArrangement) {
  const header = a.rowMin - plotMin(cardWidth(g, a.columns));
  const share = (g.height - (a.rows - 1) * CORE_GAP) / a.rows;
  const plot = Math.max(share, a.rowMin) - header;
  let score = Math.min(cardWidth(g, a.columns) / 1.4, plot) * [1, 0.93, 0.72][a.caption];
  const empty = a.rows * a.columns - g.count;
  // A last row of a few cards keeps less than one with a single gap.
  if (empty > 0) score *= 0.8 * (1 - (0.5 * empty) / a.columns);
  const needed = a.rows * (a.rowMin + CORE_GAP) - CORE_GAP;
  // Scrolling hides part of the grid: the more, the worse.
  if (needed > g.height) score *= (Math.max(0, g.height) / needed) ** 3;
  return score;
}
function coreGridColumns(g: CoreGrid, previous: number) {
  if (g.count <= 1 || g.width <= 0) return coreArrangement(g, 1);
  const limit = Math.min(g.count, Math.max(1, Math.floor((g.width + CORE_GAP) / (corePlotMinWidth + CORE_GAP))));
  let best = coreArrangement(g, limit);
  let bestScore = 0;
  for (let columns = 1; columns <= limit; columns++) {
    const a = coreArrangement(g, columns);
    const score = coreScore(g, a);
    if (score > bestScore) {
      best = a;
      bestScore = score;
    }
  }
  if (previous > 0 && previous <= limit) {
    const current = coreArrangement(g, previous);
    const score = coreScore(g, current);
    // A small dead band keeps one-pixel drags from moving the cores back and
    // forth, but does not keep captions hidden that the best shows.
    if (score > 0 && bestScore <= score * 1.08 && current.caption <= best.caption) return current;
  }
  return best;
}
/** How wide a card's caption runs, whole and short of the frequency, in the font the grid draws (performance.rs `core_caption_widths`). */
let measurer: CanvasRenderingContext2D | null = null;
function captionWidths(el: Element, highest: number, text: number) {
  measurer ??= document.createElement("canvas").getContext("2d");
  if (!measurer) return { full: 133 * text, short: 79 * text };
  const family = getComputedStyle(el).fontFamily;
  const measure = (s: string, size: number, weight: number) => {
    measurer!.font = `${weight} ${size * text}px ${family}`;
    return measurer!.measureText(s).width;
  };
  const badge = Math.max(...["P", "E"].map((b) => Math.max(measure(b, 10, 700) + 6, 15)));
  const name = measure(String(highest), 12, 500) + badge + 6;
  const frequency = 8 + measure("9.99 GHz", 11, 400);
  const load = measure("100", 13, 600) + 1 + measure("%", 10.5, 400);
  const short = name + 8 + load + 3;
  return { full: Math.ceil(short + frequency), short: Math.ceil(short) };
}

// The machine's disks, adapters and GPUs, by their ids on this page (model.ts).
const diskOf = (id: Device) => disks.find((d) => d.id === id);
const adapterOf = (id: Device) => adapters.find((a) => a.id === id);
const gpuOf = (id: Device) => gpus.find((g) => g.id === id);
/** A byte rate with its unit always shown (charts.rs `byte_rate`): "0 KB/s", "312 KB/s", "1.2 MB/s". */
const byteRate = (v: number) => (v < 1 ? "0 KB/s" : rate(v));
/** A link's speed and the same in bytes (performance.rs `link_speed`): "1.2 Gbps（143.2 MB/s）". */
function linkText(bits: number) {
  const speed = bits >= 1e9 ? (bits / 1e9).toFixed(1).replace(/\.0$/, "") + " Gbps" : Math.round(bits / 1e6) + " Mbps";
  return `${speed}（${byteRate(bits / 8)}）`;
}
/** The integrated GPU's limit reasons (its kernel sensors' performance limits). */
const GPU_LIMITS = ["功耗限制", "温度限制", "PROCHOT", "电流限制（VR TDC）", "其他限制（IccMax／PL4 等）", "低于目标频率"];
/** Utilization an engine has to reach within the span to be charted by smart display. */
const BUSY_PERCENT = 1;
const ENGINE_CELL = { width: theme.layout.engineMinWidth, height: theme.layout.engineMinHeight };
/** The engines' share of the height they and the memory charts take; the handle between them. */
const SHARE_DEFAULT = 0.6;
const HANDLE = 18;

/** A device entry's height in the list, with the gap under it. */
const ROW = 62;
const ROW_HEIGHT = 58;
const SIDE_DEFAULT = 248;
const SIDE_MIN = 200;
const SIDE_MAX = 440;
export default function Performance(p: Props) {
  const { graph, filter, cpuPage, diskPage, hidden, kernel } = p.pages;
  // A device the saved order does not name yet takes its place from the default order.
  const order = useMemo(() => {
    const out = p.pages.order.filter((d) => defaultDevices.includes(d));
    defaultDevices.forEach((d, i) => !out.includes(d) && out.splice(Math.min(i, out.length), 0, d));
    return out;
  }, [p.pages.order]);
  const devices = order;
  // A device opened from elsewhere (the overview band, the details pane)
  // comes back into the list.
  useEffect(() => {
    if (hidden.includes(p.device)) p.onPages({ hidden: hidden.filter((d) => d !== p.device) });
  }, [p.device]);
  const graphs=useRef<HTMLDivElement>(null);
  const leaving=useRef<HTMLElement|null>(null);
  const previousCards=useRef<Map<string,DOMRect>|null>(null);
  const animates=()=>!!graphs.current&&!graphs.current.closest("[data-motion-off]")&&!matchMedia("(prefers-reduced-motion: reduce)").matches;
  const setGraph=(next:PerformancePages["graph"])=>{
    if(next===graph)return;
    const current=graphs.current?.firstElementChild as HTMLElement|undefined;
    if(current&&animates())leaving.current=graphSnapshot(current);
    p.onPages({graph:next});
  };
  useIsoLayoutEffect(()=>{const parent=graphs.current,ghost=leaving.current;leaving.current=null;if(parent&&ghost)animateGraphSwitch(parent,ghost)},[graph]);
  const setFilter=(next:PerformancePages["filter"])=>{
    if(next===filter)return;
    if(animates()) previousCards.current=new Map([...graphs.current!.querySelectorAll<HTMLElement>("[data-core]")].map(card=>[card.dataset.core!,card.getBoundingClientRect()]));
    p.onPages({filter:next});
  };
  const setCpuPage = (cpuPage: PerformancePages["cpuPage"]) => p.onPages({ cpuPage });
  const setDiskPage = (diskPage: PerformancePages["diskPage"]) => p.onPages({ diskPage });
  // The tick at which the disk's details were last read: each time its
  // details page opens, and again from the head's button.
  const [diskRead, setDiskRead] = useState(0);
  // What was chosen for each GPU model's engine charts.
  const [engineChoices, setEngineChoices] = useState<Record<string, EngineChoice>>({});
  const [engineLayout, setEngineLayout] = useState<"grid" | "focus" | "rows">("grid");
  const [engineShare, setEngineShare] = useState(SHARE_DEFAULT);
  const [side, setSide] = useState(SIDE_DEFAULT);
  const previousDevice=useRef(p.device);
  const view=useRef<HTMLDivElement>(null);
  useIsoLayoutEffect(()=>{
    const previous=previousDevice.current;
    previousDevice.current=p.device;
    if(previous===p.device)return;
    const page=view.current?.closest(".a-perf-main");
    if(page)page.scrollTop=0;
    const dir=Math.sign(devices.indexOf(p.device)-devices.indexOf(previous));
    slideIn(view.current,{y:dir*16,duration:250});
  },[p.device,devices]);
  // The disk's details are read as their page opens (another device and
  // back, or the overview and back), not with every sample.
  useIsoLayoutEffect(() => {
    if (diskOf(p.device) && diskPage === "details") setDiskRead(p.tick);
  }, [p.device, diskPage]);
  const markMoves = useRef(false);
  const s = sampleAt(p.tick);
  const h = useSystemHistory(p.tick);
  const curve = { tick: p.tick, interval: p.interval, running: p.running, motion: p.motion };
  const used = (s.mem / 100) * totalMemory;
  const battery = batteryLevel;
  const discharge = batteryDischarge;
  const npuLoad=(t:number)=>3+sampleAt(t).gpu*.17;
  const series = (f: (t: number) => number) => seriesOf(samples, f, p.tick);
  // The list's readings change with the samples only: drags and other state
  // render the page again without making them anew.
  const rail: { id: Device; name: string; info: string; values: number[][]; color: string; max: number }[] = useMemo(() => [
    { id: "cpu", name: "CPU", info: `${Math.round(s.cpu)}% ${clock("p", p.tick).toFixed(2)} GHz`, values: [h.cpu], color: "var(--a-cpu)", max: 100 },
    {
      id: "memory",
      name: "内存",
      info: `${(used / GB).toFixed(1)} / 32.0 GB（${Math.round(s.mem)}%）`,
      values: [h.mem],
      color: "var(--a-mem)",
      max: 100,
    },
    // Each disk: its kind, active time and temperature.
    ...disks.map((d) => {
      const r = diskAt(d.index, p.tick);
      return {
        id: d.id,
        name: d.name,
        info: `${d.kind} ${Math.round(r.active)}% ${Math.round(r.temperature)} °C`,
        values: [series((t) => diskAt(d.index, t).active)],
        color: d.color,
        max: 100,
      };
    }),
    // Each adapter: received and sent, on the throughput steps of its peak.
    ...adapters.map((a) => {
      const r = adapterAt(a.index, p.tick);
      const rx = series((t) => adapterAt(a.index, t).rx);
      const tx = series((t) => adapterAt(a.index, t).tx);
      return { id: a.id, name: a.name, info: `收 ${byteRate(r.rx)}  发 ${byteRate(r.tx)}`, values: [rx, tx], color: a.color, max: rateCeiling(Math.max(...rx, ...tx), true) };
    }),
    // Each GPU: utilization, temperature and power.
    ...gpus.map((g) => {
      const r = gpuAt(g.index, p.tick);
      return {
        id: g.id,
        name: g.name,
        info: `${Math.round(r.util)}% ${Math.round(r.temperature)} °C ${wattsText(r.power)} W`,
        values: [series((t) => gpuAt(g.index, t).util)],
        color: g.color,
        max: 100,
      };
    }),
    {id:"npu",name:"NPU 0",info:percentOf(npuLoad(p.tick)),values:[seriesOf(samples,npuLoad,p.tick)],color:"var(--a-npu)",max:100},
    {
      id: "battery",
      name: "电池",
      info: "84% 剩余 2 小时 31 分钟",
      values: [seriesOf(samples, battery, p.tick)],
      color: "var(--a-bat)",
      max: 100,
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
  ], [p.tick, h]);
  const coreCount = logical.filter((c) => filter === "all" || c.kind === filter).length;
  // The processors' grid: the arrangement it was given for that many
  // processors, and its size when last laid out (kept out of state, so that
  // resizing renders again only when the arrangement changes). Another count
  // starts afresh in the area the grid last had, as in the app.
  const coreGrid = useRef<HTMLDivElement>(null);
  const coreArea = useRef<{ width: number; height: number; full: number; short: number; floor: number } | null>(null);
  const [coreLayout, setCoreLayout] = useState<(CoreArrangement & { floor: number }) | null>(null);
  const highestCore = logical.filter((c) => filter === "all" || c.kind === filter).at(-1)?.index ?? 0;
  const area = coreArea.current;
  const cores_ =
    coreLayout && coreLayout.count === coreCount
      ? coreLayout
      : { ...coreGridColumns({ count: coreCount, width: area?.width ?? 0, height: area?.height ?? 120, full: area?.full ?? 133, short: area?.short ?? 79 }, 0), floor: area?.floor ?? 120 };
  useIsoLayoutEffect(() => {
    const el = coreGrid.current;
    if (!el) return;
    // Its own size (the window is drawn scaled). The view scrolls only where
    // the rows reach their least height: the room the grid has is its height
    // less what the view overflows by. Where the figures leave it little, it
    // takes up to 300 px of what shows of the view below its top (less the
    // legend), and they move below it. Laid out for the first time, it knows
    // only its width, as the app's first layout does; another arrangement is
    // measured again at the size it gives the grid.
    const measure = () => {
      const main = el.closest<HTMLElement>(".a-perf-main");
      const overflow = main ? Math.max(0, main.scrollHeight - main.clientHeight) : 0;
      const top = main ? localRect(el).top - localRect(main).top + main.scrollTop : 0;
      const floor = Math.max(120, Math.min(300, main ? Math.floor(main.clientHeight - top - 23) : 300));
      const width = el.offsetWidth;
      const room = Math.floor(Math.max(0, el.offsetHeight - overflow));
      const height = Math.max(coreArea.current ? room : 0, floor);
      const text = parseFloat(getComputedStyle(el).fontSize) / 13 || 1;
      const { full, short } = captionWidths(el, highestCore, text);
      coreArea.current = { width, height, full, short, floor };
      setCoreLayout((last) => {
        const next = { ...coreGridColumns({ count: coreCount, width, height, full, short }, last && last.count === coreCount ? last.columns : 0), floor };
        return last && last.count === next.count && last.columns === next.columns && last.caption === next.caption && last.rowMin === next.rowMin && last.floor === floor ? last : next;
      });
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [p.device, cpuPage, graph, filter, coreCount, cores_.columns, cores_.caption]);
  // The processors' curves, made only where the grid shows them.
  const showsCores = p.device === "cpu" && cpuPage === "overview" && graph === "logical";
  const cores = useMemo(
    () =>
      showsCores
        ? logical
            .filter((c) => filter === "all" || c.kind === filter)
            .map((c) => ({
              ...c,
              values: seriesOf(samples, (t) => coreLoad(c.index, t), p.tick),
              // The processor's kernel time, a share of its load.
              kernel: seriesOf(samples, (t) => coreLoad(c.index, t) * (0.24 + noise(c.index * 13 + 5, t) * 0.14), p.tick),
            }))
        : [],
    [filter, p.tick, showsCores],
  );
  useIsoLayoutEffect(()=>{
    const previous=previousCards.current;previousCards.current=null;
    if(!previous||!graphs.current)return;
    const k=scaleOf(graphs.current);
    for(const card of graphs.current.querySelectorAll<HTMLElement>("[data-core]")){
      const old=previous.get(card.dataset.core!);
      if(old){const next=card.getBoundingClientRect();card.animate([{transform:`translate(${(old.left-next.left)/k}px,${(old.top-next.top)/k}px) scale(${old.width/next.width},${old.height/next.height})`},{transform:"none"}],{duration:260,easing:"cubic-bezier(.1,.9,.2,1)"})}
      else card.animate([{opacity:0,transform:"scale(.96)"},{opacity:1,transform:"none"}],{duration:230,delay:30,fill:"backwards",easing:"cubic-bezier(.1,.9,.2,1)"});
    }
  },[filter]);
  const cpuMenu=(event:ReactMouseEvent)=>{
    event.preventDefault();
    p.onMenu(event,[
      {label:"整体视图",checked:graph==="overall",run:()=>setGraph("overall")},
      {label:"多核视图",checked:graph==="logical",run:()=>setGraph("logical")},
      ...(graph==="logical"?["-" as const,...([["all","全部核心"],["p","仅性能核"],["e","仅能效核"]] as const).map(([key,label])=>({label,checked:filter===key,run:()=>setFilter(key)}))]:[]),
      "-",{label:"显示内核时间",checked:kernel,run:()=>p.onPages({kernel:!kernel})}
    ]);
  };
  // The GPU's engine charts and the memory charts under them share the
  // height; the handle between them moves the line (double-click resets it).
  const gpuCharts = useRef<HTMLDivElement>(null);
  const engineGrid = useRef<HTMLDivElement>(null);
  const [engineArea, setEngineArea] = useState({ width: 1264, height: 250 });
  useIsoLayoutEffect(() => {
    const el = engineGrid.current;
    if (!el) return;
    const measure = () => setEngineArea((a) => (a.width === el.offsetWidth && a.height === el.offsetHeight ? a : { width: el.offsetWidth, height: el.offsetHeight }));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [p.device]);
  const pressShare = (event: ReactPointerEvent<HTMLDivElement>) => {
    const area = gpuCharts.current;
    if (event.button !== 0 || !area) return;
    const handle = event.currentTarget;
    handle.setPointerCapture(event.pointerId);
    const move = (e: PointerEvent) => {
      const box = localRect(area);
      const usable = box.height - HANDLE;
      if (usable <= 0) return;
      const y = e.clientY / scaleOf(area) - box.top;
      setEngineShare(Math.min(0.85, Math.max(0.3, (y - HANDLE / 2) / usable)));
    };
    const up = () => {
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", up);
      handle.removeEventListener("pointercancel", up);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up);
    handle.addEventListener("pointercancel", up);
  };
  const uptime = (
    <>
      5<small>天</small>&nbsp;14:{String(Math.floor(p.tick / 60) % 60).padStart(2, "0")}:
      {String(((p.tick % 60) + 60) % 60).padStart(2, "0")}
    </>
  );
  let body: ReactNode;
  if (p.device === "cpu") {
    const head = (
      <Head title="CPU" model={machine.cpu}>
        <Segments
          label="CPU 页面"
          value={cpuPage}
          onChange={setCpuPage}
          items={[
            { value: "overview", label: "概览" },
            { value: "details", label: "详情" },
          ]}
        />
      </Head>
    );
    if (cpuPage === "overview") {
      const { caption, rowMin, rows } = cores_;
      // The processors' temperatures, for their tooltips in the grid.
      const temperatures = graph === "logical" ? logicalTemperatures(p.tick) : [];
      body = (
        <>
          {head}
          <div className="a-cpu-graphs" ref={graphs} onContextMenu={cpuMenu}>
          {graph === "overall" ? (
            <Chart
              title="利用率 %"
              top="100%"
              max={100}
              curve={curve}
              fill
              readout={{ names: kernel ? ["总计", "内核"] : ["总计"], format: percentOf }}
              series={[
                { values: h.cpu, color: "var(--a-cpu)" },
                ...(kernel ? [{ values: h.kernel, color: "var(--a-cpu)", dashed: true, fill: 0.18 }] : []),
              ]}
            />
          ) : (
            <div
              className="a-cores-grid"
              ref={coreGrid}
              style={{ "--cols": cores_.columns, "--row": rowMin + "px", minHeight: Math.max(rows * (rowMin + CORE_GAP) - CORE_GAP, cores_.floor) } as CSSProperties}
            >
              {cores.map((core) => {
                const color = core.kind === "p" ? "var(--a-cpu)" : "var(--a-cpu-e)";
                const ghz = clock(core.kind, p.tick + core.index).toFixed(2) + " GHz";
                const load = Math.round(core.values[core.values.length - 1]);
                // What the caption leaves out, the tooltip tells, below the processor's name.
                const tip = [
                  `逻辑处理器 ${core.index}，物理核心 ${core.kind === "p" ? core.index >> 1 : core.index - 6}（${core.kind === "p" ? "性能核" : "能效核"}）`,
                  ...(caption === 2 ? [`利用率: ${load}%`] : []),
                  ...(caption !== 0 ? [`速度: ${ghz}`] : []),
                  `温度: ${Math.round(temperatures[core.index])} °C`,
                  ...(kernel ? [`内核时间: ${Math.round(core.kernel[core.kernel.length - 1])}%`] : []),
                ].join("\n");
                return (
                  <div className="a-core" key={core.index} data-core={core.index} title={tip}>
                    {caption !== 2 && (
                      <div className="a-core-head">
                        <span className="a-core-name">
                          <span className="a-kind" style={{ "--c": color } as CSSProperties}>
                            {core.kind.toUpperCase()}
                          </span>
                          <b>{core.index}</b>
                          {caption === 0 && <span className="a-core-clock">{ghz}</span>}
                        </span>
                        <strong>
                          {load}
                          <small>%</small>
                        </strong>
                      </div>
                    )}
                    <div className="a-chart-frame" style={{ "--c": color } as CSSProperties}>
                      <div className="a-grid" aria-hidden="true" />
                      <SteadyCurve
                        series={[
                          { values: core.values, color },
                          ...(kernel ? [{ values: core.kernel, color: `color-mix(in srgb, ${color} 60%, transparent)`, fill: 0.4 }] : []),
                        ]}
                        max={100}
                        {...curve}
                      />
                    </div>
                  </div>
                );
              })}
            </div>
          )}
          </div>
          {graph === "overall" ? (
            <Legend
              items={[
                ["总计", "var(--a-cpu)"],
                ...(kernel ? [["内核时间", "var(--a-cpu)", "dashed"] as [string, string, string]] : []),
              ]}
            />
          ) : (
            <Legend
              items={[
                ["性能核", "var(--a-cpu)"],
                ["能效核", "var(--a-cpu-e)"],
              ]}
            />
          )}
          <Stats
            cpu
            figures={[
              ["利用率", Math.round(s.cpu), "%"],
              ["速度", clock("p", p.tick).toFixed(2), "GHz"],
              ["性能核频率", (clock("p", p.tick) - 0.41).toFixed(2), "GHz"],
              ["能效核频率", clock("e", p.tick).toFixed(2), "GHz"],
              ["温度", Math.round(cpuTemperature(p.tick)), "°C"],
              // One power figure, its source named on hover.
              ["功耗", wattsText(cpuPower(p.tick)), "W", undefined, "封装功耗"],
              ["进程", p.processCount],
              ["线程", thousands(p.threads)],
              ["句柄", thousands(p.handles)],
              ["正常运行时间", uptime],
            ]}
            specs={[
              ["基准速度", <span title="来源：CPUID 额定频率">2.50 GHz</span>],
              ["插槽", "1"],
              ["内核", "14（6 性能核 + 8 能效核）"],
              ["逻辑处理器", "20"],
              ["虚拟化", "已启用"],
              ["L1 缓存", "1.2 MB"],
              ["L2 缓存", "11.0 MB"],
              ["L3 缓存", "24.0 MB"],
            ]}
          />
        </>
      );
    } else
      body = (
        <>
          {head}
          <CpuDetails tick={p.tick} interval={p.interval} curve={curve} />
        </>
      );
  } else if (p.device === "memory") {
    // The composition: in use (its compressed part at its end, in stripes),
    // modified, standby and free (memory_composition, theme::memory).
    const resident = 512 * MB;
    const stored = 1.4 * GB;
    const modified = 256 * MB;
    const free = 1.26 * GB;
    const standby = totalMemory - used - modified - free;
    const legend: [string, string, string][] = [
      ["is-mem", "由进程、驱动程序或操作系统使用的内存", `使用中 ${bytes(used)}`],
      ["is-compressed", "以压缩形式保存进程页面的内存，属于使用中的内存", `已压缩 ${bytes(resident)}`],
      ["is-mem-mod", "内容必须先写入磁盘才能用于其他用途的内存", `已修改 ${bytes(modified)}`],
      ["is-mem-standby", "包含未频繁使用的缓存数据和代码的内存", `备用 ${bytes(standby)}`],
      ["is-hollow", "尚未使用，可立即分配的内存", `空闲 ${bytes(free)}`],
    ];
    const hint = [
      `使用中 ${bytes(used)}`,
      "由进程、驱动程序或操作系统使用的内存",
      "",
      `已压缩 ${bytes(resident)}`,
      `压缩内存估计存储了 ${bytes(stored)} 的数据，为系统节省了 ${bytes(stored - resident)} 内存`,
    ].join("\n");
    body = (
      <>
        <Head title="内存" model="32 GB DDR5" />
        <Chart
          title="内存使用量"
          top="32.0 GB"
          max={100}
          curve={curve}
          fill
          color="var(--a-mem)"
          series={[{ values: h.mem, color: "var(--a-mem)" }]}
          readout={{ format: (v) => gigabytes((v / 100) * 32) }}
        />
        <MemoryBar
          parts={[
            ["is-mem", used - resident],
            ["a-compressed", resident],
            ["is-mem-mod", modified],
            ["is-mem-standby", standby],
          ]}
        />
        <div className="a-mem-legend">
          {legend.map(([kind, description, text]) => (
            <span key={kind}>
              <i className={kind} title={description} />
              {text}
            </span>
          ))}
        </div>
        <Stats
          figures={[
            [
              "使用中（已压缩）",
              // "12.8 GB (512 MB)", as Task Manager's "In use (Compressed)".
              <>
                {(used / GB).toFixed(1)}
                <small>GB</small>
                <span className="a-paren">(512</span>
                <small>MB</small>)
              </>,
              undefined,
              undefined,
              hint,
            ],
            ["可用", ((totalMemory - used) / GB).toFixed(1), "GB"],
            ["已提交", `${((used / GB) * 1.27).toFixed(1)} / 48.0`, "GB"],
            ["已缓存", "7.7", "GB"],
            ["分页缓冲池", "640.0", "MB"],
            ["非分页缓冲池", "310.0", "MB"],
          ]}
          specs={[
            ["速度", "5600 MT/s"],
            ["已使用的插槽", "2 / 2"],
            ["外形规格", "SODIMM"],
            ["类型", "DDR5"],
            ["为硬件保留", "214 MB"],
          ]}
        />
      </>
    );
  } else if (diskOf(p.device)) {
    const d = diskOf(p.device)!;
    const r = diskAt(d.index, p.tick);
    const read = series((t) => diskAt(d.index, t).read);
    const write = series((t) => diskAt(d.index, t).write);
    // The transfer chart's top: the disk steps over the higher of the two.
    const diskMax = rateCeiling(Math.max(...read, ...write));
    const [readValue, readUnit] = split(byteRate(r.read));
    const [writeValue, writeUnit] = split(byteRate(r.write));
    const head = (
      <Head title={d.name} model={d.model}>
        <div className="a-disk-controls">
          <Segments
            label="磁盘页面"
            value={diskPage}
            onChange={setDiskPage}
            items={[
              { value: "overview", label: "概览" },
              { value: "details", label: "详情" },
            ]}
          />
          {diskPage === "details" && (
            <Button icon="restart" small tone="ghost" title="重新读取" onClick={() => setDiskRead(p.tick)} />
          )}
        </div>
      </Head>
    );
    if (diskPage === "details")
      body = (
        <>
          {head}
          <DiskDetails disk={d.index} read={diskRead} interval={p.interval} hex={!!p.pages.rawHex} onHex={(rawHex) => p.onPages({ rawHex })} />
        </>
      );
    else
      body = (
        <>
          {head}
          <div className="a-stack is-fill">
            <Chart title="活动时间" top="100%" max={100} curve={curve} color={d.color} series={[{ values: series((t) => diskAt(d.index, t).active), color: d.color }]} />
            <Chart
              title="磁盘传输速率"
              top={rateLabel(diskMax)}
              max={diskMax}
              curve={curve}
              readout={{ names: ["读取", "写入"], format: byteRate }}
              color={d.color}
              series={[
                { values: read, color: d.color },
                { values: write, color: d.color, dashed: true, fill: false },
              ]}
            />
          </div>
          <Legend
            items={[
              ["读取", d.color],
              ["写入", d.color, "dashed"],
            ]}
          />
          <Stats
            figures={[
              ["活动时间", Math.round(r.active), "%"],
              ["平均响应时间", r.response.toFixed(1), "毫秒"],
              ["读取速度", readValue, readUnit],
              ["写入速度", writeValue, writeUnit],
              ["温度", Math.round(r.temperature), "°C"],
            ]}
            specs={[
              ["容量", (d.size / 1024 ** 4).toFixed(1) + " TB"],
              ["类型", d.kind],
              ["卷", d.volumes.join(" ")],
              // Reported by the NVMe drive; the hard disk names no threshold.
              ...(d.ssd ? [["警告温度", "90 °C"] as [string, string]] : []),
            ]}
          />
        </>
      );
  } else if (adapterOf(p.device)) {
    const a = adapterOf(p.device)!;
    const r = adapterAt(a.index, p.tick);
    const rx = series((t) => adapterAt(a.index, t).rx);
    const tx = series((t) => adapterAt(a.index, t).tx);
    const netMax = rateCeiling(Math.max(...rx, ...tx), true);
    const [rxValue, rxUnit] = split(byteRate(r.rx));
    const [txValue, txUnit] = split(byteRate(r.tx));
    const wifi = a.kind === "wifi";
    body = (
      <>
        <Head title={a.name} model={a.model} />
        <Chart
          title="吞吐量"
          top={rateLabel(netMax)}
          max={netMax}
          curve={curve}
          // The rate in the chosen unit, then in the other one.
          readout={{ names: ["接收", "发送"], format: (v) => `${byteRate(v)}（${bitRate(v)}）` }}
          fill
          color={a.color}
          series={[
            { values: rx, color: a.color },
            { values: tx, color: a.color, dashed: true, fill: false },
          ]}
        />
        <Legend
          items={[
            ["接收", a.color],
            ["发送", a.color, "dashed"],
          ]}
        />
        <Stats
          figures={[
            ["接收", rxValue, rxUnit, bitRate(r.rx)],
            ["发送", txValue, txUnit, bitRate(r.tx)],
          ]}
          specs={[
            ["连接类型", wifi ? "Wi-Fi" : "虚拟适配器"],
            ["链路速度", linkText(a.link)],
            ["IPv4 地址", wifi ? "192.0.2.24" : "198.51.100.1"],
            [
              "IPv6 地址",
              // WSL's switch carries no IPv6 address.
              wifi ? (
                <>
                  <span>2001:db8::24</span>
                  <span>2001:db8::8a2e:370:7334</span>
                </>
              ) : (
                "—"
              ),
            ],
            ["物理地址", wifi ? "02-00-00-00-00-01" : "02-00-00-00-00-02"],
          ]}
        />
      </>
    );
  } else if (gpuOf(p.device)) {
    const g = gpuOf(p.device)!;
    const engineNames: string[] = [...g.engines];
    const engineChoice = engineChoices[g.model] ?? null;
    const setEngineChoice = (choice: EngineChoice) => setEngineChoices((all) => ({ ...all, [g.model]: choice }));
    const engineValues = engineNames.map((_, k) => series((t) => Math.min(100, gpuAt(g.index, t).engines[k])));
    const busy = (name: string) => engineValues[engineNames.indexOf(name)].some((v) => v >= BUSY_PERCENT);
    const shown = shownEngines(engineNames, engineChoice, busy);
    const smart = isSmart(engineChoice);
    const plan = planGrid(engineLayout, shown.length, engineArea.width, engineArea.height, ENGINE_CELL, CORE_GAP);
    const count = engineNames.length;
    const r = gpuAt(g.index, p.tick);
    const gb = (bytes: number) => (bytes / GB).toFixed(1);
    // The NVIDIA GPU's readings come from NVML; the integrated one's from the
    // kernel sensors (pages/performance/engines.rs).
    const figures: Figure[] = g.integrated
      ? [
          ["GPU 温度", Math.round(r.temperature), "°C"],
          ["核心频率", thousands(r.clock), "MHz", undefined, "内核读数按倍率和标称步进换算。核心休眠或关闭时可显示 0 MHz。最低工作频率为硬件规格。"],
          ["共享内存频率", "2,800", "MHz", undefined, "内核读数按内存控制器配置和标称基准时钟换算。DDR 传输率约为此频率的两倍。"],
          ["共享逻辑频率", thousands(Math.round((r.clock * 0.8 + 150) / 50) * 50), "MHz", undefined, "核显非切片部分（Unslice）的时钟，与图形计算切片可分别停钟。"],
          ["核心 VID", (0.58 + r.clock / 5000 + noise(291, p.tick) * 0.01).toFixed(3), "V", undefined, "核显请求的目标供电电压（VID）。"],
          ["GT 活动率", r.engines[0].toFixed(1), "%", undefined, "硬件活动率与 Windows 引擎利用率的统计范围不同。"],
          ["媒体活动率", (r.engines[2] + r.engines[3] * 0.5).toFixed(1), "%", undefined, "硬件活动率与 Windows 引擎利用率的统计范围不同。"],
          ["功耗", wattsText(r.power), "W"],
        ]
      : [
          ["GPU 温度", Math.round(r.temperature), "°C"],
          ["核心频率", thousands(r.clock), "MHz"],
          ["显存频率", "7,001", "MHz"],
          ["功耗", wattsText(r.power), "W"],
        ];
    const specs: [string, ReactNode][] = g.integrated
      ? [
          ["功耗来源", "Intel RAPL MSR · 核显"],
          ["核显内核传感器", <span data-i18n="sensor">已连接</span>],
          ["驱动版本", "32.0.101.6881"],
          ["驱动日期", "2026-09-17"],
          ["DirectX 版本", "12（FL 12_1）"],
          ["引擎", String(count)],
          ["最高核心频率", "1,550 MHz"],
          ["最低工作频率", "100 MHz"],
          ["基准时钟", "38.4 MHz"],
          [
            "性能限制",
            // Each reason, whether it limits now, and whether it did since the last read.
            <span title={GPU_LIMITS.map((reason) => `${reason}: 未触发`).join("\n")}>未触发</span>,
          ],
        ]
      : [
          ["功耗来源", "NVIDIA NVML · 板卡"],
          ["驱动版本", "32.0.15.8157"],
          ["驱动日期", "2026-10-08"],
          ["DirectX 版本", "12（FL 12_2）"],
          ["功耗上限", "115 W"],
          ["最高温度", "105 °C"],
          ["引擎", String(count)],
          ["最高核心频率", "3,105 MHz"],
        ];
    body = (
      <>
        <Head title={g.name} model={g.model} />
        <div className="a-perf-tools">
          <Select
            key={g.id}
            label="显示的引擎"
            value={smart ? "smart" : String(shown.length)}
            onChange={(v) => setEngineChoice(v === "smart" ? withSmart(engineNames, engineChoice) : withCount(engineNames, engineChoice, Number(v), shown))}
            title={smart ? "仅显示曲线时间范围内有占用的引擎" : undefined}
            options={[
              { value: "smart", label: "智能显示" },
              ...Array.from({ length: count }, (_, i) => ({ value: String(i + 1), label: i + 1 === count ? `全部 ${count} 个引擎` : `${i + 1} 个引擎` })),
            ]}
          />
          <Segments
            key="gpu-layout"
            label="引擎图布局"
            value={engineLayout}
            onChange={setEngineLayout}
            items={[
              { value: "grid", label: "网格" },
              { value: "focus", label: "焦点" },
              { value: "rows", label: "列表" },
            ]}
          />
        </div>
        <div className="a-chart-caption a-engine-caption">
          <span>引擎利用率</span>
          <span>100%</span>
        </div>
        <div className="a-gpu-charts" ref={gpuCharts}>
          <div
            className="a-engines"
            ref={engineGrid}
            style={{
              flexGrow: engineShare,
              gridTemplateColumns: `repeat(${plan.columns}, minmax(0, 1fr))`,
              gridTemplateRows: `repeat(${plan.rows}, minmax(${ENGINE_CELL.height}px, 1fr))`,
              minHeight: (ENGINE_CELL.height + CORE_GAP) * plan.stack - CORE_GAP,
            }}
          >
            {shown.map((name, slot) => {
              const at = plan.cells[slot] ?? { column: 1, columns: 1, row: 1, rows: 1 };
              const values = engineValues[engineNames.indexOf(name)];
              return (
                <div
                  className="a-engine"
                  key={g.id + ":" + name}
                  style={{ gridColumn: `${at.column} / span ${at.columns}`, gridRow: `${at.row} / span ${at.rows}` }}
                >
                  <div className="a-engine-head">
                    <Select
                      className="a-engine-select"
                      label="图中显示的引擎"
                      value={name}
                      displayLabel={name}
                      options={engineNames.map((e) => ({ value: e, label: e }))}
                      onChange={(engine) => setEngineChoice(withEngine(engineNames, engineChoice, slot, engine, shown))}
                    />
                    <b>
                      {Math.round(values[values.length - 1])}
                      <small>%</small>
                    </b>
                  </div>
                  <div className="a-chart-frame" style={{ "--c": "var(--a-gpu)" } as CSSProperties}>
                    <div className="a-grid" aria-hidden="true" />
                    <SteadyCurve series={[{ values, color: "var(--a-gpu)" }]} max={100} {...curve} />
                  </div>
                </div>
              );
            })}
          </div>
          <div
            className="a-engine-split"
            role="separator"
            aria-orientation="horizontal"
            onPointerDown={pressShare}
            onDoubleClick={() => setEngineShare(SHARE_DEFAULT)}
          >
            <i />
          </div>
          <div className="a-two a-gpu-memory" style={{ flexGrow: 1 - engineShare }}>
            {/* Memory in GB; a GPU without memory of its own scales to 1 (its top reads 0.0 GB). */}
            {(
              [
                ["专用 GPU 内存", g.dedicated, (t: number) => gpuAt(g.index, t).dedicated],
                ["共享 GPU 内存", g.shared, (t: number) => gpuAt(g.index, t).shared],
              ] as const
            ).map(([title, total, used]) => (
              <Chart
                key={g.id + title}
                title={title}
                top={`${gb(total)} GB`}
                max={total > 0 ? total / GB : 1}
                curve={curve}
                fill
                min={64}
                readout={{ format: gigabytes }}
                color={g.color}
                series={[{ values: series((t) => used(t) / GB), color: g.color }]}
              />
            ))}
          </div>
        </div>
        <Stats
          figures={[
            ["利用率", Math.round(r.util), "%", "Windows 引擎", "整体占用取最忙引擎，并参考进程和驱动读数。各来源的统计范围和时段可能不同。"],
            ["专用 GPU 内存", `${gb(r.dedicated)} / ${gb(g.dedicated)}`, "GB"],
            ["共享 GPU 内存", `${gb(r.shared)} / ${gb(g.shared)}`, "GB"],
            ...figures,
          ]}
          specs={specs}
        />
      </>
    );
  } else if (p.device==="npu") {
    body=<><Head title="NPU 0" model="Contoso Neural Accelerator"/><Chart title="利用率 %" top="100%" max={100} curve={curve} color="var(--a-npu)" fill series={[{values:seriesOf(samples,npuLoad,p.tick),color:"var(--a-npu)"}]} readout={{format:percentOf}}/><Stats figures={[["利用率",Math.round(npuLoad(p.tick)),"%"]]} specs={[]}/></>;
  } else {
    body = (
      <>
        <Head title="电池" model="Contoso CT-4410" />
        <div className="a-stack is-fill">
          <Chart title="电量" top="100%" max={100} curve={curve} color="var(--a-bat)" series={[{ values: seriesOf(samples, battery, p.tick), color: "var(--a-bat)" }]} />
          <Chart title="放电功率" top="20 W" max={20} curve={curve} readout={{ format: watts }} color="var(--a-bat)" series={[{ values: seriesOf(samples, discharge, p.tick), color: "var(--a-bat)" }]} />
        </div>
        <Stats
          figures={[
            ["电量", "84", "%"],
            ["状态", "正在使用电池"],
            ["放电功率", wattsText(discharge(p.tick)), "W"],
            ["电压", (16.2 + noise(72, p.tick) * 0.05).toFixed(2), "V"],
            ["剩余时间", "2 小时 31 分钟"],
            ["健康度", "90", "%"],
            ["循环次数", "120"],
          ]}
          specs={[
            ["当前容量", "60.5 Wh"],
            ["完全充电容量", "72.0 Wh"],
            ["设计容量", "80.0 Wh"],
            ["化学成分", "锂离子"],
            ["制造商", "Contoso"],
            ["型号", "CT-4410"],
            ["序列号", "0001"],
          ]}
        />
      </>
    );
  }
  // The list in its order, without the devices taken out of it.
  const listed = order.map((id) => rail.find((r) => r.id === id)!).filter((r) => !hidden.includes(r.id));
  const index = listed.findIndex((r) => r.id === p.device);
  const moving = markMoves.current;
  markMoves.current = true;
  // Hiding the device shown shows the one after it, or before it at the end.
  const hide = (id: Device, on: boolean) => {
    if (on && listed.length <= 1) return;
    p.onPages({ hidden: on ? [...hidden, id] : hidden.filter((d) => d !== id) });
    if (on && id === p.device) {
      const at = listed.findIndex((r) => r.id === id);
      p.onDevice((listed[at + 1] ?? listed[at - 1]).id);
    }
  };
  const deviceMenu = (event: ReactMouseEvent, target: Device | null) => {
    event.preventDefault();
    const item = target ? rail.find((r) => r.id === target) : undefined;
    p.onMenu(event, [
      ...(item
        ? ([
            {
              label: `隐藏“${item.name}”`,
              icon: "eyeOff",
              disabled: listed.length <= 1,
              run: () => hide(item.id, true),
            },
            "-",
          ] satisfies MenuEntry[])
        : []),
      {
        label: "显示的设备",
        icon: "eye",
        detail: `${listed.length}/${rail.length}`,
        children: order.map((id) => {
          const shown = !hidden.includes(id);
          return {
            label: rail.find((r) => r.id === id)!.name,
            checked: shown,
            disabled: shown && listed.length <= 1,
            stay: true,
            run: () => hide(id, shown),
          };
        }),
      },
      // One disk: nothing to order (the app's needs two).
      {
        label: "磁盘排序",
        icon: "order",
        disabled: true,
        children: [
          { label: "按磁盘编号", checked: true },
          { label: "按盘符" },
          { label: "自定义（拖动调整）" },
        ],
      },
      "-",
      { label: "恢复默认显示", icon: "reset", disabled: !hidden.length, run: () => p.onPages({ hidden: [] }) },
      {
        label: "恢复默认顺序",
        icon: "reset",
        disabled: order.join() === defaultDevices.join(),
        run: () => p.onPages({ order: defaultDevices }),
      },
    ]);
  };
  // Dragging an entry up or down: it rises over the list under the pointer
  // and its place stays as a tinted slot, the others make way, and it settles
  // into its new place when let go.
  const [dragging, setDragging] = useState<{ id: Device; dy: number; to: number } | null>(null);
  const drag = useRef<{ id: Device; dy: number; to: number } | null>(null);
  const dragged = useRef(false);
  const press = (event: ReactPointerEvent<HTMLButtonElement>, id: Device) => {
    if (event.button !== 0) return;
    const el = event.currentTarget;
    const k = scaleOf(el);
    const y0 = event.clientY;
    const from = listed.findIndex((r) => r.id === id);
    let started = false;
    dragged.current = false;
    const move = (e: PointerEvent) => {
      // Drawn within the list.
      const dy = Math.max(-from * ROW, Math.min((listed.length - 1 - from) * ROW, (e.clientY - y0) / k));
      if (!started) {
        if (Math.abs(dy) < 4) return;
        started = true;
        dragged.current = true;
        el.setPointerCapture(e.pointerId);
      }
      const to = Math.max(0, Math.min(listed.length - 1, from + Math.round(dy / ROW)));
      setDragging((drag.current = { id, dy, to }));
    };
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", up);
      if (!started) return;
      const d = drag.current;
      drag.current = null;
      setDragging(null);
      if (d && d.to !== from) {
        const ids = listed.map((r) => r.id);
        ids.splice(d.to, 0, ids.splice(from, 1)[0]);
        // The hidden devices keep their places among the rest.
        const next = [...ids];
        order.forEach((o, i) => hidden.includes(o) && next.splice(Math.min(i, next.length), 0, o));
        p.onPages({ order: next });
      }
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", up);
  };
  const shift = (i: number, id: Device) => {
    if (!dragging) return 0;
    if (id === dragging.id) return dragging.dy;
    const from = listed.findIndex((r) => r.id === dragging.id);
    if (from < i && i <= dragging.to) return -ROW;
    if (dragging.to <= i && i < from) return ROW;
    return 0;
  };
  const liftedFrom = dragging ? listed.findIndex((r) => r.id === dragging.id) : -1;
  // A row the dragged one passes over fades as far as it is covered.
  const covered = (i: number, id: Device) => {
    if (!dragging || id === dragging.id) return 0;
    const top = liftedFrom * ROW + dragging.dy;
    const own = i * ROW + shift(i, id);
    return Math.max(0, Math.min(own + ROW_HEIGHT, top + ROW_HEIGHT) - Math.max(own, top)) / ROW_HEIGHT;
  };
  // The list's width: dragged at the line between it and the device's card
  // (double-click restores it), at most half the page.
  const perf = useRef<HTMLDivElement>(null);
  const railBox = useRef<HTMLElement>(null);
  const pressSplitter = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || !railBox.current || !perf.current) return;
    const handle = event.currentTarget;
    handle.setPointerCapture(event.pointerId);
    const k = scaleOf(handle);
    const x0 = event.clientX;
    const start = railBox.current.offsetWidth;
    const page = perf.current.clientWidth - 24;
    const move = (e: PointerEvent) =>
      setSide(Math.round(Math.max(SIDE_MIN, Math.min(SIDE_MAX, page / 2, start + (e.clientX - x0) / k))));
    const up = () => {
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", up);
      handle.removeEventListener("pointercancel", up);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up);
    handle.addEventListener("pointercancel", up);
  };
  return (
    <div className="a-perf" ref={perf}>
      <nav
        ref={railBox}
        className={"a-rail" + (dragging ? " is-dragging" : "")}
        style={{ "--side": side + "px" } as CSSProperties}
        aria-label="设备"
        onContextMenu={(event) => event.target === event.currentTarget && deviceMenu(event, null)}
      >
        {dragging && <span className="a-rail-slot" style={{ transform: `translateY(${dragging.to * ROW}px)` }} aria-hidden="true" />}
        <span
          className={"a-rail-mark" + (moving ? " is-moving" : "")}
          style={{ transform: `translateY(${index * ROW + shift(index, p.device)}px)` }}
          aria-hidden="true"
        />
        {listed.map((item, i) => (
          <button
            key={item.id}
            type="button"
            className={"a-rail-item" + (dragging?.id === item.id ? " is-lifted" : "")}
            style={dragging ? { transform: `translateY(${shift(i, item.id)}px)`, opacity: 1 - covered(i, item.id) } : undefined}
            aria-current={p.device === item.id ? "true" : undefined}
            data-tour={"device-" + item.id}
            onPointerDown={(event) => press(event, item.id)}
            onClick={() => {
              if (dragged.current) dragged.current = false;
              else p.onDevice(item.id);
            }}
            onContextMenu={(event) => deviceMenu(event, item.id)}
          >
            <span className="a-rail-text">
              <b>{item.name}</b>
              <span>{item.info}</span>
            </span>
            <span className="a-rail-chart" style={{ "--c": item.color } as CSSProperties}>
              <SteadyCurve
                // Moved by a drag or the list's width, it is drawn again to stay on whole pixels.
                moving={dragging ? dragging.dy : side}
                series={item.values.map((values, i) =>
                  i === 0 ? { values, color: item.color } : { values, color: item.color, fill: false, faint: true },
                )}
                max={item.max}
                {...curve}
              />
            </span>
          </button>
        ))}
      </nav>
      <div
        className="a-perf-gap"
        role="separator"
        aria-orientation="vertical"
        onPointerDown={pressSplitter}
        onDoubleClick={() => setSide(SIDE_DEFAULT)}
      >
        <i />
      </div>
      <div className="a-perf-main a-scroll">
        {/* The view moves inside a clip: moved in the scroll area itself, its
            offset counted in the scroll range and showed a scrollbar. */}
        <div className="a-perf-stage">
          <div
            key={p.device}
            ref={view}
            className={
              "a-perf-body" +
              ((p.device === "cpu" && cpuPage === "details") || (diskOf(p.device) && diskPage === "details") ? " is-details" : "")
            }
          >
            {body}
          </div>
        </div>
      </div>
    </div>
  );
}
