// Generate a deterministic rebucketing scenario for the isolated native window.
// Run from the repository root, then pass target/pr76-history-refresh-steps.json
// and target/pr76-history-refresh.json to scripts/ui-shots.mjs. The peak and
// regrouped captures also need visual inspection of their CPU readouts.
import { readFileSync, writeFileSync } from "node:fs";

const fixture = JSON.parse(readFileSync("tools/window-probe/fixtures/desktop.json", "utf8"));
const base = Math.floor(Date.now() / 5000) * 5000 - 120000;
const point = { ...fixture.historySeries[0], ts: base + 37500, frameTs: base,
  cpuPeakTs: base, cpu: 34, cpuPeak: 98 };
const earlier = { ...point, ts: base - 5000, frameTs: base - 5000,
  cpuPeakTs: base - 5000, cpu: 2, cpuPeak: 2 };
fixture.historySeries = [earlier, point];
fixture.historyWindow = { from: base - 60000, to: base + 60000, buckets: 2 };
fixture.historyMoment = { system: { ...point, ts: base }, processes: [
  { pid: 44101, name: "selected-frame.exe", cpu: 98, mem: 1048576, io: 0, gpu: 0, net: 0 },
] };
const regrouped = [earlier, { ...point, ts: base + 20000, frameTs: base + 5000, cpu: 44, cpuPeak: 88 }];
const steps = [
  { control: { page: "history", pointerOutside: true }, wait: 700 },
  { control: { mouse: { type: "down", button: "left", x: 96, y: 577 } }, wait: 200 },
  { control: { mouse: { type: "up", button: "left", x: 96, y: 577 } }, wait: 400,
    inspect: "layout", label: "peak-near-other-bucket", expect: { "history.cursor": base, "history.requestedFrame": base }, capture: "peak" },
  { control: { keys: ["end"] }, wait: 400, inspect: "layout", label: "ordinary-selected",
    expect: { "history.cursor": base + 37500, "history.requestedFrame": base }, capture: "ordinary" },
  { control: { resolve: { command: "history_series", cache: "historySeries", ok: true,
    args: { range: "1h", from: base - 60000, to: base + 60000, buckets: 2 }, value: regrouped } },
    wait: 500, inspect: "layout", label: "regrouped", expect: { "history.cursor": base + 37500, "history.requestedFrame": base }, capture: "regrouped" },
];
writeFileSync("target/pr76-history-refresh.json", JSON.stringify(fixture));
writeFileSync("target/pr76-history-refresh-steps.json", JSON.stringify({ window: [1200, 800], steps }, null, 2));
