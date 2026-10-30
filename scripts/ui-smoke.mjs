// Owned native-window validation. Capture only the test HWND and reject foreground changes.
import { spawn, spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { createInterface } from "node:readline";
import assert from "node:assert/strict";
import { cleanupTestProfile, testDirectories } from "./lib/owned-test.mjs";
const root = resolve(import.meta.dirname, "..");
process.chdir(root);
const label = process.argv[2] ?? `smoke-${Date.now()}`;
if (!/^[a-z0-9-]+$/.test(label)) throw Error("Invalid test label");
const live = process.argv.includes("--live");
const retained = process.argv.includes("--retained");
const snapshots = process.argv.includes("--snapshots");
assert(!(live && retained), "retained comparisons require a fixed fixture");
assert(!(snapshots && (live || retained)), "snapshot comparisons require a fixed fixture");
const output = resolve("target/native-validation", label);
mkdirSync(output, { recursive: true });
const metadata = JSON.parse(readFileSync("resources/app.json"));
const identifier = `${metadata.identifier}.test.${label}`;
const [config] = testDirectories(identifier);
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
const probeFlag = process.argv.indexOf("--probe");
const probe = resolve(probeFlag >= 0 ? process.argv[probeFlag + 1] : "target/release/argus-window-probe.exe");
const executable = resolve(output, "argus.exe");
const executableFlag = process.argv.indexOf("--executable");
copyFileSync(resolve(executableFlag >= 0 ? process.argv[executableFlag + 1] : process.argv.includes("--dev") ? "target/debug/argus.exe" : "target/release/argus.exe"), executable);
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
  { windowsHide: true, stdio: ["pipe", "pipe", "pipe"], env: { ...process.env, ...(retained ? { ARGUS_GPUI_DIAGNOSTICS: "1" } : {}), ...(snapshots ? { ARGUS_GPUI_FRAME_TRACE: "1" } : {}) } },
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
const capture = async (name, keepPointer = false, inspect = true) => {
  if (inspect) {
    send({ pointerOutside: !keepPointer, inspectLayout: name });
    // Pointer dispatch is deferred to a frame. Capture after that paint, not
    // the backbuffer that can still contain the user's previous hover target.
    await sleep(250);
  }
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
    if (process.argv.includes("--exercise-updates")) {
      for (let step=0;step<80;step++) {
        send({page:"processes",query:step%2?"argus":"__argus_snapshot_no_match__",view:step%2?"apps":"tree",sort:step%2?"cpu":"mem"});
        await sleep(25);
      }
      send({query:"__argus_snapshot_no_match__",view:"tree",sort:"mem"});
      await sleep(2500);
      await capture("empty-query");
      send({query:"",view:"apps",sort:"cpu"});
    }
    send({ page: "performance", motion: process.argv.includes("--fluid") ? "fluid" : "balanced" });
    child.stdin.write("watch\n");
    send({ inspectLayout: "live-begin" });
    await sleep(18000);
    await capture("live-performance");
    if (process.argv.includes("--exercise-updates")) {
      send({hoverChart:"cpu-overall-plot"});
      await sleep(1000);
      await capture("hovered-performance",true);
      send({pointerOutside:true});
      child.stdin.write(`size ${metadata.window.minWidth} ${metadata.window.minHeight}\n`);
      await sleep(1800);
      await capture("live-narrow");
      child.stdin.write(`size ${metadata.window.width} ${metadata.window.height}\n`);
      await sleep(1800);
      await capture("live-resized");
    }
    if (process.argv.includes("--move")) {
      child.stdin.write("begin-move\n");
      for (let step = 0; step < 120; step++) {
        child.stdin.write(`move ${step % 2 ? -4 : 4} 0\n`);
        await sleep(50);
      }
      child.stdin.write("end-move\n");
      await sleep(600);
      await capture("moved-performance");
    }
    child.stdin.write("hide\n");
    await sleep(3500);
    child.stdin.write("show\n");
    await sleep(4500);
    await capture("restored-performance");
  } else if (snapshots) {
    const compare = (left, right) => {
      const result = spawnSync(probe, ["compare", resolve(output, `${left}.png`), resolve(output, `${right}.png`)], { windowsHide: true, encoding: "utf8", timeout: 20000 });
      assert.equal(result.status, 0, result.stderr);
      const delta = JSON.parse(result.stdout);
      events.push({ event: "snapshot-comparison", left, right, ...delta });
      return delta;
    };
    for (const [index, page] of ["services", "startup", "network", "settings"].entries()) {
      send({ page, pointerOutside: true });
      await sleep(1800);
      await capture(`${page}-before`, false, false);
      const begin = events.length;
      send({ sample: { set: { "/system/threadCount": 10000 + index, "/system/handleCount": 20000 + index } } });
      await sleep(350);
      await capture(`${page}-partial`, false, false);
      assert(events.slice(begin).some((event) => event.event === "frame-phases" && event.reused >= 3 && !event.rendered.includes("Region")), `${page}: unchanged page was not replayed`);
      send({ fullPaint: true });
      await sleep(250);
      await capture(`${page}-full`, false, false);
      assert(compare(`${page}-partial`, `${page}-full`).maxChannelDelta <= 1, `${page}: partial snapshot paint differs`);
      assert(compare(`${page}-before`, `${page}-partial`).changedPixels > 0, `${page}: status bar did not update`);
    }
    // A settings dependency changes: its body must render with the status bar.
    send({ data: { settings: { ...fixture.settings, kernelSensors: true } }, settingsSection: "data" });
    await sleep(1200);
    const begin = events.length;
    send({ sample: { set: { "/hardware/cpuSensors": { state: "unsupported" } } } });
    await sleep(350);
    await capture("sensor-partial", false, false);
    assert(events.slice(begin).some((event) => event.event === "frame-phases" && event.rendered.includes("Region")), "sensor state did not invalidate settings");
    send({ fullPaint: true });
    await sleep(250);
    await capture("sensor-full", false, false);
    assert(compare("sensor-partial", "sensor-full").maxChannelDelta <= 1, "sensor dependency paint differs");
  } else if (retained) {
    // Light-theme cards are translucent white over the backdrop: composited
    // curve pixels must cover it exactly as a full paint does.
    for (const [name, state] of [
      ["processes", { page: "processes", theme: "dark" }],
      ["overall", { page: "performance", cpuMode: "overall" }],
      ["logical", { page: "performance", cpuMode: "logical" }],
      ["processes-light", { page: "processes", theme: "light" }],
      ["overall-light", { page: "performance", cpuMode: "overall" }],
    ]) {
      send({ ...state, pointerOutside: true });
      await sleep(1500);
      for (const [index, offset] of [50, 200, 700].entries()) {
        // The first retained presentation builds the static prefix; the second reuses it.
        send({ chartTime: fixture.snapshot.timestamp + offset - 1 });
        await sleep(150);
        send({ chartTime: fixture.snapshot.timestamp + offset });
        await sleep(250);
        const before = `${name}-${index}-retained`, after = `${name}-${index}-full`;
        const animated = `${name}-${index}-animated`;
        await capture(animated, false, false);
        // Compare an ordinary parent redraw separately from retained animation.
        send({ inspectLayout: `${before}-reuse` });
        await sleep(250);
        await capture(before, false, false);
        send({ fullPaint: true });
        await sleep(250);
        await capture(after, false, false);
        for (const picture of [animated, before]) {
        const compared = spawnSync(probe, ["compare", resolve(output, `${picture}.png`), resolve(output, `${after}.png`)], {
          windowsHide: true, encoding: "utf8", timeout: 20000,
        });
        assert.equal(compared.status, 0, compared.stderr);
        const difference = JSON.parse(compared.stdout);
        events.push({ event: "pixel-comparison", label: picture, ...difference });
        // A parent redraw replays the same draws: one code value of rounding at
        // most. Animation frames may composite curves in the shader, whose float
        // blend rounds antialiased curve pixels up to two code values away from
        // the fixed-function blend; larger changes indicate a real regression.
        assert(difference.maxChannelDelta <= (picture === animated ? 2 : 1),
          `${picture}: retained rendering differs from a full paint: ${compared.stdout}`);
        }
      }
    }
    const steps = events.filter((event) => event.event === "ui-chart-step");
    assert.equal(steps.length, 30);
    assert(steps.every((event) => event.retained), "comparison did not exercise retained presentation");
    if (!process.env.ARGUS_GPUI_DISABLE_PREFIX_COMPOSITION)
      assert(events.some((event) => event.event === "gpu-scene-cache" && event.composed),
        "comparison did not exercise prefix composition");
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
      await capture(page);
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
      ["process-menu", { rowMenu: 1 }],
      ["logical-cores", { dismiss: true, page: "performance", cpuMode: "logical" }],
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
      ["numbers-wide", { page: "processes", view: "list", navPosition: "top", details: false, fontSize: 13, uiScale: 100 }],
      ["numbers-large", { fontSize: 20 }],
      ["numbers-small", { fontSize: 11 }],
      ["numbers-zoom", { fontSize: 13, uiScale: 125 }],
      ["numbers-zoom-large", { fontSize: 20, uiScale: 150 }],
      ["numbers-reset", { fontSize: 13, uiScale: 100, details: true, view: "apps" }],
    ]) {
      send(state);
      // Fixed snapshots still animate newly placed load bars for one interval.
      // Wait past that interval so cross-build captures sample the same endpoint.
      await sleep(1200);
      await capture(name);
    }
    send({ page: "processes", language: "zh-CN", theme: "light", navPosition: "top" });
    child.stdin.write(`size ${metadata.window.minWidth} ${metadata.window.minHeight}\n`);
    await sleep(800);
    await capture("narrow");
    send({ page: "performance", device: "cpu", cpuMode: "overall" });
    await sleep(800);
    await capture("narrow-performance");
    child.stdin.write("size 1600 1000\n");
    await sleep(800);
    await capture("large-performance");
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
  if (exited) cleanupTestProfile(identifier);
}
if (exitCode !== 0 || error || events.some((e) => e.event === "focusViolation"))
  throw Error(`Native validation failed: ${error}`);
for (const event of events.filter((e) => e.event === "ui-caption" && !e.label.endsWith("-dialog"))) {
  assert(event.dragPoints > 0, `${event.label}: empty title bar lost its native drag region`);
  assert(event.actionsAreClient, `${event.label}: toolbar controls were classified as draggable caption`);
  assert(event.minimize && event.maximize && event.close, `${event.label}: native caption controls were lost`);
}
if (!live && !retained && !snapshots) {
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
