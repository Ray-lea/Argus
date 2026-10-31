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
const chartScroll = process.argv.includes("--chart-scroll");
const opaqueWindow = process.argv.includes("--opaque-window");
const snapshots = process.argv.includes("--snapshots");
const previewFlag = process.argv.indexOf("--readme");
const preview = previewFlag < 0 ? null : process.argv[previewFlag + 1];
if (previewFlag >= 0) assert(["zh-CN", "en-US"].includes(preview) && !(live || retained || chartScroll || snapshots));
assert(!(chartScroll && (live || retained || snapshots)), "chart scroll checks use a fixed CPU spike");
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
  JSON.stringify({ language: preview ?? "zh-CN", theme: "dark", closeToTray: false, updateCheck: false }),
);
let fixture;
if (preview) {
  const { previewFixture } = await import("./lib/readme-fixture.mjs");
  fixture = previewFixture(preview);
  writeFileSync(resolve(output, "fixture.json"), JSON.stringify(fixture));
} else if (!live) {
  const fixtureFlag = process.argv.indexOf("--fixture");
  fixture = JSON.parse(readFileSync(fixtureFlag >= 0 ? process.argv[fixtureFlag + 1] : "tools/window-probe/fixtures/desktop.json"));
  if (chartScroll) {
    fixture.snapshot.timestamp = Date.now();
    fixture.snapshot.system.cpu = 12;
    fixture.snapshot.system.cpuKernel = 4;
  }
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
    if (chartScroll) frame.system.cpu = i === Math.floor(samples * 0.42) ? 89 : 12;
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
    ...(opaqueWindow ? ["--opaque-window"] : []),
    ...(live ? [] : ["--fixture", resolve(output, "fixture.json")]),
    ...(preview ? ["--width", "1440", "--height", "900"] : []),
  ],
  { windowsHide: true, stdio: ["pipe", "pipe", "pipe"], env: { ...process.env, ...(retained || chartScroll ? { ARGUS_GPUI_DIAGNOSTICS: "1" } : {}), ...(snapshots ? { ARGUS_GPUI_FRAME_TRACE: "1" } : {}) } },
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
const resizeLogical = (width, height) => {
  // The probe accepts physical pixels; manifest window bounds are logical.
  const scale = events.findLast((event) => event.event === "ui-conditions")?.dpiScale;
  assert(Number.isFinite(scale) && scale > 0, "window scale missing before resize");
  child.stdin.write(`size ${Math.ceil(width * scale)} ${Math.ceil(height * scale)}\n`);
};
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
  if (preview) {
    send({ page: "processes", view: "apps", navPosition: "top", details: true, sort: "name" });
    for (const theme of ["light", "dark"]) {
      send({ theme, pointerOutside: true });
      await sleep(1200);
      await capture(theme);
      const state = events.findLast(e => e.event === "ui-render-counts" && e.label === theme);
      assert(state?.sort === "name" && state.descending === false, "README preview must use ascending name order");
      const conditions = events.findLast(e => e.event === "ui-conditions" && e.label === theme);
      assert.equal(conditions?.processes, fixture.snapshot.processes.length, "README preview fixture missing");
    }
  } else if (live) {
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
      resizeLogical(metadata.window.minWidth, metadata.window.minHeight);
      await sleep(1800);
      await capture("live-narrow");
      resizeLogical(metadata.window.width, metadata.window.height);
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
  } else if (chartScroll) {
    const widthFlag = process.argv.indexOf("--width"), heightFlag = process.argv.indexOf("--height");
    if (widthFlag >= 0 || heightFlag >= 0) {
      const width = Number(process.argv[widthFlag + 1]), height = Number(process.argv[heightFlag + 1]);
      assert(widthFlag >= 0 && heightFlag >= 0 && Number.isInteger(width) && width > 0 && Number.isInteger(height) && height > 0, "invalid chart test dimensions");
      child.stdin.write(`size ${width} ${height}\n`);
      for (let i = 0; i < 100 && !events.some(e => e.event === "resized" && e.width === width && e.height === height); i++) await sleep(50);
      assert(events.some(e => e.event === "resized" && e.width === width && e.height === height), "chart test dimensions unavailable");
    }
    const scaleFlag = process.argv.indexOf("--ui-scale");
    const uiScale = scaleFlag >= 0 ? Number(process.argv[scaleFlag + 1]) : 100;
    assert(Number.isInteger(uiScale) && uiScale >= 80 && uiScale <= 150, "invalid chart test UI scale");
    send({page: "performance", device: "cpu", cpuMode: "overall", theme: "dark", motion: "fluid", uiMotion: false, uiScale});
    await sleep(1500);
    send({pointerOutside: true, inspectLayout: "scroll-plot"});
    await sleep(300);
    const layout = events.findLast(e => e.event === "ui-layout" && e.label === "scroll-plot");
    assert(layout?.cpuPlot, "missing CPU plot bounds");
    const [x, y, w, h] = layout.cpuPlot.map(v => v * layout.scaleFactor);
    const crop = [x + w * 0.04, y + h * 0.35, w * 0.49, Math.min(12, h * 0.025)].map(Math.round);
    let seq = 0;
    const append = () => {
      seq++;
      send({sample: {at: 0, set: {"/system/cpu": 12 + seq % 8, "/system/threadCount": 13000 + seq}}});
    };
    append();
    await sleep(800);
    send({pointerOutside: true});
    await sleep(300);
    for (const phase of ["samples", "repaints"]) {
      const start = events.length;
      const dir = `target/native-validation/${label}/${phase}`;
      child.stdin.write(`record ${dir} 6000 ${crop.join(" ")}\n`);
      for (let tick = 0; tick < 12; tick++) {
        if (tick % 2 === 0) append();
        else if (phase === "repaints") send({fullPaint: true});
        await sleep(500);
      }
      for (let i = 0; i < 150 && !events.slice(start).some(e => e.event === "recorded"); i++) await sleep(100);
      const recording = events.slice(start).find(e => e.event === "recorded");
      assert(recording && !recording.error && recording.frames > 50, JSON.stringify(recording));
      const result = spawnSync(probe, ["track-chart", resolve(dir)], {windowsHide: true, encoding: "utf8", timeout: 30000});
      assert.equal(result.status, 0, result.stderr);
      const track = JSON.parse(result.stdout);
      events.push({event: "chart-motion-check", phase, ...track});
      assert(track.tracked >= track.frames * 0.95 && track.travelPx > w * 0.04, `CPU spike did not traverse the recording: ${result.stdout}`);
      assert(track.maxReversePx <= 0.75, `CPU chart moved backwards: ${result.stdout}`);
      if (process.argv.includes("--require-chart-layers")) {
        assert(events.slice(start).some(e => e.event === "gpu-chart-handoff"), `${phase}: no chart handoff observed`);
      }
    }
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
    let expectedSteps = 0;
    // Light-theme cards are translucent white over the backdrop: composited
    // curve pixels must cover it exactly as a full paint does.
    for (const [name, state] of [
      ["processes", { page: "processes", theme: "dark" }],
      ["overall", { page: "performance", cpuMode: "overall" }],
      ["logical", { page: "performance", cpuMode: "logical" }],
      ["processes-light", { page: "processes", theme: "light" }],
      ["overall-light", { page: "performance", cpuMode: "overall" }],
    ]) {
      send({ ...state, uiMotion: false, pointerOutside: true });
      await sleep(1500);
      send({ pointerOutside: true });
      await sleep(250);
      const settled = events.length;
      for (const [index, offset] of [50, 200, 700].entries()) {
        if (name.startsWith("overall")) {
          // Give the asynchronous backdrop scan its own presentation before measuring.
          send({ chartTime: fixture.snapshot.timestamp + offset - 2 });
          expectedSteps++;
          await sleep(150);
        }
        expectedSteps += 2;
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
      if (process.argv.includes("--require-chart-layers")) {
        const layers = events.slice(settled).filter((event) => event.event === "gpu-chart-layers" && event.images > 0);
        if (name.startsWith("overall")) {
          if (process.env.ARGUS_GPUI_DISABLE_CHART_BACKDROP_CACHE) {
            assert.equal(layers.length, 0, `${name}: expected the in-scene control`);
          } else {
            assert(layers.some((event) => event.opaqueStrips > 0), `${name}: the opaque chart cache was not presented`);
            assert(events.slice(settled).some(e => e.event === "gpu-chart-handoff" && layers.some(l => l.revision === e.revision)),
              `${name}: a cached chart did not hand off to a complete frame on its composition root`);
          }
        } else if (!process.env.ARGUS_GPUI_DISABLE_CHART_BACKDROP_CACHE || name.startsWith("processes")) {
          assert(layers.length > 0, `${name}: chart visuals were not presented`);
        }
      }
    }
    const steps = events.filter((event) => event.event === "ui-chart-step");
    assert.equal(steps.length, expectedSteps);
    assert(steps.every((event) => event.retained), "comparison did not exercise retained presentation");
    const chartLayers = events.some((event) => event.event === "gpu-chart-layers" && event.images > 0);
    if (process.env.ARGUS_GPUI_DISABLE_CHART_LAYERS) assert(!chartLayers, "disabled chart visuals were presented");
    if (!process.env.ARGUS_GPUI_DISABLE_PREFIX_COMPOSITION)
      assert(chartLayers || events.some((event) => event.event === "gpu-scene-cache" && event.composed),
        "comparison did not exercise a retained composition path");
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
    resizeLogical(metadata.window.minWidth, metadata.window.minHeight);
    await sleep(800);
    await capture("narrow");
    send({ page: "performance", device: "cpu", cpuMode: "overall" });
    await sleep(800);
    await capture("narrow-performance");
    resizeLogical(1600, 1000);
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
if (!live && !retained && !snapshots && !chartScroll && !preview) {
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
