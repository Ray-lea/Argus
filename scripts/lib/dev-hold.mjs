// A hold on the development script's rebuilds (scripts/desktop.mjs): while an
// edit spans several files, the sources in between may not compile or may
// show half a change, and each save would otherwise restart the running
// development instance. The hold is a small file with an expiry, so one that
// is forgotten ends by itself.
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

/** Where the hold of a checkout lives, relative to its root. */
export const HOLD = "target/native-dev/hold.json";
export const DEFAULT_MINUTES = 20;
export const MAX_MINUTES = 120;

/** The hold in force at `now`, as `{ until, reason }`, or null: no file, an expired or an unreadable one. */
export function readHold(path, now = Date.now()) {
  let hold;
  try {
    hold = JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return null;
  }
  if (!Number.isFinite(hold?.until) || hold.until <= now || hold.until > now + MAX_MINUTES * 60000) return null;
  return { until: hold.until, reason: typeof hold.reason === "string" ? hold.reason : "" };
}

/** Starts or extends the hold for `minutes` from `now`; returns it. */
export function writeHold(path, minutes = DEFAULT_MINUTES, reason = "", now = Date.now()) {
  if (!Number.isFinite(minutes) || minutes <= 0 || minutes > MAX_MINUTES) throw new Error(`a hold lasts up to ${MAX_MINUTES} minutes`);
  const hold = { until: Math.round(now + minutes * 60000), reason };
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify(hold)}\n`);
  return hold;
}

/** Ends the hold; true when there was one in force. */
export function clearHold(path, now = Date.now()) {
  const held = readHold(path, now) !== null;
  rmSync(path, { force: true });
  return held;
}

/**
 * What the watcher does with a source change: `changed()` builds at once
 * unless a hold is in force (`active()` returns it); then it reports the hold
 * once (`report(hold)`), looks every `poll` ms whether it has ended, and
 * builds a single time for everything that changed meanwhile
 * (`report(null)` first). `stop()` ends the waiting.
 */
export function holdGate({ active, build, report, poll = 1000 }) {
  let waiting = null;
  return {
    changed() {
      if (waiting) return;
      const hold = active();
      if (!hold) return void build();
      report(hold);
      waiting = setInterval(() => {
        if (active()) return;
        clearInterval(waiting);
        waiting = null;
        report(null);
        build();
      }, poll);
    },
    stop() {
      clearInterval(waiting);
      waiting = null;
    },
  };
}
