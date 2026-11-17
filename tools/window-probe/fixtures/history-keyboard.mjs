// Generate a peak near the previous bucket's mean, then move using actual keys.
// Run from the repository root; ui-shots consumes the two generated target files.
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";

const fixture = JSON.parse(readFileSync(new URL("desktop.json", import.meta.url), "utf8"));
const base = Math.floor(Date.now() / 5000) * 5000 - 21600000 + 60000;
const point = fixture.historySeries[0];
fixture.historySeries = [
  [20000, 10000, 20000, 2, 2],
  [140000, 90000, 140000, 4, 4],
  [190000, 145000, 145000, 34, 98],
  [285000, 220000, 285000, 3, 3],
].map(([ts, frameTs, cpuPeakTs, cpu, cpuPeak]) => ({
  ...point, ts: base + ts, frameTs: base + frameTs,
  cpuPeakTs: base + cpuPeakTs, cpu, cpuPeak,
}));
fixture.historyWindow = { from: base, to: base + 21600000, buckets: 300 };
fixture.historyMoment = { system: { ...fixture.historySeries[2], ts: base + 145000 }, processes: [] };
const peak = { control: { preference: { historyPeak: base + 145000 } }, wait: 200 };
const steps = [
  { control: { preference: { historyRange: "6h" }, page: "history", pointerOutside: true }, wait: 500 },
  peak,
  { control: { keys: ["right"] }, wait: 300, inspect: "layout", label: "next-bucket",
    expect: { "history.cursor": base + 285000, "history.requestedFrame": base + 220000 }, capture: "next" },
  peak,
  { control: { keys: ["left"] }, wait: 300, inspect: "layout", label: "previous-bucket",
    expect: { "history.cursor": base + 140000, "history.requestedFrame": base + 90000 }, capture: "previous" },
  peak,
  { control: { keys: ["home"] }, wait: 200, inspect: "layout", label: "first-bucket",
    expect: { "history.cursor": base + 20000, "history.requestedFrame": base + 10000 } },
  peak,
  { control: { keys: ["end"] }, wait: 200, inspect: "layout", label: "last-bucket",
    expect: { "history.cursor": base + 285000, "history.requestedFrame": base + 220000 } },
];
mkdirSync("target", { recursive: true });
writeFileSync("target/history-keyboard.json", JSON.stringify(fixture));
writeFileSync("target/history-keyboard-steps.json", JSON.stringify({ window: [1200, 800], steps }, null, 2));
