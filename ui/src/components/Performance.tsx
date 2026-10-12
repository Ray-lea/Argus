import { createEffect, createMemo, For, type JSX, Match, on, Show, Switch } from "solid-js";
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
import { EASE_OUT, play } from "../lib/motion";
import { persisted } from "../lib/prefs";
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
import type { BatteryStats, DiskStats, Hardware, NicStats, Snapshot, SystemStats } from "../lib/types";
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
  return secs >= 120 ? `${secs / 60} 分钟` : `${secs} 秒`;
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
  `磁盘 ${d.info.index}${d.info.volumes.length ? ` (${d.info.volumes.join(" ")})` : ""}`;

const diskKind = (d: DiskStats) => {
  const bus =
    d.info.bus === "nvme" ? "NVMe" : d.info.bus === "usb" ? "USB" : d.info.bus === "sata" ? "SATA" : "";
  const media = d.info.ssd === true ? "SSD" : d.info.ssd === false ? "HDD" : d.info.removable ? "可移动" : "";
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
      title: "内存",
      sub: `${fmtGB(sys.memUsed)} / ${fmtGB(sys.memTotal)} GB（${Math.round((sys.memUsed / sys.memTotal) * 100)}%）`,
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
      sub: `收 ${fmtNet(n.rx)}  发 ${fmtNet(n.tx)}`,
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
      sub: `${Math.round(g.util)}%${g.temperature !== null ? `  ${Math.round(g.temperature)} °C` : ""}`,
      colorVar: "--gpu",
      series: [{ data: () => series(`gpu.${g.info.id}.util`), colorVar: "--gpu" }],
      max: () => 100,
    });
  });
  for (const b of hw.batteries) {
    list.push({
      key: `bat:${b.info.id}`,
      title: hw.batteries.length > 1 ? `电池 ${b.info.id}` : "电池",
      sub: batterySub(b),
      colorVar: "--bat",
      series: [{ data: () => series(`bat.${b.info.id}.pct`), colorVar: "--bat" }],
      max: () => 100,
    });
  }
  return list;
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
    <div class="perf page">
      <nav class="dev-list" aria-label="设备">
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
const CORE_FILTERS: { id: CoreFilter; label: string; title: string }[] = [
  { id: "all", label: "全部", title: "显示所有逻辑处理器" },
  { id: "p", label: "性能核", title: "只显示性能核（P 核）" },
  { id: "e", label: "能效核", title: "只显示能效核（E 核）" },
];

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
  const temp = () => (hw().thermal.length ? Math.max(...hw().thermal.map((z) => z.celsius)) : null);
  const powerMax = autoMax(5, "cpu.pkgW", "cpu.coresW");
  /** Mean clock of the P or E cores, MHz. */
  const meanMhz = (eff: boolean) => {
    const list = cpu()
      .threads.map((t, i) => (t.efficient === eff ? (hw().coreMhz[i] ?? 0) : 0))
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
      ? `${c.cores}（${c.performanceCores} 性能核 + ${c.efficiencyCores} 能效核）`
      : String(c.cores);
  };

  return (
    <>
      <Head title="CPU" model={cpu().name} />
      <div class="perf-toolbar">
        <Show when={hybrid() && cpuMode() === "logical"}>
          <div class="segmented" role="group" aria-label="核心类型">
            <Indicator selector="[aria-pressed='true']" track={coreFilter} class="seg-thumb" />
            <For each={CORE_FILTERS}>
              {(f) => (
                <button
                  type="button"
                  title={f.title}
                  aria-pressed={coreFilter() === f.id}
                  onClick={() => setCoreFilter(f.id)}
                >
                  {f.label}
                </button>
              )}
            </For>
          </div>
        </Show>
        <div class="segmented" role="group" aria-label="CPU 图表">
          <Indicator selector="[aria-pressed='true']" track={cpuMode} class="seg-thumb" />
          <button type="button" aria-pressed={cpuMode() === "overall"} onClick={() => setCpuMode("overall")}>
            整体
          </button>
          <button type="button" aria-pressed={cpuMode() === "logical"} onClick={() => setCpuMode("logical")}>
            逻辑处理器
          </button>
        </div>
      </div>

      <div ref={charts}>
        <Show
          when={cpuMode() === "logical"}
          fallback={
            <div>
              <ChartBox
                label="利用率 %"
                max="100%"
                colorVar="--cpu"
                size="main"
                scale={() => 100}
                format={pct}
                series={[
                  { data: () => series("cpu"), colorVar: "--cpu", label: "总计" },
                  {
                    data: () => series("cpu.kernel"),
                    colorVar: "--cpu",
                    dashed: true,
                    fill: 0.18,
                    label: "内核",
                  },
                ]}
              />
              <div class="legend" style={{ "--c": "var(--cpu)" }}>
                <span>总计</span>
                <span class="dashed">内核时间</span>
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
                  return (
                    <div
                      class="chart-frame core-cell"
                      style={{ "--c": `var(${color()})` }}
                      title={`逻辑处理器 ${i}，物理核心 ${cpu().threads[i]?.core ?? "?"}${
                        hybrid() ? (eff() ? "（能效核）" : "（性能核）") : ""
                      }`}
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
                <span style={{ "--c": "var(--cpu)" }}>性能核</span>
                <span style={{ "--c": "var(--cpu-e)" }}>能效核</span>
              </div>
            </Show>
          </div>
        </Show>
      </div>

      <Show when={temp() !== null || hw().power.package !== null}>
        <div class="chart-pair section-gap">
          <Show when={temp() !== null}>
            <ChartBox
              label="温度"
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
              label="功耗"
              max={watts(powerMax())}
              colorVar="--cpu"
              size="small"
              scale={powerMax}
              format={watts}
              series={[
                { data: () => series("cpu.pkgW"), colorVar: "--cpu", label: "封装" },
                { data: () => series("cpu.coresW"), colorVar: "--cpu", dashed: true, fill: 0, label: "核心" },
              ]}
            />
          </Show>
        </div>
      </Show>

      <div class="perf-stats">
        <div class="stat-grid">
          <Stat label="利用率" value={Math.round(sys().cpu)} unit="%" />
          <Stat label="速度" value={((hw().cpuMhz ?? 0) / 1000).toFixed(2)} unit="GHz" />
          <Show when={hybrid() && meanMhz(false)}>
            {(m) => <Stat label="性能核频率" value={(m() / 1000).toFixed(2)} unit="GHz" />}
          </Show>
          <Show when={hybrid() && meanMhz(true)}>
            {(m) => <Stat label="能效核频率" value={(m() / 1000).toFixed(2)} unit="GHz" />}
          </Show>
          <Show when={temp()}>{(t) => <Stat label="温度" value={Math.round(t())} unit="°C" />}</Show>
          <Show when={hw().power.package}>{(w) => <Stat label="封装功耗" value={fmtW(w())} unit="W" />}</Show>
          <Show when={hw().power.cores}>{(w) => <Stat label="核心功耗" value={fmtW(w())} unit="W" />}</Show>
          <Show when={hw().power.graphics}>
            {(w) => <Stat label="核显功耗" value={fmtW(w())} unit="W" />}
          </Show>
          <Stat label="进程" value={fmtCount(sys().processCount)} />
          <Stat label="线程" value={fmtCount(sys().threadCount)} />
          <Stat label="句柄" value={fmtCount(sys().handleCount)} />
          <Stat label="正常运行时间" value={fmtUptime(sys().uptimeMs)} />
        </div>
        <dl class="info-list">
          <dt>基准速度</dt>
          <dd>{cpu().baseMhz ? fmtGHz(cpu().baseMhz) : "—"}</dd>
          <dt>插槽</dt>
          <dd>{cpu().sockets}</dd>
          <dt>内核</dt>
          <dd>{coresText()}</dd>
          <dt>逻辑处理器</dt>
          <dd>{cpu().logical}</dd>
          <dt>虚拟化</dt>
          <dd>{cpu().virtualization ? "已启用" : "未启用"}</dd>
          <dt>L1 缓存</dt>
          <dd>{fmtCache(cpu().l1)}</dd>
          <dt>L2 缓存</dt>
          <dd>{fmtCache(cpu().l2)}</dd>
          <dt>L3 缓存</dt>
          <dd>{fmtCache(cpu().l3)}</dd>
          <Show when={hw().thermal.length > 0}>
            <dt>温度来源</dt>
            <dd>
              ACPI 热区{" "}
              {hw()
                .thermal.map((z) => z.name)
                .join("、")}
            </dd>
          </Show>
        </dl>
      </div>
      <Show when={hw().thermal.length > 0}>
        <p class="perf-note">
          温度来自主板的 ACPI 热区，通常反映 CPU 封装附近的温度。每个核心的温度要读取处理器内部寄存器，Windows
          只允许内核驱动读取，将随特权服务（路线图 M5）提供。
        </p>
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
        title="内存"
        model={`${fmtGB(mem().installed || sys().memTotal, 0)} GB${mem().kind ? ` ${mem().kind}` : ""}`}
      />
      <ChartBox
        label="内存使用量"
        max={`${fmtGB(sys().memTotal)} GB`}
        colorVar="--mem"
        size="main"
        scale={() => sys().memTotal}
        format={gb}
        series={[{ data: () => series("mem"), colorVar: "--mem" }]}
      />

      <div class="mem-bar" role="img" aria-label="内存组成">
        <span class="used" style={{ "--f": frac(sys().memUsed) }} />
        <span class="cached" style={{ "--u": frac(sys().memUsed), "--f": frac(cachedShown()) }} />
      </div>
      <div class="mem-legend">
        <span>
          <i style={{ background: "color-mix(in srgb, var(--mem) 55%, transparent)" }} />
          使用中 {fmtGB(sys().memUsed)} GB
        </span>
        <span>
          <i style={{ background: "color-mix(in srgb, var(--mem) 20%, transparent)" }} />
          缓存 {fmtGB(cachedShown())} GB
        </span>
        <span>
          <i style={{ border: "1px solid var(--stroke-strong)" }} />
          空闲 {fmtGB(Math.max(0, sys().memAvailable - cachedShown()))} GB
        </span>
      </div>

      <div class="perf-stats">
        <div class="stat-grid">
          <Stat label="使用中" value={fmtGB(sys().memUsed)} unit="GB" />
          <Stat label="可用" value={fmtGB(sys().memAvailable)} unit="GB" />
          <Stat label="已提交" value={`${fmtGB(sys().commitUsed)} / ${fmtGB(sys().commitLimit)}`} unit="GB" />
          <Stat label="已缓存" value={fmtGB(sys().cached)} unit="GB" />
          <Stat label="分页缓冲池" {...parts(fmtMem(sys().kernelPaged))} />
          <Stat label="非分页缓冲池" {...parts(fmtMem(sys().kernelNonpaged))} />
        </div>
        <dl class="info-list">
          <dt>速度</dt>
          <dd>{mem().speed ? `${mem().speed} MT/s` : "—"}</dd>
          <dt>已使用的插槽</dt>
          <dd>{mem().slotsTotal ? `${mem().slotsUsed} / ${mem().slotsTotal}` : "—"}</dd>
          <dt>外形规格</dt>
          <dd>{mem().formFactor ?? "—"}</dd>
          <dt>类型</dt>
          <dd>{mem().kind ?? "—"}</dd>
          <Show when={reserved() > 0}>
            <dt>为硬件保留</dt>
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
    <Show when={disk()} fallback={<p class="empty">该磁盘已移除。</p>}>
      {(d) => (
        <>
          <Head title={diskTitle(d())} model={d().info.model} />
          <ChartBox
            label="活动时间"
            max="100%"
            colorVar="--io"
            size="half"
            scale={() => 100}
            format={pct}
            series={[{ data: () => series(`${k()}.active`), colorVar: "--io" }]}
          />
          <div class="section-gap">
            <ChartBox
              label="磁盘传输速率"
              max={fmtRate(rateMax())}
              colorVar="--io"
              size="half"
              scale={rateMax}
              format={(v) => fmtRate(v, false)}
              series={[
                { data: () => series(`${k()}.read`), colorVar: "--io", label: "读取" },
                {
                  data: () => series(`${k()}.write`),
                  colorVar: "--io",
                  dashed: true,
                  fill: 0,
                  label: "写入",
                },
              ]}
            />
            <div class="legend" style={{ "--c": "var(--io)" }}>
              <span>读取</span>
              <span class="dashed">写入</span>
            </div>
          </div>
          <div class="perf-stats">
            <div class="stat-grid">
              <Stat label="活动时间" value={Math.round(d().active)} unit="%" />
              <Stat label="平均响应时间" value={d().responseMs.toFixed(1)} unit="毫秒" />
              <Stat label="读取速度" value={rateParts(d().read)[0]} unit={rateParts(d().read)[1]} />
              <Stat label="写入速度" value={rateParts(d().write)[0]} unit={rateParts(d().write)[1]} />
              <Show when={d().temperature}>
                {(t) => <Stat label="温度" value={Math.round(t())} unit="°C" />}
              </Show>
            </div>
            <dl class="info-list">
              <dt>容量</dt>
              <dd>{d().info.size ? fmtCapacity(d().info.size) : "—"}</dd>
              <dt>类型</dt>
              <dd>{diskKind(d()) || "—"}</dd>
              <dt>卷</dt>
              <dd>{d().info.volumes.join("  ") || "无"}</dd>
              <Show when={d().info.removable}>
                <dt>可移动</dt>
                <dd>是</dd>
              </Show>
              <Show when={d().temperatureWarning}>
                <dt>警告温度</dt>
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

const NIC_KIND: Record<NicStats["info"]["kind"], string> = {
  ethernet: "以太网",
  wifi: "Wi-Fi",
  virtual: "虚拟适配器",
  other: "其他",
};

function NicView(props: { id: string }) {
  const nic = () => now().hardware.nics.find((n) => n.info.id === props.id);
  const k = () => `nic.${props.id}`;
  const scale = autoMax(64 * 1024, `${k()}.rx`, `${k()}.tx`);
  const listAddrs = (list: string[]) =>
    list.length > 3 ? `${list.slice(0, 3).join("\n")}\n等 ${list.length} 个` : list.join("\n") || "—";

  return (
    <Show when={nic()} fallback={<p class="empty">该网络连接已断开。</p>}>
      {(n) => (
        <>
          <Head title={n().info.name} model={n().info.description} />
          <ChartBox
            label="吞吐量"
            max={fmtNet(scale())}
            colorVar="--net"
            size="main"
            scale={scale}
            format={(v) => `${fmtNet(v)}（${fmtNetAlt(v)}）`}
            series={[
              { data: () => series(`${k()}.rx`), colorVar: "--net", label: "接收" },
              { data: () => series(`${k()}.tx`), colorVar: "--net", dashed: true, fill: 0, label: "发送" },
            ]}
          />
          <div class="legend" style={{ "--c": "var(--net)" }}>
            <span>接收</span>
            <span class="dashed">发送</span>
          </div>
          <div class="perf-stats">
            <div class="stat-grid">
              <Stat
                label="接收"
                value={netParts(n().rx)[0]}
                unit={netParts(n().rx)[1]}
                note={fmtNetAlt(n().rx)}
              />
              <Stat
                label="发送"
                value={netParts(n().tx)[0]}
                unit={netParts(n().tx)[1]}
                note={fmtNetAlt(n().tx)}
              />
            </div>
            <dl class="info-list selectable">
              <dt>连接类型</dt>
              <dd>{NIC_KIND[n().info.kind]}</dd>
              <dt>链路速度</dt>
              <dd>
                {n().info.linkSpeed
                  ? `${fmtLinkSpeed(n().info.linkSpeed)}（${fmtRate(n().info.linkSpeed / 8, false)}）`
                  : "—"}
              </dd>
              <dt>IPv4 地址</dt>
              <dd style={{ "white-space": "pre-line" }}>{listAddrs(n().info.ipv4)}</dd>
              <dt>IPv6 地址</dt>
              <dd style={{ "white-space": "pre-line" }}>{listAddrs(n().info.ipv6)}</dd>
              <dt>物理地址</dt>
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
    <Show when={gpu()} fallback={<p class="empty">找不到该 GPU。</p>}>
      {(g) => (
        <>
          <Head title={`GPU ${index()}`} model={g().info.name} />
          <div class="chart-labels">
            <span>引擎利用率</span>
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
              label="专用 GPU 内存"
              max={`${fmtGB(g().info.dedicatedTotal)} GB`}
              colorVar="--gpu"
              size="small"
              scale={() => g().info.dedicatedTotal || 1}
              format={gb}
              series={[{ data: () => series(`${k()}.ded`), colorVar: "--gpu" }]}
            />
            <ChartBox
              label="共享 GPU 内存"
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
              <Stat label="利用率" value={Math.round(g().util)} unit="%" />
              <Stat
                label="专用 GPU 内存"
                value={`${fmtGB(g().dedicatedUsed)} / ${fmtGB(g().info.dedicatedTotal)}`}
                unit="GB"
              />
              <Stat
                label="共享 GPU 内存"
                value={`${fmtGB(g().sharedUsed)} / ${fmtGB(g().info.sharedTotal)}`}
                unit="GB"
              />
              <Show when={g().temperature !== null}>
                <Stat label="GPU 温度" value={Math.round(g().temperature ?? 0)} unit="°C" />
              </Show>
              <Show when={g().fanRpm !== null}>
                <Stat label="风扇" value={fmtCount(g().fanRpm ?? 0)} unit="RPM" />
              </Show>
              <Show when={g().clockMhz}>
                {(m) => <Stat label="核心频率" value={fmtCount(m())} unit="MHz" />}
              </Show>
              <Show when={g().memoryClockMhz}>
                {(m) => <Stat label="显存频率" value={fmtCount(m())} unit="MHz" />}
              </Show>
              <Show when={g().voltageMv}>
                {(v) => <Stat label="核心电压" value={(v() / 1000).toFixed(3)} unit="V" />}
              </Show>
              <Show when={g().power}>
                {(p) => <Stat label="功耗" value={Math.round(p())} unit="% TDP" />}
              </Show>
            </div>
            <dl class="info-list">
              <dt>专用 GPU 内存</dt>
              <dd>{fmtGB(g().info.dedicatedTotal)} GB</dd>
              <dt>共享 GPU 内存</dt>
              <dd>{fmtGB(g().info.sharedTotal)} GB</dd>
              <Show when={g().info.temperatureMax}>
                <dt>最高温度</dt>
                <dd>{Math.round(g().info.temperatureMax ?? 0)} °C</dd>
              </Show>
              <dt>引擎</dt>
              <dd>{g().info.engines.length}</dd>
              <Show when={g().clockMaxMhz}>
                <dt>最高核心频率</dt>
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

const BATTERY_STATE: Record<BatteryStats["state"], string> = {
  charging: "正在充电",
  discharging: "正在使用电池",
  full: "已充满",
  idle: "已接通电源",
};

const CHEMISTRY: Record<string, string> = {
  LION: "锂离子",
  "LI-I": "锂离子",
  LIP: "锂聚合物",
  LIPO: "锂聚合物",
  PBAC: "铅酸",
  NICD: "镍镉",
  NIMH: "镍氢",
  RAM: "可充电碱性",
};

const fmtDuration = (secs: number) => {
  const h = Math.floor(secs / 3600);
  const m = Math.round((secs % 3600) / 60);
  return h > 0 ? `${h} 小时 ${m} 分钟` : `${m} 分钟`;
};
const wh = (mwh: number | null) => (mwh ? `${(mwh / 1000).toFixed(1)} Wh` : "—");

function batterySub(b: BatteryStats): string {
  const pct = `${Math.round(b.percent)}%`;
  if (b.state === "discharging") {
    return b.remaining ? `${pct}  剩余 ${fmtDuration(b.remaining)}` : `${pct}  使用电池`;
  }
  return `${pct}  ${BATTERY_STATE[b.state]}`;
}

function BatteryView(props: { id: string }) {
  const bat = () => now().hardware.batteries.find((b) => b.info.id === props.id);
  const k = `bat.${props.id}`;
  const powerMax = autoMax(5, `${k}.w`);

  return (
    <Show when={bat()} fallback={<p class="empty">电池已移除。</p>}>
      {(b) => {
        const health = () => {
          const { fullCapacity: full, designedCapacity: design } = b().info;
          return full && design ? (full / design) * 100 : null;
        };
        const flow = () => (b().state === "charging" ? "充电功率" : "放电功率");
        return (
          <>
            <Head title="电池" model={[b().info.manufacturer, b().info.name].filter(Boolean).join(" ")} />
            <ChartBox
              label="电量"
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
                <Stat label="电量" value={Math.round(b().percent)} unit="%" />
                <Stat label="状态" value={BATTERY_STATE[b().state]} />
                <Show when={b().rate !== null}>
                  <Stat label={flow()} value={fmtW(Math.abs(b().rate ?? 0) / 1000)} unit="W" />
                </Show>
                <Show when={b().voltage}>
                  {(v) => <Stat label="电压" value={(v() / 1000).toFixed(2)} unit="V" />}
                </Show>
                <Show when={b().remaining}>{(s) => <Stat label="剩余时间" value={fmtDuration(s())} />}</Show>
                <Show when={health()}>{(h) => <Stat label="健康度" value={Math.round(h())} unit="%" />}</Show>
                <Show when={b().info.cycleCount}>{(c) => <Stat label="循环次数" value={c()} />}</Show>
                <Show when={b().temperature}>
                  {(t) => <Stat label="温度" value={Math.round(t())} unit="°C" />}
                </Show>
              </div>
              <dl class="info-list">
                <dt>当前容量</dt>
                <dd>{wh(b().capacity)}</dd>
                <dt>完全充电容量</dt>
                <dd>{wh(b().info.fullCapacity)}</dd>
                <dt>设计容量</dt>
                <dd>{wh(b().info.designedCapacity)}</dd>
                <dt>化学成分</dt>
                <dd>{CHEMISTRY[b().info.chemistry.toUpperCase()] ?? (b().info.chemistry || "—")}</dd>
                <dt>制造商</dt>
                <dd>{b().info.manufacturer || "—"}</dd>
                <dt>型号</dt>
                <dd>{b().info.name || "—"}</dd>
                <dt>序列号</dt>
                <dd>{b().info.serial || "—"}</dd>
                <Show when={b().info.manufactured}>
                  <dt>制造日期</dt>
                  <dd>{b().info.manufactured}</dd>
                </Show>
              </dl>
            </div>
            <Show when={health() !== null && (health() ?? 100) < 80}>
              <p class="perf-note">
                完全充电容量已降到设计容量的 {Math.round(health() ?? 0)}%，充满后的续航会比新电池短。
              </p>
            </Show>
          </>
        );
      }}
    </Show>
  );
}
