/** @jsxImportSource @/components/replica/i18n */
"use client";
import { useRef, useState, type CSSProperties, type ReactNode } from "react";
import { machine, thousands } from "./model";
import { useIsoLayoutEffect } from "./widgets";

// The disk's details page as the app draws it (performance/disk_details.rs):
// health, device information and the NVMe SMART/health log as cards, read
// once each time the page opens and again from the head's button.

// The demo's clock: 21:36 when the window opened (Pages.tsx's history).
const START = 21 * 3600 + 36 * 60;
// An NVMe data unit is a thousand 512-byte sectors.
const UNIT = 512_000;
const TB = 1024 ** 4;
// Health tiles three a row where each gets 170 px, else one a row; SMART
// groups at least 380 px (at the 13 px base), up to three a row.
const TILE_MIN = 170;
const TILE_GAP = 24;
const GROUP_MIN = 380;
const GROUP_GAP = 28;
// Remaining life at or below which the drive counts as worn.
const WORN = 10;

// What the drive reports. The live temperature is the overview's; the log
// is as it was when read, its counters moving on with the disk's activity.
const identity = {
  // Protocol identity first: Windows' generic serial can be the namespace's
  // EUI-64 instead of the manufacturer's SN (fictional values).
  sources: [
    { model: machine.disk, firmware: "4B2QJXD7", serial: "S7DPNJ0X402719K" },
    { model: "Samsung SSD 990 PRO 2TB", firmware: "4B2QJXD7", serial: "0025_3852_41B0_7C1E." },
  ],
  capacity: 2_000_398_934_016,
  warning: 90,
  critical: 95,
};
const pick = (field: "model" | "firmware" | "serial") =>
  identity.sources.map((s) => s[field].trim()).find(Boolean) ?? "—";
function healthLog(seconds: number) {
  const t = Math.max(0, seconds);
  return {
    criticalWarning: 0,
    availableSpare: 100,
    spareThreshold: 10,
    percentageUsed: 3,
    mediaErrors: 0,
    errorLogEntries: 0,
    dataUnitsRead: 38_215_904 + Math.floor(t * 61),
    dataUnitsWritten: 29_604_118 + Math.floor(t * 23),
    hostReadCommands: 412_906_537 + Math.floor(t * 1_480),
    hostWriteCommands: 318_442_091 + Math.floor(t * 620),
    controllerBusyMinutes: 1_734 + Math.floor(t / 600),
    powerCycles: 612,
    powerOnHours: 2_846 + Math.floor(t / 3600),
    unsafeShutdowns: 23,
    kelvin: 314,
    sensors: [314, 320],
    warningMinutes: 0,
    criticalMinutes: 0,
    transitions: [3, 0],
    thermalSeconds: [127, 0],
  };
}
type Log = ReturnType<typeof healthLog>;

// Critical warning bits, the reason each names and how bad it is: running
// hot is a caution, the rest mean the drive is failing.
const WARNINGS: [number, string, number][] = [
  [0x01, "可用备用空间低于阈值", 2],
  [0x02, "温度超出阈值", 1],
  [0x04, "可靠性下降", 2],
  [0x08, "介质已转为只读", 2],
  [0x10, "易失性存储备份设备故障", 2],
  [0x20, "持久性内存区域已转为只读", 2],
];
const STATUS = ["良好", "注意", "异常"];
const TONE = ["var(--a-net)", "var(--a-io)", "var(--a-danger)"];
const life = (log: Log) => Math.max(0, 100 - log.percentageUsed);
const spareLow = (log: Log) => log.availableSpare < log.spareThreshold;
function warnings(log: Log) {
  const named = WARNINGS.filter(([bit]) => log.criticalWarning & bit);
  // A later revision may define more bits: worth a look, not a verdict.
  if (log.criticalWarning & ~0x3f) named.push([0, "其他严重警告", 1]);
  return named;
}
// The drive's condition and the reasons for it; the warning bits are kept
// apart from the remaining life.
function assess(log: Log): [number, string[]] {
  const found = warnings(log);
  if (spareLow(log) && !(log.criticalWarning & 0x01)) found.push([0x01, "可用备用空间低于阈值", 2]);
  if (log.mediaErrors > 0) found.push([0, "存在介质与数据完整性错误", 1]);
  if (life(log) <= WORN) found.push([0, "剩余寿命不足 10%", 1]);
  return [Math.max(0, ...found.map(([, , level]) => level)), found.map(([, reason]) => reason)];
}
// Reasons in a list, each translated on its own.
const list = (reasons: string[]) => reasons.flatMap((reason, i) => (i ? ["、", reason] : [reason]));
const capacity = (bytes: number) => (bytes >= TB ? (bytes / TB).toFixed(1) + " TB" : Math.round(bytes / 1024 ** 3) + " GB");
const celsius = (kelvin: number) => Math.round(kelvin - 273.15) + " °C";
function clockText(seconds: number) {
  const s = ((Math.round(seconds) % 86400) + 86400) % 86400;
  const pad = (v: number) => String(Math.floor(v)).padStart(2, "0");
  return `${pad(s / 3600)}:${pad((s % 3600) / 60)}:${pad(s % 60)}`;
}

// Splits weights into at most `columns` runs, in order, keeping the tallest
// run as short as possible: its height and the run lengths.
function balance(weights: number[], columns: number): [number, number[]] {
  const sum = (w: number[]) => w.reduce((a, b) => a + b, 0);
  if (columns <= 1 || weights.length <= 1) return [sum(weights), [weights.length]];
  let best: [number, number[]] = [Infinity, []];
  for (let first = 1; first < weights.length; first++) {
    const [rest, runs] = balance(weights.slice(first), columns - 1);
    const tallest = Math.max(rest, sum(weights.slice(0, first)));
    if (tallest < best[0]) best = [tallest, [first, ...runs]];
  }
  return best;
}

interface Attribute {
  name: string;
  value: ReactNode;
  raw: string;
  alert?: boolean;
}
// The SMART/health log's fields in three groups: health, usage and temperature.
function attributes(log: Log): [string, Attribute[]][] {
  const n = thousands;
  const named = warnings(log).map(([, reason]) => reason);
  return [
    [
      "健康",
      [
        {
          name: "严重警告",
          value: named.length ? list(named) : "无",
          raw: "0x" + log.criticalWarning.toString(16).toUpperCase().padStart(2, "0"),
          alert: log.criticalWarning !== 0,
        },
        { name: "可用备用空间", value: log.availableSpare + "%", raw: n(log.availableSpare), alert: spareLow(log) },
        { name: "可用备用空间阈值", value: log.spareThreshold + "%", raw: n(log.spareThreshold) },
        { name: "已用寿命", value: log.percentageUsed + "%", raw: n(log.percentageUsed), alert: life(log) <= WORN },
        { name: "介质与数据完整性错误", value: n(log.mediaErrors), raw: n(log.mediaErrors), alert: log.mediaErrors > 0 },
        { name: "错误日志条目", value: n(log.errorLogEntries), raw: n(log.errorLogEntries) },
      ],
    ],
    [
      "使用情况",
      [
        { name: "读取数据单位", value: capacity(log.dataUnitsRead * UNIT), raw: n(log.dataUnitsRead) },
        { name: "写入数据单位", value: capacity(log.dataUnitsWritten * UNIT), raw: n(log.dataUnitsWritten) },
        { name: "主机读取命令", value: n(log.hostReadCommands), raw: n(log.hostReadCommands) },
        { name: "主机写入命令", value: n(log.hostWriteCommands), raw: n(log.hostWriteCommands) },
        { name: "控制器忙碌时间", value: n(log.controllerBusyMinutes) + " 分钟", raw: n(log.controllerBusyMinutes) },
        { name: "通电次数", value: n(log.powerCycles), raw: n(log.powerCycles) },
        { name: "通电时间", value: n(log.powerOnHours) + " 小时", raw: n(log.powerOnHours) },
        { name: "不安全关机次数", value: n(log.unsafeShutdowns), raw: n(log.unsafeShutdowns) },
      ],
    ],
    [
      "温度",
      [
        { name: "综合温度", value: celsius(log.kelvin), raw: n(log.kelvin) },
        ...log.sensors.map((kelvin, i) => ({ name: `温度传感器 ${i + 1}`, value: celsius(kelvin), raw: n(kelvin) })),
        { name: "警告温度时间", value: n(log.warningMinutes) + " 分钟", raw: n(log.warningMinutes) },
        { name: "严重温度时间", value: n(log.criticalMinutes) + " 分钟", raw: n(log.criticalMinutes) },
        ...log.transitions.map((count, i) => ({ name: `热管理温度 ${i + 1} 转换次数`, value: n(count), raw: n(count) })),
        ...log.thermalSeconds.map((s, i) => ({ name: `热管理温度 ${i + 1} 总时间`, value: n(s) + " 秒", raw: n(s) })),
      ],
    ],
  ];
}

function Card({
  title,
  note,
  summary,
  alert,
  children,
}: {
  title: string;
  note?: string;
  summary?: string;
  alert?: boolean;
  children: ReactNode;
}) {
  return (
    <section className={"a-card" + (alert ? " is-alert" : "")}>
      <div className="a-card-head">
        <b>
          {title}
          {note && <small className="a-card-note">{note}</small>}
        </b>
        <span>{summary}</span>
      </div>
      {children}
    </section>
  );
}
// A figure, a level bar (0–1) with an optional mark such as a threshold, and a line or two under it.
function Tile({
  label,
  figure,
  level,
  mark,
  color,
  caption,
  alert,
}: {
  label: string;
  figure: ReactNode;
  level: number;
  mark?: number;
  color: string;
  caption?: ReactNode;
  alert?: [ReactNode, string];
}) {
  const unit = (v: number) => (Number.isFinite(v) ? Math.min(1, Math.max(0, v)) * 100 + "%" : "0%");
  return (
    <div className="a-disk-tile">
      <span className="a-disk-label">{label}</span>
      <div className="a-disk-figure">{figure}</div>
      <div className="a-disk-meter" style={{ "--c": color } as CSSProperties}>
        <i style={{ width: unit(level) }} />
        {mark !== undefined && <b style={{ left: unit(mark) }} />}
      </div>
      {caption && <p>{caption}</p>}
      {alert && <p style={{ color: alert[1] }}>{alert[0]}</p>}
    </div>
  );
}
function Reading({ label, value, unit }: { label: string; value: string; unit?: string }) {
  return (
    <div className="a-disk-reading">
      <span>{label}</span>
      <b>
        {value}
        {unit && <small>{unit}</small>}
      </b>
    </div>
  );
}
// "17.8 TB" as a reading's figure and unit.
const reading = (text: string) => {
  const at = text.lastIndexOf(" ");
  return { value: text.slice(0, at), unit: text.slice(at + 1) };
};

// The card column's width, less a card's padding and border, and the text scale.
function useCardWidth() {
  const ref = useRef<HTMLDivElement>(null);
  const [box, setBox] = useState({ width: 1240, text: 1 });
  useIsoLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const measure = () => {
      const width = el.offsetWidth - 34;
      const text = parseFloat(getComputedStyle(el).fontSize) / 13 || 1;
      setBox((b) => (b.width === width && b.text === text ? b : { width, text }));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return [ref, box] as const;
}

export default function DiskDetails({ read, interval }: { read: number; interval: number }) {
  const [ref, { width, text }] = useCardWidth();
  const [serialShown, setSerialShown] = useState(false);
  const seconds = (read * interval) / 1000;
  const log = healthLog(seconds);
  const [condition, reasons] = assess(log);
  const left = life(log);
  // The live reading, as on the overview; the bar runs to the critical
  // temperature, its mark at the warning one.
  const temperature = 41;
  const hot = temperature >= identity.warning;
  const top = Math.max(1, identity.critical);
  const tiles = width >= 3 * TILE_MIN + 2 * TILE_GAP ? 3 : 1;
  const wide = width >= 2 * 300 + GROUP_GAP;
  const columns = Math.min(3, Math.max(1, Math.floor((width + GROUP_GAP) / (GROUP_MIN * text + GROUP_GAP))));
  const groups = attributes(log);
  // Each group's rows plus its header and the gap under it.
  const [, runs] = balance(
    groups.map(([, rows]) => rows.length + 2),
    columns,
  );
  const stacks = runs.map((run, i) => {
    const from = runs.slice(0, i).reduce((a, b) => a + b, 0);
    return groups.slice(from, from + run);
  });
  const serial = pick("serial");
  const identityRows: [string, ReactNode][] = [
    ["型号", pick("model")],
    ["固件版本", pick("firmware")],
    [
      "序列号",
      // Hidden until asked for: pages like this one end up in screenshots.
      <div className="a-disk-serial">
        {serialShown ? serial : "•".repeat(12)}
        <button type="button" onClick={() => setSerialShown((v) => !v)}>
          {serialShown ? "隐藏" : "显示"}
        </button>
      </div>,
    ],
    ["容量", `${capacity(identity.capacity)}（${thousands(identity.capacity)} 字节）`],
    ["卷", "C:"],
  ];
  const linkRows: [string, ReactNode][] = [
    ["接口", "NVMe"],
    ["传输模式", "PCIe 4.0 ×4"],
    ["标准", "NVMe 2.0"],
    ["扇区大小", "逻辑 512 B，物理 4 KB"],
    [
      "支持的功能",
      <div className="a-disk-tags">
        {["SMART", "TRIM", "易失性写入缓存"].map((feature) => (
          <span key={feature}>{feature}</span>
        ))}
      </div>,
    ],
  ];
  const specs = (rows: [string, ReactNode][]) => (
    <dl className="a-specs">
      {rows.map(([label, value]) => [<dt key={label + ":t"}>{label}</dt>, <dd key={label + ":d"}>{value}</dd>])}
    </dl>
  );
  return (
    <div className="a-cards a-disk" ref={ref}>
      <Card title="健康状况" summary={`读取于 ${clockText(START + seconds)}`} alert={condition === 2}>
        <div className="a-disk-tiles" style={{ gridTemplateColumns: `repeat(${tiles}, minmax(0, 1fr))` }}>
          <Tile
            label="状态"
            figure={
              <span className="a-disk-state" style={{ "--c": TONE[condition] } as CSSProperties}>
                {STATUS[condition]}
              </span>
            }
            level={left / 100}
            color={left <= WORN ? "var(--a-danger)" : "var(--a-io)"}
            caption={`剩余寿命 ${left}%`}
            alert={reasons.length ? [list(reasons), condition === 2 ? "var(--a-danger)" : "var(--a-ink)"] : undefined}
          />
          <Tile
            label="温度"
            figure={
              <>
                {temperature}
                <small>°C</small>
              </>
            }
            level={temperature / top}
            mark={identity.warning / top}
            color={hot ? "var(--a-danger)" : "var(--a-io)"}
            caption={[`警告 ${identity.warning} °C`, " · ", `严重 ${identity.critical} °C`]}
          />
          <Tile
            label="可用备用空间"
            figure={
              <>
                {log.availableSpare}
                <small>%</small>
              </>
            }
            level={log.availableSpare / 100}
            mark={log.spareThreshold / 100}
            color={spareLow(log) ? "var(--a-danger)" : "var(--a-io)"}
            caption={`阈值 ${log.spareThreshold}%`}
          />
        </div>
        <div className="a-disk-readings">
          <Reading label="读取总量" {...reading(capacity(log.dataUnitsRead * UNIT))} />
          <Reading label="写入总量" {...reading(capacity(log.dataUnitsWritten * UNIT))} />
          <Reading label="通电时间" value={thousands(log.powerOnHours)} unit="小时" />
          <Reading label="通电次数" value={thousands(log.powerCycles)} />
          <Reading label="不安全关机" value={thousands(log.unsafeShutdowns)} />
          <Reading label="介质错误" value={thousands(log.mediaErrors)} />
        </div>
      </Card>
      <Card title="设备信息">
        {wide ? (
          <div className="a-disk-device">
            {specs(identityRows)}
            {specs(linkRows)}
          </div>
        ) : (
          specs([...identityRows, ...linkRows])
        )}
      </Card>
      <Card title="SMART 属性" note="NVMe 健康日志">
        <div className="a-smart">
          {stacks.map((stack, column) => (
            <div className="a-smart-col" key={column}>
              {stack.map(([group, rows]) => (
                <div className="a-smart-group" key={group}>
                  <div className="a-smart-row is-head">
                    <span>{group}</span>
                    <span>数值</span>
                    <span>原始值</span>
                  </div>
                  {rows.map((row) => (
                    <div className="a-smart-row" key={row.name}>
                      <span data-i18n="smart">{row.name}</span>
                      <span className={row.alert ? "is-alert" : undefined}>{row.value}</span>
                      <span>{row.raw}</span>
                    </div>
                  ))}
                </div>
              ))}
            </div>
          ))}
        </div>
      </Card>
    </div>
  );
}
