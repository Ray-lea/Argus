import {
  createEffect,
  createMemo,
  createSignal,
  For,
  type JSX,
  Match,
  on,
  onCleanup,
  Show,
  Switch,
  untrack,
} from "solid-js";
import { type RateUnit, rateCeiling, rateCeilingLabel, roundCeiling } from "../lib/chart-scale";
import { coreGridColumns } from "../lib/core-grid";
import { ENGINE_LAYOUTS, type EngineLayout, type GridPlan, planGrid, samePlan } from "../lib/engine-grid";
import {
  fmtCache,
  fmtCapacity,
  fmtCount,
  fmtGB,
  fmtGHz,
  fmtLinkSpeed,
  fmtMem,
  fmtNet,
  fmtNetAlt,
  fmtRate,
  netParts,
  netUnit,
  rateParts,
} from "../lib/format";
import {
  type EngineChoice,
  isSmart,
  shownEngines,
  wasBusy,
  withCount,
  withEngine,
  withSmart,
} from "../lib/gpu-engines";
import { gridReflow } from "../lib/grid-reflow";
import { rich, t } from "../lib/i18n";
import { memoryComposition } from "../lib/memory-composition";
import { chartMotion, EASE_OUT, play } from "../lib/motion";
import { persisted } from "../lib/prefs";
import { cpuTemperature, hasCpuDieTemperature } from "../lib/sensors";
import {
  cpuMode,
  HISTORY,
  intervalMs,
  perfDevice,
  series,
  setCpuMode,
  setPerfDevice,
  snapshot,
} from "../lib/store";
import type { BatteryStats, DiskStats, Hardware, Snapshot, SystemStats } from "../lib/types";
import {
  BoardSensors,
  boardSummary,
  CoreTable,
  coreSummary,
  hasCoreColumns,
  IdleStates,
  idleSummary,
  limitSummary,
  PackageLimits,
  SensorGroups,
  sensorSummary,
} from "./CpuTelemetry";
import Indicator from "./Indicator";
import { ChevronDown, ChevronUp } from "./icons";
import Select from "./Select";
import Splitter from "./Splitter";
import { Chart, type Series } from "./Trace";

const MB = 1024 * 1024;

/** Detail views render only once a snapshot exists (guarded by the page). */
const now = () => snapshot() as Snapshot;

// ---- Building blocks ---------------------------------------------------------

/** The highest point of the series, at least `floor` so an idle device stays flat. */
const peak = (floor: number, ...keys: string[]) =>
  Math.max(floor, ...keys.flatMap((k) => series(k)).filter(Number.isFinite));
/** A round top for a rate chart, in the unit its label shows; the chart eases between tops. */
const rateTop = (unit: () => RateUnit, floor: number, ...keys: string[]) =>
  createMemo(() => rateCeiling(peak(floor, ...keys), unit()));
/** The same for watts. */
const wattTop = (floor: number, ...keys: string[]) => createMemo(() => roundCeiling(peak(floor, ...keys)));
const bytes = (): RateUnit => "bytes";

const pct = (v: number) => `${Math.round(v)}%`;
const gb = (v: number) => `${fmtGB(v)} GB`;

/** Time a chart spans at the current refresh rate. */
function spanLabel() {
  const secs = (HISTORY * intervalMs()) / 1000;
  return secs >= 120
    ? t("performance.span.minutes", { count: secs / 60 })
    : t("performance.span.seconds", { count: secs });
}

/** A chart's top; a new one fades in while the curve eases to it. */
function ScaleLabel(props: { text: string }) {
  let el!: HTMLSpanElement;
  // Worked out again on every sample (memory sizes come with each snapshot):
  // only a different text fades in.
  const text = createMemo(() => props.text);
  createEffect(
    on(
      text,
      () => {
        if (chartMotion()) el.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 320, easing: EASE_OUT });
      },
      { defer: true },
    ),
  );
  return <span ref={el}>{text()}</span>;
}

function ChartBox(props: {
  label: string;
  max: string;
  colorVar: string;
  /** main: the view's headline chart; half: one of a stacked pair; small: side by side. */
  size: "main" | "half" | "small";
  series: Series[];
  scale: () => number;
  format: (v: number) => string;
}) {
  return (
    <div class="chart-box">
      <div class="chart-labels">
        <span>{props.label}</span>
        <ScaleLabel text={props.max} />
      </div>
      <div class={`chart-frame ${props.size}`} style={{ "--c": `var(${props.colorVar})` }}>
        <Chart series={props.series} max={props.scale} format={props.format} />
      </div>
      <div class="chart-labels">
        <span>{spanLabel()}</span>
        <span>0</span>
      </div>
    </div>
  );
}

/**
 * Keeps the logical processors' grid filling the height the view leaves it:
 * rows stretch (CSS), the column count follows the area (lib/core-grid.ts).
 * Minimum cell size and gap come from the grid's CSS.
 */
function fitGrid(el: HTMLElement, count: () => number, chooseColumns: typeof coreGridColumns) {
  let size = { width: 0, height: 0 };
  let columns = 0;
  let previousCount = 0;
  const reflow = gridReflow(el);
  const fit = () => {
    if (!size.width || !size.height) return;
    const style = getComputedStyle(el);
    const total = count();
    const sameCount = total === previousCount;
    const cols = chooseColumns(
      {
        count: total,
        ...size,
        cellWidth: Number.parseFloat(style.getPropertyValue("--cell-w")),
        cellHeight: Number.parseFloat(style.getPropertyValue("--cell-h")),
        headerHeight: Number.parseFloat(style.getPropertyValue("--cell-head")) || 0,
        gap: Number.parseFloat(style.columnGap) || 0,
      },
      sameCount ? columns : 0,
    );
    previousCount = total;
    if (!sameCount) reflow.cancel();
    if (cols === columns) return;
    const animate = columns > 0 && sameCount;
    columns = cols;
    const apply = () => el.style.setProperty("--cols", String(cols));
    if (animate) reflow.change(apply);
    else apply();
  };
  // Layout sizes: the view's settle animation scales the grid for a moment.
  const observer = new ResizeObserver(([entry]) => {
    size = { width: entry.contentRect.width, height: entry.contentRect.height };
    fit();
  });
  observer.observe(el);
  createEffect(on(count, fit, { defer: true }));
  onCleanup(() => observer.disconnect());
}

/**
 * Lays out the GPU's engine charts (lib/engine-grid.ts) in the area the view
 * leaves them, each in the place the plan gives it, and carries each change
 * through the grid's transition (lib/grid-reflow.ts): another arrangement as
 * the area changes, and through `change`, another layout or engines coming
 * and going.
 */
function engineGrid(el: HTMLElement, count: () => number, layout: () => EngineLayout) {
  const reflow = gridReflow(el);
  let size = { width: 0, height: 0 };
  let plan: GridPlan | null = null;
  /** The plan for the grid as it is now. */
  const fresh = () => {
    if (!size.width || !size.height) return null;
    const style = getComputedStyle(el);
    return planGrid(
      layout(),
      count(),
      size.width,
      size.height,
      {
        width: Number.parseFloat(style.getPropertyValue("--cell-w")),
        height: Number.parseFloat(style.getPropertyValue("--cell-h")),
      },
      Number.parseFloat(style.columnGap) || 0,
    );
  };
  /** Gives the grid its tracks and each chart its place: the charts can change while the plan stays. */
  const place = (p: GridPlan | null) => {
    if (!p) return;
    plan = p;
    el.style.setProperty("--cols", String(p.columns));
    el.style.setProperty("--rows", String(p.rows));
    el.style.setProperty("--stack", String(p.stack));
    const cells = [...el.children].filter((cell) => !cell.classList.contains("grid-leaving"));
    cells.forEach((cell, i) => {
      const at = p.cells[i];
      (cell as HTMLElement).style.gridArea = at
        ? `${at.row} / ${at.column} / span ${at.rows} / span ${at.columns}`
        : "";
    });
  };
  const observer = new ResizeObserver(([entry]) => {
    size = { width: entry.contentRect.width, height: entry.contentRect.height };
    const p = fresh();
    if (!p || (plan && samePlan(plan, p))) return;
    if (plan) reflow.change(() => place(p));
    else place(p);
  });
  observer.observe(el);
  onCleanup(() => observer.disconnect());
  return {
    /** Runs `update` (which changes the charts) and the layout that follows as one transition. */
    change: (update?: () => void) =>
      reflow.change(() => {
        update?.();
        place(fresh());
      }),
  };
}

function Stat(props: { label: string; value: JSX.Element; unit?: string; note?: string; hint?: string }) {
  return (
    <div title={props.hint}>
      <div class="stat-label">{props.label}</div>
      <div class="stat-value">
        {props.value}
        <Show when={props.unit}>
          <small>{props.unit}</small>
        </Show>
      </div>
      <Show when={props.note}>
        <div class="stat-note">{props.note}</div>
      </Show>
    </div>
  );
}

/**
 * Uptime to the second: days, then hh:mm:ss, which keeps its width as it
 * ticks. Hours, minutes and seconds spelled out overflow the cell.
 */
function uptime(ms: number): JSX.Element {
  const s = Math.floor(ms / 1000);
  const days = Math.floor(s / 86400);
  const pad = (n: number) => String(n).padStart(2, "0");
  const clock = `${pad(Math.floor((s % 86400) / 3600))}:${pad(Math.floor((s % 3600) / 60))}:${pad(s % 60)}`;
  if (days === 0) return clock;
  return rich("format.uptimeExact", { days, clock, u: (unit) => <small>{unit}</small> });
}

/** A figure split into number and unit, e.g. memory as "912.0" + "MB". */
const parts = (text: string) => {
  const [value, unit] = text.split(" ");
  return { value, unit };
};

/** A view's title and model; controls given as children follow the title. */
function Head(props: { title: string; model?: string; children?: JSX.Element }) {
  return (
    <header class="perf-head">
      <div class="perf-head-start">
        <h2 class="perf-title">{props.title}</h2>
        {props.children}
      </div>
      <Show when={props.model}>
        <span class="perf-model" title={props.model}>
          {props.model}
        </span>
      </Show>
    </header>
  );
}

/** A details card's title, with a one-line summary on the right. */
function CardHead(props: { title: string; note?: string | null; summary?: string | null; alert?: boolean }) {
  return (
    <header class="cpu-card-head">
      <h3>
        {props.title}
        <Show when={props.note}>
          <small>{props.note}</small>
        </Show>
      </h3>
      {/* The title shows a summary cut short by a narrow window in full. */}
      <span class="cpu-card-summary" classList={{ alert: props.alert }} title={props.summary ?? undefined}>
        {props.summary}
      </span>
    </header>
  );
}

// ---- Device list ---------------------------------------------------------------

interface Device {
  key: string;
  title: string;
  sub: string;
  colorVar: string;
  series: Series[];
  max: () => number;
}

const diskTitle = (d: DiskStats) =>
  d.info.volumes.length
    ? t("performance.disk.titleVolumes", { index: d.info.index, volumes: d.info.volumes.join(" ") })
    : t("performance.disk.title", { index: d.info.index });

/** A disk's medium and bus, e.g. "SSD (NVMe)". */
export const diskKind = (d: DiskStats) => {
  const bus =
    d.info.bus === "nvme" ? "NVMe" : d.info.bus === "usb" ? "USB" : d.info.bus === "sata" ? "SATA" : "";
  const media =
    d.info.ssd === true
      ? "SSD"
      : d.info.ssd === false
        ? "HDD"
        : d.info.removable
          ? t("performance.disk.removableMedia")
          : "";
  return [media, bus && `(${bus})`].filter(Boolean).join(" ");
};

function devices(sys: SystemStats, hw: Hardware): Device[] {
  const list: Device[] = [
    {
      key: "cpu",
      title: "CPU",
      sub: `${Math.round(sys.cpu)}%${hw.cpuMhz ? `  ${fmtGHz(hw.cpuMhz)}` : ""}`,
      colorVar: "--cpu",
      series: [{ data: () => series("cpu"), colorVar: "--cpu" }],
      max: () => 100,
    },
    {
      key: "memory",
      title: t("common.resource.memory"),
      sub: t("performance.memory.sub", {
        used: fmtGB(sys.memUsed),
        total: fmtGB(sys.memTotal),
        percent: Math.round((sys.memUsed / sys.memTotal) * 100),
      }),
      colorVar: "--mem",
      series: [{ data: () => series("mem"), colorVar: "--mem" }],
      max: () => sys.memTotal,
    },
  ];
  for (const d of hw.disks) {
    const k = `disk.${d.info.index}`;
    list.push({
      key: `disk:${d.info.index}`,
      title: diskTitle(d),
      sub: `${diskKind(d)}  ${Math.round(d.active)}%${d.temperature !== null ? `  ${Math.round(d.temperature)} °C` : ""}`,
      colorVar: "--io",
      series: [{ data: () => series(`${k}.active`), colorVar: "--io" }],
      max: () => 100,
    });
  }
  for (const n of hw.nics) {
    const k = `nic.${n.info.id}`;
    list.push({
      key: `nic:${n.info.id}`,
      title: n.info.name,
      sub: t("performance.network.sub", { rx: fmtNet(n.rx), tx: fmtNet(n.tx) }),
      colorVar: "--net",
      series: [
        { data: () => series(`${k}.rx`), colorVar: "--net" },
        { data: () => series(`${k}.tx`), colorVar: "--net", dashed: true, fill: 0 },
      ],
      max: () => rateCeiling(peak(16 * 1024, `${k}.rx`, `${k}.tx`), netUnit()),
    });
  }
  hw.gpus.forEach((g, i) => {
    list.push({
      key: `gpu:${g.info.id}`,
      title: `GPU ${i}`,
      sub: [
        `${Math.round(g.util)}%`,
        g.temperature !== null && `${Math.round(g.temperature)} °C`,
        g.boardPower !== null && `${fmtW(g.boardPower)} W`,
      ]
        .filter(Boolean)
        .join("  "),
      colorVar: "--gpu",
      series: [{ data: () => series(`gpu.${g.info.id}.util`), colorVar: "--gpu" }],
      max: () => 100,
    });
  });
  for (const b of hw.batteries) {
    list.push({
      key: `bat:${b.info.id}`,
      title:
        hw.batteries.length > 1
          ? t("performance.battery.titleNumbered", { id: b.info.id })
          : t("performance.battery.title"),
      sub: batterySub(b),
      colorVar: "--bat",
      series: [{ data: () => series(`bat.${b.info.id}.pct`), colorVar: "--bat" }],
      max: () => 100,
    });
  }
  return list;
}

const SIDE_DEFAULT = 248;
const SIDE_MIN = 200;
const SIDE_MAX = 440;
const [savedSide, saveSide] = persisted("perfSideWidth", SIDE_DEFAULT);

export default function Performance() {
  const sys = () => snapshot()?.system;
  const hw = () => snapshot()?.hardware;
  // Device identity changes rarely; keep list items (and their charts) stable.
  const keys = createMemo(
    () => {
      const s = sys();
      const h = hw();
      return s && h ? devices(s, h).map((d) => d.key) : [];
    },
    [],
    { equals: (a, b) => a.length === b.length && a.every((k, i) => k === b[i]) },
  );
  const deviceMap = createMemo(() => {
    const s = sys();
    const h = hw();
    return new Map((s && h ? devices(s, h) : []).map((d) => [d.key, d]));
  });
  const current = () => (keys().includes(perfDevice()) ? perfDevice() : "cpu");
  let detail!: HTMLDivElement;
  let page!: HTMLDivElement;
  // At most half the page, so the detail never gets squeezed on a narrow window.
  const clampSide = (width: number) =>
    Math.round(Math.max(SIDE_MIN, Math.min(width, SIDE_MAX, (page?.clientWidth ?? 2 * SIDE_MAX) / 2)));
  const [side, setSide] = createSignal(savedSide());

  // The detail view arrives from the direction of the chosen device.
  createEffect(
    on(current, (key, prev) => {
      if (prev === undefined || key === prev) return;
      detail.scrollTop = 0;
      const dir = Math.sign(keys().indexOf(key) - keys().indexOf(prev));
      play(
        detail.firstElementChild,
        [
          { opacity: 0, transform: `translateY(${dir * 16}px)` },
          { opacity: 1, transform: "none" },
        ],
        { duration: 250, easing: EASE_OUT },
      );
    }),
  );

  return (
    <div ref={page} class="perf page" style={{ "--perf-side": `${side()}px` }}>
      <div class="perf-side">
        <nav class="dev-list" aria-label={t("performance.devices")}>
          <Indicator selector="[aria-current='true']" track={current} class="dev-mark" />
          <For each={keys()}>
            {(key) => {
              const d = () => deviceMap().get(key);
              const max = createMemo(() => d()?.max() ?? 1);
              return (
                <Show when={d()}>
                  {(dev) => (
                    <button
                      type="button"
                      class="dev"
                      aria-current={current() === key ? "true" : undefined}
                      onClick={() => setPerfDevice(key)}
                    >
                      <div style={{ "min-width": 0 }}>
                        <div class="dev-title" title={dev().title}>
                          {dev().title}
                        </div>
                        <div class="dev-sub">{dev().sub}</div>
                      </div>
                      <div class="dev-trace" style={{ "--c": `var(${dev().colorVar})` }}>
                        <Chart series={dev().series} max={max} />
                      </div>
                    </button>
                  )}
                </Show>
              );
            }}
          </For>
        </nav>
      </div>
      <Splitter
        width={side()}
        min={SIDE_MIN}
        max={SIDE_MAX}
        initial={SIDE_DEFAULT}
        grows={1}
        label={t("performance.resizeList")}
        class="perf-splitter"
        onResize={(w) => setSide(clampSide(w))}
        onCommit={() => saveSide(side())}
      />

      <section class="perf-detail" aria-live="off">
        <div ref={detail} class="perf-scroll">
          <Show when={sys() && hw()}>
            <Show when={current()} keyed>
              {(key) => (
                <div>
                  <Switch>
                    <Match when={key === "cpu"}>
                      <CpuView />
                    </Match>
                    <Match when={key === "memory"}>
                      <MemoryView />
                    </Match>
                    <Match when={key.startsWith("disk:")}>
                      <DiskView index={Number(key.slice(5))} />
                    </Match>
                    <Match when={key.startsWith("nic:")}>
                      <NicView id={key.slice(4)} />
                    </Match>
                    <Match when={key.startsWith("gpu:")}>
                      <GpuView id={key.slice(4)} />
                    </Match>
                    <Match when={key.startsWith("bat:")}>
                      <BatteryView id={key.slice(4)} />
                    </Match>
                  </Switch>
                </div>
              )}
            </Show>
          </Show>
        </div>
      </section>
    </div>
  );
}

// ---- CPU ---------------------------------------------------------------------

type CoreFilter = "all" | "p" | "e";
const [coreFilter, setCoreFilter] = persisted<CoreFilter>("coreFilter", "all");
const CORE_FILTERS: CoreFilter[] = ["all", "p", "e"];
/** The CPU view's pages: the charts, live figures and specs, or the rest (processor details, sensors, idle states). */
type CpuPage = "overview" | "details";
const [cpuPage, setCpuPage] = persisted<CpuPage>("cpuPage", "overview");
const CPU_PAGES: CpuPage[] = ["overview", "details"];

/** Watts with sensible precision: 7.4, 78. */
const fmtW = (v: number) => (v < 10 ? v.toFixed(1) : String(Math.round(v)));
const watts = (v: number) => `${fmtW(v)} W`;
const celsius = (v: number) => `${Math.round(v)} °C`;

function CpuView() {
  const sys = () => now().system;
  const hw = () => now().hardware;
  const cpu = () => hw().cpu;
  const hybrid = () => cpu().performanceCores > 0;
  const efficient = (i: number) => cpu().threads[i]?.efficient ?? false;
  const shown = createMemo(
    () => {
      const f = hybrid() ? coreFilter() : "all";
      return sys()
        .cores.map((_, i) => i)
        .filter((i) => f === "all" || (f === "e") === efficient(i));
    },
    [],
    { equals: (a, b) => a.length === b.length && a.every((v, i) => v === b[i]) },
  );
  const temp = () => cpuTemperature(hw());
  const coreTemperatures = createMemo(
    () =>
      new Map(
        hw().cpuSensors?.coreTemperatures.map((core) => [`${core.group},${core.processor}`, core.celsius]) ??
          [],
      ),
  );
  const powerMax = wattTop(5, "cpu.pkgW", "cpu.coresW");
  /** Mean clock of the P or E cores, MHz. */
  const meanMhz = (eff: boolean) => {
    const list = cpu()
      .threads.map((th, i) => (th.efficient === eff ? (hw().coreMhz[i] ?? 0) : 0))
      .filter((m) => m > 0);
    return list.length ? list.reduce((a, b) => a + b, 0) / list.length : null;
  };

  let charts: HTMLDivElement | undefined;
  let body: HTMLDivElement | undefined;
  const settle = () =>
    play(
      charts?.firstElementChild,
      [
        { opacity: 0, transform: "scale(0.985)" },
        { opacity: 1, transform: "none" },
      ],
      { duration: 200, easing: EASE_OUT },
    );
  createEffect(on(cpuMode, settle, { defer: true }));
  createEffect(on(coreFilter, () => cpuMode() === "logical" && settle(), { defer: true }));
  createEffect(
    on(
      cpuPage,
      () =>
        play(
          body,
          [
            { opacity: 0, transform: "translateY(4px)" },
            { opacity: 1, transform: "none" },
          ],
          { duration: 200, easing: EASE_OUT },
        ),
      { defer: true },
    ),
  );

  const coresText = () => {
    const c = cpu();
    return c.performanceCores
      ? t("performance.cpu.coresHybrid", {
          cores: c.cores,
          performance: c.performanceCores,
          efficiency: c.efficiencyCores,
        })
      : String(c.cores);
  };
  const identity = () =>
    `0x${cpu().family.toString(16).toUpperCase()} / 0x${cpu().model.toString(16).toUpperCase()} / ${cpu().stepping}`;
  const microcode = () => `0x${cpu().microcode?.toString(16).toUpperCase()}`;

  /** Lowest to highest over what a chart shows, e.g. "54–61 °C". */
  const extent = (key: string, format: (v: number) => string, unit: string) => {
    const values = series(key).slice(-HISTORY).filter(Number.isFinite);
    if (!values.length) return null;
    const [low, high] = [format(Math.min(...values)), format(Math.max(...values))];
    return `${low === high ? low : `${low}–${high}`} ${unit}`;
  };
  const chartsTitle = () =>
    t(
      temp() === null
        ? "performance.cpu.charts.power"
        : hw().power.package === null
          ? "performance.cpu.charts.temperature"
          : "performance.cpu.charts.both",
    );
  const chartsSummary = () => {
    const values = [
      temp() !== null && extent("cpu.temp", (v) => String(Math.round(v)), "°C"),
      hw().power.package !== null && extent("cpu.pkgW", fmtW, "W"),
    ].filter(Boolean);
    return values.length
      ? t("performance.cpu.chartsRange", { span: spanLabel(), values: values.join(" · ") })
      : null;
  };

  return (
    <div class="perf-main">
      <Head title="CPU" model={cpu().name}>
        <div class="segmented perf-tabs" role="tablist" aria-label={t("performance.cpu.pages")}>
          <Indicator selector="[aria-selected='true']" track={cpuPage} class="seg-thumb" />
          <For each={CPU_PAGES}>
            {(p) => (
              <button type="button" role="tab" aria-selected={cpuPage() === p} onClick={() => setCpuPage(p)}>
                {t(`performance.cpu.page.${p}`)}
              </button>
            )}
          </For>
        </div>
      </Head>

      <Show
        when={cpuPage() === "overview"}
        fallback={
          // Processor details, sensors and the rest, as cards that stay open;
          // two short ones share a row.
          <div ref={body} class="cpu-details" role="tabpanel">
            <section class="cpu-card">
              <CardHead title={t("performance.cpu.sectionInfo")} />
              <dl class="info-list">
                <Show when={cpu().vendor}>
                  <dt>{t("performance.cpu.vendor")}</dt>
                  <dd>{cpu().vendor}</dd>
                  <dt>{t("performance.cpu.identity")}</dt>
                  <dd>{identity()}</dd>
                </Show>
                <Show when={cpu().microcode != null}>
                  <dt>{t("performance.cpu.microcode")}</dt>
                  <dd>{microcode()}</dd>
                </Show>
                <dt>{t("performance.cpu.thermalSource")}</dt>
                <dd>
                  {hasCpuDieTemperature(hw())
                    ? t("performance.cpu.driverSource")
                    : hw().thermal.length > 0
                      ? t("performance.cpu.thermalZones", {
                          zones: hw()
                            .thermal.map((z) => z.name)
                            .join(t("common.listSeparator")),
                        })
                      : t("performance.cpu.thermalNone")}
                </dd>
                <Show when={hw().cpuSensors}>
                  {(sensors) => (
                    <>
                      <dt>{t("performance.cpu.driverStatus")}</dt>
                      <dd>{t(`performance.cpu.sensorState.${sensors().state}`)}</dd>
                    </>
                  )}
                </Show>
                <Show when={cpu().instructions.length > 0}>
                  <dt class="cpu-instructions-label">{t("performance.cpu.instructions")}</dt>
                  <dd class="cpu-instructions">
                    <For each={cpu().instructions}>{(name) => <span>{name}</span>}</For>
                  </dd>
                </Show>
              </dl>
            </section>

            <Show when={temp() !== null || hw().power.package !== null}>
              <section class="cpu-card">
                <CardHead title={chartsTitle()} summary={chartsSummary()} />
                <div class="chart-pair">
                  <Show when={temp() !== null}>
                    <ChartBox
                      label={t("common.resource.temperature")}
                      max="100 °C"
                      colorVar="--cpu"
                      size="small"
                      scale={() => 100}
                      format={celsius}
                      series={[{ data: () => series("cpu.temp"), colorVar: "--cpu" }]}
                    />
                  </Show>
                  <Show when={hw().power.package !== null}>
                    <ChartBox
                      label={t("performance.cpu.power")}
                      max={`${powerMax()} W`}
                      colorVar="--cpu"
                      size="small"
                      scale={powerMax}
                      format={watts}
                      series={[
                        {
                          data: () => series("cpu.pkgW"),
                          colorVar: "--cpu",
                          get label() {
                            return t("performance.cpu.powerSeries.package");
                          },
                        },
                        {
                          data: () => series("cpu.coresW"),
                          colorVar: "--cpu",
                          dashed: true,
                          fill: 0,
                          get label() {
                            return t("performance.cpu.powerSeries.cores");
                          },
                        },
                      ]}
                    />
                  </Show>
                </div>
              </section>
            </Show>

            <Show when={hw().cpuSensors}>
              {(sensors) => {
                const board = () => sensors().boardFans.length > 0 || sensors().boardMetrics.length > 0;
                const limited = () => sensors().limits.some((flag) => flag.active);
                return (
                  <>
                    <section class="cpu-card">
                      <CardHead
                        title={t("performance.cpu.telemetryTitle")}
                        summary={sensorSummary(sensors(), hw().power.package)}
                      />
                      <SensorGroups sensors={sensors()} />
                    </section>
                    <Show when={sensors().cores.length > 0 && hasCoreColumns(sensors())}>
                      <section class="cpu-card">
                        <CardHead title={t("performance.cpu.perCore")} summary={coreSummary(sensors())} />
                        <CoreTable sensors={sensors()} />
                      </section>
                    </Show>
                    <Show when={board() || sensors().limits.length > 0}>
                      <div class="cpu-pair">
                        <Show when={board()}>
                          <section class="cpu-card">
                            <CardHead
                              title={t("performance.cpu.boardSensor")}
                              note={sensors().boardSensor}
                              summary={boardSummary(sensors())}
                            />
                            <BoardSensors sensors={sensors()} />
                          </section>
                        </Show>
                        <Show when={sensors().limits.length > 0}>
                          <section class="cpu-card" classList={{ alert: limited() }}>
                            <CardHead
                              title={t("performance.cpu.packageLimits")}
                              summary={limitSummary(sensors().limits)}
                              alert={limited()}
                            />
                            <PackageLimits sensors={sensors()} />
                          </section>
                        </Show>
                      </div>
                    </Show>
                  </>
                );
              }}
            </Show>

            <Show when={idleSummary(hw().cpuSensors, hw().cpuIdle)}>
              {(summary) => (
                <section class="cpu-card">
                  <CardHead title={t("performance.cpu.idleTitle")} summary={summary()} />
                  <IdleStates idle={hw().cpuIdle} sensors={hw().cpuSensors} />
                </section>
              )}
            </Show>
          </div>
        }
      >
        <div ref={body} class="perf-grow" role="tabpanel">
          <div class="perf-toolbar">
            <Show when={hybrid() && cpuMode() === "logical"}>
              <div class="segmented" role="group" aria-label={t("performance.cpu.coreType")}>
                <Indicator selector="[aria-pressed='true']" track={coreFilter} class="seg-thumb" />
                <For each={CORE_FILTERS}>
                  {(f) => (
                    <button
                      type="button"
                      title={t(`performance.cpu.filterTitle.${f}`)}
                      aria-pressed={coreFilter() === f}
                      onClick={() => setCoreFilter(f)}
                    >
                      {f === "all" ? t("performance.cpu.filterAll") : t(`performance.cpu.filterShort.${f}`)}
                    </button>
                  )}
                </For>
              </div>
            </Show>
            <div class="segmented" role="group" aria-label={t("performance.cpu.graph")}>
              <Indicator selector="[aria-pressed='true']" track={cpuMode} class="seg-thumb" />
              <button
                type="button"
                title={t("performance.cpu.modeTitle.overall")}
                aria-pressed={cpuMode() === "overall"}
                onClick={() => setCpuMode("overall")}
              >
                {t("performance.cpu.overall")}
              </button>
              <button
                type="button"
                title={t("performance.cpu.modeTitle.logical")}
                aria-pressed={cpuMode() === "logical"}
                onClick={() => setCpuMode("logical")}
              >
                {t("performance.cpu.logical")}
              </button>
            </div>
          </div>

          <div ref={charts} class="perf-grow">
            <Show
              when={cpuMode() === "logical"}
              fallback={
                <div class="perf-grow">
                  <ChartBox
                    label={t("performance.cpu.utilizationChart")}
                    max="100%"
                    colorVar="--cpu"
                    size="main"
                    scale={() => 100}
                    format={pct}
                    series={[
                      {
                        data: () => series("cpu"),
                        colorVar: "--cpu",
                        get label() {
                          return t("performance.cpu.total");
                        },
                      },
                      {
                        data: () => series("cpu.kernel"),
                        colorVar: "--cpu",
                        dashed: true,
                        fill: 0.18,
                        get label() {
                          return t("performance.cpu.kernel");
                        },
                      },
                    ]}
                  />
                  <div class="legend" style={{ "--c": "var(--cpu)" }}>
                    <span>{t("performance.cpu.total")}</span>
                    <span class="dashed">{t("performance.cpu.kernelTime")}</span>
                  </div>
                </div>
              }
            >
              <div class="perf-grow">
                <div
                  class="chart-grid cores"
                  ref={(el) => fitGrid(el, () => shown().length, coreGridColumns)}
                >
                  <For each={shown()}>
                    {(i) => {
                      const eff = () => hybrid() && efficient(i);
                      const color = () => (eff() ? "--cpu-e" : "--cpu");
                      const mhz = () => hw().coreMhz[i] ?? 0;
                      const load = () => sys().cores[i];
                      const temperature = () => {
                        const thread = cpu().threads[i];
                        return thread && coreTemperatures().get(`${thread.group},${thread.processor}`);
                      };
                      return (
                        <div
                          class="core-cell"
                          style={{ "--c": `var(${color()})` }}
                          title={`${t(
                            hybrid()
                              ? eff()
                                ? "performance.cpu.coreTipE"
                                : "performance.cpu.coreTipP"
                              : "performance.cpu.coreTip",
                            { index: i, core: cpu().threads[i]?.core ?? "?" },
                          )}${temperature() !== undefined ? `\n${t("common.resource.temperature")}: ${Math.round(temperature() ?? 0)} °C` : ""}`}
                        >
                          <div class="cell-head">
                            <span class="core-id">
                              <Show when={hybrid()}>
                                <i class="core-kind">{eff() ? "E" : "P"}</i>
                              </Show>
                              <span class="core-index">{i}</span>
                              <Show when={mhz() > 0}>
                                <span class="core-clock">{fmtGHz(mhz())}</span>
                              </Show>
                            </span>
                            <span class="cell-load">
                              <Show when={load() != null} fallback="—">
                                {Math.round(load() ?? 0)}
                                <small>%</small>
                              </Show>
                            </span>
                          </div>
                          <div class="chart-frame">
                            <Chart
                              series={[
                                {
                                  data: () => {
                                    const thread = cpu().threads[i];
                                    return thread ? series(`core.${thread.group}.${thread.processor}`) : [];
                                  },
                                  colorVar: color(),
                                },
                              ]}
                              max={() => 100}
                            />
                          </div>
                        </div>
                      );
                    }}
                  </For>
                </div>
                <Show when={hybrid()}>
                  <div class="legend">
                    <span style={{ "--c": "var(--cpu)" }}>{t("performance.cpu.coreKind.p")}</span>
                    <span style={{ "--c": "var(--cpu-e)" }}>{t("performance.cpu.coreKind.e")}</span>
                  </div>
                </Show>
              </div>
            </Show>
          </div>

          <div class="perf-stats cpu-stats">
            <div class="stat-grid">
              <Stat label={t("performance.cpu.utilization")} value={Math.round(sys().cpu)} unit="%" />
              <Stat
                label={t("performance.cpu.speed")}
                value={((hw().cpuMhz ?? 0) / 1000).toFixed(2)}
                unit="GHz"
              />
              <Show when={hybrid() && meanMhz(false)}>
                {(m) => (
                  <Stat label={t("performance.cpu.pCoreSpeed")} value={(m() / 1000).toFixed(2)} unit="GHz" />
                )}
              </Show>
              <Show when={hybrid() && meanMhz(true)}>
                {(m) => (
                  <Stat label={t("performance.cpu.eCoreSpeed")} value={(m() / 1000).toFixed(2)} unit="GHz" />
                )}
              </Show>
              <Show when={temp() !== null}>
                <Stat label={t("common.resource.temperature")} value={Math.round(temp() ?? 0)} unit="°C" />
              </Show>
              <Show when={hw().power.package !== null}>
                <Stat
                  label={t("performance.cpu.packagePower")}
                  value={fmtW(hw().power.package ?? 0)}
                  unit="W"
                />
              </Show>
              <Show when={hw().power.cores !== null}>
                <Stat label={t("performance.cpu.corePower")} value={fmtW(hw().power.cores ?? 0)} unit="W" />
              </Show>
              <Show when={hw().power.graphics !== null}>
                <Stat
                  label={t("performance.cpu.graphicsPower")}
                  value={fmtW(hw().power.graphics ?? 0)}
                  unit="W"
                />
              </Show>
              <Stat label={t("performance.cpu.processes")} value={fmtCount(sys().processCount)} />
              <Stat label={t("performance.cpu.threads")} value={fmtCount(sys().threadCount)} />
              <Stat label={t("performance.cpu.handles")} value={fmtCount(sys().handleCount)} />
              <Stat label={t("performance.cpu.uptime")} value={uptime(sys().uptimeMs)} />
            </div>
            <dl class="info-list">
              <dt>{t("performance.cpu.baseSpeed")}</dt>
              <dd
                title={
                  cpu().baseSource
                    ? t(
                        cpu().baseSource === "cpuid"
                          ? "performance.cpu.baseSourceCpuid"
                          : "performance.cpu.baseSourceWindows",
                      )
                    : undefined
                }
              >
                {cpu().baseMhz ? fmtGHz(cpu().baseMhz) : "—"}
              </dd>
              <dt>{t("performance.cpu.sockets")}</dt>
              <dd>{cpu().sockets}</dd>
              <dt>{t("performance.cpu.cores")}</dt>
              <dd>{coresText()}</dd>
              <dt>{t("performance.cpu.logicalProcessors")}</dt>
              <dd>{cpu().logical}</dd>
              <dt>{t("performance.cpu.virtualization")}</dt>
              <dd>{cpu().virtualization ? t("common.enabled") : t("performance.cpu.notEnabled")}</dd>
              <dt>{t("performance.cpu.cache", { level: 1 })}</dt>
              <dd>{fmtCache(cpu().l1)}</dd>
              <dt>{t("performance.cpu.cache", { level: 2 })}</dt>
              <dd>{fmtCache(cpu().l2)}</dd>
              <dt>{t("performance.cpu.cache", { level: 3 })}</dt>
              <dd>{fmtCache(cpu().l3)}</dd>
            </dl>
          </div>
          <Show when={!hw().cpuSensors && hw().thermal.length === 0}>
            <p class="perf-note">{t("performance.cpu.thermalMissing")}</p>
          </Show>
        </div>
      </Show>
    </div>
  );
}

// ---- Memory ------------------------------------------------------------------

function MemoryView() {
  const sys = () => now().system;
  const mem = () => now().hardware.memory;
  const composition = createMemo(() => memoryComposition(sys()));
  const frac = (v: number | null) => Math.max(0, Math.min(1, (v ?? 0) / Math.max(1, sys().memTotal)));
  const value = (bytes: number | null) => (bytes === null ? "—" : fmtMem(bytes));
  const usedLabel = () => t("performance.memory.legendInUse", { value: value(composition().used) });
  const modifiedLabel = () =>
    t("performance.memory.legendModified", { value: value(composition().modified) });
  const standbyLabel = () => t("performance.memory.legendStandby", { value: value(composition().standby) });
  const freeLabel = () => t("performance.memory.legendFree", { value: value(composition().free) });
  const reserved = () => Math.max(0, mem().installed - sys().memTotal);
  let heldLegend: HTMLElement[] = [];
  const releaseLegend = () => {
    for (const item of heldLegend) {
      for (const property of ["width", "min-width", "flex-shrink", "white-space"]) {
        item.style.removeProperty(property);
      }
    }
    heldLegend = [];
  };
  const holdLegend = (event: PointerEvent & { currentTarget: HTMLDivElement }) => {
    if (!(event.target instanceof HTMLElement) || event.target.tagName !== "I") return;
    releaseLegend();
    heldLegend = [...event.currentTarget.children] as HTMLElement[];
    const widths = heldLegend.map((item) => item.getBoundingClientRect().width);
    // Keep the existing layout while a tooltip is open. A digit/unit change
    // in an earlier reading must not move the icon away from the pointer.
    heldLegend.forEach((item, index) => {
      item.style.width = `${widths[index]}px`;
      item.style.minWidth = "0";
      item.style.flexShrink = "0";
      item.style.whiteSpace = "nowrap";
    });
  };
  onCleanup(releaseLegend);

  return (
    <div class="perf-main">
      <Head
        title={t("common.resource.memory")}
        model={`${fmtGB(mem().installed || sys().memTotal, 0)} GB${mem().kind ? ` ${mem().kind}` : ""}`}
      />
      <ChartBox
        label={t("performance.memory.usage")}
        max={`${fmtGB(sys().memTotal)} GB`}
        colorVar="--mem"
        size="main"
        scale={() => sys().memTotal}
        format={gb}
        series={[{ data: () => series("mem"), colorVar: "--mem" }]}
      />

      <div
        class="mem-bar"
        role="img"
        aria-label={`${t("performance.memory.composition")}: ${usedLabel()}, ${modifiedLabel()}, ${standbyLabel()}, ${freeLabel()}`}
      >
        <span class="used" style={{ "--f": frac(composition().used) }} />
        <span
          class="modified"
          style={{ "--u": frac(composition().used), "--f": frac(composition().modified) }}
        />
        <span
          class="standby"
          style={{
            "--u": frac(composition().used + (composition().modified ?? 0)),
            "--f": frac(composition().standby),
          }}
        />
      </div>
      <div class="mem-legend" onPointerOver={holdLegend} onPointerOut={releaseLegend}>
        <span>
          <i
            style={{ background: "color-mix(in srgb, var(--mem) 55%, transparent)" }}
            title={t("performance.memory.inUseDescription")}
          />
          {usedLabel()}
        </span>
        <span>
          <i
            style={{ background: "color-mix(in srgb, var(--mem) 90%, transparent)" }}
            title={t("performance.memory.modifiedDescription")}
          />
          {modifiedLabel()}
        </span>
        <span>
          <i
            style={{ background: "color-mix(in srgb, var(--mem) 20%, transparent)" }}
            title={t("performance.memory.standbyDescription")}
          />
          {standbyLabel()}
        </span>
        <span>
          <i
            style={{ border: "1px solid var(--stroke-strong)" }}
            title={t("performance.memory.freeDescription")}
          />
          {freeLabel()}
        </span>
      </div>

      <div class="perf-stats">
        <div class="stat-grid">
          <Stat label={t("performance.memory.inUse")} value={fmtGB(composition().used)} unit="GB" />
          <Stat label={t("performance.memory.available")} value={fmtGB(sys().memAvailable)} unit="GB" />
          <Stat
            label={t("performance.memory.committed")}
            value={`${fmtGB(sys().commitUsed)} / ${fmtGB(sys().commitLimit)}`}
            unit="GB"
          />
          <Stat label={t("performance.memory.cached")} value={fmtGB(sys().cached)} unit="GB" />
          <Stat label={t("performance.memory.pagedPool")} {...parts(fmtMem(sys().kernelPaged))} />
          <Stat label={t("performance.memory.nonPagedPool")} {...parts(fmtMem(sys().kernelNonpaged))} />
        </div>
        <dl class="info-list">
          <dt>{t("performance.memory.speed")}</dt>
          <dd>{mem().speed ? `${mem().speed} MT/s` : "—"}</dd>
          <dt>{t("performance.memory.slotsUsed")}</dt>
          <dd>{mem().slotsTotal ? `${mem().slotsUsed} / ${mem().slotsTotal}` : "—"}</dd>
          <dt>{t("performance.memory.formFactor")}</dt>
          <dd>
            {mem().formFactor === "onboard" ? t("performance.memory.onboard") : (mem().formFactor ?? "—")}
          </dd>
          <dt>{t("performance.memory.type")}</dt>
          <dd>{mem().kind ?? "—"}</dd>
          <Show when={reserved() > 0}>
            <dt>{t("performance.memory.reserved")}</dt>
            <dd>{(reserved() / MB).toFixed(0)} MB</dd>
          </Show>
        </dl>
      </div>
    </div>
  );
}

// ---- Disk --------------------------------------------------------------------

function DiskView(props: { index: number }) {
  const disk = () => now().hardware.disks.find((d) => d.info.index === props.index);
  const k = () => `disk.${props.index}`;
  const rateMax = rateTop(bytes, MB, `${k()}.read`, `${k()}.write`);
  return (
    <Show when={disk()} fallback={<p class="empty">{t("performance.disk.removed")}</p>}>
      {(d) => (
        <div class="perf-main">
          <Head title={diskTitle(d())} model={d().info.model} />
          <ChartBox
            label={t("performance.disk.activeTime")}
            max="100%"
            colorVar="--io"
            size="half"
            scale={() => 100}
            format={pct}
            series={[{ data: () => series(`${k()}.active`), colorVar: "--io" }]}
          />
          <div class="perf-grow section-gap">
            <ChartBox
              label={t("performance.disk.transferRate")}
              max={rateCeilingLabel(rateMax(), "bytes")}
              colorVar="--io"
              size="half"
              scale={rateMax}
              format={(v) => fmtRate(v, false)}
              series={[
                {
                  data: () => series(`${k()}.read`),
                  colorVar: "--io",
                  get label() {
                    return t("performance.disk.read");
                  },
                },
                {
                  data: () => series(`${k()}.write`),
                  colorVar: "--io",
                  dashed: true,
                  fill: 0,
                  get label() {
                    return t("performance.disk.write");
                  },
                },
              ]}
            />
            <div class="legend" style={{ "--c": "var(--io)" }}>
              <span>{t("performance.disk.read")}</span>
              <span class="dashed">{t("performance.disk.write")}</span>
            </div>
          </div>
          <div class="perf-stats">
            <div class="stat-grid">
              <Stat label={t("performance.disk.activeTime")} value={Math.round(d().active)} unit="%" />
              <Stat
                label={t("performance.disk.responseTime")}
                value={d().responseMs.toFixed(1)}
                unit={t("performance.disk.ms")}
              />
              <Stat
                label={t("performance.disk.readSpeed")}
                value={rateParts(d().read)[0]}
                unit={rateParts(d().read)[1]}
              />
              <Stat
                label={t("performance.disk.writeSpeed")}
                value={rateParts(d().write)[0]}
                unit={rateParts(d().write)[1]}
              />
              <Show when={d().temperature}>
                {(c) => <Stat label={t("common.resource.temperature")} value={Math.round(c())} unit="°C" />}
              </Show>
            </div>
            <dl class="info-list">
              <dt>{t("performance.disk.capacity")}</dt>
              <dd>{d().info.size ? fmtCapacity(d().info.size) : "—"}</dd>
              <dt>{t("performance.disk.type")}</dt>
              <dd>{diskKind(d()) || "—"}</dd>
              <dt>{t("performance.disk.volumes")}</dt>
              <dd>{d().info.volumes.join("  ") || t("common.none")}</dd>
              <Show when={d().info.removable}>
                <dt>{t("performance.disk.removable")}</dt>
                <dd>{t("performance.disk.yes")}</dd>
              </Show>
              <Show when={d().temperatureWarning}>
                <dt>{t("performance.disk.warningTemperature")}</dt>
                <dd>{d().temperatureWarning} °C</dd>
              </Show>
            </dl>
          </div>
        </div>
      )}
    </Show>
  );
}

// ---- Network -----------------------------------------------------------------

/** Addresses listed before the rest fold behind a button (closed again on another device). */
const ADDRESSES_SHOWN = 3;

function Addresses(props: { list: string[] }) {
  const [all, setAll] = createSignal(false);
  const shown = () => (all() ? props.list : props.list.slice(0, ADDRESSES_SHOWN));
  return (
    <dd>
      <Show when={props.list.length > 0} fallback="—">
        <For each={shown()}>{(address) => <div>{address}</div>}</For>
        <Show when={props.list.length > ADDRESSES_SHOWN}>
          <button
            type="button"
            class="link addresses-toggle"
            aria-expanded={all()}
            onClick={() => setAll(!all())}
          >
            {all()
              ? t("performance.network.showFewer")
              : t("performance.network.showAll", { count: props.list.length })}
            {all() ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
          </button>
        </Show>
      </Show>
    </dd>
  );
}

function NicView(props: { id: string }) {
  const nic = () => now().hardware.nics.find((n) => n.info.id === props.id);
  const k = () => `nic.${props.id}`;
  const scale = rateTop(netUnit, 64 * 1024, `${k()}.rx`, `${k()}.tx`);

  return (
    <Show when={nic()} fallback={<p class="empty">{t("performance.network.disconnected")}</p>}>
      {(n) => (
        <div class="perf-main">
          <Head title={n().info.name} model={n().info.description} />
          <ChartBox
            label={t("performance.network.throughput")}
            max={rateCeilingLabel(scale(), netUnit())}
            colorVar="--net"
            size="main"
            scale={scale}
            format={(v) => t("performance.withAlt", { value: fmtNet(v), alt: fmtNetAlt(v) })}
            series={[
              {
                data: () => series(`${k()}.rx`),
                colorVar: "--net",
                get label() {
                  return t("performance.network.receive");
                },
              },
              {
                data: () => series(`${k()}.tx`),
                colorVar: "--net",
                dashed: true,
                fill: 0,
                get label() {
                  return t("performance.network.send");
                },
              },
            ]}
          />
          <div class="legend" style={{ "--c": "var(--net)" }}>
            <span>{t("performance.network.receive")}</span>
            <span class="dashed">{t("performance.network.send")}</span>
          </div>
          <div class="perf-stats">
            <div class="stat-grid">
              <Stat
                label={t("performance.network.receive")}
                value={netParts(n().rx)[0]}
                unit={netParts(n().rx)[1]}
                note={fmtNetAlt(n().rx)}
              />
              <Stat
                label={t("performance.network.send")}
                value={netParts(n().tx)[0]}
                unit={netParts(n().tx)[1]}
                note={fmtNetAlt(n().tx)}
              />
            </div>
            <dl class="info-list selectable">
              <dt>{t("performance.network.connectionType")}</dt>
              <dd>{t(`performance.network.kind.${n().info.kind}`)}</dd>
              <dt>{t("performance.network.linkSpeed")}</dt>
              <dd>
                {n().info.linkSpeed
                  ? t("performance.withAlt", {
                      value: fmtLinkSpeed(n().info.linkSpeed),
                      alt: fmtRate(n().info.linkSpeed / 8, false),
                    })
                  : "—"}
              </dd>
              <dt>{t("performance.network.ipv4")}</dt>
              <Addresses list={n().info.ipv4} />
              <dt>{t("performance.network.ipv6")}</dt>
              <Addresses list={n().info.ipv6} />
              <dt>{t("performance.network.mac")}</dt>
              <dd>{n().info.mac ?? "—"}</dd>
            </dl>
          </div>
        </div>
      )}
    </Show>
  );
}

// ---- GPU ---------------------------------------------------------------------

/**
 * Which engines each GPU charts, keyed by model: the id comes from the
 * adapter LUID, which changes at every boot.
 */
const [engineChoices, setEngineChoices] = persisted<Record<string, EngineChoice>>("gpuEngines", {});
/** How the engine charts are laid out, the same for every GPU. */
const [engineLayout, setEngineLayout] = persisted<EngineLayout>("gpuLayout", "grid");
const layoutOf = (): EngineLayout => (ENGINE_LAYOUTS.includes(engineLayout()) ? engineLayout() : "grid");

/** The engine charts' part of the height they share with the memory charts. */
const SHARE_DEFAULT = 0.6;
const SHARE_MIN = 0.3;
const SHARE_MAX = 0.85;
const clampShare = (v: number) =>
  Number.isFinite(v) ? Math.min(SHARE_MAX, Math.max(SHARE_MIN, v)) : SHARE_DEFAULT;
const [savedShare, saveShare] = persisted("gpuEngineShare", SHARE_DEFAULT);

/**
 * The handle between the engine charts and the memory charts: drag it, use
 * the arrow keys, or double-click for the default. It sets the engines' part
 * of the height, so the split follows the window; saved when a drag ends.
 */
function ShareSplitter(props: { share: number; onResize: (share: number) => void; onCommit: () => void }) {
  const [dragging, setDragging] = createSignal(false);
  let handle!: HTMLDivElement;
  /**
   * Sets `share` and saves the split as laid out: the charts' minimum heights
   * can stop it short, and the share kept (and stepped from by the arrow
   * keys) is then the one on screen.
   */
  const settle = (share: number) => {
    props.onResize(share);
    const above = handle.previousElementSibling?.getBoundingClientRect().height ?? 0;
    const below = handle.nextElementSibling?.getBoundingClientRect().height ?? 0;
    const seen = above / (above + below);
    if (Math.abs(seen - share) > 0.01) props.onResize(clampShare(seen));
    props.onCommit();
  };
  const begin = (e: PointerEvent) => {
    if (e.button !== 0) return;
    const above = handle.previousElementSibling?.getBoundingClientRect().height ?? 0;
    const total = above + (handle.nextElementSibling?.getBoundingClientRect().height ?? 0);
    if (!(total > 0)) return;
    handle.setPointerCapture(e.pointerId);
    setDragging(true);
    const startY = e.clientY;
    let next = props.share;
    let frame = 0;
    const move = (m: PointerEvent) => {
      next = clampShare((above + m.clientY - startY) / total);
      // One layout per frame, however fast the pointer reports.
      frame ||= requestAnimationFrame(() => {
        frame = 0;
        props.onResize(next);
      });
    };
    const end = () => {
      cancelAnimationFrame(frame);
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("lostpointercapture", end);
      settle(next);
      setDragging(false);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("lostpointercapture", end);
  };
  const key = (e: KeyboardEvent) => {
    const step = e.key === "ArrowUp" ? -0.05 : e.key === "ArrowDown" ? 0.05 : 0;
    if (step === 0 && e.key !== "Home" && e.key !== "End") return;
    e.preventDefault();
    settle(e.key === "Home" ? SHARE_MIN : e.key === "End" ? SHARE_MAX : clampShare(props.share + step));
  };
  return (
    <div
      ref={handle}
      class="share-splitter"
      classList={{ dragging: dragging() }}
      role="separator"
      tabIndex={0}
      aria-orientation="horizontal"
      aria-label={t("performance.gpu.resize")}
      aria-valuemin={Math.round(SHARE_MIN * 100)}
      aria-valuemax={Math.round(SHARE_MAX * 100)}
      aria-valuenow={Math.round(props.share * 100)}
      onPointerDown={begin}
      onKeyDown={key}
      onDblClick={() => settle(SHARE_DEFAULT)}
    />
  );
}

/** What the engine list shows: smart display, or that many charts. */
type EngineDisplay = "smart" | number;

const sameList = (a: readonly string[], b: readonly string[]) =>
  a.length === b.length && a.every((v, i) => v === b[i]);

function GpuView(props: { id: string }) {
  const gpus = () => now().hardware.gpus;
  const gpu = () => gpus().find((g) => g.info.id === props.id);
  const index = () => gpus().findIndex((g) => g.info.id === props.id);
  const k = () => `gpu.${props.id}`;
  const engines = () => gpu()?.info.engines ?? [];
  const choice = () => engineChoices()[gpu()?.info.name ?? ""];
  /** Smart display: an engine counts while its chart shows any work. */
  const busy = (name: string) => wasBusy(series(`${k()}.e${engines().indexOf(name)}`));
  const shown = createMemo(() => shownEngines(engines(), choice(), busy), [], { equals: sameList });
  const choose = (next: EngineChoice) => {
    const model = gpu()?.info.name;
    if (model) setEngineChoices({ ...engineChoices(), [model]: next });
  };
  const display = (): EngineDisplay => (isSmart(choice()) ? "smart" : shown().length);
  const displayOptions = () => [
    { value: "smart" as EngineDisplay, label: t("performance.gpu.engineSmart") },
    ...engines().map((_, i) => ({
      value: (i + 1) as EngineDisplay,
      label:
        i + 1 === engines().length
          ? t("performance.gpu.engineCountAll", { count: i + 1 })
          : t("performance.gpu.engineCountOption", { count: i + 1 }),
    })),
  ];
  const setDisplay = (value: EngineDisplay) =>
    choose(
      value === "smart" ? withSmart(engines(), choice()) : withCount(engines(), choice(), value, cells()),
    );
  const engineOptions = () => engines().map((e) => ({ value: e, label: e }));

  // The charts on screen follow `shown` through the grid's transition: those
  // that stay glide to their new places, new ones grow in, and ones taken out
  // fade where they were. Run after Solid has settled, so the grid's DOM
  // changes at once and can be measured on both sides.
  const [cells, setCells] = createSignal<string[]>(untrack(shown));
  let grid: ReturnType<typeof engineGrid> | undefined;
  createEffect(
    on(shown, (list) => queueMicrotask(() => (grid ? grid.change(() => setCells(list)) : setCells(list))), {
      defer: true,
    }),
  );
  createEffect(on(layoutOf, () => queueMicrotask(() => grid?.change()), { defer: true }));
  const [share, setShare] = createSignal(clampShare(savedShare()));

  return (
    <Show when={gpu()} fallback={<p class="empty">{t("performance.gpu.notFound")}</p>}>
      {(g) => (
        <div class="perf-main gpu-view" style={{ "--engine-share": String(share()) }}>
          <Head title={`GPU ${index()}`} model={g().info.name} />
          <Show when={engines().length > 1}>
            <div class="perf-toolbar">
              <Select<EngineDisplay>
                value={display()}
                options={displayOptions()}
                label={t("performance.gpu.engineCount")}
                title={isSmart(choice()) ? t("performance.gpu.engineSmartTitle") : undefined}
                onChange={setDisplay}
              />
              <div class="segmented" role="group" aria-label={t("performance.gpu.layout")}>
                <Indicator selector="[aria-pressed='true']" track={layoutOf} class="seg-thumb" />
                <For each={ENGINE_LAYOUTS}>
                  {(layout) => (
                    <button
                      type="button"
                      title={t(`performance.gpu.layoutTitles.${layout}`)}
                      aria-pressed={layoutOf() === layout}
                      onClick={() => setEngineLayout(layout)}
                    >
                      {t(`performance.gpu.layouts.${layout}`)}
                    </button>
                  )}
                </For>
              </div>
            </div>
          </Show>
          <div class="chart-labels">
            <span>{t("performance.gpu.engineUtilization")}</span>
            <span>100%</span>
          </div>
          <div
            class="chart-grid engines"
            ref={(el) => (grid = engineGrid(el, () => cells().length, layoutOf))}
          >
            {/* Keyed by engine: a chart that moves to another place keeps its trace. */}
            <For each={cells()}>
              {(name, slot) => {
                const e = () => engines().indexOf(name);
                return (
                  <div class="engine-cell" style={{ "--c": "var(--gpu)" }}>
                    <div class="cell-head">
                      <Select<string>
                        class="compact"
                        value={name}
                        options={engineOptions()}
                        label={t("performance.gpu.engineType")}
                        onChange={(next) => choose(withEngine(engines(), choice(), slot(), next, cells()))}
                      />
                      <span class="cell-load">
                        {Math.round(g().engines[e()] ?? 0)}
                        <small>%</small>
                      </span>
                    </div>
                    <div class="chart-frame">
                      <Chart
                        series={[{ data: () => series(`${k()}.e${e()}`), colorVar: "--gpu" }]}
                        max={() => 100}
                      />
                    </div>
                  </div>
                );
              }}
            </For>
          </div>
          <ShareSplitter
            share={share()}
            onResize={(v) => setShare(clampShare(v))}
            onCommit={() => saveShare(share())}
          />
          <div class="chart-pair">
            <ChartBox
              label={t("performance.gpu.dedicatedMemory")}
              max={`${fmtGB(g().info.dedicatedTotal)} GB`}
              colorVar="--gpu"
              size="small"
              scale={() => g().info.dedicatedTotal || 1}
              format={gb}
              series={[{ data: () => series(`${k()}.ded`), colorVar: "--gpu" }]}
            />
            <ChartBox
              label={t("performance.gpu.sharedMemory")}
              max={`${fmtGB(g().info.sharedTotal)} GB`}
              colorVar="--gpu"
              size="small"
              scale={() => g().info.sharedTotal || 1}
              format={gb}
              series={[{ data: () => series(`${k()}.shared`), colorVar: "--gpu" }]}
            />
          </div>

          <div class="perf-stats">
            <div class="stat-grid">
              <Stat
                label={t("performance.gpu.utilization")}
                value={Math.round(g().util)}
                unit="%"
                note={t(`performance.gpu.utilSource.${g().utilSource ?? "windows"}`)}
                hint={t("performance.gpu.utilizationHint")}
              />
              <Stat
                label={t("performance.gpu.dedicatedMemory")}
                value={`${fmtGB(g().dedicatedUsed)} / ${fmtGB(g().info.dedicatedTotal)}`}
                unit="GB"
              />
              <Stat
                label={t("performance.gpu.sharedMemory")}
                value={`${fmtGB(g().sharedUsed)} / ${fmtGB(g().info.sharedTotal)}`}
                unit="GB"
              />
              <Show when={g().temperature !== null}>
                <Stat
                  label={t("performance.gpu.temperature")}
                  value={Math.round(g().temperature ?? 0)}
                  unit="°C"
                />
              </Show>
              <Show when={g().fanRpm !== null}>
                <Stat label={t("performance.gpu.fan")} value={fmtCount(g().fanRpm ?? 0)} unit="RPM" />
              </Show>
              <Show when={g().clockMhz}>
                {(m) => <Stat label={t("performance.gpu.coreClock")} value={fmtCount(m())} unit="MHz" />}
              </Show>
              <Show when={g().memoryClockMhz}>
                {(m) => <Stat label={t("performance.gpu.memoryClock")} value={fmtCount(m())} unit="MHz" />}
              </Show>
              <Show when={g().voltageMv}>
                {(v) => (
                  <Stat label={t("performance.gpu.coreVoltage")} value={(v() / 1000).toFixed(3)} unit="V" />
                )}
              </Show>
              <Show
                when={g().boardPower !== null}
                fallback={
                  <Show when={g().power}>
                    {(p) => <Stat label={t("performance.gpu.power")} value={Math.round(p())} unit="% TDP" />}
                  </Show>
                }
              >
                <Stat label={t("performance.gpu.power")} value={fmtW(g().boardPower ?? 0)} unit="W" />
              </Show>
            </div>
            <dl class="info-list">
              <Show when={g().powerSource}>
                {(source) => (
                  <>
                    <dt>{t("performance.gpu.powerSource")}</dt>
                    <dd>{t(`performance.gpu.sources.${source()}`)}</dd>
                  </>
                )}
              </Show>
              <dt>{t("performance.gpu.dedicatedMemory")}</dt>
              <dd>{fmtGB(g().info.dedicatedTotal)} GB</dd>
              <dt>{t("performance.gpu.sharedMemory")}</dt>
              <dd>{fmtGB(g().info.sharedTotal)} GB</dd>
              <Show when={g().boardPowerLimit}>
                {(w) => (
                  <>
                    <dt>{t("performance.gpu.powerLimit")}</dt>
                    <dd>{watts(w())}</dd>
                  </>
                )}
              </Show>
              <Show when={g().info.temperatureMax}>
                <dt>{t("performance.gpu.maxTemperature")}</dt>
                <dd>{Math.round(g().info.temperatureMax ?? 0)} °C</dd>
              </Show>
              <dt>{t("performance.gpu.engines")}</dt>
              <dd>{g().info.engines.length}</dd>
              <Show when={g().clockMaxMhz}>
                <dt>{t("performance.gpu.maxCoreClock")}</dt>
                <dd>{fmtCount(g().clockMaxMhz ?? 0)} MHz</dd>
              </Show>
            </dl>
          </div>
        </div>
      )}
    </Show>
  );
}

// ---- Battery -----------------------------------------------------------------

const batteryState = (state: BatteryStats["state"]) => t(`performance.battery.state.${state}`);

type Chemistry = "liIon" | "liPoly" | "leadAcid" | "niCd" | "niMh" | "alkaline";
const CHEMISTRY: Record<string, Chemistry> = {
  LION: "liIon",
  "LI-I": "liIon",
  LIP: "liPoly",
  LIPO: "liPoly",
  PBAC: "leadAcid",
  NICD: "niCd",
  NIMH: "niMh",
  RAM: "alkaline",
};

const chemistryName = (code: string) => {
  const id = CHEMISTRY[code.toUpperCase()];
  return id ? t(`performance.battery.chemistries.${id}`) : code || "—";
};

const fmtDuration = (secs: number) => {
  const h = Math.floor(secs / 3600);
  const m = Math.round((secs % 3600) / 60);
  return h > 0
    ? t("performance.battery.hoursMinutes", { hours: h, minutes: m })
    : t("performance.battery.minutes", { minutes: m });
};
const wh = (mwh: number | null) => (mwh ? `${(mwh / 1000).toFixed(1)} Wh` : "—");

function batterySub(b: BatteryStats): string {
  const pct = `${Math.round(b.percent)}%`;
  if (b.state === "discharging") {
    return b.remaining
      ? `${pct}  ${t("performance.battery.timeLeft", { time: fmtDuration(b.remaining) })}`
      : `${pct}  ${t("performance.battery.onBattery")}`;
  }
  return `${pct}  ${batteryState(b.state)}`;
}

function BatteryView(props: { id: string }) {
  const bat = () => now().hardware.batteries.find((b) => b.info.id === props.id);
  const k = `bat.${props.id}`;
  const powerMax = wattTop(5, `${k}.w`);

  return (
    <Show when={bat()} fallback={<p class="empty">{t("performance.battery.removed")}</p>}>
      {(b) => {
        const health = () => {
          const { fullCapacity: full, designedCapacity: design } = b().info;
          return full && design ? (full / design) * 100 : null;
        };
        const flow = () =>
          b().state === "charging"
            ? t("performance.battery.chargeRate")
            : t("performance.battery.dischargeRate");
        return (
          <div class="perf-main">
            <Head
              title={t("performance.battery.title")}
              model={[b().info.manufacturer, b().info.name].filter(Boolean).join(" ")}
            />
            <ChartBox
              label={t("performance.battery.level")}
              max="100%"
              colorVar="--bat"
              size="half"
              scale={() => 100}
              format={pct}
              series={[{ data: () => series(`${k}.pct`), colorVar: "--bat" }]}
            />
            <Show when={b().rate !== null}>
              <div class="perf-grow section-gap">
                <ChartBox
                  label={flow()}
                  max={`${powerMax()} W`}
                  colorVar="--bat"
                  size="half"
                  scale={powerMax}
                  format={watts}
                  series={[{ data: () => series(`${k}.w`), colorVar: "--bat" }]}
                />
              </div>
            </Show>

            <div class="perf-stats">
              <div class="stat-grid">
                <Stat label={t("performance.battery.level")} value={Math.round(b().percent)} unit="%" />
                <Stat label={t("common.status")} value={batteryState(b().state)} />
                <Show when={b().rate !== null}>
                  <Stat label={flow()} value={fmtW(Math.abs(b().rate ?? 0) / 1000)} unit="W" />
                </Show>
                <Show when={b().voltage}>
                  {(v) => (
                    <Stat label={t("performance.battery.voltage")} value={(v() / 1000).toFixed(2)} unit="V" />
                  )}
                </Show>
                <Show when={b().remaining}>
                  {(s) => <Stat label={t("performance.battery.remaining")} value={fmtDuration(s())} />}
                </Show>
                <Show when={health()}>
                  {(h) => <Stat label={t("performance.battery.health")} value={Math.round(h())} unit="%" />}
                </Show>
                <Show when={b().info.cycleCount}>
                  {(c) => <Stat label={t("performance.battery.cycles")} value={c()} />}
                </Show>
                <Show when={b().temperature}>
                  {(c) => <Stat label={t("common.resource.temperature")} value={Math.round(c())} unit="°C" />}
                </Show>
              </div>
              <dl class="info-list">
                <dt>{t("performance.battery.capacity")}</dt>
                <dd>{wh(b().capacity)}</dd>
                <dt>{t("performance.battery.fullCapacity")}</dt>
                <dd>{wh(b().info.fullCapacity)}</dd>
                <dt>{t("performance.battery.designCapacity")}</dt>
                <dd>{wh(b().info.designedCapacity)}</dd>
                <dt>{t("performance.battery.chemistry")}</dt>
                <dd>{chemistryName(b().info.chemistry)}</dd>
                <dt>{t("performance.battery.manufacturer")}</dt>
                <dd>{b().info.manufacturer || "—"}</dd>
                <dt>{t("performance.battery.model")}</dt>
                <dd>{b().info.name || "—"}</dd>
                <dt>{t("performance.battery.serial")}</dt>
                <dd>{b().info.serial || "—"}</dd>
                <Show when={b().info.manufactured}>
                  <dt>{t("performance.battery.manufactured")}</dt>
                  <dd>{b().info.manufactured}</dd>
                </Show>
              </dl>
            </div>
            <Show when={health() !== null && (health() ?? 100) < 80}>
              <p class="perf-note">
                {t("performance.battery.wornNote", { percent: Math.round(health() ?? 0) })}
              </p>
            </Show>
          </div>
        );
      }}
    </Show>
  );
}
