import { language, translate } from "./i18n";
import { dateLocale } from "@/lib/languages";
export const DAY = 86400;
export const base = Date.UTC(2026, 10, 3, 21, 36) / 1000;
const clocks = new Map<string, Intl.DateTimeFormat>();
function format(key: string, options: Intl.DateTimeFormatOptions) {
  const locale = dateLocale(language());
  let f = clocks.get(locale + key);
  if (!f) clocks.set(locale + key, (f = new Intl.DateTimeFormat(locale, { timeZone: "UTC", ...options })));
  return f;
}
const pad = (n: number) => String(n).padStart(2, "0");
// The time of day as the app writes it (history/mod.rs `clock`).
export function clock(time: number, seconds = true) {
  const d = new Date((base + time) * 1000);
  return pad(d.getUTCHours()) + ":" + pad(d.getUTCMinutes()) + (seconds ? ":" + pad(d.getUTCSeconds()) : "");
}
// The date as format.monthDay writes it: "11月3日", "Nov 3".
export function monthDay(time: number) {
  const d = new Date((base + time) * 1000);
  if (language() === "zh") return d.getUTCMonth() + 1 + "月" + d.getUTCDate() + "日";
  return format("/md", { month: "short", day: "numeric" }).format(d);
}
export const momentLabel = (time: number, seconds = true) => monthDay(time) + " " + clock(time, seconds);
export const sameDay = (a: number, b: number) => Math.floor((base + a) / DAY) === Math.floor((base + b) / DAY);
// A span of time, the date said once when both ends share it (`range_label`).
export const rangeLabel = (from: number, to: number, seconds: boolean) =>
  sameDay(from, to)
    ? momentLabel(from, seconds) + "–" + clock(to, seconds)
    : momentLabel(from, seconds) + " – " + momentLabel(to, seconds);
// A clock tick: the date at midnight or for whole days, else the time (`tick_label`).
export const tickLabel = (time: number, step: number) =>
  step >= DAY || ((base + time) % DAY + DAY) % DAY === 0 ? monthDay(time) : clock(time, step < 60);
// A navigator end: the date for long ranges, the time today, both otherwise (`edge_label`).
export function edgeLabel(time: number, span: number, now: number) {
  if (span > 2 * DAY) return monthDay(time);
  if (sameDay(time, now)) return clock(time, span <= 900);
  return monthDay(time) + " " + clock(time, false);
}
// A duration in words: "2 小时 5 分钟" (`duration_label`).
export function durationLabel(seconds: number) {
  const minutes = Math.floor(seconds / 60);
  if (seconds < 60) return Math.max(1, Math.floor(seconds)) + " 秒";
  if (minutes < 60) return minutes + " 分钟";
  if (minutes < 1440) return Math.floor(minutes / 60) + " 小时 " + (minutes % 60) + " 分钟";
  return Math.floor(minutes / 1440) + " 天 " + Math.floor((minutes % 1440) / 60) + " 小时";
}
export function stamp(time: number, long = false) {
  return format(long ? "/date" : "/time", {
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
    ...(long ? { month: "2-digit", day: "2-digit" } : { second: "2-digit" }),
  }).format((base + time) * 1000);
}
export type Span = [number, number];
// An app message with placeholders, translated whole and then filled: for
// messages a pattern cannot tell apart ("{time} 的进程", "{range} 的进程").
export const fill = (template: string, values: Record<string, string>) =>
  translate(template).replace(/\{(\w+)\}/g, (hole, key: string) => values[key] ?? hole);
// A month as format.monthNames names it: a number in Chinese, an abbreviation elsewhere.
export const monthName = (year: number, month: number) =>
  language() === "zh" ? String(month + 1) : format("/m", { month: "short" }).format(Date.UTC(year, month, 15));

// The simulated computer sleeps between 01:00 and 07:00 each day.
export function asleep(time: number) {
  const local = (((base + time) % DAY) + DAY) % DAY;
  return local >= 3600 && local < 7 * 3600;
}
