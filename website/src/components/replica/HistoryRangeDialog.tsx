/** @jsxImportSource @/components/replica/i18n */
"use client";
import { useMemo, useRef, useState, type CSSProperties } from "react";
import { Button, Select } from "./widgets";
import { Icon, type UiIcon } from "./icons";
import { processes, readings, sampleAt } from "./model";
import { byteRate } from "./metrics";
import { asleep, base, clock, DAY, durationLabel, monthDay, monthName, type Span } from "./historyTime";
import { translate } from "./i18n";

// The custom range (history/picker.rs): the common ranges, a month of days with
// the exact ends, and folded under them the last 30 days with their events.
const HOUR = 3600;
const presets: [string, number | string][] = [
  ["最近 5 分钟", 300],
  ["最近 15 分钟", 900],
  ["最近 1 小时", HOUR],
  ["最近 6 小时", 6 * HOUR],
  ["最近 24 小时", DAY],
  ["最近 7 天", 7 * DAY],
  ["最近 30 天", 30 * DAY],
  ["全部记录", "全部"],
  ["今天", "今天"],
  ["昨天", "昨天"],
];
// The first record, as the table's footer names it.
export const FIRST = -30 * DAY + 9000;
const midnight = (t: number) => Math.floor((base + t) / DAY) * DAY - base;
const pad = (n: number) => String(n).padStart(2, "0");
const date = (t: number) => new Date((base + t) * 1000);
// "2026/11/19 16:40", as the picker's boxes show times.
const formatLocal = (t: number) => {
  const d = date(t);
  return `${d.getUTCFullYear()}/${pad(d.getUTCMonth() + 1)}/${pad(d.getUTCDate())} ${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}`;
};
function parseLocal(text: string) {
  const n = text.split(/\D+/).filter(Boolean).map(Number);
  if (n.length !== 3 && n.length !== 5) return null;
  const [y, m, d, h = 0, min = 0] = n;
  if (m < 1 || m > 12 || d < 1 || d > 31 || h > 24 || min > 59 || (h === 24 && min > 0)) return null;
  return Date.UTC(y, m - 1, d, h, min) / 1000 - base;
}
const heats: [string, string, string][] = [
  ["cpu", "CPU 使用率", "var(--a-cpu)"],
  ["gpu", "GPU 使用率", "var(--a-gpu)"],
  ["mem", "内存使用率", "var(--a-mem)"],
  ["temp", "CPU 温度", "var(--a-temp)"],
  ["net", "网络接收", "var(--a-net)"],
];
const hourMean = (heat: string, t: number) => {
  let sum = 0;
  for (let k = 0; k < 4; k++) {
    const s = sampleAt(t + k * 900 + 300);
    sum += heat === "gpu" ? s.gpu : heat === "mem" ? s.mem : heat === "temp" ? 62 + s.cpu * 0.32 : heat === "net" ? s.rx : s.cpu;
  }
  return sum / 4;
};
const heatText = (heat: string, v: number) => (heat === "net" ? byteRate(v) : heat === "temp" ? Math.round(v) + "°C" : Math.round(v) + "%");
type Event = { kind: string; from: number; to: number; peak?: string; top?: string };
const kinds: Record<string, [string, UiIcon, string]> = {
  cpu: ["CPU 高负载", "chip", "var(--a-cpu)"],
  temp: ["CPU 温度过高", "thermometer", "var(--a-temp)"],
  net: ["大量下载", "download", "var(--a-net)"],
  gap: ["睡眠", "moon", "var(--a-ink-3)"],
};
const filters: [string, string][] = [
  ["all", "全部"],
  ["cpu", "CPU"],
  ["temp", "温度"],
  ["mem", "内存"],
  ["gpu", "GPU"],
  ["net", "网络"],
  ["disk", "磁盘"],
  ["gap", "无记录"],
];
// The events of a day: stretches of five minutes where a reading stayed high, and the night's sleep.
function eventsOf(day: number, now: number): Event[] {
  const out: Event[] = [];
  const rules: [string, (t: number) => number, number, (v: number) => string][] = [
    ["cpu", (t) => sampleAt(t).cpu, 52, (v) => Math.round(v) + "%"],
    ["temp", (t) => 62 + sampleAt(t).cpu * 0.32, 82, (v) => Math.round(v) + "°C"],
    ["net", (t) => sampleAt(t).rx, 17 * 1024 ** 2, byteRate],
  ];
  for (const [kind, read, threshold, text] of rules) {
    let open: Event | null = null,
      peak = 0,
      peakAt = 0;
    for (let t = day; t < Math.min(day + DAY, now); t += 300) {
      if (asleep(t)) continue;
      let sum = 0,
        top = 0,
        topAt = t;
      for (let k = 0; k < 30; k++) {
        const v = read(t + k * 10);
        sum += v;
        if (v > top) [top, topAt] = [v, t + k * 10];
      }
      if (sum / 30 >= threshold) {
        if (open && open.to === t) open.to = t + 300;
        else out.push((open = { kind, from: t, to: t + 300 }));
        if (top > peak) [peak, peakAt] = [top, topAt];
      } else if (open) {
        open.peak = text(peak);
        open.top = busiestAt(kind, peakAt);
        open = null;
        peak = 0;
      }
    }
    if (open) {
      open.peak = text(peak);
      open.top = busiestAt(kind, peakAt);
    }
  }
  if (now > day + HOUR) out.push({ kind: "gap", from: day + HOUR, to: Math.min(now, day + 7 * HOUR) });
  return out.sort((a, b) => a.from - b.from);
}
function busiestAt(kind: string, t: number) {
  let best = "",
    top = -1;
  for (const [pid, r] of readings(t, processes)) {
    const v = kind === "net" ? r.net : r.cpu;
    if (v > top) [top, best] = [v, processes.find((p) => p.pid === pid)!.exe];
  }
  return best;
}
export default function RangeDialog({
  now,
  span,
  following,
  current,
  apply,
  close,
}: {
  now: number;
  span: Span;
  following: boolean;
  current: number | string | null;
  apply: (s: Span, follow: boolean, name?: string) => void;
  close: () => void;
}) {
  const [range, setRange] = useState<Span>(span);
  const [from, setFrom] = useState(formatLocal(span[0])),
    [to, setTo] = useState(formatLocal(span[1]));
  const [follow, setFollow] = useState(following);
  const [picking, setPicking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [overview, setOverview] = useState(false);
  const [heat, setHeat] = useState("cpu");
  const [filter, setFilter] = useState("all");
  const [hover, setHover] = useState<number | null>(null);
  const [drag, setDrag] = useState<[number, number] | null>(null);
  const end = new Date((base + now) * 1000);
  const [month, setMonth] = useState<[number, number]>([end.getUTCFullYear(), end.getUTCMonth()]);
  const today = midnight(now);
  const set = (a: number, b: number) => {
    setRange([a, b]);
    setFrom(formatLocal(a));
    setTo(formatLocal(b));
    setFollow(false);
    setError(null);
  };
  const preset = (value: number | string) => {
    if (value === "全部") apply([FIRST, now], true, "全部");
    else if (value === "今天") apply([today, now], true, "今天");
    else if (value === "昨天") apply([today - DAY, today], false, "昨天");
    else apply([now - (value as number), now], true);
  };
  const submit = () => {
    const a = parseLocal(from),
      b = follow ? now : parseLocal(to);
    if (a === null || b === null) return setError(`时间格式应为 ${formatLocal(now)}`);
    if (b <= a) return setError("结束时间应晚于开始时间");
    if (b < FIRST || a > now) return setError(`所选时间内没有记录（最早记录于 ${monthDay(FIRST)} ${clock(FIRST, false)}）`);
    const start = Math.max(a, FIRST);
    if (follow) apply([start, now], true);
    else apply([start, Math.min(b, now)], false);
  };
  // The month: each day with how much of it was recorded and how busy the CPU was.
  const first = Date.UTC(month[0], month[1], 1) / 1000 - base,
    days = new Date(Date.UTC(month[0], month[1] + 1, 0)).getUTCDate(),
    lead = (new Date(Date.UTC(month[0], month[1], 1)).getUTCDay() + 6) % 7;
  const stats = useMemo(
    () =>
      Array.from({ length: days }, (_, i) => {
        const day = first + i * DAY;
        let hours = 0,
          cpu = 0;
        for (let h = 0; h < 24; h++) {
          const t = day + h * HOUR;
          if (t + HOUR <= FIRST || t >= now || asleep(t)) continue;
          hours++;
          cpu += hourMean("cpu", t);
        }
        return { hours, cpu: hours ? cpu / hours : 0 };
      }),
    [first, days, Math.floor(now / HOUR)],
  );
  const startDay = midnight(range[0]),
    endDay = midnight(range[1] - 1);
  // The last 30 days hour by hour.
  const mapDays = Array.from({ length: 30 }, (_, i) => today - (29 - i) * DAY);
  const cells = useMemo(
    () => mapDays.flatMap((day) => Array.from({ length: 24 }, (_, h) => (day + h * HOUR >= now || asleep(day + h * HOUR) ? null : hourMean(heat, day + h * HOUR)))),
    [heat, today, Math.floor(now / HOUR)],
  );
  const scale = (v: number) => (heat === "net" ? Math.log10(1 + v / 1024) : v);
  const shown = cells.filter((v): v is number => v !== null).map(scale);
  const low = ["cpu", "gpu", "mem", "net"].includes(heat) ? 0 : Math.min(...shown),
    high = Math.max(...shown);
  const color = heats.find(([key]) => key === heat)![2];
  const map = useRef<HTMLDivElement>(null);
  const cellAt = (e: React.PointerEvent) => {
    const r = map.current!.getBoundingClientRect();
    const x = (e.clientX - r.left) / r.width,
      y = (e.clientY - r.top) / r.height;
    if (x < 0 || x >= 1 || y < 0 || y >= 1) return null;
    return Math.floor(x * 30) * 24 + Math.floor(y * 24);
  };
  const cellStart = (cell: number) => mapDays[Math.floor(cell / 24)] + (cell % 24) * HOUR;
  const outline: Span = drag ? [cellStart(Math.min(...drag)), cellStart(Math.max(...drag)) + HOUR] : range;
  const eventDay = midnight(range[0]),
    eventEnd = Math.max(range[1], eventDay + DAY);
  const events = useMemo(() => {
    const list: Event[] = [];
    for (let d = eventDay; d < eventEnd && list.length < 40; d += DAY) list.push(...eventsOf(d, now));
    return list;
  }, [eventDay, eventEnd, Math.floor(now / 300)]);
  const listed = events.filter((e) => filter === "all" || e.kind === filter).slice(0, 40);
  const hint =
    hover === null
      ? "每格 1 小时"
      : `${monthDay(cellStart(hover))} ${clock(cellStart(hover), false)}–${clock(cellStart(hover) + HOUR, false)} · ${
          cells[hover] === null ? "无记录" : heatText(heat, cells[hover]!)
        }`;
  return (
    <div
      className="a-scrim"
      onKeyDown={(e) => {
        if (e.key === "Escape") close();
      }}
      onPointerDown={(e) => {
        if (e.target === e.currentTarget) close();
      }}
    >
      <form
        className="a-dialog h-range-dialog"
        data-overview={overview || undefined}
        role="dialog"
        aria-modal="true"
        aria-label="自定义时间范围"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <div className="a-dialog-body">
          <h3>自定义时间范围</h3>
          <div className="h-range-body">
            <div className="h-range-left">
              <div className="h-range-grid">
                <aside>
                  {presets.map(([label, value]) => (
                    <button
                      type="button"
                      key={label}
                      data-on={current === value || undefined}
                      data-break={value === "今天" || undefined}
                      onClick={() => preset(value)}
                    >
                      {label}
                    </button>
                  ))}
                </aside>
                <div className="h-range-main">
                  <div className="h-month">
                    <button type="button" onClick={() => setMonth(([y, m]) => (m ? [y, m - 1] : [y - 1, 11]))}>
                      <Icon name="chevronLeft" size={14} />
                    </button>
                    <b>{`${month[0]} 年 ${monthName(month[0], month[1])} 月`}</b>
                    <button type="button" onClick={() => setMonth(([y, m]) => (m === 11 ? [y + 1, 0] : [y, m + 1]))}>
                      <Icon name="chevronRight" size={14} />
                    </button>
                  </div>
                  <div className="h-week">
                    {translate("一 二 三 四 五 六 日")
                      .split(" ")
                      .map((d) => (
                        <small key={d} translate="no">
                          {d}
                        </small>
                      ))}
                  </div>
                  <div className="h-calendar">
                    {Array.from({ length: lead }, (_, i) => (
                      <span key={"l" + i} />
                    ))}
                    {stats.map(({ hours, cpu }, i) => {
                      const day = first + i * DAY;
                      const off = day > today || day + DAY <= FIRST;
                      const inside = day >= startDay && day <= endDay,
                        ends = inside && (day === startDay || day === endDay);
                      const next = day + DAY;
                      return (
                        <button
                          type="button"
                          key={day}
                          disabled={off}
                          data-inside={(inside && !ends) || undefined}
                          data-end={ends || undefined}
                          data-start={(ends && day === startDay && day !== endDay) || undefined}
                          data-last={(ends && day === endDay && day !== startDay) || undefined}
                          data-today={day === today || undefined}
                          title={off ? undefined : `${monthDay(day)} · 记录 ${hours} 小时 · 平均 CPU ${Math.round(cpu)}%`}
                          style={{ "--cover": Math.min(1, hours / 24), "--tint": 0.35 + 0.65 * Math.min(1, cpu / 30) } as CSSProperties}
                          onClick={(e) => {
                            if (e.detail >= 2) {
                              apply([Math.max(FIRST, day), Math.min(now, next)], false);
                              return;
                            }
                            if (picking && day >= range[0]) {
                              set(range[0], next);
                              setPicking(false);
                            } else {
                              set(day, next);
                              setPicking(true);
                            }
                          }}
                        >
                          {i + 1}
                          <i>
                            <i />
                          </i>
                        </button>
                      );
                    })}
                  </div>
                  <label>
                    <span>开始</span>
                    <input className="a-input" autoFocus value={from} onChange={(e) => setFrom(e.target.value)} />
                  </label>
                  <label>
                    <span>结束</span>
                    <input className="a-input" value={follow ? formatLocal(now) : to} disabled={follow} onChange={(e) => setTo(e.target.value)} />
                  </label>
                  <label className="h-range-follow">
                    <input
                      type="checkbox"
                      checked={follow}
                      onChange={(e) => {
                        setFollow(e.target.checked);
                        if (e.target.checked) setTo(formatLocal(now));
                      }}
                    />
                    结束于当前时间时跟随最新记录
                  </label>
                  {error && <p className="h-range-error">{error}</p>}
                </div>
              </div>
              <button className="h-overview-toggle" type="button" aria-expanded={overview} onClick={() => setOverview(!overview)}>
                <Icon name={overview ? "chevronDown" : "chevronRight"} size={14} />
                显示 30 天概览与事件
              </button>
              {overview && (
                <div className="h-overview">
                  <div className="h-overview-head">
                    <b>30 天概览</b>
                    <small>{hint}</small>
                    <Select label="30 天概览" value={heat} onChange={setHeat} options={heats.map(([value, label]) => ({ value, label }))} />
                  </div>
                  <div className="h-map-box">
                    {[0, 6, 12, 18, 24].map((h) => (
                      <small key={h} className="h-map-hour" style={{ top: (h / 24) * 150 - 7 }}>
                        {pad(h)}:00
                      </small>
                    ))}
                    <div
                      ref={map}
                      className="h-map"
                      onPointerDown={(e) => {
                        const c = cellAt(e);
                        if (c === null) return;
                        e.currentTarget.setPointerCapture(e.pointerId);
                        setDrag([c, c]);
                      }}
                      onPointerMove={(e) => {
                        const c = cellAt(e);
                        setHover(c);
                        if (drag && c !== null) setDrag([drag[0], c]);
                      }}
                      onPointerLeave={() => setHover(null)}
                      onPointerUp={() => {
                        if (!drag) return;
                        set(cellStart(Math.min(...drag)), cellStart(Math.max(...drag)) + HOUR);
                        setDrag(null);
                      }}
                    >
                      {cells.map((v, cell) => {
                        const t = cellStart(cell);
                        if (t >= now) return null;
                        const f = v === null ? 0 : Math.min(1, Math.max(0, (scale(v) - low) / Math.max(1e-6, high - low)));
                        return (
                          <i
                            key={cell}
                            style={{
                              left: (Math.floor(cell / 24) / 30) * 100 + "%",
                              top: ((cell % 24) / 24) * 100 + "%",
                              background: v === null ? "var(--a-stroke)" : color,
                              opacity: v === null ? 0.35 : 0.1 + 0.9 * f,
                            }}
                          />
                        );
                      })}
                      {mapDays.map((day, i) => {
                        const a = Math.max(outline[0], day),
                          b = Math.min(outline[1], day + DAY);
                        return b > a ? (
                          <b
                            key={day}
                            style={{ left: (i / 30) * 100 + "%", top: ((a - day) / DAY) * 100 + "%", height: `max(2px, ${((b - a) / DAY) * 100}%)` }}
                          />
                        ) : null;
                      })}
                    </div>
                    {mapDays.map((day, i) =>
                      i === 29 || (29 - i) % 7 === 0 ? (
                        <button
                          type="button"
                          key={day}
                          className="h-map-day"
                          data-today={i === 29 || undefined}
                          style={{ left: `calc(34px + (100% - 34px) * ${(i + 0.5) / 30})` }}
                          onClick={() => set(day, day + DAY)}
                        >
                          {i === 29 ? "今天" : monthDay(day)}
                        </button>
                      ) : null,
                    )}
                  </div>
                </div>
              )}
            </div>
            {overview && (
              <div className="h-events">
                <div className="h-events-head">
                  <b>事件</b>
                  <small>{`${monthDay(eventDay)} · ${listed.length} 项`}</small>
                </div>
                <div className="h-filters">
                  {filters.map(([key, label]) => (
                    <button type="button" key={key} aria-pressed={filter === key} onClick={() => setFilter(key)}>
                      {label}
                    </button>
                  ))}
                </div>
                <div className="h-event-list a-scroll">
                  {listed.map((e) => {
                    const [name, icon, tone] = kinds[e.kind];
                    let detail = `${clock(e.from, false)}–${clock(e.to, false)}`;
                    if (e.peak) detail = `${detail} · 最高 ${e.peak}`;
                    if (e.top) detail = `${detail} · ${e.top}`;
                    return (
                      <button
                        type="button"
                        key={e.kind + e.from}
                        className="h-event"
                        style={{ "--tone": tone } as CSSProperties}
                        onClick={() => {
                          const pad = Math.max(600, ((e.to - e.from) * 3) / 2);
                          set(e.from - pad, e.to + pad);
                        }}
                      >
                        <span className="h-event-icon">
                          <Icon name={icon} size={14} />
                        </span>
                        <span>
                          <span className="h-event-title">
                            <b>{name}</b>
                            <small>{durationLabel(e.to - e.from)}</small>
                          </span>
                          <small>{detail}</small>
                        </span>
                      </button>
                    );
                  })}
                  {!listed.length && <p>所选日期内没有事件</p>}
                </div>
              </div>
            )}
          </div>
        </div>
        <div className="a-dialog-foot">
          <Button onClick={close}>取消</Button>
          <button className="a-btn is-accent">应用</button>
        </div>
      </form>
    </div>
  );
}
