/** @jsxImportSource @/components/replica/i18n */
"use client";
import { cpuPower, cpuTemperature } from "./Processes";
import { logical, noise, sampleAt, samples } from "./model";
import { Card, Chart, HeadCell, Reading, SensorTable, rangeText, roundCeiling, seriesOf, spanLabel, wattsText, type CurveProps } from "./perfParts";

// The CPU's details page as the app lays it out with the kernel sensors on
// (pages/performance/telemetry.rs): processor information, the temperature
// and power charts, the internal sensors' readings, per-core readings, the
// package's limit status and the idle states. The machine is a fictional
// i5-13500HX: six hyper-threaded performance cores and eight efficiency cores.

const FLAGS = ["SSE", "SSE2", "SSE3", "SSSE3", "SSE4.1", "SSE4.2", "AVX", "AVX2", "FMA", "AES", "SHA", "BMI1", "BMI2", "AVX-VNNI"];
/** Bus clock (MHz); the cores and the ring run at whole multiples of it. */
const BUS = 99.56;
/** The package's long-term power limit (W) and its temperature target (TjMax, °C). */
const PL1 = 100;
const TJMAX = 100;
const fixed = (v: number, digits: number) => v.toFixed(digits);
const mean = (list: number[]) => list.reduce((a, b) => a + b, 0) / Math.max(1, list.length);
// The limit reasons the package's thermal status register reports.
const PACKAGE_LIMITS = ["温度限制", "PROCHOT 信号", "临界温度", "功耗限制"];
const HISTORY_HINT = "记录可能早于本次启动 Argus。";
/** The package runs into its long-term power limit at the load's peaks. */
export const powerLimited = (t: number) => cpuPower(t) >= PL1 - 4;

interface Core {
  /** Physical core number (from 0). */
  core: number;
  efficient: boolean;
  clockMhz: number;
  effectiveMhz: number;
  celsius: number;
  vid: number;
  multiplier: number;
  c0: number;
  idle: number[];
}
/** The Windows idle classes C1–C3, averaged over the logical processors. */
export const idleAt = (t: number) => [10 + noise(81, t) * 2, 5 + noise(82, t), 30 + noise(83, t) * 4];
// The cores as the thermal and performance status registers read them: the
// performance cores at turbo ratios 41–42, active a part of the time (their
// effective clock well below), the efficiency cores at 34, busy throughout
// under load; the package's temperature is the hottest core's.
function coresAt(t: number): Core[] {
  const package_ = cpuTemperature(t);
  const load = sampleAt(t).cpu;
  const idle = idleAt(t);
  return Array.from({ length: 14 }, (_, core) => {
    const efficient = core >= 6;
    const multiplier = efficient ? 34 : noise(200 + core, t) < 0.55 ? 42 : 41;
    const clockMhz = multiplier * BUS;
    const c0 = Math.min(100, efficient ? 30 + load * 1.1 + noise(210 + core, t) * 6 : 8 + load * 0.45 + noise(210 + core, t) * 10);
    // Hotter where busier: the efficiency cluster a few degrees under the hottest performance core.
    const celsius = package_ - (efficient ? 4 + noise(220 + core, t) * 4 : core === 2 ? 0 : 1 + noise(220 + core, t) * 6);
    const vid = efficient ? 1.18 + noise(230 + core, t) * 0.02 : 1.2 + (multiplier - 41) * 0.025 + noise(230 + core, t) * 0.018;
    return {
      core,
      efficient,
      clockMhz,
      effectiveMhz: (clockMhz * c0 * (efficient ? 0.96 : 0.98)) / 100,
      celsius,
      vid,
      multiplier,
      c0,
      idle: idle.map((v, k) => Math.max(0, v + (noise(240 + core * 3 + k, t) - 0.5) * (k === 2 ? 6 : 3))),
    };
  });
}
/** Each logical processor's core temperature, as the processors' cards tell it on hover. */
export function logicalTemperatures(t: number) {
  const cores = coresAt(t);
  return logical.map((c) => cores[c.index < 12 ? c.index >> 1 : c.index - 6].celsius);
}
/** "54 – 61 °C" over a list of readings. */
const extent = (list: number[], digits: number, unit: string) =>
  rangeText(fixed(Math.min(...list), digits), fixed(Math.max(...list), digits), unit);

export default function CpuDetails({ tick, interval, curve }: { tick: number; interval: number; curve: CurveProps }) {
  const cores = coresAt(tick);
  // The charts' card: the range over the last minute of samples.
  const temperatures = seriesOf(60, cpuTemperature, tick).map(Math.round);
  const powers = seriesOf(60, cpuPower, tick);
  const powerTop = roundCeiling(Math.max(5, ...seriesOf(samples, cpuPower, tick)));
  const packageWatts = cpuPower(tick);
  // The package's VID: the highest core's.
  const coreVid = Math.max(...cores.map((c) => c.vid));
  const ringRatio = 36;
  // The package's long-term power limit holds the load's peaks back: the
  // cores report it among their limit reasons while it does.
  const limited = powerLimited(tick);
  // The sensor groups in the order of the app's metric list
  // (resources/cpu-metrics.json); the cores' and graphics' power show in the
  // charts above and on the GPU's page.
  const groups: [string, [string, string, string][]][] = [
    [
      "功耗与限制",
      [
        ["内存功耗", fixed(1.4 + sampleAt(tick).mem * 0.012 + noise(92, tick) * 0.3, 1), "W"],
        ["平台功耗", fixed(packageWatts + 9 + noise(91, tick) * 2, 1), "W"],
        ["长时功耗上限（PL1）", fixed(PL1, 1), "W"],
        ["短时功耗上限（PL2）", "157.0", "W"],
        ["PL1 时间窗口", "56.000", "s"],
        ["PL2 时间窗口", "0.002", "s"],
        ["标称热设计功耗", "55.0", "W"],
      ],
    ],
    ["电压与电流", [["核心 VID", fixed(coreVid, 3), "V"]]],
    [
      "频率",
      [
        ["Ring 频率", fixed(ringRatio * BUS, 0), "MHz"],
        ["Ring 最低频率限制", fixed(8 * BUS, 0), "MHz"],
        ["Ring 最高频率限制", fixed(43 * BUS, 0), "MHz"],
        ["Ring 倍频", fixed(ringRatio, 2), "×"],
        ["平均核心频率", fixed(mean(cores.map((c) => c.clockMhz)), 0), "MHz"],
        ["平均有效频率", fixed(mean(cores.map((c) => c.effectiveMhz)), 0), "MHz"],
        ["基准时钟", fixed(BUS, 2), "MHz"],
      ],
    ],
    ["温度", [["温度上限", String(TJMAX), "°C"]]],
  ];
  const idle = idleAt(tick);
  const columns: [string, string, (c: Core) => string][] = [
    ["频率", "GHz", (c) => fixed(c.clockMhz / 1000, 2)],
    ["有效频率", "GHz", (c) => fixed(c.effectiveMhz / 1000, 2)],
    ["温度", "°C", (c) => fixed(c.celsius, 0)],
    ["温度余量", "°C", (c) => fixed(TJMAX - Math.round(c.celsius), 0)],
    ["VID", "V", (c) => fixed(c.vid, 3)],
    ["倍频", "×", (c) => fixed(c.multiplier, 2)],
    ["平均活跃（C0）", "%", (c) => fixed(c.c0, 1)],
  ];
  return (
    <div className="a-cards">
      <Card title="处理器信息">
        <div className="a-cpu-info">
          <dl className="a-specs">
            <dt>厂商</dt>
            <dd>GenuineIntel</dd>
            <dt>家族／型号／步进</dt>
            <dd>0x6 / 0xBF / 2</dd>
            <dt>微码版本</dt>
            <dd>0x4122</dd>
            <dt>温度来源</dt>
            <dd>处理器内部传感器（PawnIO）</dd>
            <dt>内核传感器</dt>
            <dd data-i18n="sensor">已连接</dd>
          </dl>
          <span className="a-cpu-info-label">指令集</span>
          <div className="a-flags">
            {FLAGS.map((flag) => (
              <span key={flag}>{flag}</span>
            ))}
          </div>
        </div>
      </Card>
      <Card
        title="温度与功耗曲线"
        summary={`最近 ${spanLabel(interval)}：${extent(temperatures, 0, "°C")} · ${rangeText(wattsText(Math.min(...powers)), wattsText(Math.max(...powers)), "W")}`}
      >
        <div className="a-two a-cpu-charts">
          <Chart
            title="温度"
            top="100 °C"
            max={100}
            curve={curve}
            readout={{ format: (v) => Math.round(v) + " °C" }}
            series={[{ values: seriesOf(samples, cpuTemperature, tick), color: "var(--a-cpu)" }]}
          />
          <Chart
            title="功耗"
            top={`${powerTop} W`}
            max={powerTop}
            curve={curve}
            readout={{ names: ["封装", "核心"], format: (v) => wattsText(v) + " W" }}
            series={[
              { values: seriesOf(samples, cpuPower, tick), color: "var(--a-cpu)" },
              { values: seriesOf(samples, (t) => cpuPower(t) * 0.72, tick), color: "var(--a-cpu)", dashed: true, fill: false },
            ]}
          />
        </div>
      </Card>
      <Card
        title="内部传感器"
        summary={["温度", ` ${Math.round(cpuTemperature(tick))} °C · `, "核心 VID", ` ${fixed(coreVid, 3)} V · `, "封装功耗", ` ${fixed(packageWatts, 1)} W`]}
      >
        <div className="a-sensor-groups">
          {groups.map(([title, readings]) => (
            <div key={title}>
              <b className="a-group-title" data-i18n="group">
                {title}
              </b>
              <div className="a-readings">
                {readings.map(([label, value, unit]) => (
                  <Reading key={label} label={label} value={value} unit={unit} title={label === "核心 VID" ? undefined : "来源：MSR"} />
                ))}
              </div>
            </div>
          ))}
        </div>
      </Card>
      <Card
        title="每核读数"
        summary={[`${cores.length} 个核心`, ` · ${extent(cores.map((c) => c.clockMhz / 1000), 2, "GHz")} · ${extent(cores.map((c) => Math.round(c.celsius)), 0, "°C")}`]}
      >
        <SensorTable
          head={[
            <HeadCell key="core" name="核心" context="telemetry" />,
            <HeadCell key="type" name="类型" />,
            ...columns.map(([name, unit]) => <HeadCell key={name} name={name} unit={unit} />),
            <HeadCell key="limits" name="限制原因" />,
          ]}
          rows={cores.map((c) => [
            `核心 ${c.core + 1}`,
            <span key="type" data-i18n="telemetry">
              {c.efficient ? "E 核" : "P 核"}
            </span>,
            ...columns.map(([, , value]) => value(c)),
            // Every core names the package's power limit while it holds them
            // back, and has logged it since the counters were cleared.
            <span key="limits" data-i18n="limit" className={limited ? "is-alert" : undefined} title={`${limited ? "功耗限制" : "未触发"}\n记录: 功耗限制`}>
              {limited ? "功耗限制" : "未触发"}
            </span>,
          ])}
        />
      </Card>
      <Card title="封装限制状态" summary={limited ? "生效中：功耗限制" : "当前无限制"} alert={limited}>
        <SensorTable
          head={[<HeadCell key="reason" name="限制原因" />, <HeadCell key="now" name="当前" />, <HeadCell key="history" name="记录" title={HISTORY_HINT} />]}
          rows={PACKAGE_LIMITS.map((reason) => {
            const active = limited && reason === "功耗限制";
            return [
              reason,
              <span key="now" className={active ? "is-alert" : undefined}>
                {active ? "正在节流" : "未触发"}
              </span>,
              <span key="history" data-i18n="limit" title={HISTORY_HINT}>
                {reason === "功耗限制" ? "曾触发" : "无记录"}
              </span>,
            ];
          })}
        />
      </Card>
      <Card title="空闲状态" summary={`Windows：C1 ${fixed(idle[0], 1)}% · C2 ${fixed(idle[1], 1)}% · C3 ${fixed(idle[2], 1)}%`}>
        <div className="a-idle">
          <div title="由 Windows 统计的空闲状态类别，按逻辑处理器平均。">
            <b className="a-group-title">空闲状态（Windows）</b>
            <div className="a-readings is-idle">
              {idle.map((v, i) => (
                <Reading key={i} label={`C${i + 1}`} value={fixed(v, 1)} unit="%" />
              ))}
            </div>
          </div>
          <div>
            <b className="a-group-title">每核空闲状态</b>
            <SensorTable
              head={[
                <HeadCell key="core" name="核心" context="telemetry" />,
                ...[1, 2, 3].map((k) => <HeadCell key={k} name={`C${k}（Windows）`} unit="%" />),
              ]}
              rows={cores.map((c) => [`核心 ${c.core + 1}`, ...c.idle.map((v) => fixed(v, 1))])}
            />
          </div>
        </div>
      </Card>
    </div>
  );
}
