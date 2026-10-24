// Owned native-window validation. Capture only the test HWND and reject foreground changes.
import { spawn, spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { createInterface } from "node:readline";
import assert from "node:assert/strict";
const root = resolve(import.meta.dirname, "..");
process.chdir(root);
const label = process.argv[2] ?? `smoke-${Date.now()}`;
if (!/^[a-z0-9-]+$/.test(label)) throw Error("Invalid test label");
const live = process.argv.includes("--live");
const output = resolve("target/native-validation", label);
mkdirSync(output, { recursive: true });
const metadata = JSON.parse(readFileSync("resources/app.json"));
const identifier = `${metadata.identifier}.test.${label}`;
const config = resolve(process.env.APPDATA, identifier);
mkdirSync(config, { recursive: true });
writeFileSync(
  resolve(config, "settings.json"),
  JSON.stringify({ language: "zh-CN", theme: "dark", closeToTray: false, updateCheck: false }),
);
let fixture;
if (!live) {
  const fixtureFlag = process.argv.indexOf("--fixture");
  fixture = JSON.parse(readFileSync(fixtureFlag >= 0 ? process.argv[fixtureFlag + 1] : "tools/window-probe/fixtures/desktop.json"));
  const samples = JSON.parse(readFileSync("resources/motion.json")).chartSamples;
  const history = fixture.historySeries ?? [];
  fixture.snapshots = Array.from({ length: samples - 1 }, (_, i) => {
    const frame = structuredClone(fixture.snapshot),
      past = history[Math.floor((i * Math.max(0, history.length - 1)) / samples)];
    frame.timestamp = fixture.snapshot.timestamp - (samples - 1 - i) * 1000;
    frame.seq = i + 1;
    if (past) {
      frame.system.cpu = past.cpu;
      frame.system.memUsed = past.memUsed;
    }
    return frame;
  });
  fixture.snapshot.seq = samples;
  writeFileSync(resolve(output, "fixture.json"), JSON.stringify(fixture));
}
const probe = resolve("target/release/argus-window-probe.exe");
const executable = resolve(output, "argus.exe");
copyFileSync(resolve(process.argv.includes("--dev") ? "target/debug/argus.exe" : "target/release/argus.exe"), executable);
const child = spawn(
  probe,
  [
    "run",
    executable,
    "--hidden",
    "--test",
    "--identifier",
    identifier,
    "--stop-after",
    "180",
    ...(live ? [] : ["--fixture", resolve(output, "fixture.json")]),
  ],
  { windowsHide: true, stdio: ["pipe", "pipe", "pipe"] },
);
const events = [];
let error = "",
  shown,
  exited = false,
  exitCode;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
child.stderr.on("data", (b) => (error += b));
child.on("error", (e) => {
  error += e.message;
  exited = true;
});
child.on("exit", (code) => {
  exited = true;
  exitCode = code;
});
createInterface({ input: child.stdout }).on("line", (line) => {
  try {
    const event = JSON.parse(line);
    events.push(event);
    if (event.event === "shown") shown = event;
  } catch {
    error += line;
  }
});
const send = (control) => child.stdin.write(`app ${JSON.stringify(control)}\n`);
const capture = (name) => {
  send({ inspectLayout: name });
  const result = spawnSync(probe, ["capture", String(shown.pid), resolve(output, `${name}.png`)], {
    windowsHide: true,
    encoding: "utf8",
    timeout: 20000,
  });
  if (result.status !== 0) throw Error(`Capture ${name}: ${result.stderr}`);
  console.log(`Captured ${name}`);
};
try {
  for (let i = 0; i < 600 && !shown && !exited; i++) await sleep(50);
  if (!shown) throw Error(`Window unavailable: ${error}`);
  await sleep(2500);
  if (live) {
    send({ page: "performance", motion: process.argv.includes("--fluid") ? "fluid" : "balanced" });
    child.stdin.write("watch\n");
    send({ inspectLayout: "live-begin" });
    await sleep(18000);
    capture("live-performance");
    if (process.argv.includes("--move")) {
      child.stdin.write("begin-move\n");
      for (let step = 0; step < 120; step++) {
        child.stdin.write(`move ${step % 2 ? -4 : 4} 0\n`);
        await sleep(50);
      }
      child.stdin.write("end-move\n");
      await sleep(600);
      capture("moved-performance");
    }
    child.stdin.write("hide\n");
    await sleep(3500);
    child.stdin.write("show\n");
    await sleep(4500);
    capture("restored-performance");
  } else {
    for (const page of [
      "processes",
      "performance",
      "services",
      "startup",
      "network",
      "history",
      "settings",
    ]) {
      send({ page });
      await sleep(650);
      capture(page);
    }
    for (const [name, state] of [
      ["processes-light", { page: "processes", theme: "light" }],
      ["processes-en-light", { language: "en-US" }],
      ["processes-en-dark", { theme: "dark" }],
      ["sorted-name", { clickHeader: "name" }],
      ["sorted-name-reversed", { clickHeader: "name" }],
      ["sorted-cpu", { clickHeader: "cpu" }],
      ["expanded-app", { foldRow: 0 }],
      ["selected-process", { selectRow: 0 }],
      ["logical-cores", { page: "performance", cpuMode: "logical" }],
      ["logical-p-cores", { coreFilter: "p" }],
      ["logical-e-cores", { coreFilter: "e" }],
      ["logical-all-cores", { coreFilter: "all" }],
      ["cpu-telemetry", { cpuMode: "details" }],
      ["task-dialog", { dialog: "task" }],
      ["file-lock-dialog", { dismiss: true, dialog: "filelock" }],
      ["welcome-dialog", { dismiss: true, dialog: "onboarding", args: { step: 0 } }],
      ["settings-hotkey", { dismiss: true, page: "settings", settingsSection: "hotkey" }],
      ["settings-rules", { settingsSection: "rules" }],
      ["settings-update", { settingsSection: "update" }],
      ["settings-startup", { settingsSection: "startup" }],
      ["settings-permission", { settingsSection: "privilege" }],
      ["settings-motion", { settingsSection: "motion" }],
      ["settings-data", { settingsSection: "data" }],
      ["settings-about", { settingsSection: "about" }],
      ["memory", { device: "memory" }],
      ...fixture.snapshot.hardware.disks.map((d, i) => [`disk-${i}`, { device: `disk:${d.info.index}` }]),
      ...fixture.snapshot.hardware.gpus.map((g, i) => [`gpu-${i}`, { device: `gpu:${g.info.id}` }]),
      ...fixture.snapshot.hardware.nics.map((n, i) => [`nic-${i}`, { device: `nic:${n.info.id}` }]),
      ...fixture.snapshot.hardware.batteries.map((b, i) => [`battery-${i}`, { device: `bat:${b.info.id}` }]),
      ["processes-tree", { page: "processes", view: "tree" }],
      ["processes-list", { view: "list" }],
      ["processes-side", { view: "apps", navPosition: "side", navExpanded: true }],
    ]) {
      send(state);
      await sleep(650);
      capture(name);
    }
    send({ page: "processes", language: "zh-CN", theme: "light", navPosition: "top" });
    child.stdin.write(`size ${metadata.window.minWidth} ${metadata.window.minHeight}\n`);
    await sleep(800);
    capture("narrow");
    send({ page: "performance", device: "cpu", cpuMode: "overall" });
    await sleep(800);
    capture("narrow-performance");
    child.stdin.write("size 1600 1000\n");
    await sleep(800);
    capture("large-performance");
  }
} finally {
  if (!exited) {
    child.stdin.write("stop\n");
    for (let i = 0; i < 240 && !exited; i++) await sleep(50);
  }
  writeFileSync(
    resolve(output, "events.json"),
    JSON.stringify({ identifier, events, stderr: error, exitCode }, null, 2),
  );
}
if (exitCode !== 0 || error || events.some((e) => e.event === "focusViolation"))
  throw Error(`Native validation failed: ${error}`);
for (const event of events.filter((e) => e.event === "ui-caption" && !e.label.endsWith("-dialog"))) {
  assert(event.dragPoints > 0, `${event.label}: empty title bar lost its native drag region`);
  assert(event.actionsAreClient, `${event.label}: toolbar controls were classified as draggable caption`);
  assert(event.minimize && event.maximize && event.close, `${event.label}: native caption controls were lost`);
}
if (!live) {
  for (const [label, sort, descending] of [["sorted-name", "name", false], ["sorted-name-reversed", "name", true], ["sorted-cpu", "cpu", true]]) {
    const state = events.find((event) => event.event === "ui-render-counts" && event.label === label);
    assert(state && state.sort === sort && state.descending === descending, `${label}: clicking the full header did not sort`);
  }
  const overviews = events.filter((event) => event.event === "ui-layout" && /^(performance|logical-.*|memory|disk-\d+|gpu-\d+|nic-\d+|battery-\d+)$/.test(event.label));
  for (const view of overviews) assert(view.performanceScrollY <= 1, `${view.label}: default-size performance overview scrolls by ${view.performanceScrollY}px`);
  const narrow = events.find((event) => event.event === "ui-layout" && event.label === "narrow-performance");
  assert(narrow?.performanceScrollY > 0, "minimum-size overview must scroll instead of overlapping its content");
}
console.log(`Native validation passed: ${identifier}; foreground violations: 0`);
