import { createEffect, createMemo, createSignal, For, type JSX, Match, on, Show, Switch } from "solid-js";
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
  fmtUptime,
  netParts,
  rateParts,
} from "../lib/format";
import { t } from "../lib/i18n";
import { EASE_OUT, play } from "../lib/motion";
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
import Indicator from "./Indicator";
import { Chart, easedMax, type Series } from "./Trace";

const MB = 1024 * 1024;

/** Detail views render only once a snapshot exists (guarded by the page). */
const now = () => snapshot() as Snapshot;

// ---- Building blocks ---------------------------------------------------------

/** Auto-scaling ceiling for rate charts, with a floor so idle devices stay flat. */
const rawMax = (floor: number, ...keys: string[]) =>
  Math.max(floor, ...keys.flatMap((k) => series(k)).filter(Number.isFinite)) * 1.15;
/** The same, easing down after a spike instead of jumping. Create once per view. */
const autoMax = (floor: number, ...keys: string[]) => easedMax(() => rawMax(floor, ...keys));

const pct = (v: number) => `${Math.round(v)}%`;
const gb = (v: number) => `${fmtGB(v)} GB`;

/** Time a chart spans at the current refresh rate. */
function spanLabel() {
  const secs = (HISTORY * intervalMs()) / 1000;
  return secs >= 120
    ? t("performance.span.minutes", { count: secs / 60 })
    : t("performance.span.seconds", { count: secs });
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
    <div>
      <div class="chart-labels">
        <span>{props.label}</span>
        <span>{props.max}</span>
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

function Stat(props: { label: string; value: JSX.Element; unit?: string; note?: string }) {
  return (
    <div>
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

/** A figure split into number and unit, e.g. memory as "912.0" + "MB". */
const parts = (text: string) => {
  const [value, unit] = text.split(" ");
  return { value, unit };
};

function Head(props: { title: string; model?: string }) {
  return (
    <header class="perf-head">
      <h2 class="perf-title">{props.title}</h2>
      <Show when={props.model}>
        <span class="perf-model" title={props.model}>
          {props.model}
        </span>
      </Show>
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

const diskKind = (d: DiskStats) => {
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
      max: () => rawMax(16 * 1024, `${k}.rx`, `${k}.tx`),
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

/**
 * The handle between the device list and the detail: drag it, use the arrow
 * keys, or double-click to go back to the default width. The width is only
 * saved when a drag ends.
 */
function Splitter(props: { width: number; onResize: (width: number) => void; onCommit: () => void }) {
  const [dragging, setDragging] = createSignal(false);
  const begin = (e: PointerEvent) => {
    if (e.button !== 0) return;
    const handle = e.currentTarget as HTMLElement;
    handle.setPointerCapture(e.pointerId);
    setDragging(true);
    const startX = e.clientX;
    const start = props.width;
    let next = start;
    let frame = 0;
    const move = (m: PointerEvent) => {
      next = start + m.clientX - startX;
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
      props.onResize(next);
      props.onCommit();
      setDragging(false);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("lostpointercapture", end);
  };
  const key = (e: KeyboardEvent) => {
    const step = e.key === "ArrowLeft" ? -16 : e.key === "ArrowRight" ? 16 : 0;
    if (step === 0 && e.key !== "Home" && e.key !== "End") return;
    e.preventDefault();
    props.onResize(e.key === "Home" ? SIDE_MIN : e.key === "End" ? SIDE_MAX : props.width + step);
    props.onCommit();
  };
  return (
    <div
      class="perf-splitter"
      classList={{ dragging: dragging() }}
      role="separator"
      tabIndex={0}
      aria-orientation="vertical"
      aria-label={t("performance.resizeList")}
      aria-valuemin={SIDE_MIN}
      aria-valuemax={SIDE_MAX}
      aria-valuenow={props.width}
      onPointerDown={begin}
      onKeyDown={key}
      onDblClick={() => {
        props.onResize(SIDE_DEFAULT);
        props.onCommit();
      }}
    />
  );
}

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
      <nav class="dev-list" aria-label={t("performance.devices")}>
        <Indicator selector="[aria-current='true']" track={current} class="dev-mark" />
        <For each={keys()}>
          {(key) => {
            const d = () => deviceMap().get(key);
            const max = easedMax(() => d()?.max() ?? 1);
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
      <Splitter width={side()} onResize={(w) => setSide(clampSide(w))} onCommit={() => saveSide(side())} />

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
        hw().cpuSensors?.coreTemperatures.map((core) => [core.group * 64 + core.processor, core.celsius]) ??
          [],
      ),
  );
  const powerMax = autoMax(5, "cpu.pkgW", "cpu.coresW");
  /** Mean clock of the P or E cores, MHz. */
  const meanMhz = (eff: boolean) => {
    const list = cpu()
      .threads.map((th, i) => (th.efficient === eff ? (hw().coreMhz[i] ?? 0) : 0))
      .filter((m) => m > 0);
    return list.length ? list.reduce((a, b) => a + b, 0) / list.length : null;
  };

  let charts!: HTMLDivElement;
  const settle = () =>
    play(
      charts.firstElementChild,
      [
        { opacity: 0, transform: "scale(0.985)" },
        { opacity: 1, transform: "none" },
      ],
      { duration: 200, easing: EASE_OUT },
    );
  createEffect(on(cpuMode, settle, { defer: true }));
  createEffect(on(coreFilter, () => cpuMode() === "logical" && settle(), { defer: true }));

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

  return (
    <>
      <Head title="CPU" model={cpu().name} />
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
                  {f === "all" ? t("performance.cpu.filterAll") : t(`performance.cpu.coreKind.${f}`)}
                </button>
              )}
            </For>
          </div>
        </Show>
        <div class="segmented" role="group" aria-label={t("performance.cpu.graph")}>
          <Indicator selector="[aria-pressed='true']" track={cpuMode} class="seg-thumb" />
          <button type="button" aria-pressed={cpuMode() === "overall"} onClick={() => setCpuMode("overall")}>
            {t("performance.cpu.overall")}
          </button>
          <button type="button" aria-pressed={cpuMode() === "logical"} onClick={() => setCpuMode("logical")}>
            {t("performance.cpu.logical")}
          </button>
        </div>
      </div>

      <div ref={charts}>
        <Show
          when={cpuMode() === "logical"}
          fallback={
            <div>
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
          <div>
            <div class="chart-grid cores">
              <For each={shown()}>
                {(i) => {
                  const eff = () => hybrid() && efficient(i);
                  const color = () => (eff() ? "--cpu-e" : "--cpu");
                  const mhz = () => hw().coreMhz[i] ?? 0;
                  const temperature = () => coreTemperatures().get(i);
                  return (
                    <div
                      class="chart-frame core-cell"
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
                      <span class="chart-cell-label">
                        <Show when={hybrid()}>
                          <i class="core-kind">{eff() ? "E" : "P"}</i>
                        </Show>
                        {i} <b>{Math.round(sys().cores[i] ?? 0)}%</b>
                      </span>
                      <Show when={mhz() > 0}>
                        <span class="chart-cell-note">{fmtGHz(mhz())}</span>
                      </Show>
                      <Chart
                        series={[{ data: () => series(`core.${i}`), colorVar: color() }]}
                        max={() => 100}
                      />
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

      <Show when={temp() !== null || hw().power.package !== null}>
        <div class="chart-pair section-gap">
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
              max={watts(powerMax())}
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
      </Show>

      <div class="perf-stats">
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
            <Stat label={t("performance.cpu.packagePower")} value={fmtW(hw().power.package ?? 0)} unit="W" />
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
          <Stat label={t("performance.cpu.uptime")} value={fmtUptime(sys().uptimeMs)} />
        </div>
        <dl class="info-list">
          <dt>{t("performance.cpu.baseSpeed")}</dt>
          <dd>{cpu().baseMhz ? fmtGHz(cpu().baseMhz) : "—"}</dd>
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
                <Show when={sensors().controlCelsius !== null}>
                  <dt>{t("performance.cpu.controlTemperature")}</dt>
                  <dd>{Math.round(sensors().controlCelsius ?? 0)} °C</dd>
                </Show>
                <Show when={sensors().coreVoltage !== null}>
                  <dt>{t("performance.cpu.coreVoltage")}</dt>
                  <dd>{sensors().coreVoltage?.toFixed(3)} V</dd>
                </Show>
                <Show when={sensors().coreVid !== null}>
                  <dt>{t("performance.cpu.coreVid")}</dt>
                  <dd>{sensors().coreVid?.toFixed(3)} V</dd>
                </Show>
                <For each={sensors().ccdTemperatures.map((ccd) => ccd.index)}>
                  {(index) => (
                    <>
                      <dt>{t("performance.cpu.ccdTemperature", { index: index + 1 })}</dt>
                      <dd>
                        {Math.round(
                          sensors().ccdTemperatures.find((ccd) => ccd.index === index)?.celsius ?? 0,
                        )}{" "}
                        °C
                      </dd>
                    </>
                  )}
                </For>
              </>
            )}
          </Show>
        </dl>
      </div>
      <Show when={!hw().cpuSensors && hw().thermal.length === 0}>
        <p class="perf-note">{t("performance.cpu.thermalMissing")}</p>
      </Show>
    </>
  );
}

// ---- Memory ------------------------------------------------------------------

function MemoryView() {
  const sys = () => now().system;
  const mem = () => now().hardware.memory;
  const frac = (v: number) => Math.max(0, Math.min(1, v / sys().memTotal));
  const cachedShown = () => Math.min(sys().cached, sys().memAvailable);
  const reserved = () => Math.max(0, mem().installed - sys().memTotal);

  return (
    <>
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

      <div class="mem-bar" role="img" aria-label={t("performance.memory.composition")}>
        <span class="used" style={{ "--f": frac(sys().memUsed) }} />
        <span class="cached" style={{ "--u": frac(sys().memUsed), "--f": frac(cachedShown()) }} />
      </div>
      <div class="mem-legend">
        <span>
          <i style={{ background: "color-mix(in srgb, var(--mem) 55%, transparent)" }} />
          {t("performance.memory.legendInUse", { value: fmtGB(sys().memUsed) })}
        </span>
        <span>
          <i style={{ background: "color-mix(in srgb, var(--mem) 20%, transparent)" }} />
          {t("performance.memory.legendCached", { value: fmtGB(cachedShown()) })}
        </span>
        <span>
          <i style={{ border: "1px solid var(--stroke-strong)" }} />
          {t("performance.memory.legendFree", {
            value: fmtGB(Math.max(0, sys().memAvailable - cachedShown())),
          })}
        </span>
      </div>

      <div class="perf-stats">
        <div class="stat-grid">
          <Stat label={t("performance.memory.inUse")} value={fmtGB(sys().memUsed)} unit="GB" />
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
    </>
  );
}

// ---- Disk --------------------------------------------------------------------

function DiskView(props: { index: number }) {
  const disk = () => now().hardware.disks.find((d) => d.info.index === props.index);
  const k = () => `disk.${props.index}`;
  const rateMax = autoMax(MB, `${k()}.read`, `${k()}.write`);
  return (
    <Show when={disk()} fallback={<p class="empty">{t("performance.disk.removed")}</p>}>
      {(d) => (
        <>
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
          <div class="section-gap">
            <ChartBox
              label={t("performance.disk.transferRate")}
              max={fmtRate(rateMax())}
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
        </>
      )}
    </Show>
  );
}

// ---- Network -----------------------------------------------------------------

function NicView(props: { id: string }) {
  const nic = () => now().hardware.nics.find((n) => n.info.id === props.id);
  const k = () => `nic.${props.id}`;
  const scale = autoMax(64 * 1024, `${k()}.rx`, `${k()}.tx`);
  const listAddrs = (list: string[]) =>
    list.length > 3
      ? `${list.slice(0, 3).join("\n")}\n${t("performance.network.more", { count: list.length })}`
      : list.join("\n") || "—";

  return (
    <Show when={nic()} fallback={<p class="empty">{t("performance.network.disconnected")}</p>}>
      {(n) => (
        <>
          <Head title={n().info.name} model={n().info.description} />
          <ChartBox
            label={t("performance.network.throughput")}
            max={fmtNet(scale())}
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
              <dd style={{ "white-space": "pre-line" }}>{listAddrs(n().info.ipv4)}</dd>
              <dt>{t("performance.network.ipv6")}</dt>
              <dd style={{ "white-space": "pre-line" }}>{listAddrs(n().info.ipv6)}</dd>
              <dt>{t("performance.network.mac")}</dt>
              <dd>{n().info.mac ?? "—"}</dd>
            </dl>
          </div>
        </>
      )}
    </Show>
  );
}

// ---- GPU ---------------------------------------------------------------------

function GpuView(props: { id: string }) {
  const gpus = () => now().hardware.gpus;
  const gpu = () => gpus().find((g) => g.info.id === props.id);
  const index = () => gpus().findIndex((g) => g.info.id === props.id);
  const k = () => `gpu.${props.id}`;

  return (
    <Show when={gpu()} fallback={<p class="empty">{t("performance.gpu.notFound")}</p>}>
      {(g) => (
        <>
          <Head title={`GPU ${index()}`} model={g().info.name} />
          <div class="chart-labels">
            <span>{t("performance.gpu.engineUtilization")}</span>
            <span>100%</span>
          </div>
          <div class="chart-grid">
            <For each={g().info.engines}>
              {(name, i) => (
                <div class="chart-frame engine-cell" style={{ "--c": "var(--gpu)" }}>
                  <span class="chart-cell-label">
                    {name} <b>{Math.round(g().engines[i()] ?? 0)}%</b>
                  </span>
                  <Chart
                    series={[{ data: () => series(`${k()}.e${i()}`), colorVar: "--gpu" }]}
                    max={() => 100}
                  />
                </div>
              )}
            </For>
          </div>

          <div class="chart-pair section-gap">
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
              <Stat label={t("performance.gpu.utilization")} value={Math.round(g().util)} unit="%" />
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
        </>
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
  const powerMax = autoMax(5, `${k}.w`);

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
          <>
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
              <div class="section-gap">
                <ChartBox
                  label={flow()}
                  max={watts(powerMax())}
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
          </>
        );
      }}
    </Show>
  );
}
