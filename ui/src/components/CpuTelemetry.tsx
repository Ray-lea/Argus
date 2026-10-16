// The CPU view's sections of driver readings (grouped readings, per-core and
// idle tables, package limits, the motherboard's sensor chip) and the one-line
// summaries their expanders show while closed.
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

/** `short` names the state in a summary, after its source. */
const IDLE_COLUMNS = [
  { key: "smuC0Percent", label: "performance.cpu.columnSmuC0", short: "C0", scale: 1, digits: 1, unit: "%" },
  { key: "c1Percent", label: "performance.cpu.columnC1", short: "C1", scale: 1, digits: 1, unit: "%" },
  { key: "c6Percent", label: "performance.cpu.columnC6", short: "CC6", scale: 1, digits: 1, unit: "%" },
  { key: "osC1Percent", label: "performance.cpu.columnOsC1", short: "C1", scale: 1, digits: 1, unit: "%" },
  { key: "osC2Percent", label: "performance.cpu.columnOsC2", short: "C2", scale: 1, digits: 1, unit: "%" },
  { key: "osC3Percent", label: "performance.cpu.columnOsC3", short: "C3", scale: 1, digits: 1, unit: "%" },
] as const;
const OS_IDLE = ["c1", "c2", "c3"] as const;

const columnsOf = <C extends { key: keyof CpuCoreSensors }>(list: readonly C[], sensors: CpuSensors) =>
  list.filter((column) => sensors.cores.some((core) => core[column.key] != null));
export const hasCoreColumns = (sensors: CpuSensors) => columnsOf(COLUMNS, sensors).length > 0;

const cell = (
  core: CpuCoreSensors | undefined,
  column: (typeof COLUMNS)[number] | (typeof IDLE_COLUMNS)[number],
) => {
  const value = core?.[column.key];
  return number(value == null ? null : value * column.scale, column.digits);
};

/** Mean of the Windows idle class across logical processors, or null if any lacks it. */
const osIdle = (idle: CpuIdle[], key: (typeof OS_IDLE)[number]) =>
  !idle.length || idle.some((value) => value[key] == null)
    ? null
    : idle.reduce((sum, value) => sum + (value[key] ?? 0), 0) / idle.length;

// ---- Summaries ----------------------------------------------------------------

const join = (parts: (string | null | false | undefined)[]) => parts.filter(Boolean).join(" · ");

/** "54–61 °C": the lowest and highest of the readings, or one value when they agree. */
function extent(values: (number | null)[], scale: number, digits: number, unit: string) {
  const list = values.filter((v): v is number => v != null && Number.isFinite(v)).map((v) => v * scale);
  if (!list.length) return null;
  const [low, high] = [number(Math.min(...list), digits), number(Math.max(...list), digits)];
  return `${low === high ? low : `${low}–${high}`} ${unit}`;
}

export function sensorSummary(sensors: CpuSensors, packageWatts: number | null) {
  const temperature = sensors.controlCelsius ?? sensors.packageCelsius;
  return join([
    temperature != null && `${t("common.resource.temperature")} ${number(temperature, 0)} °C`,
    sensors.coreVoltage != null
      ? `${t("performance.cpu.coreVoltage")} ${number(sensors.coreVoltage, 3)} V`
      : sensors.coreVid != null && `${t("performance.cpu.coreVid")} ${number(sensors.coreVid, 3)} V`,
    packageWatts != null && `${t("performance.cpu.packagePower")} ${number(packageWatts, 1)} W`,
  ]);
}

export function coreSummary(sensors: CpuSensors) {
  return join([
    t("performance.cpu.coreCount", { count: sensors.cores.length }),
    extent(
      sensors.cores.map((core) => core.clockMhz),
      0.001,
      2,
      "GHz",
    ),
    extent(
      sensors.cores.map((core) => core.celsius),
      1,
      0,
      "°C",
    ),
  ]);
}

export function boardSummary(sensors: CpuSensors) {
  const temperature = sensors.boardMetrics.find((metric) => metric.kind.endsWith("Celsius"));
  return join([
    sensors.boardFans.length > 0 && t("performance.cpu.fanCount", { count: sensors.boardFans.length }),
    temperature &&
      `${t(`performance.cpu.boardMetrics.${temperature.kind}`)} ${number(temperature.value, 1)} °C`,
  ]);
}

/** The limits in effect now; history stays in the table. */
export function limitSummary(flags: CpuLimitStatus[]) {
  const active = flags
    .filter((flag) => flag.active)
    .map((flag) => t(`performance.cpu.limits.${flag.reason}`));
  return active.length
    ? t("performance.cpu.limitsActive", { reasons: active.join(t("common.listSeparator")) })
    : t("performance.cpu.limitsNone");
}

/** SMU residency averaged over the cores when the driver maps it, else the Windows classes. */
export function idleSummary(sensors: CpuSensors | null, idle: CpuIdle[]) {
  const smu = sensors ? columnsOf(IDLE_COLUMNS.slice(0, 3), sensors) : [];
  if (sensors && smu.length) {
    const values = smu.map((column) => {
      const list = sensors.cores.map((core) => core[column.key]).filter((v): v is number => v != null);
      return `${column.short} ${number(list.reduce((sum, v) => sum + v, 0) / list.length, 1)}%`;
    });
    return t("performance.cpu.idleSummary", { source: "SMU", values: join(values) });
  }
  const values = OS_IDLE.map((key) => {
    const value = osIdle(idle, key);
    return value != null && `${key.toUpperCase()} ${number(value, 1)}%`;
  });
  return values.some(Boolean)
    ? t("performance.cpu.idleSummary", { source: "Windows", values: join(values) })
    : null;
}

// ---- Sections -----------------------------------------------------------------

/** A reading the chip reports outside the metric list (Tctl, CCD, core voltage and VID). */
function Reading(props: { label: string; value: number | null | undefined; digits: number; unit: string }) {
  return (
    <div>
      <dt>{props.label}</dt>
      <dd>
        {number(props.value, props.digits)} <small>{props.unit}</small>
      </dd>
    </div>
  );
}

/** SMU state, then the readings by kind; the chip's own figures lead their group. */
export function SensorGroups(props: { sensors: CpuSensors }) {
  const metrics = createMemo(() => new Map(props.sensors.metrics.map((m) => [m.kind, m])));
  const keys = (group: string) =>
    (Object.keys(CPU_METRICS) as CpuMetricKind[])
      .filter((kind) => CPU_METRICS[kind].group === group && metrics().has(kind))
      // These domains already appear in the headline figures and power chart.
      .filter((kind) => kind !== "coresWatts" && kind !== "graphicsWatts");
  const coreVoltage = () => props.sensors.coreVoltage != null && !metrics().has("coreVoltage");
  const chipReadings = (group: string) =>
    group === "voltage"
      ? coreVoltage() || props.sensors.coreVid != null
      : group === "temperature" &&
        (props.sensors.controlCelsius != null || props.sensors.ccdTemperatures.length > 0);
  return (
    <>
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
            <Show when={keys(group).length > 0 || chipReadings(group)}>
              <section class="cpu-sensor-group">
                <h3>{t(label)}</h3>
                <dl class="cpu-sensor-values">
                  <Show when={group === "voltage"}>
                    <Show when={coreVoltage()}>
                      <Reading
                        label={t("performance.cpu.coreVoltage")}
                        value={props.sensors.coreVoltage}
                        digits={3}
                        unit="V"
                      />
                    </Show>
                    <Show when={props.sensors.coreVid != null}>
                      <Reading
                        label={t("performance.cpu.coreVid")}
                        value={props.sensors.coreVid}
                        digits={3}
                        unit="V"
                      />
                    </Show>
                  </Show>
                  <Show when={group === "temperature"}>
                    <Show when={props.sensors.controlCelsius != null}>
                      <Reading
                        label={t("performance.cpu.controlTemperature")}
                        value={props.sensors.controlCelsius}
                        digits={0}
                        unit="°C"
                      />
                    </Show>
                    <For each={props.sensors.ccdTemperatures.map((ccd) => ccd.index)}>
                      {(index) => (
                        <Reading
                          label={t("performance.cpu.ccdTemperature", { index: index + 1 })}
                          value={props.sensors.ccdTemperatures.find((ccd) => ccd.index === index)?.celsius}
                          digits={0}
                          unit="°C"
                        />
                      )}
                    </For>
                  </Show>
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
    </>
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

export function CoreTable(props: { sensors: CpuSensors }) {
  const cores = createMemo(() => new Map(props.sensors.cores.map((core) => [core.core, core])));
  const coreIds = createMemo(() => props.sensors.cores.map((core) => core.core));
  const columns = createMemo(() => columnsOf(COLUMNS, props.sensors));
  const throttling = () => props.sensors.cores.some((core) => core.thermalThrottling != null);
  const limitFlags = () => props.sensors.cores.some((core) => core.limits.length > 0);
  const hybrid = () => props.sensors.cores.some((core) => core.efficient);
  return (
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
                  <td>{t(cores().get(id)?.efficient ? "performance.cpu.eCore" : "performance.cpu.pCore")}</td>
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
  );
}

export function PackageLimits(props: { sensors: CpuSensors }) {
  return (
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
                      flag()?.active ? "performance.cpu.throttleActive" : "performance.cpu.throttleInactive",
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
  );
}

export function BoardSensors(props: { sensors: CpuSensors }) {
  return (
    <dl class="cpu-sensor-values">
      <For each={props.sensors.boardMetrics.map((metric) => metric.kind)}>
        {(kind) => {
          const temperature = kind.endsWith("Celsius");
          return (
            <Reading
              label={t(`performance.cpu.boardMetrics.${kind}`)}
              value={props.sensors.boardMetrics.find((metric) => metric.kind === kind)?.value}
              digits={temperature ? 1 : 3}
              unit={temperature ? "°C" : "V"}
            />
          );
        }}
      </For>
      <For each={props.sensors.boardFans.map((fan) => fan.channel)}>
        {(channel) => (
          <Reading
            label={t("performance.cpu.fan", { index: channel + 1 })}
            value={props.sensors.boardFans.find((fan) => fan.channel === channel)?.rpm}
            digits={0}
            unit="RPM"
          />
        )}
      </For>
    </dl>
  );
}

/** Windows idle classes averaged over the logical processors, then SMU residency per core. */
export function IdleStates(props: { idle: CpuIdle[]; sensors: CpuSensors | null }) {
  const idleColumns = createMemo(() => (props.sensors ? columnsOf(IDLE_COLUMNS, props.sensors) : []));
  const coreIds = createMemo(() => props.sensors?.cores.map((core) => core.core) ?? []);
  const cores = createMemo(() => new Map(props.sensors?.cores.map((core) => [core.core, core])));
  return (
    <>
      <Show when={OS_IDLE.some((key) => osIdle(props.idle, key) != null)}>
        <section class="cpu-sensor-group" title={t("performance.cpu.idleSource")}>
          <h3>{t("performance.cpu.idleStates")}</h3>
          <dl class="cpu-sensor-values">
            <For each={OS_IDLE}>
              {(key) => (
                <Show when={osIdle(props.idle, key) != null}>
                  <Reading label={key.toUpperCase()} value={osIdle(props.idle, key)} digits={1} unit="%" />
                </Show>
              )}
            </For>
          </dl>
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
    </>
  );
}
