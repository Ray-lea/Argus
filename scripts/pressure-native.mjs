// CPU/GPU load, a small commit budget, and recovery for an owned Argus window.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { availableParallelism } from "node:os";
import { createHash } from "node:crypto";
import { cleanupTestProfile, testDirectories } from "./lib/owned-test.mjs";
import { summarizeFrames } from "./perf-frames.mjs";

process.chdir(resolve(import.meta.dirname, ".."));
const [label, input] = process.argv.slice(2);
assert(
  /^[a-z0-9-]+$/.test(label ?? "") && input,
  "usage: node scripts/pressure-native.mjs LABEL EXE [--probe EXE --seconds 20]",
);
const option = (name, fallback) => {
  const i = process.argv.indexOf(name);
  return i < 0 ? fallback : process.argv[i + 1];
};
const seconds = Number(option("--seconds", 20));
const headroom = Number(option("--headroom-mib", 48));
const rounds = Number(option("--rounds", 3));
const observed = option("--observe", "").split(",").filter(Boolean).map(Number);
assert(
  observed.every((pid) => Number.isInteger(pid) && pid > 0),
  "invalid observed PID",
);
assert(Number.isInteger(seconds) && seconds >= 5 && seconds <= 40);
assert(Number.isInteger(headroom) && headroom >= 16 && headroom <= 256);
assert(Number.isInteger(rounds) && rounds >= 1 && rounds <= 4);
const output = resolve("target/native-validation", label);
assert(!existsSync(join(output, "result.json")), "use a new label to preserve earlier measurements");
mkdirSync(output, { recursive: true });
const identifier = `dev.argus.taskmanager.test.${label}`;
const [config] = testDirectories(identifier);
mkdirSync(config, { recursive: true });
writeFileSync(
  join(config, "settings.json"),
  JSON.stringify({
    language: "zh-CN",
    theme: "dark",
    onboarding: 1,
    seenVersion: JSON.parse(readFileSync("resources/app.json")).version,
    history: false,
    hotkeyEnabled: false,
    kernelSensors: false,
    updateCheck: false,
    closeToTray: false,
  }),
);
const executable = join(output, "argus.exe");
copyFileSync(resolve(input), executable);
const probe = resolve(option("--probe", "target/debug/argus-window-probe.exe"));
const loader = resolve("target/leak-tools/pressure-worker.exe");
assert(existsSync(loader), "build the load helper: node scripts/build-leak-tools.mjs");
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
    String(seconds * (4 + rounds) + 150),
  ],
  {
    windowsHide: true,
    stdio: ["pipe", "pipe", "pipe"],
    env: { ...process.env, ARGUS_PROBE_HIGH_PRIORITY: "1" },
  },
);
const report = {
  identifier,
  seconds,
  rounds,
  observed,
  sha256: createHash("sha256").update(readFileSync(executable)).digest("hex"),
  events: [],
  phases: [],
  load: [],
};
let exited = false,
  code,
  stderr = "";
child.on("exit", (value) => {
  exited = true;
  code = value;
});
child.on("error", (error) => {
  exited = true;
  stderr += error.message;
});
child.stderr.on("data", (data) => {
  stderr += data;
});
createInterface({ input: child.stdout }).on("line", (line) => {
  try {
    report.events.push(JSON.parse(line));
  } catch {
    stderr += `${line}\n`;
  }
});
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
const send = (text) => child.stdin.write(`${text}\n`);
const app = (value) => send(`app ${JSON.stringify(value)}`);
async function waitFor(predicate, ms = 30000) {
  const until = performance.now() + ms;
  while (!predicate() && !exited && performance.now() < until) await sleep(10);
  assert(predicate(), `probe response missing: ${stderr}`);
}
async function mark(tag) {
  send(`mark ${tag}`);
  await waitFor(() => report.events.some((e) => e.event === "mark" && e.tag === tag));
  return report.events.find((e) => e.event === "mark" && e.tag === tag);
}
async function inspect(label, control = {}) {
  const began = performance.now();
  app({ ...control, inspectLayout: label });
  await waitFor(() => report.events.some((e) => e.event === "ui-layout" && e.label === label), 15000);
  return {
    latencyMs: performance.now() - began,
    ...report.events.find((e) => e.event === "ui-layout" && e.label === label),
  };
}
let activeLoad, activeService;
async function serviceReads(executable, phase) {
  report.serviceReads ??= [];
  for (let round = 0; round < 4; round++) {
    const child = spawn(
      resolve(executable),
      [
        "--exact",
        "pipe::tests::resource_soak_running_service_reads",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
      ],
      { windowsHide: true, stdio: ["ignore", "pipe", "pipe"] },
    );
    activeService = child;
    const record = { phase, round, startedMs: performance.now(), stdout: "", stderr: "" };
    report.serviceReads.push(record);
    child.stdout.on("data", (data) => {
      record.stdout += data;
    });
    child.stderr.on("data", (data) => {
      record.stderr += data;
    });
    const timeout = setTimeout(() => child.kill(), 60000);
    record.code = await new Promise((done) => {
      child.once("exit", done);
      child.once("error", (error) => {
        record.stderr += error.message;
        done(-1);
      });
    });
    clearTimeout(timeout);
    activeService = null;
    record.endedMs = performance.now();
    assert.equal(record.code, 0, "running service read stress failed");
    assert(record.stdout.includes("1 passed"), "service read test was not selected");
    const values = record.stdout.split(/\r?\n/).flatMap((line) => {
      const start = line.indexOf("{");
      if (start < 0) return [];
      try {
        return [JSON.parse(line.slice(start))];
      } catch {
        return [];
      }
    });
    const last = values.findLast((value) => value.batch === 8);
    const noData = values.findLast((value) => Array.isArray(value.sensorNoDataMs));
    assert(last && Object.keys(last.requestErrors).length === 0, "service transport errors under load");
    assert(noData && noData.sensorNoDataMs.length === 0, "service returned NoData under load");
  }
}
async function phase(name, loadArgs, limited = false) {
  app({ page: "processes", motion: "fluid" });
  await sleep(1000);
  let loadDone, loadCode;
  if (loadArgs) {
    const process = spawn(loader, [String(Math.min(60, seconds + 40)), ...loadArgs.map(String)], {
      windowsHide: true,
      stdio: ["pipe", "pipe", "pipe"],
    });
    activeLoad = process;
    loadDone = new Promise((done) => {
      process.once("exit", (code) => {
        loadCode = code;
        done();
      });
      process.once("error", (error) => {
        stderr += error.message;
        loadCode = -1;
        done();
      });
    });
    process.stderr.on("data", (data) => {
      stderr += data;
    });
    createInterface({ input: process.stdout }).on("line", (line) => {
      try {
        report.load.push({ phase: name, receivedMs: performance.now(), ...JSON.parse(line) });
      } catch {
        stderr += line;
      }
    });
    send(`observe ${[...observed, process.pid].join(",")}`);
    await sleep(1500);
  } else send(`observe ${observed.join(",")}`);
  const beforeMs = performance.now();
  const before = await mark(`${name}-begin`);
  const shown = report.events.find((e) => e.event === "shown");
  let limitMiB;
  if (limited) {
    const bytes = before.processes.reduce((total, p) => total + (p.privateBytes ?? 0), 0);
    limitMiB = Math.max(128, Math.ceil(bytes / 1048576) + headroom);
    send(`commit-limit ${limitMiB}`);
    await waitFor(() =>
      report.events.some((e) => e.event === "commit-limit" && e.bytes === limitMiB * 1048576),
    );
  }
  const serviceTest = limited && option("--service-test", null);
  let serviceError;
  const serviceDone =
    serviceTest &&
    serviceReads(serviceTest, name).catch((error) => {
      serviceError = error;
    });
  const interactions = [];
  const until = performance.now() + seconds * 1000;
  let index = 0;
  const states = [
    { page: "processes", query: "argus" },
    { query: "", page: "performance", deviceKind: "cpu", cpuMode: "logical" },
    { deviceKind: "gpu" },
    { page: "settings" },
    { page: "processes" },
  ];
  while (performance.now() < until) {
    interactions.push(await inspect(`${name}-${index}`, states[index % states.length]));
    index++;
    await sleep(400);
  }
  if (serviceDone) {
    await serviceDone;
    if (serviceError) throw serviceError;
  }
  if (loadDone) assert.equal(loadCode, undefined, "external load ended before the measurement interval");
  if (limited) {
    interactions.push(
      await inspect(`${name}-capture`, { page: "performance", deviceKind: "cpu", cpuMode: "logical" }),
    );
    await sleep(500);
    const capture = spawnSync(probe, ["capture", String(shown.pid), join(output, `${name}.png`)], {
      windowsHide: true,
      encoding: "utf8",
      timeout: 20000,
    });
    assert.equal(capture.status, 0, capture.stderr);
  }
  const after = await mark(`${name}-end`);
  const afterMs = performance.now();
  if (loadDone) assert.equal(loadCode, undefined, "external load ended during the final capture");
  if (limited) send("commit-limit 2048");
  if (loadDone) {
    activeLoad.stdin.end("stop\n");
    await loadDone;
    activeLoad = null;
    assert.equal(loadCode, 0, "external load failed or exceeded its time budget");
    if (loadArgs[2]) {
      assert(
        report.load.some((item) => item.phase === name && item.gpuAvailable === true),
        "CUDA load unavailable; GPU pressure was not exercised",
      );
      const result = report.load.find((item) => item.phase === name && "gpuError" in item);
      assert(result && result.gpuError === 0 && result.maxGpuBatchMs <= 500, "GPU load stopped early");
    }
  }
  const latency = interactions.map((i) => i.latencyMs).sort((a, b) => a - b);
  const process = after.processes.find((p) => p.pid === shown.pid);
  const gpu = (key) =>
    Object.entries(after.gpu?.[key] ?? {})
      .filter(([name]) => name.startsWith(`pid_${shown.pid}_`))
      .reduce((n, [, v]) => n + v, 0) / 1048576;
  const summary = {
    name,
    limitMiB,
    beforeMs,
    afterMs,
    interactions: interactions.length,
    p95UiMs: latency[Math.floor(latency.length * 0.95)],
    maxUiMs: latency.at(-1),
    sampleAdvance: interactions.at(-1).sequence - interactions[0].sequence,
    privateMiB: process.privateBytes / 1048576,
    dedicatedMiB: gpu("gpuDedicated"),
    handles: process.handles,
    gdi: process.gdiObjects,
    ...summarizeFrames(report.events, before.qpc100ns, after.qpc100ns, true),
    before,
    after,
  };
  report.phases.push(summary);
  writeFileSync(join(output, "result.json"), JSON.stringify(report, null, 2));
  console.log(
    JSON.stringify({
      name,
      limitMiB,
      p95UiMs: summary.p95UiMs,
      maxUiMs: summary.maxUiMs,
      sampleAdvance: summary.sampleAdvance,
      privateMiB: summary.privateMiB,
    }),
  );
  assert(summary.maxUiMs < 2000, "owned UI stalled under pressure");
  assert(summary.sampleAdvance >= Math.floor(seconds / 3), "sampling stopped under pressure");
}
try {
  await waitFor(() => report.events.some((e) => e.event === "shown"));
  send("size 1240 800");
  send("watch");
  app({ page: "performance", deviceKind: "cpu", cpuMode: "logical", motion: "fluid" });
  await sleep(12000);
  await phase("baseline", null);
  await phase("cpu", [Math.max(1, availableParallelism() - 2), 512, 0, 0]);
  await phase("gpu", [0, 512, 100, 512]);
  for (let round = 1; round <= rounds; round++)
    await phase(`combined-${round}`, [Math.max(1, availableParallelism() - 2), 2048, 100, 1024], true);
  const beforeStall = await inspect("stall-before");
  send("stall-ui 5000");
  await waitFor(() => report.events.some((e) => e.event === "ui-stall"));
  const afterStall = await inspect("stall-after");
  report.uiStall = {
    requestedMs: 5000,
    resumeLatencyMs: afterStall.latencyMs,
    sampleAdvance: afterStall.sequence - beforeStall.sequence,
  };
  assert(afterStall.latencyMs < 1500, "UI did not recover after a scheduling stall");
  await phase("recovery", null);
  const pid = report.events.find((e) => e.event === "shown").pid;
  const capture = spawnSync(probe, ["capture", String(pid), join(output, "recovered.png")], {
    windowsHide: true,
    encoding: "utf8",
    timeout: 20000,
  });
  assert.equal(capture.status, 0, capture.stderr);
} finally {
  activeLoad?.kill();
  activeService?.kill();
  if (!exited) {
    send("unwatch");
    send("stop");
  }
  for (let i = 0; i < 400 && !exited; i++) await sleep(50);
  if (!exited) child.kill();
  report.exitCode = code;
  report.stderr = stderr;
  writeFileSync(join(output, "result.json"), JSON.stringify(report, null, 2));
  if (exited) cleanupTestProfile(identifier);
}
assert.equal(code, 0, stderr);
assert(!report.events.some((e) => e.event === "focusViolation"));
console.log(`Pressure validation: ${output}`);
