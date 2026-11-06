// Reproducible real-clock or process-local accelerated resource soak.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createSocket } from "node:dgram";
import { copyFileSync, createWriteStream, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { once } from "node:events";

const root = resolve(import.meta.dirname, "..");
process.chdir(root);
const [label, executable] = process.argv.slice(2);
assert(/^[a-z0-9-]+$/.test(label ?? "") && executable,
  "usage: node scripts/leak-native.mjs LABEL EXE [--rate 60 --seconds 60 --rounds 4 --load 600 --observe PID --probe EXE]");
const option = (name, fallback) => {
  const at = process.argv.indexOf(name);
  return at < 0 ? fallback : process.argv[at + 1];
};
const rate = Number(option("--rate", 60));
const seconds = Number(option("--seconds", 60));
const rounds = Number(option("--rounds", 4));
const loadCount = Number(option("--load", 0));
assert(Number.isInteger(rate) && rate >= 1 && rate <= 3600, "invalid clock rate");
assert(Number.isFinite(seconds) && seconds >= 10 && seconds <= 300, "invalid phase duration");
assert(Number.isInteger(rounds) && rounds >= 2 && rounds <= 8, "invalid round count");
assert(Number.isInteger(loadCount) && loadCount >= 0 && loadCount <= 4000, "invalid external load count");
const output = resolve("target/native-validation", label);
assert(!existsSync(join(output, "result.json")), "use a new label to preserve earlier measurements");
mkdirSync(output, { recursive: true });
const helper = resolve("target/leak-tools");
if (rate > 1 || loadCount) assert(existsSync(join(helper, "clock-hook.dll")), "run node scripts/build-leak-tools.mjs first");
const repeat = [
  { app: { page: "processes", details: "open", selectRow: 1 }, waitMs: 700 },
  { app: { page: "performance", deviceKind: "cpu", cpuMode: "logical" }, waitMs: 900 },
  { app: { deviceKind: "gpu" }, waitMs: 700 },
  { app: { deviceKind: "memory" }, waitMs: 500 },
  { app: { deviceKind: "disk" }, waitMs: 500 },
  { app: { deviceKind: "network" }, waitMs: 500 },
  { app: { page: "services" }, waitMs: 400 },
  { app: { page: "network" }, waitMs: 400 },
  { app: { page: "settings", theme: "light" }, waitMs: 500 },
  { app: { page: "processes", theme: "dark", dialog: "columns" }, waitMs: 400 },
  { app: { dismiss: true }, waitMs: 400 },
  { probe: "hide", waitMs: 250 },
  { probe: "show", waitMs: 500 },
  { probe: "mark cycle", waitMs: 100 },
];
const phases = [{ name: "warm", state: { page: "processes" }, seconds: 20 }];
for (let i = 0; i < rounds; i++) phases.push({ name: `churn-${i}`, state: { page: "processes" }, seconds, repeat });
phases.push({ name: "settle", state: { page: "processes", theme: "dark", dismiss: true }, seconds: 30 },
  { name: "hidden", state: {}, hidden: true, seconds: 30 });
const scenario = join(output, "scenario.json");
writeFileSync(scenario, JSON.stringify(phases, null, 2));
const clockLog = join(output, "clock.ndjson");
const observed = option("--observe", "");
assert(!observed || /^\d+(,\d+)*$/.test(observed), "invalid observed PIDs");
const env = { ...process.env, ARGUS_PROBE_OBSERVE_PIDS: observed };
// A rate-one control starts without injecting a DLL.
for (const key of ["ARGUS_PROBE_CLOCK_DLL", "ARGUS_TEST_CLOCK_RATE", "ARGUS_TEST_CLOCK_LOG"]) delete env[key];
if (rate > 1) Object.assign(env, {
  ARGUS_PROBE_CLOCK_DLL: join(helper, "clock-hook.dll"),
  ARGUS_TEST_CLOCK_RATE: String(rate), ARGUS_TEST_CLOCK_LOG: clockLog,
});
const args = ["scripts/perf-native.mjs", label, executable, "--scenario", scenario, "--warmup", "15",
  "--probe", option("--probe", "target/release/argus-window-probe.exe")];
if (!process.argv.includes("--capture")) args.push("--no-capture");
const log = createWriteStream(join(output, "run.log"));
const perf = spawn(process.execPath, args, { env, windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
const completed = once(perf, "exit");
perf.stdout.pipe(log, { end: false });
perf.stderr.pipe(log, { end: false });
createInterface({ input: perf.stdout }).on("line", line => {
  if (line.startsWith("Warmup")) console.log(line);
  else { try { const value = JSON.parse(line); if (value.tag) console.log(JSON.stringify({ phase: value.tag, privateMiB: value.privateMiB, cpuPercent: value.cpuPercent, fps: value.presentedFps })); } catch {} }
});
let stopped = false;
const owned = new Set();
const load = { launched: 0, completed: 0, failures: [], udpBytes: 0, requested: loadCount };
perf.on("exit", () => { stopped = true; for (const child of owned) child.kill(); });
const sleep = ms => new Promise(done => setTimeout(done, ms));
async function externalLoad() {
  if (!loadCount) return;
  const socket = createSocket("udp4");
  socket.on("message", bytes => { load.udpBytes += bytes.length; });
  socket.bind(0, "127.0.0.1");
  await once(socket, "listening");
  const port = socket.address().port;
  try {
    await sleep(20000);
    await Promise.all(Array.from({ length: 8 }, async () => {
      while (!stopped && load.launched < loadCount) {
        const index = load.launched++;
        const directory = join(output, "external-load", String(index));
        mkdirSync(directory, { recursive: true });
        const program = join(directory, "load.exe");
        copyFileSync(join(helper, "load-worker.exe"), program);
        const child = spawn(program, [join(directory, "io.tmp"), String(port)], { windowsHide: true, stdio: "ignore" });
        owned.add(child);
        const timer = setTimeout(() => child.kill(), 10000);
        try {
          const [code] = await once(child, "exit");
          if (code === 0) load.completed++;
          else load.failures.push({ index, code });
        } catch (error) { load.failures.push({ index, error: error.code }); }
        finally { clearTimeout(timer); owned.delete(child); }
      }
    }));
  } finally { socket.close(); }
}
let loadError;
const loading = externalLoad().catch(error => { loadError = error; perf.kill(); });
let exitCode;
try { [exitCode] = await completed; }
finally { stopped = true; for (const child of owned) child.kill(); await loading; log.end(); }
writeFileSync(join(output, "external-load.json"), JSON.stringify(load, null, 2));
if (loadError) throw loadError;
assert.equal(exitCode, 0, `probe run failed; see ${join(output, "run.log")}`);
assert.equal(load.completed, loadCount, "external load did not complete");
assert.equal(load.failures.length, 0, "external load failed");
const result = JSON.parse(readFileSync(join(output, "result.json")));
const pid = result.events.find(event => event.event === "shown")?.pid;
const marks = result.events.filter(event => event.event === "mark").map(event => {
  const process = event.processes.find(process => process.pid === pid);
  const gpu = key => {
    if (event.gpu?.status !== 0 || !event.gpu[key]) return null;
    return Object.entries(event.gpu[key]).filter(([name]) => name.startsWith(`pid_${pid}_`)).reduce((sum, [, value]) => sum + value, 0);
  };
  return { tag: event.tag, realMs: event.atMs, process, gpuDedicated: gpu("gpuDedicated"), gpuShared: gpu("gpuShared"), system: event.system, service: event.observedProcesses };
});
assert(marks.length >= rounds * 2, "resource samples missing");
const identity = marks[0].process?.created;
for (const mark of marks) {
  assert(mark.process?.created === identity, "owned process identity changed");
  for (const key of ["privateBytes", "handles", "gdiObjects", "userObjects"]) {
    assert(Number.isFinite(mark.process[key]), `missing ${key} at ${mark.tag}`);
  }
  for (const pid of observed.split(",").filter(Boolean).map(Number)) {
    assert(mark.service?.some(process => process.pid === pid && Number.isFinite(process.privateBytes)),
      `observed process ${pid} unavailable at ${mark.tag}`);
  }
}
const clock = existsSync(clockLog) ? readFileSync(clockLog, "utf8").trim().split(/\r?\n/).map(line => JSON.parse(line)) : [];
const summary = { rate, load, clock: clock.at(-1) ?? null, hooks: clock.filter(row => row.hook).map(row => row.hook),
  samples: result.events.filter(event => event.event === "ui-layout").map(({ label, sequence, timestamp, processIcons }) => ({ label, sequence, timestamp, processIcons })), marks };
writeFileSync(join(output, "resources.json"), JSON.stringify(summary, null, 2));
if (rate > 1) assert(summary.clock?.qpcCalls > 100 && summary.clock?.wallCalls > 10 && summary.clock?.waitCalls > 10, "clock acceleration coverage missing");
console.log(`Resources: ${join(output, "resources.json")}`);
