/** @jsxImportSource @/components/replica/i18n */
"use client";
import { useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import Curve, { type Motion, type Readout, type Series } from "./Curve";
import { Icon } from "./icons";
import { cpuPower, cpuTemperature, gpuPower, gpuTemperature, useSystemHistory, type Device } from "./Processes";
import {
  clock,
  coreLoad,
  logical,
  machine,
  noise,
  rate,
  rateCeiling,
  rateLabel,
  sampleAt,
  samples,
  span,
  thousands,
  totalMemory,
} from "./model";
import { Segments, Select, slideIn, useIsoLayoutEffect } from "./widgets";

interface Props {
  tick: number;
  interval: number;
  running: boolean;
  motion: Motion;
  device: Device;
  processCount: number;
  threads: number;
  handles: number;
  onDevice: (device: Device) => void;
}
interface CurveProps {
  tick: number;
  interval: number;
  running: boolean;
  motion: Motion;
}
const GB = 1024 ** 3;
// How the hover readout writes each kind of value.
const percentOf = (v: number) => Math.round(v) + "%";
const gigabytes = (v: number) => v.toFixed(1) + " GB";
const celsius = (v: number) => Math.round(v) + " °C";
const watts = (v: number) => Math.round(v) + " W";
const seriesOf = (count: number, f: (t: number) => number, tick: number) =>
  Array.from({ length: count }, (_, i) => f(tick - count + 1 + i));
export function spanLabel(interval: number) {
  const seconds = Math.round((span * interval) / 1000);
  return seconds >= 120 ? `${Math.round(seconds / 60)} 分钟` : `${seconds} 秒`;
}

function Chart({
  title,
  top,
  series,
  max,
  curve,
  fill,
  color = "var(--a-cpu)",
  foot = true,
  height,
  readout = { format: percentOf },
}: {
  title?: ReactNode;
  top?: ReactNode;
  series: Series[];
  max: number;
  curve: CurveProps;
  fill?: boolean;
  color?: string;
  foot?: boolean;
  height?: number;
  readout?: Readout;
}) {
  return (
    <figure className={"a-chart" + (fill ? " is-fill" : "")}>
      {title !== undefined && (
        <figcaption className="a-chart-caption">
          <span>{title}</span>
          <span>{top}</span>
        </figcaption>
      )}
      <div className="a-chart-frame" style={{ "--c": color, height } as CSSProperties}>
        <div className="a-grid" aria-hidden="true" />
        <Curve series={series} max={max} {...curve} label={typeof title === "string" ? title : undefined} readout={readout} />
      </div>
      {foot && (
        <div className="a-chart-caption">
          <span>{spanLabel(curve.interval)}</span>
          <span>0</span>
        </div>
      )}
    </figure>
  );
}

type Figure = [string, ReactNode, string?, string?];
function Stats({ figures, specs, cpu }: { figures: Figure[]; specs: [string, ReactNode][]; cpu?: boolean }) {
  return (
    <div className={"a-stats" + (cpu ? " is-cpu" : "")}>
      <dl className="a-figures">
        {figures.map(([label, value, unit, note]) => (
          <div key={label}>
            <dt>{label}</dt>
            <dd>
              {value}
              {unit && <small>{unit}</small>}
            </dd>
            {note && <dd>{note}</dd>}
          </div>
        ))}
      </dl>
      <dl className="a-specs">
        {specs.map(([label, value]) => [
          <dt key={label + ":t"}>{label}</dt>,
          <dd key={label + ":d"}>{value}</dd>,
        ])}
      </dl>
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
const split = (text: string): [string, string] => {
  const at = text.lastIndexOf(" ");
  return at < 0 ? [text, ""] : [text.slice(0, at), text.slice(at + 1)];
};

const devices: Device[] = ["cpu", "memory", "disk", "wifi", "gpu", "battery"];
export default function Performance(p: Props) {
  const [graph, setGraph] = useState<"overall" | "logical">("overall");
  const [filter, setFilter] = useState<"all" | "p" | "e">("all");
  const [cpuPage, setCpuPage] = useState<"overview" | "details">("overview");
  const [engineCount, setEngineCount] = useState<"smart" | "3" | "6">("smart");
  const [engineLayout, setEngineLayout] = useState<"grid" | "focus" | "rows">("grid");
  const last = useRef({ device: p.device, dir: 1 });
  if (last.current.device !== p.device) {
    last.current = {
      device: p.device,
      dir: Math.sign(devices.indexOf(p.device) - devices.indexOf(last.current.device)) || 1,
    };
  }
  // A device's view moves in from the side of its entry in the list.
  const view = useRef<HTMLDivElement>(null);
  useIsoLayoutEffect(() => slideIn(view.current, { y: last.current.dir * 16 }), [p.device]);
  const markMoves = useRef(false);
  const s = sampleAt(p.tick);
  const h = useSystemHistory(p.tick);
  const curve = { tick: p.tick, interval: p.interval, running: p.running, motion: p.motion };
  const used = (s.mem / 100) * totalMemory;
  const diskMax = rateCeiling(Math.max(...h.disk));
  const netMax = rateCeiling(Math.max(...h.net), true);
  const vram = (t: number) => 3.3 + sampleAt(t).gpu / 900 + noise(41, t) * 0.04;
  const battery = (t: number) => 84 - ((((t % 600) + 600) % 600) / 300);
  const discharge = (t: number) => 16.5 + noise(71, Math.floor(t / 3)) * 1.4;
  const rail: { id: Device; name: string; info: string; values: number[][]; color: string; max: number }[] = [
    { id: "cpu", name: "CPU", info: `${Math.round(s.cpu)}% ${clock("p", p.tick).toFixed(2)} GHz`, values: [h.cpu], color: "var(--a-cpu)", max: 100 },
    {
      id: "memory",
      name: "内存",
      info: `${(used / GB).toFixed(1)} / 32.0 GB（${Math.round(s.mem)}%）`,
      values: [h.mem],
      color: "var(--a-mem)",
      max: 100,
    },
    {
      id: "disk",
      name: "磁盘 0 (C:)",
      info: `SSD (NVMe) ${Math.round(s.active)}% 41 °C`,
      values: [h.active],
      color: "var(--a-io)",
      max: 100,
    },
    { id: "wifi", name: "WLAN", info: `收 ${rate(s.rx)}  发 ${rate(s.tx)}`, values: [h.rx, h.tx], color: "var(--a-net)", max: netMax },
    {
      id: "gpu",
      name: "GPU 0",
      info: `${Math.round(s.gpu)}% ${Math.round(gpuTemperature(p.tick))} °C ${Math.round(gpuPower(p.tick))} W`,
      values: [h.gpu],
      color: "var(--a-gpu)",
      max: 100,
    },
    {
      id: "battery",
      name: "电池",
      info: "84% 剩余 2 小时 31 分钟",
      values: [seriesOf(samples, battery, p.tick)],
      color: "var(--a-bat)",
      max: 100,
    },
  ];
  const coreCols = filter === "all" ? 5 : 4;
  const cores = useMemo(
    () =>
      logical
        .filter((c) => filter === "all" || c.kind === filter)
        .map((c) => ({ ...c, values: seriesOf(samples, (t) => coreLoad(c.index, t), p.tick) })),
    [filter, p.tick],
  );
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
    if (cpuPage === "overview")
      body = (
        <>
          {head}
          <div className="a-perf-tools">
            {graph === "logical" && (
              <Segments
                label="核心类型"
                value={filter}
                onChange={setFilter}
                items={[
                  { value: "all", label: "全部", title: "显示所有逻辑处理器" },
                  { value: "p", label: "P 核", title: "只显示性能核（P 核）" },
                  { value: "e", label: "E 核", title: "只显示能效核（E 核）" },
                ]}
              />
            )}
            <Segments
              label="CPU 图表"
              value={graph}
              onChange={setGraph}
              items={[
                { value: "overall", label: "整体", title: "所有逻辑处理器的整体利用率", tour: "graph-overall" },
                { value: "logical", label: "多核", title: "每个逻辑处理器一张曲线图", tour: "graph-logical" },
              ]}
            />
          </div>
          {graph === "overall" ? (
            <Chart
              title="利用率 %"
              top="100%"
              max={100}
              curve={curve}
              fill
              readout={{ names: ["利用率", "内核时间"], format: percentOf }}
              series={[
                { values: h.cpu, color: "var(--a-cpu)" },
                { values: h.kernel, color: "var(--a-cpu)", dashed: true, fill: false },
              ]}
            />
          ) : (
            <div className="a-cores-grid" style={{ "--cols": coreCols } as CSSProperties} key={filter}>
              {cores.map((core) => {
                const color = core.kind === "p" ? "var(--a-cpu)" : "var(--a-cpu-e)";
                return (
                  <div
                    className="a-core"
                    key={core.index}
                    title={`逻辑处理器 ${core.index}，物理核心 ${core.kind === "p" ? core.index >> 1 : core.index - 6}（${core.kind === "p" ? "性能核" : "能效核"}）`}
                  >
                    <div className="a-core-head">
                      <span className="a-kind" style={{ "--c": color } as CSSProperties}>
                        {core.kind.toUpperCase()}
                      </span>
                      <b>{core.index}</b>
                      <span>{clock(core.kind, p.tick + core.index).toFixed(2)} GHz</span>
                      <strong>
                        {Math.round(core.values[core.values.length - 1])}
                        <small>%</small>
                      </strong>
                    </div>
                    <div className="a-chart-frame" style={{ "--c": color } as CSSProperties}>
                      <div className="a-grid" aria-hidden="true" />
                      <Curve series={[{ values: core.values, color }]} max={100} {...curve} />
                    </div>
                  </div>
                );
              })}
            </div>
          )}
          {graph === "overall" ? (
            <Legend
              items={[
                ["总计", "var(--a-cpu)"],
                ["内核时间", "var(--a-cpu)", "dashed"],
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
              ["功耗", Math.round(cpuPower(p.tick)), "W"],
              ["进程", p.processCount],
              ["线程", thousands(p.threads)],
              ["句柄", thousands(p.handles)],
              ["正常运行时间", uptime],
            ]}
            specs={[
              ["基准速度", "2.50 GHz"],
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
    else {
      const temp = Math.round(cpuTemperature(p.tick));
      const power = Math.round(cpuPower(p.tick));
      const powerTop = Math.max(90, Math.ceil(Math.max(...seriesOf(samples, cpuPower, p.tick)) / 10) * 10);
      const idle = [10 + noise(81, p.tick) * 2, 5 + noise(82, p.tick), 30 + noise(83, p.tick) * 4];
      body = (
        <>
          {head}
          <div className="a-cards">
            <section className="a-card">
              <div className="a-card-head">
                <b>处理器信息</b>
              </div>
              <dl className="a-specs">
                <dt>厂商</dt>
                <dd>GenuineIntel</dd>
                <dt>家族／型号／步进</dt>
                <dd>0x6 / 0xBF / 2</dd>
                <dt>微码版本</dt>
                <dd>0x4122</dd>
                <dt>温度来源</dt>
                <dd>处理器内部传感器（PawnIO）</dd>
                <dt>指令集</dt>
                <dd />
              </dl>
              <div className="a-flags">
                {["SSE", "SSE2", "SSE3", "SSSE3", "SSE4.1", "SSE4.2", "AVX", "AVX2", "FMA", "AES", "SHA", "BMI1", "BMI2", "AVX-VNNI"].map(
                  (flag) => (
                    <span key={flag}>{flag}</span>
                  ),
                )}
              </div>
            </section>
            <section className="a-card">
              <div className="a-card-head">
                <b>温度与功耗曲线</b>
                <span>
                  最近 {spanLabel(p.interval)}：{temp} °C · {power} W
                </span>
              </div>
              <div className="a-two">
                <Chart
                  title="温度"
                  top="100 °C"
                  max={100}
                  curve={curve}
                  readout={{ format: celsius }}
                  height={146}
                  series={[{ values: seriesOf(samples, cpuTemperature, p.tick), color: "var(--a-cpu)" }]}
                />
                <Chart
                  title="功耗"
                  top={`${powerTop} W`}
                  max={powerTop}
                  curve={curve}
                  readout={{ names: ["封装", "核心"], format: watts }}
                  height={146}
                  series={[
                    { values: seriesOf(samples, cpuPower, p.tick), color: "var(--a-cpu)" },
                    { values: seriesOf(samples, (t) => cpuPower(t) * 0.72, p.tick), color: "var(--a-cpu)", dashed: true, fill: false },
                  ]}
                />
              </div>
              <Legend
                items={[
                  ["封装", "var(--a-cpu)"],
                  ["核心", "var(--a-cpu)", "dashed"],
                ]}
              />
            </section>
            <section className="a-card">
              <div className="a-card-head">
                <b>空闲状态</b>
                <span>
                  Windows：C1 {idle[0].toFixed(1)}% · C2 {idle[1].toFixed(1)}% · C3 {idle[2].toFixed(1)}%
                </span>
              </div>
              <p className="a-card-sub">空闲状态（Windows）</p>
              <dl className="a-figures">
                {idle.map((v, i) => (
                  <div key={i}>
                    <dt>C{i + 1}</dt>
                    <dd>
                      {v.toFixed(1)}
                      <small>%</small>
                    </dd>
                  </div>
                ))}
              </dl>
            </section>
          </div>
        </>
      );
    }
  } else if (p.device === "memory") {
    const inUse = used / GB;
    const modified = 0.25;
    const free = 1.26;
    const standby = 32 - inUse - modified - free;
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
        <div className="a-membar" role="img" aria-label="内存组成">
          <i style={{ width: `${(inUse / 32) * 100}%`, background: "color-mix(in srgb, var(--a-mem) 62%, transparent)" }} title="由进程、驱动程序或操作系统使用的内存" />
          <i style={{ width: `${(modified / 32) * 100}%`, background: "var(--a-mem)" }} title="内容必须先写入磁盘才能用于其他用途的内存" />
          <i style={{ width: `${(standby / 32) * 100}%`, background: "color-mix(in srgb, var(--a-mem) 22%, transparent)" }} title="包含未频繁使用的缓存数据和代码的内存" />
        </div>
        <div className="a-legend">
          <span className="is-square is-mem">使用中 {inUse.toFixed(1)} GB</span>
          <span className="is-square is-mem-mod">已修改 256.0 MB</span>
          <span className="is-square is-mem-standby">备用 {standby.toFixed(2)} GB</span>
          <span className="is-hollow">空闲 {free.toFixed(2)} GB</span>
        </div>
        <Stats
          figures={[
            ["使用中", inUse.toFixed(1), "GB"],
            ["可用", (32 - inUse).toFixed(1), "GB"],
            ["已提交", `${(inUse * 1.27).toFixed(1)} / 48.0`, "GB"],
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
  } else if (p.device === "disk") {
    const [readValue, readUnit] = split(s.read < 1 ? "0 KB/s" : rate(s.read));
    const [writeValue, writeUnit] = split(s.write < 1 ? "0 KB/s" : rate(s.write));
    body = (
      <>
        <Head title="磁盘 0 (C:)" model={machine.disk} />
        <div className="a-stack is-fill">
          <Chart title="活动时间" top="100%" max={100} curve={curve} color="var(--a-io)" series={[{ values: h.active, color: "var(--a-io)" }]} />
          <Chart
            title="磁盘传输速率"
            top={rateLabel(diskMax)}
            max={diskMax}
            curve={curve}
            readout={{ names: ["读取", "写入"], format: rate }}
            color="var(--a-io)"
            series={[
              { values: h.read, color: "var(--a-io)" },
              { values: h.write, color: "var(--a-io)", dashed: true, fill: false },
            ]}
          />
        </div>
        <Legend
          items={[
            ["读取", "var(--a-io)"],
            ["写入", "var(--a-io)", "dashed"],
          ]}
        />
        <Stats
          figures={[
            ["活动时间", Math.round(s.active), "%"],
            ["平均响应时间", (0.2 + noise(51, p.tick) * 0.4).toFixed(1), "毫秒"],
            ["读取速度", readValue, readUnit],
            ["写入速度", writeValue, writeUnit],
            ["温度", "41", "°C"],
          ]}
          specs={[
            ["容量", "1.8 TB"],
            ["类型", "SSD (NVMe)"],
            ["卷", "C:"],
            ["警告温度", "90 °C"],
          ]}
        />
      </>
    );
  } else if (p.device === "wifi") {
    const [rxValue, rxUnit] = split(s.rx < 1 ? "0 KB/s" : rate(s.rx));
    const [txValue, txUnit] = split(s.tx < 1 ? "0 KB/s" : rate(s.tx));
    body = (
      <>
        <Head title="WLAN" model={machine.wifi} />
        <Chart
          title="吞吐量"
          top={rateLabel(netMax)}
          max={netMax}
          curve={curve}
          readout={{ names: ["接收", "发送"], format: rate }}
          fill
          color="var(--a-net)"
          series={[
            { values: h.rx, color: "var(--a-net)" },
            { values: h.tx, color: "var(--a-net)", dashed: true, fill: false },
          ]}
        />
        <Legend
          items={[
            ["接收", "var(--a-net)"],
            ["发送", "var(--a-net)", "dashed"],
          ]}
        />
        <Stats
          figures={[
            ["接收", rxValue, rxUnit, `${((s.rx * 8) / 1e6).toFixed(2)} Mbps`],
            ["发送", txValue, txUnit, `${((s.tx * 8) / 1e6).toFixed(2)} Mbps`],
          ]}
          specs={[
            ["连接类型", "Wi-Fi"],
            ["链路速度", "1.2 Gbps (143.2 MB/s)"],
            ["IPv4 地址", "192.0.2.24"],
            [
              "IPv6 地址",
              <>
                <span>2001:db8::24</span>
                <span>2001:db8::8a2e:370:7334</span>
              </>,
            ],
            ["物理地址", "02-00-00-00-00-01"],
          ]}
        />
      </>
    );
  } else if (p.device === "gpu") {
    const all: [string, (t: number) => number][] = [
      ["3D", (t) => sampleAt(t).gpu],
      ["Video Encode", (t) => 12 + noise(62, Math.floor(t / 3)) * 14],
      ["Compute 0", (t) => sampleAt(t).gpu * 0.22],
      ["Copy", (t) => sampleAt(t).gpu * 0.12 + noise(61, t) * 2],
      ["Video Decode", (t) => noise(63, Math.floor(t / 4)) * 6],
      ["Compute 1", (t) => noise(64, t) * 1.5],
    ];
    const engines = engineCount === "6" ? all : all.slice(0, 3);
    body = (
      <>
        <Head title="GPU 0" model={machine.gpu} />
        <div className="a-perf-tools">
          <Select
            label="显示的引擎"
            value={engineCount}
            onChange={setEngineCount}
            title="仅显示曲线时间范围内有占用的引擎"
            options={[
              { value: "smart", label: "智能显示" },
              { value: "3", label: "3 个引擎" },
              { value: "6", label: "全部 6 个引擎" },
            ]}
          />
          <Segments
            label="引擎图布局"
            value={engineLayout}
            onChange={setEngineLayout}
            items={[
              { value: "grid", label: "网格", title: "引擎图均匀排列" },
              { value: "focus", label: "焦点", title: "放大第一个引擎，其余排在旁边" },
              { value: "rows", label: "列表", title: "每个引擎占一整行，便于对照时间" },
            ]}
          />
        </div>
        <div className="a-chart-caption">
          <span>引擎利用率</span>
          <span>100%</span>
        </div>
        <div className={"a-engines is-" + engineLayout}>
          {engines.map(([name, f]) => {
            const values = seriesOf(samples, (t) => Math.min(100, f(t)), p.tick);
            return (
              <Chart
                key={name}
                title={
                  <span className="a-engine-head">
                    {name}
                    <Icon name="chevronDown" size={12} />
                  </span>
                }
                top={
                  <b>
                    {Math.round(values[values.length - 1])}
                    <small style={{ color: "var(--a-ink-2)", fontWeight: 400 }}>%</small>
                  </b>
                }
                max={100}
                curve={curve}
                foot={false}
                color="var(--a-gpu)"
                series={[{ values, color: "var(--a-gpu)" }]}
              />
            );
          })}
        </div>
        <div className="a-engine-split" aria-hidden="true" />
        <div className="a-two">
          <Chart
            title="专用 GPU 内存"
            top="8.0 GB"
            max={8}
            curve={curve}
            readout={{ format: gigabytes }}
            height={80}
            color="var(--a-gpu)"
            series={[{ values: seriesOf(samples, vram, p.tick), color: "var(--a-gpu)" }]}
          />
          <Chart
            title="共享 GPU 内存"
            top="16.0 GB"
            max={16}
            curve={curve}
            readout={{ format: gigabytes }}
            height={80}
            color="var(--a-gpu)"
            series={[{ values: seriesOf(samples, (t) => 0.3 + noise(42, Math.floor(t / 5)) * 0.03, p.tick), color: "var(--a-gpu)" }]}
          />
        </div>
        <Stats
          figures={[
            ["利用率", Math.round(s.gpu), "%", "Windows 引擎"],
            ["专用 GPU 内存", `${vram(p.tick).toFixed(1)} / 8.0`, "GB"],
            ["共享 GPU 内存", "0.3 / 16.0", "GB"],
            ["GPU 温度", Math.round(gpuTemperature(p.tick)), "°C"],
            ["核心频率", thousands(1650 + s.gpu * 4 + noise(43, p.tick) * 30), "MHz"],
            ["显存频率", "7,001", "MHz"],
            ["功耗", Math.round(gpuPower(p.tick)), "W"],
          ]}
          specs={[
            ["功耗来源", "NVIDIA NVML · 板卡"],
            ["专用 GPU 内存", "8.0 GB"],
            ["共享 GPU 内存", "16.0 GB"],
            ["功耗上限", "115 W"],
            ["最高温度", "105 °C"],
            ["引擎", "6"],
            ["最高核心频率", "3,105 MHz"],
          ]}
        />
      </>
    );
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
            ["放电功率", Math.round(discharge(p.tick)), "W"],
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
  const index = rail.findIndex((r) => r.id === p.device);
  const moving = markMoves.current;
  markMoves.current = true;
  return (
    <div className="a-perf">
      <nav className="a-rail" aria-label="设备">
        <span
          className={"a-rail-mark" + (moving ? " is-moving" : "")}
          style={{ transform: `translateY(${index * 62}px)` }}
          aria-hidden="true"
        />
        {rail.map((item) => (
          <button
            key={item.id}
            type="button"
            className="a-rail-item"
            aria-current={p.device === item.id ? "true" : undefined}
            data-tour={"device-" + item.id}
            onClick={() => p.onDevice(item.id)}
          >
            <span className="a-rail-text">
              <b>{item.name}</b>
              <span>{item.info}</span>
            </span>
            <span className="a-rail-chart" style={{ "--c": item.color } as CSSProperties}>
              <Curve
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
      <div className="a-perf-gap" />
      <div className="a-perf-main a-scroll">
        {/* The view moves inside a clip: moved in the scroll area itself, its
            offset counted in the scroll range and showed a scrollbar. */}
        <div className="a-perf-stage">
          <div
            key={p.device}
            ref={view}
            className={"a-perf-body" + (p.device === "cpu" && cpuPage === "details" ? " is-details" : "")}
          >
            {body}
          </div>
        </div>
      </div>
    </div>
  );
}
