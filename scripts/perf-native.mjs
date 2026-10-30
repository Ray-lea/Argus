// Measure only an owned, isolated window; the probe rejects foreground activation.
import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync, copyFileSync, existsSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { createInterface } from "node:readline";
import { createHash } from "node:crypto";
import { availableParallelism } from "node:os";
import assert from "node:assert/strict";
import { summarizeFrames } from "./perf-frames.mjs";
import { cleanupTestProfile, testDirectories } from "./lib/owned-test.mjs";

const root = resolve(import.meta.dirname, "..");
process.chdir(root);
const [label, input] = process.argv.slice(2);
assert(/^[a-z0-9-]+$/.test(label ?? ""), "test label required");
const flag = (name, fallback) => {
  const index = process.argv.indexOf(name);
  return index < 0 ? fallback : process.argv[index + 1];
};
const output = resolve("target/native-validation", label);
mkdirSync(output, { recursive: true });
const executable = resolve(output, "argus.exe");
const sourceExecutable = resolve(input ?? "target/release/argus.exe");
copyFileSync(sourceExecutable, executable);
if (process.env.ARGUS_PROTOTYPE_STACK_FILE && existsSync(resolve(dirname(sourceExecutable), "argus.pdb")))
  copyFileSync(resolve(dirname(sourceExecutable), "argus.pdb"), resolve(output, "argus.pdb"));
const identifier = flag("--identifier", `dev.argus.taskmanager.test.${label}`);
const [config] = testDirectories(identifier);
mkdirSync(config, { recursive: true });
writeFileSync(
  resolve(config, "settings.json"),
  JSON.stringify({
    language: "zh-CN",
    theme: "dark",
    onboarding: 1,
    closeToTray: false,
    history: false,
    hotkeyEnabled: false,
    updateCheck: false,
  }),
);
const probe = resolve(flag("--probe", "target/release/argus-window-probe.exe"));
const seconds = Number(flag("--seconds", 20));
const warmup = Number(flag("--warmup", 25));
const captureCycles = Number(flag("--capture-cycles", 0));
assert(Number.isInteger(captureCycles) && captureCycles >= 0 && captureCycles <= 100, "invalid capture cycle count");
const scenario = flag("--scenario", null);
const fixture = flag("--fixture", null);
const phases = scenario ? JSON.parse(readFileSync(resolve(scenario), "utf8")) : flag(
  "--phases",
  "processes:fluid,processes:off,performance:fluid,logical:fluid,settings:off,hidden:off",
).split(",").map((phase) => {
  const [page, motion] = phase.split(":");
  return { name: `${page}-${motion}`, hidden: page === "hidden", state: {
    page: page === "logical" ? "performance" : page,
    cpuMode: page === "logical" ? "logical" : "overall", motion,
  }};
});
assert(Array.isArray(phases) && phases.length > 0, "measurement phases required");
assert(seconds > 0 && warmup >= 0 && Number.isFinite(seconds + warmup), "invalid measurement duration");
assert(phases.every((phase) => /^[a-z0-9-]+$/.test(phase.name)), "invalid phase name");
const durationOf = (phase) => phase.seconds ?? seconds;
assert(phases.every((phase) => Number.isFinite(durationOf(phase)) && durationOf(phase) > 0), "invalid phase duration");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const result = {
  measurementVersion: 3,
  label,
  identifier,
  renderer: "gpui",
  sha256: createHash("sha256").update(readFileSync(executable)).digest("hex"),
  logicalProcessors: availableParallelism(),
  seconds,
  captureCycles,
  phases,
  fixture: fixture ? createHash("sha256").update(readFileSync(fixture)).digest("hex") : null,
  instrumentation: {
    frameTrace: !!process.env.ARGUS_GPUI_FRAME_TRACE,
    viewTrace: !!process.env.ARGUS_GPUI_VIEW_TRACE,
    prefixCompositionDisabled: !!process.env.ARGUS_GPUI_DISABLE_PREFIX_COMPOSITION,
    performanceCores: !!process.env.ARGUS_PROBE_PERFORMANCE_CORES,
  },
  events: [],
  summaries: [],
  invalidStructuredLines: 0,
};
const child = spawn(
  probe,
  [
    "run",
    executable,
    "--hidden",
    "--test", "--identifier", identifier, "--stop-after", String(Math.ceil(warmup + phases.reduce((total, phase) => total + durationOf(phase) + 10, 0) + 120)),
    ...(fixture ? ["--fixture", resolve(fixture)] : []),
  ],
  {
    windowsHide: true,
    stdio: ["pipe", "pipe", "pipe"],
    env: process.env,
  },
);
let shown,
  exited = false,
  stderr = "";
child.on("error", (error) => {
  stderr += error.message;
  exited = true;
});
child.on("exit", (code) => {
  result.exitCode = code;
  exited = true;
});
child.stderr.on("data", (bytes) => {
  stderr += bytes;
});
createInterface({ input: child.stdout }).on("line", (line) => {
  try {
    const event = JSON.parse(line);
    result.events.push(event);
    if (event.event === "shown") shown = event;
  } catch {
    if (line.trimStart().startsWith("{")) result.invalidStructuredLines++;
    stderr += line;
  }
});
const send = (command) => child.stdin.write(`${command}\n`);
const app = (value) => send(`app ${JSON.stringify(value)}`);
async function stopWatching() {
  const previous = result.events.filter((event) => event.event === "unwatched").length;
  send("unwatch");
  for (let i = 0; i < 200 && !exited; i++) {
    if (result.events.filter((event) => event.event === "unwatched").length > previous) return true;
    await sleep(50);
  }
  return false;
}
async function mark(tag) {
  send(`mark ${tag}`);
  for (let i = 0; i < 200 && !exited; i++) {
    const mark = result.events.find((event) => event.event === "mark" && event.tag === tag);
    if (mark) return mark;
    await sleep(50);
  }
  throw Error(`Missing mark ${tag}: ${stderr}`);
}
const sum = (rows, field) => rows.reduce((total, row) => total + row[field], 0);
function summarize(tag, before, after) {
  const ms = (after.qpc100ns - before.qpc100ns) / 10000;
  const uiBefore = before.threads?.find((t) => t.pid === shown.pid && t.tid === shown.uiThread);
  const uiAfter = after.threads?.find((t) => t.pid === shown.pid && t.tid === shown.uiThread && t.created === uiBefore?.created);
  const frames = summarizeFrames(result.events, before.qpc100ns, after.qpc100ns);
  const systemTotal = after.system.kernel + after.system.user - before.system.kernel - before.system.user;
  const affinity = result.events.find((event) => event.event === "started")?.performanceCoreAffinity;
  const mask = affinity ? BigInt(affinity) : null;
  const frequencies = result.events.filter((event) => event.atMs >= before.atMs && event.atMs <= after.atMs)
    .flatMap((event) => Object.entries(event.gpu?.cpuMhz ?? {}))
    .filter(([key, value]) => /^0,\d+$/.test(key) && Number.isFinite(value) && value > 0 &&
      (mask === null || (mask & (1n << BigInt(key.split(",")[1]))) !== 0n))
    .map(([, value]) => value);
  const threads = (after.threads ?? []).map((thread) => {
    const prior = before.threads?.find((old) => old.pid === thread.pid && old.tid === thread.tid && old.created === thread.created);
    return { name: thread.name, tid: thread.tid, cpuMsPerSecond: prior ? (thread.cpuMs - prior.cpuMs) / ms * 1000 : 0 };
  }).sort((a, b) => b.cpuMsPerSecond - a.cpuMsPerSecond).slice(0, 8);
  const engines = {};
  for (const [key, value] of Object.entries(after.gpu?.gpuTime100ns ?? {})) {
    const engine = key.replace(/^pid_\d+_/, "");
    const delta = Math.max(0, value - (before.gpu?.gpuTime100ns?.[key] ?? value));
    engines[engine] = (engines[engine] ?? 0) + (delta / (ms * 10000)) * 100;
  }
  return {
    tag,
    durationMs: ms,
    systemBusyPercent: systemTotal > 0 ? (1 - (after.system.idle - before.system.idle) / systemTotal) * 100 : null,
    processorMhz: frequencies.length ? frequencies.reduce((total, value) => total + value, 0) / frequencies.length : null,
    topThreads: threads,
    uiThreadCpuMsPerSecond: uiBefore && uiAfter ? ((uiAfter.cpuMs-uiBefore.cpuMs)/ms)*1000 : null,
    cpuMsPerSecond: ((sum(after.processes, "cpuMs") - sum(before.processes, "cpuMs")) / ms) * 1000,
    cpuPercent:
      (((sum(after.processes, "cpuMs") - sum(before.processes, "cpuMs")) / ms) * 100) /
      result.logicalProcessors,
    gpuBusiestEnginePercent: Math.max(0, ...Object.values(engines)),
    gpuEngines: engines,
    privateMiB: sum(after.processes, "privateBytes") / 1048576,
    workingSetMiB: sum(after.processes, "workingSet") / 1048576,
    ...frames,
  };
}
try {
  for (let i = 0; i < 1200 && !shown && !exited; i++) await sleep(50);
  assert(shown, `Window unavailable: ${stderr}`);
  send("size 1240 800");
  for (let i = 0; i < 200 && !result.events.some((event) => event.event === "resized") && !exited; i++)
    await sleep(50);
  result.client = result.events.findLast((event) => event.event === "resized");
  assert(result.client?.width === 1240 && result.client?.height === 800, "client size mismatch");
  send("watch");
  app({page: "processes", motion: "fluid"});
  console.log(`Warmup ${warmup}s: ${label}`);
  await sleep(warmup * 1000);
  for (let cycle = 0; cycle < captureCycles; cycle++) {
    assert(await stopWatching(), "capture cycle teardown failed");
    send("watch");
    await sleep(150);
  }
  for (const [index, phase] of phases.entries()) {
    if (phase.hidden) send("hide");
    else {
      if (index > 0 && phases[index - 1].hidden) send("show");
      app(phase.state);
    }
    const tag = `${index}-${phase.name}`;
    app({ pointerOutside: true, inspectLayout: `${tag}-begin` });
    // Inspection invalidates the test UI. Let it settle before the timing mark.
    await sleep(2500);
    {
      const layout = result.events.findLast((event) => event.event === "ui-layout" && event.label === `${tag}-begin`);
      const conditions = result.events.findLast((event) => event.event === "ui-conditions" && event.label === `${tag}-begin`);
      assert(layout && conditions, "native viewport inspection missing");
      assert.equal(layout.width * conditions.dpiScale, 1240, "native physical width mismatch");
      assert.equal(layout.height * conditions.dpiScale, 800, "native physical height mismatch");
    }
    const before = await mark(`${tag}-begin`);
    if (phase.repeat) {
      const until = performance.now() + durationOf(phase) * 1000;
      let step = 0;
      while (performance.now() < until) {
        const control = phase.repeat[step++ % phase.repeat.length];
        if (control.app) app(control.app);
        if (control.probe) send(control.probe);
        await sleep(Math.min(control.waitMs ?? 100, Math.max(0, until - performance.now())));
      }
    } else await sleep(durationOf(phase) * 1000);
    const after = await mark(`${tag}-end`);
    app({ inspectLayout: `${tag}-end` });
    const summary = summarize(tag, before, after);
    result.summaries.push(summary);
    writeFileSync(resolve(output, "result.json"), JSON.stringify(result, null, 2));
    console.log(JSON.stringify(summary));
    if (!phase.hidden && process.argv.includes("--capture")) {
      const shot = spawnSync(probe, ["capture", String(shown.pid), resolve(output, `${tag}.png`)], {
        windowsHide: true,
        encoding: "utf8",
        timeout: 20000,
      });
      assert.equal(shot.status, 0, shot.stderr);
    }
  }
} finally {
  writeFileSync(resolve(output, "result.json"), JSON.stringify(result, null, 2));
  if (!exited) {
    result.captureClosedBeforeExit = await stopWatching();
    app({ exit: true });
    send("stop");
  }
  for (let i = 0; i < 240 && !exited; i++) await sleep(50);
  child.stdin.end();
  result.stderr = stderr;
  if (exited) cleanupTestProfile(identifier);
  writeFileSync(resolve(output, "result.json"), JSON.stringify(result, null, 2));
}
assert.equal(result.exitCode, 0, stderr);
assert.equal(result.invalidStructuredLines, 0, "measurement stream contained corrupted JSON lines");
assert(result.captureClosedBeforeExit, "capture teardown did not complete before window shutdown");
assert(!result.events.some((event) => event.event === "focusViolation"), "focus isolation failed");
assert(
  result.events.filter((event) => event.capture).every((event) => !event.capture.error),
  "frame observer failed",
);
