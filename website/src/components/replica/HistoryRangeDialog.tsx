/** @jsxImportSource @/components/replica/i18n */
"use client";
import { useState } from "react";
import { dateLocale } from "@/lib/languages";
import { language } from "./i18n";
import { Button } from "./widgets";
import { sampleAt } from "./model";
import { base, DAY, stamp, type Span } from "./historyTime";
export default function RangeDialog({
  now,
  span,
  apply,
  close,
}: {
  now: number;
  span: Span;
  apply: (s: Span, follow?: boolean) => void;
  close: () => void;
}) {
  const input = (t: number) => new Date((base + t) * 1000).toISOString().slice(0, 19);
  const parse = (s: string) => Date.parse(s + "Z") / 1000 - base;
  const [from, setFrom] = useState(input(span[0])),
    [to, setTo] = useState(input(span[1]));
  const [overview, setOverview] = useState(false);
  const today = Math.floor((base + now) / DAY) * DAY - base;
  const [day, setDay] = useState(today);
  const [firstDay, setFirstDay] = useState<number | null>(null);
  const [month, setMonth] = useState(() => new Date((base + now) * 1000).getUTCMonth());
  const year = new Date((base + now) * 1000).getUTCFullYear();
  const first = Date.UTC(year, month, 1) / 1000 - base,
    nextMonth = Date.UTC(year, month + 1, 1) / 1000 - base;
  const calendar = Array.from({ length: Math.round((nextMonth - first) / DAY) }, (_, i) => first + i * DAY);
  const [follow, setFollow] = useState(span[1] === now);
  const days = Array.from({ length: 30 }, (_, i) => today - (29 - i) * DAY);
  const select = (a: number, b: number) => {
    setFrom(input(a));
    setTo(input(b));
  };
  const valid = parse(from) < parse(to) && parse(to) <= now && parse(from) >= now - 30 * DAY;
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
        role="dialog"
        aria-modal="true"
        aria-label="自定义时间范围"
        onSubmit={(e) => {
          e.preventDefault();
          if (valid) apply([parse(from), follow ? now : parse(to)], follow);
        }}
      >
        <div className="a-dialog-body">
          <h3>自定义时间范围</h3>
          <div className="h-range-grid" data-overview={overview || undefined}>
            <aside>
              {(
                [
                  ["最近 5 分钟", 300],
                  ["最近 15 分钟", 900],
                  ["最近 1 小时", 3600],
                  ["最近 6 小时", 21600],
                  ["最近 24 小时", DAY],
                  ["最近 7 天", 7 * DAY],
                  ["最近 30 天", 30 * DAY],
                  ["全部记录", 30 * DAY],
                ] as const
              ).map(([label, n]) => (
                <Button key={label} onClick={() => apply([now - n, now], true)}>
                  {label}
                </Button>
              ))}
              <Button onClick={() => apply([today, now], true)}>今天</Button>
              <Button onClick={() => apply([today - DAY, today])}>昨天</Button>
            </aside>
            <div>
              <div className="h-month">
                <Button disabled={first <= now - 30 * DAY} onClick={() => setMonth(month - 1)}>
                  ‹
                </Button>
                <b translate="no">
                  {new Intl.DateTimeFormat(dateLocale(language()), {
                    year: "numeric",
                    month: "long",
                    timeZone: "UTC",
                  }).format((base + first) * 1000)}
                </b>
                <Button disabled={nextMonth > now} onClick={() => setMonth(month + 1)}>
                  ›
                </Button>
              </div>
              <div className="h-week">
                {Array.from({ length: 7 }, (_, i) => (
                  <small key={i} translate="no">
                    {new Intl.DateTimeFormat(dateLocale(language()), { weekday: "narrow", timeZone: "UTC" }).format(
                      Date.UTC(2026, 10, 2 + i),
                    )}
                  </small>
                ))}
              </div>
              <div className="h-calendar">
                {calendar.map((t, i) => (
                  <button
                    type="button"
                    key={t}
                    style={
                      i === 0 ? { gridColumnStart: 1 + ((new Date((base + t) * 1000).getUTCDay() + 6) % 7) } : undefined
                    }
                    disabled={t > now || t + DAY < now - 30 * DAY}
                    aria-pressed={day === t}
                    title={stamp(t, true)}
                    onClick={() => {
                      setDay(t);
                      const a = firstDay !== null && t >= firstDay ? firstDay : t;
                      setFirstDay(a === t ? t : null);
                      setFollow(false);
                      select(Math.max(now - 30 * DAY, a), Math.min(now, t + DAY));
                    }}
                    onDoubleClick={() => apply([Math.max(now - 30 * DAY, t), Math.min(now, t + DAY)])}
                  >
                    {i + 1}
                  </button>
                ))}
              </div>
              <label>
                开始
                <input
                  autoFocus
                  type="datetime-local"
                  step="1"
                  value={from}
                  onChange={(e) => setFrom(e.target.value)}
                />
              </label>
              <label>
                结束
                <input type="datetime-local" step="1" value={to} onChange={(e) => setTo(e.target.value)} />
              </label>
              <label>
                <input
                  type="checkbox"
                  checked={follow}
                  onChange={(e) => {
                    setFollow(e.target.checked);
                    if (e.target.checked) setTo(input(now));
                  }}
                />
                结束于当前时间时跟随最新记录
              </label>
              {overview && (
                <>
                  <b>30 天概览</b>
                  <div className="h-heat">
                    {days.map((t, i) => (
                      <div key={i}>
                        {Array.from({ length: 24 }, (_, h) => {
                          const hour = Math.floor((base + t) / DAY) * DAY - base + h * 3600;
                          return (
                            <button
                              type="button"
                              key={h}
                              disabled={hour > now}
                              title={stamp(hour, true)}
                              style={{ opacity: 0.15 + sampleAt(hour).cpu / 100 }}
                              onClick={() => select(hour, Math.min(now, hour + 3600))}
                            />
                          );
                        })}
                      </div>
                    ))}
                  </div>
                </>
              )}
            </div>
            {overview && (
              <div>
                <b>事件</b>
                {Array.from({ length: 24 }, (_, h) => day + h * 3600)
                  .filter((t) => t < now && sampleAt(t).cpu >= 65)
                  .map((t) => (
                    <Button key={t} onClick={() => select(t, Math.min(now, t + 300))}>
                      <span>CPU 高负载</span> · {stamp(t)} · {Math.round(sampleAt(t).cpu)}%
                    </Button>
                  ))}
              </div>
            )}
          </div>
          <button
            className="h-overview-toggle"
            type="button"
            aria-expanded={overview}
            onClick={() => setOverview(!overview)}
          >
            {overview ? "⌄" : "›"} <span>显示 30 天概览与事件</span>
          </button>
        </div>
        <div className="a-dialog-foot">
          <Button onClick={close}>取消</Button>
          <button className="a-btn is-accent" disabled={!valid}>
            应用
          </button>
        </div>
      </form>
    </div>
  );
}
