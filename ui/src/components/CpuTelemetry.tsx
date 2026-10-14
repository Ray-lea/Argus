import { createMemo, For, Show } from "solid-js";
import { CPU_METRICS } from "../lib/cpu-sensors";
import { localized, t } from "../lib/i18n";
import type { CpuCoreSensors, CpuIdle, CpuLimitStatus, CpuMetricKind, CpuSensors } from "../lib/types";

const GROUPS = [
  ["power", "performance.cpu.telemetryPower"],
  ["voltage", "performance.cpu.telemetryVoltage"],
  ["clock", "performance.cpu.telemetryClock"],
  ["temperature", "performance.cpu.telemetryTemperature"],
] as const;
const SOURCES = { msr: "MSR", smu: "SMU", svi2: "SVI2", svi3: "SVI3" } as const;
const numbers = localized((locale) =>
  [0, 1, 2, 3].map(
    (digits) =>
      new Intl.NumberFormat(locale, { minimumFractionDigits: digits, maximumFractionDigits: digits }),
  ),
);
const number = (value: number | null | undefined, digits: number) =>
  value == null || !Number.isFinite(value) ? "—" : numbers()[digits].format(value);

const COLUMNS = [
  { key: "clockMhz", label: "performance.cpu.columnClock", scale: 0.001, digits: 2, unit: "GHz" },
  { key: "effectiveMhz", label: "performance.cpu.columnEffective", scale: 0.001, digits: 2, unit: "GHz" },
  { key: "celsius", label: "performance.cpu.columnTemperature", scale: 1, digits: 0, unit: "°C" },
  { key: "distanceToLimit", label: "performance.cpu.columnHeadroom", scale: 1, digits: 0, unit: "°C" },
  { key: "watts", label: "performance.cpu.columnPower", scale: 1, digits: 2, unit: "W" },
  { key: "vid", label: "performance.cpu.columnVid", scale: 1, digits: 3, unit: "V" },
  { key: "voltage", label: "performance.cpu.columnVoltage", scale: 1, digits: 3, unit: "V" },
  { key: "multiplier", label: "performance.cpu.columnMultiplier", scale: 1, digits: 2, unit: "×" },
  { key: "c0Percent", label: "performance.cpu.columnActivity", scale: 1, digits: 1, unit: "%" },
] as const;

const IDLE_COLUMNS = [
  { key: "smuC0Percent", label: "performance.cpu.columnSmuC0", scale: 1, digits: 1, unit: "%" },
  { key: "c1Percent", label: "performance.cpu.columnC1", scale: 1, digits: 1, unit: "%" },
  { key: "c6Percent", label: "performance.cpu.columnC6", scale: 1, digits: 1, unit: "%" },
  { key: "osC1Percent", label: "performance.cpu.columnOsC1", scale: 1, digits: 1, unit: "%" },
  { key: "osC2Percent", label: "performance.cpu.columnOsC2", scale: 1, digits: 1, unit: "%" },
  { key: "osC3Percent", label: "performance.cpu.columnOsC3", scale: 1, digits: 1, unit: "%" },
] as const;

export function CpuIdleSummary(props: { idle: CpuIdle[] }) {
  const average = (key: keyof CpuIdle) => {
    if (!props.idle.length || props.idle.some((value) => value[key] == null)) return null;
    return props.idle.reduce((sum, value) => sum + (value[key] ?? 0), 0) / props.idle.length;
  };
  return (
    <Show when={(["c1", "c2", "c3"] as const).some((key) => average(key) != null)}>
      <section class="cpu-sensor-group section-gap" title={t("performance.cpu.idleSource")}>
        <h3>{t("performance.cpu.idleStates")}</h3>
        <dl class="cpu-sensor-values">
          <For each={["c1", "c2", "c3"] as const}>
            {(key) => (
              <Show when={average(key) != null}>
                <div>
                  <dt>{key.toUpperCase()}</dt>
                  <dd>
                    {number(average(key), 1)} <small>%</small>
                  </dd>
                </div>
              </Show>
            )}
          </For>
        </dl>
      </section>
    </Show>
  );
}

function limitText(flags: CpuLimitStatus[], history = false) {
  if (!flags.length) return "—";
  return (
    flags
      .filter((flag) => (history ? flag.occurred : flag.active))
      .map((flag) => t(`performance.cpu.limits.${flag.reason}`))
      .join(t("common.listSeparator")) ||
    t(history ? "performance.cpu.limitNever" : "performance.cpu.throttleInactive")
  );
}

export default function CpuTelemetry(props: { sensors: CpuSensors }) {
  const metrics = createMemo(() => new Map(props.sensors.metrics.map((m) => [m.kind, m])));
  const keys = (group: string) =>
    (Object.keys(CPU_METRICS) as CpuMetricKind[])
      .filter((kind) => CPU_METRICS[kind].group === group && metrics().has(kind))
      // These domains already appear in the headline figures and power chart.
      .filter((kind) => kind !== "coresWatts" && kind !== "graphicsWatts");
  const cores = createMemo(() => new Map(props.sensors.cores.map((core) => [core.core, core])));
  const coreIds = createMemo(() => props.sensors.cores.map((core) => core.core));
  const columns = createMemo(() =>
    COLUMNS.filter((column) => props.sensors.cores.some((core) => core[column.key] != null)),
  );
  const throttling = () => props.sensors.cores.some((core) => core.thermalThrottling != null);
  const limitFlags = () => props.sensors.cores.some((core) => core.limits.length > 0);
  const hybrid = () => props.sensors.cores.some((core) => core.efficient);
  const idleColumns = createMemo(() =>
    IDLE_COLUMNS.filter((column) => props.sensors.cores.some((core) => core[column.key] != null)),
  );
  const cell = (
    core: CpuCoreSensors | undefined,
    column: (typeof COLUMNS)[number] | (typeof IDLE_COLUMNS)[number],
  ) => {
    const value = core?.[column.key];
    return number(value == null ? null : value * column.scale, column.digits);
  };
  return (
    <div class="cpu-telemetry section-gap">
      <Show when={props.sensors.smuState && props.sensors.smuState !== "ready"}>
        <div class="cpu-sensor-status" role="status">
          {props.sensors.smuState === "unsupportedTable"
            ? t("performance.cpu.smuUnsupported", {
                version: `0x${(props.sensors.smuTableVersion ?? 0).toString(16).toUpperCase().padStart(8, "0")}`,
              })
            : props.sensors.smuState === "pending"
              ? t("performance.cpu.smuPending")
              : t("performance.cpu.smuUnavailable")}
        </div>
      </Show>
      <div class="cpu-sensor-groups">
        <For each={GROUPS}>
          {([group, label]) => (
            <Show when={keys(group).length > 0}>
              <section class="cpu-sensor-group">
                <h3>{t(label)}</h3>
                <dl class="cpu-sensor-values">
                  <For each={keys(group)}>
                    {(kind) => {
                      const metric = () => metrics().get(kind);
                      const spec = CPU_METRICS[kind];
                      return (
                        <div
                          title={t("performance.cpu.source", { source: SOURCES[metric()?.source ?? "msr"] })}
                        >
                          <dt>{t(`performance.cpu.metrics.${kind}`)}</dt>
                          <dd>
                            {number(metric()?.value, spec.digits)} <small>{spec.unit}</small>
                          </dd>
                        </div>
                      );
                    }}
                  </For>
                </dl>
              </section>
            </Show>
          )}
        </For>
      </div>
      <Show when={props.sensors.limits.length > 0}>
        <section class="cpu-sensor-group">
          <h3>{t("performance.cpu.packageLimits")}</h3>
          <div class="cpu-core-scroll">
            <table class="cpu-core-table">
              <thead>
                <tr>
                  <th>{t("performance.cpu.limitReasons")}</th>
                  <th>{t("performance.cpu.limitCurrent")}</th>
                  <th title={t("performance.cpu.limitHistoryHint")}>{t("performance.cpu.limitHistory")}</th>
                </tr>
              </thead>
              <tbody>
                <For each={props.sensors.limits.map((flag) => flag.reason)}>
                  {(reason) => {
                    const flag = () => props.sensors.limits.find((item) => item.reason === reason);
                    return (
                      <tr>
                        <th scope="row">{t(`performance.cpu.limits.${reason}`)}</th>
                        <td classList={{ "cpu-throttled": flag()?.active }}>
                          {t(
                            flag()?.active
                              ? "performance.cpu.throttleActive"
                              : "performance.cpu.throttleInactive",
                          )}
                        </td>
                        <td title={t("performance.cpu.limitHistoryHint")}>
                          {t(flag()?.occurred ? "performance.cpu.limitLogged" : "performance.cpu.limitNever")}
                        </td>
                      </tr>
                    );
                  }}
                </For>
              </tbody>
            </table>
          </div>
        </section>
      </Show>
      <Show when={props.sensors.boardFans.length > 0}>
        <section class="cpu-sensor-group">
          <h3>
            {t("performance.cpu.boardSensor")} <small>{props.sensors.boardSensor}</small>
          </h3>
          <dl class="cpu-sensor-values">
            <For each={props.sensors.boardFans.map((fan) => fan.channel)}>
              {(channel) => (
                <div>
                  <dt>{t("performance.cpu.fan", { index: channel + 1 })}</dt>
                  <dd>
                    {number(props.sensors.boardFans.find((fan) => fan.channel === channel)?.rpm, 0)}{" "}
                    <small>RPM</small>
                  </dd>
                </div>
              )}
            </For>
          </dl>
        </section>
      </Show>
      <Show when={coreIds().length > 0 && columns().length > 0}>
        <section class="cpu-sensor-group">
          <h3>{t("performance.cpu.perCore")}</h3>
          <div class="cpu-core-scroll">
            <table class="cpu-core-table">
              <thead>
                <tr>
                  <th scope="col">{t("performance.cpu.columnCore")}</th>
                  <Show when={hybrid()}>
                    <th scope="col">{t("performance.cpu.columnCoreType")}</th>
                  </Show>
                  <For each={columns()}>
                    {(column) => (
                      <th scope="col">
                        {t(column.label)} <small>{column.unit}</small>
                      </th>
                    )}
                  </For>
                  <Show when={limitFlags()}>
                    <th scope="col">{t("performance.cpu.limitReasons")}</th>
                  </Show>
                  <Show when={!limitFlags() && throttling()}>
                    <th scope="col">{t("performance.cpu.thermalThrottle")}</th>
                  </Show>
                </tr>
              </thead>
              <tbody>
                <For each={coreIds()}>
                  {(id) => (
                    <tr>
                      <th scope="row">{t("performance.cpu.coreName", { index: id + 1 })}</th>
                      <Show when={hybrid()}>
                        <td>
                          {t(cores().get(id)?.efficient ? "performance.cpu.eCore" : "performance.cpu.pCore")}
                        </td>
                      </Show>
                      <For each={columns()}>{(column) => <td>{cell(cores().get(id), column)}</td>}</For>
                      <Show when={limitFlags()}>
                        <td
                          class="cpu-limit-cell"
                          classList={{
                            "cpu-throttled": cores()
                              .get(id)
                              ?.limits.some((flag) => flag.active),
                          }}
                          title={`${limitText(cores().get(id)?.limits ?? [])}\n${t("performance.cpu.limitHistory")}: ${limitText(cores().get(id)?.limits ?? [], true)}`}
                        >
                          {limitText(cores().get(id)?.limits ?? [])}
                        </td>
                      </Show>
                      <Show when={!limitFlags() && throttling()}>
                        <td classList={{ "cpu-throttled": cores().get(id)?.thermalThrottling === true }}>
                          {cores().get(id)?.thermalThrottling == null
                            ? "—"
                            : cores().get(id)?.thermalThrottling
                              ? t("performance.cpu.throttleActive")
                              : t("performance.cpu.throttleInactive")}
                        </td>
                      </Show>
                    </tr>
                  )}
                </For>
              </tbody>
            </table>
          </div>
        </section>
      </Show>
      <Show when={idleColumns().length > 0}>
        <section class="cpu-sensor-group">
          <h3>{t("performance.cpu.idlePerCore")}</h3>
          <div class="cpu-core-scroll">
            <table class="cpu-core-table">
              <thead>
                <tr>
                  <th scope="col">{t("performance.cpu.columnCore")}</th>
                  <For each={idleColumns()}>
                    {(column) => (
                      <th scope="col">
                        {t(column.label)} <small>%</small>
                      </th>
                    )}
                  </For>
                </tr>
              </thead>
              <tbody>
                <For each={coreIds()}>
                  {(id) => (
                    <tr>
                      <th scope="row">{t("performance.cpu.coreName", { index: id + 1 })}</th>
                      <For each={idleColumns()}>{(column) => <td>{cell(cores().get(id), column)}</td>}</For>
                    </tr>
                  )}
                </For>
              </tbody>
            </table>
          </div>
        </section>
      </Show>
    </div>
  );
}
