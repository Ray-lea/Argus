import { createEffect, Index, type JSX, Show, untrack } from "solid-js";
import { fmtGB, fmtNet, netParts, rateParts } from "../lib/format";
import { t } from "../lib/i18n";
import { balancedFrameMs, chartMode } from "../lib/motion";
import { intervalMs, paused, series, setPage, setPerfDevice, snapshot } from "../lib/store";
import type { DiskStats, GpuStats } from "../lib/types";
import Trace, { easedMax } from "./Trace";

const MB = 1024 * 1024;

/**
 * Per-core load bars. Each fill glides, linearly and on the compositor, from
 * where it is to the new sample over exactly one interval, arriving as the
 * next sample does, so the bars never stand still (same modes as the charts).
 */
function CoreBars(props: { values: () => readonly number[] }) {
  const fills: HTMLElement[] = [];
  const bars: { from: number; to: number; start: number; duration: number; anim: Animation | null }[] = [];

  createEffect(() => {
    const values = props.values();
    const mode = paused() ? "off" : chartMode();
    const interval = intervalMs();
    untrack(() => {
      const now = performance.now();
      values.forEach((v, i) => {
        const el = fills[i];
        if (!el) return;
        bars[i] ??= { from: 0, to: 0, start: 0, duration: 0, anim: null };
        const b = bars[i];
        const target = Math.max(0, Math.min(1, v / 100));
        const progress = b.duration > 0 ? Math.min(1, Math.max(0, (now - b.start) / b.duration)) : 1;
        const current = b.from + (b.to - b.from) * progress;
        b.anim?.cancel();
        b.anim = null;
        el.style.transform = `scaleY(${target})`;
        if (mode !== "fluid" && mode !== "balanced") {
          b.from = b.to = target;
          b.duration = 0;
          return;
        }
        let start = now;
        let easing = "linear";
        if (mode === "balanced") {
          // Step on the charts' shared grid so the bars change on their frames.
          const step = balancedFrameMs();
          start = Math.floor(now / step) * step;
          easing = `steps(${Math.max(1, Math.round(interval / step))}, end)`;
        }
        b.anim = el.animate([{ transform: `scaleY(${current})` }, { transform: `scaleY(${target})` }], {
          duration: interval,
          easing,
          fill: "forwards",
        });
        b.anim.startTime = start;
        Object.assign(b, { from: current, to: target, start, duration: interval });
      });
    });
  });

  return (
    <div class="comb" title={t("processes.vitals.cores")}>
      <Index each={props.values()}>
        {(_, i) => (
          <span>
            <i
              ref={(el) => {
                fills[i] = el;
              }}
            />
          </span>
        )}
      </Index>
    </div>
  );
}

/** One band segment; clicking it opens that device on the performance page. */
function Vital(props: { device: string; title: string; children: JSX.Element }) {
  return (
    <button
      type="button"
      class="vital"
      title={t("processes.vitals.open", { device: props.title })}
      onClick={() => {
        setPerfDevice(props.device);
        setPage("performance");
      }}
    >
      {props.children}
    </button>
  );
}

export default function Vitals() {
  const sys = () => snapshot()?.system;
  const hw = () => snapshot()?.hardware;
  const memPct = () => {
    const s = sys();
    return s?.memTotal ? Math.round((s.memUsed / s.memTotal) * 100) : 0;
  };
  const diskTotal = () => hw()?.disks.reduce((a, d) => a + d.read + d.write, 0) ?? 0;
  const busiestDisk = () =>
    hw()?.disks.reduce<DiskStats | null>((a, d) => (!a || d.active > a.active ? d : a), null);
  const netRx = () => hw()?.nics.reduce((a, n) => a + n.rx, 0) ?? 0;
  const netTx = () => hw()?.nics.reduce((a, n) => a + n.tx, 0) ?? 0;
  const gpu = () => hw()?.gpus.reduce<GpuStats | null>((a, g) => (!a || g.util > a.util ? g : a), null);
  const autoMax = (key: string, floor: number) => easedMax(() => Math.max(floor, ...series(key)) * 1.2);
  const diskMax = autoMax("disk.total", MB);
  const netMax = autoMax("net.total", 128 * 1024);

  return (
    <section class="vitals" classList={{ "no-gpu": !gpu() }} aria-label={t("processes.vitals.label")}>
      <Vital device="cpu" title="CPU">
        <div class="vital-head">
          <span class="vital-label">CPU</span>
          <CoreBars values={() => sys()?.cores ?? []} />
        </div>
        <div class="vital-figure">
          <span>
            <span class="vital-value">{Math.round(sys()?.cpu ?? 0)}</span>
            <span class="vital-unit">%</span>
          </span>
          <Show when={hw()?.cpuMhz}>
            {(mhz) => <span class="vital-sub">{(mhz() / 1000).toFixed(2)} GHz</span>}
          </Show>
        </div>
        <Trace class="vital-trace" data={() => series("cpu")} max={() => 100} colorVar="--cpu" />
      </Vital>

      <Vital device="memory" title={t("common.resource.memory")}>
        <div class="vital-head">
          <span class="vital-label">{t("common.resource.memory")}</span>
          <span class="vital-sub">{memPct()}%</span>
        </div>
        <div class="vital-figure">
          <span>
            <span class="vital-value">{fmtGB(sys()?.memUsed ?? 0)}</span>
            <span class="vital-unit">/ {fmtGB(sys()?.memTotal ?? 0)} GB</span>
          </span>
        </div>
        <Trace
          class="vital-trace"
          data={() => series("mem")}
          max={() => sys()?.memTotal ?? 1}
          colorVar="--mem"
        />
      </Vital>

      <Vital device={`disk:${busiestDisk()?.info.index ?? 0}`} title={t("common.resource.disk")}>
        <div class="vital-head">
          <span class="vital-label">{t("common.resource.disk")}</span>
          <Show when={busiestDisk()}>
            {(d) => (
              <span class="vital-sub">
                {t("processes.vitals.diskActive", { value: Math.round(d().active) })}
              </span>
            )}
          </Show>
        </div>
        <div class="vital-figure">
          <span>
            <span class="vital-value">{rateParts(diskTotal())[0]}</span>
            <span class="vital-unit">{rateParts(diskTotal())[1]}</span>
          </span>
        </div>
        <Trace class="vital-trace" data={() => series("disk.total")} max={diskMax} colorVar="--io" />
      </Vital>

      <Vital device={`nic:${hw()?.nics[0]?.info.id ?? ""}`} title={t("common.resource.network")}>
        <div class="vital-head">
          <span class="vital-label">{t("common.resource.network")}</span>
          <span class="vital-sub" title={t("processes.vitals.netTitle")}>
            {t("processes.vitals.netRates", { received: fmtNet(netRx()), sent: fmtNet(netTx()) })}
          </span>
        </div>
        <div class="vital-figure">
          <span>
            <span class="vital-value">{netParts(netRx() + netTx())[0]}</span>
            <span class="vital-unit">{netParts(netRx() + netTx())[1]}</span>
          </span>
        </div>
        <Trace class="vital-trace" data={() => series("net.total")} max={netMax} colorVar="--net" />
      </Vital>

      <Show when={gpu()}>
        {(g) => (
          <Vital device={`gpu:${g().info.id}`} title="GPU">
            <div class="vital-head">
              <span class="vital-label">GPU</span>
              <Show when={g().temperature !== null}>
                <span class="vital-sub">{Math.round(g().temperature ?? 0)} °C</span>
              </Show>
            </div>
            <div class="vital-figure">
              <span>
                <span class="vital-value">{Math.round(g().util)}</span>
                <span class="vital-unit">%</span>
              </span>
            </div>
            <Trace class="vital-trace" data={() => series("gpu.max")} max={() => 100} colorVar="--gpu" />
          </Vital>
        )}
      </Show>
    </section>
  );
}
