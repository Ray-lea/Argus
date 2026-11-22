import { language } from "./i18n";
import { dateLocale } from "@/lib/languages";
export const DAY = 86400;
export const base = Date.UTC(2026, 10, 3, 21, 36) / 1000;
const clocks = new Map<string, Intl.DateTimeFormat>();
export function stamp(time: number, long = false) {
  const locale = dateLocale(language());
  const key = locale + (long ? "/date" : "/time");
  let clock = clocks.get(key);
  if (!clock) {
    clock = new Intl.DateTimeFormat(locale, {
      timeZone: "UTC",
      hour: "2-digit",
      minute: "2-digit",
      hourCycle: "h23",
      ...(long ? { month: "2-digit", day: "2-digit" } : { second: "2-digit" }),
    });
    clocks.set(key, clock);
  }
  return clock.format((base + time) * 1000);
}
export type Span = [number, number];

// The simulated computer sleeps between 01:00 and 07:00 each day.
export function asleep(time: number) {
  const local = (((base + time) % DAY) + DAY) % DAY;
  return local >= 3600 && local < 7 * 3600;
}
