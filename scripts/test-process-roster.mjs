// Live, isolated-window check of roster changes while counter replies are delayed.
// The only terminated targets are the idle Node children created by this script.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve, join } from "node:path";
import { createInterface } from "node:readline";
import { cleanupTestProfile, testDirectories } from "./lib/owned-test.mjs";

process.chdir(resolve(import.meta.dirname, ".."));
const [label = "process-roster", input = "target/debug/argus.exe"] = process.argv.slice(2);
assert(/^[a-z0-9-]+$/.test(label));
const output = resolve("target/native-validation", label);
mkdirSync(output, { recursive: true });
const executable = join(output, "argus.exe");
copyFileSync(resolve(input), executable);
const identifier = `dev.argus.taskmanager.test.${label}`;
const [config] = testDirectories(identifier);
mkdirSync(config, { recursive: true });
writeFileSync(join(config, "settings.json"), JSON.stringify({
  language: "zh-CN", theme: "dark", onboarding: 1,
  seenVersion: JSON.parse(readFileSync("resources/app.json")).version,
  history: false, hotkeyEnabled: false, kernelSensors: false,
  updateCheck: false, closeToTray: false,
}));
const probe = resolve("target/release/argus-window-probe.exe");
const child = spawn(probe, ["run", executable, "--hidden", "--test", "--identifier", identifier, "--stop-after", "90"], {
  windowsHide: true, stdio: ["pipe", "pipe", "pipe"],
});
const events = [];
const helpers = [];
let exited = false, exitCode, stderr = "", serial = 0;
child.on("exit", code => { exited = true; exitCode = code; });
child.on("error", error => { exited = true; stderr += error.message; });
child.stderr.on("data", data => { stderr += data; });
createInterface({ input: child.stdout }).on("line", line => {
  try { events.push(JSON.parse(line)); } catch { stderr += `${line}\n`; }
});
const sleep = ms => new Promise(done => setTimeout(done, ms));
const control = value => child.stdin.write(`app ${JSON.stringify(value)}\n`);
async function until(predicate, ms = 20000) {
  const end = performance.now() + ms;
  while (!predicate() && !exited && performance.now() < end) await sleep(20);
  assert(predicate(), `deadline: ${stderr}`);
}
async function inspect(pids = [], extra = {}) {
  const label = `sample-${++serial}`;
  const at = performance.now();
  control({ ...extra, inspectProcessSampling: label, pids });
  await until(() => events.some(e => e.event === "process-sampling" && e.label === label), 3000);
  return { ...events.find(e => e.event === "process-sampling" && e.label === label), responseMs: performance.now() - at };
}
async function observe(pids, predicate, ms = 15000) {
  const end = performance.now() + ms;
  do {
    const value = await inspect(pids);
    if (predicate(value)) return value;
    await sleep(150);
  } while (performance.now() < end);
  throw Error(`sampling condition missed for owned PID(s) ${pids.join(",")}`);
}
function idle() {
  const process = spawn(processExec, ["-e", "setTimeout(() => {}, 60000)"], { windowsHide: true, stdio: "ignore" });
  helpers.push(process);
  return process;
}
const processExec = process.execPath;
function capture(name) {
  const pid = events.find(e => e.event === "shown").pid;
  const result = spawnSync(probe, ["capture", String(pid), join(output, `${name}.png`)], { windowsHide: true, encoding: "utf8", timeout: 20000 });
  assert.equal(result.status, 0, result.stderr);
}
const report = { identifier, events };
try {
  await until(() => events.some(e => e.event === "shown"));
  control({ page: "processes", view: "list", details: false });
  await sleep(2500);
  const initial = await inspect();
  assert(initial.delaySupported, "fault injection requires a development build");
  assert(initial.roster > 0, "basic process information unavailable on this machine");
  const first = idle();
  report.before = await observe([first.pid], e => e.targets.some(p => p.pid === first.pid && !p.pending && p.memory > 0));
  control({ delayProcessQuery: 6000 });
  report.delaying = await observe([first.pid], e => e.delayed && e.targets.some(p => p.reused));
  child.stdin.write("watch\nmark delayed-start\n");
  // Keep the following reply delayed too, so the older full reply is published
  // without immediately being superseded by a new full snapshot.
  control({ delayProcessQuery: 1800 });
  first.kill();
  await new Promise(done => first.once("exit", done));
  const second = idle();
  report.discovered = await observe([first.pid, second.pid], e => !e.targets.some(p=>p.pid===first.pid) && e.targets.some(p=>p.pid===second.pid && p.pending));
  const identity = report.discovered.targets.find(p=>p.pid===second.pid).key;
  control({ query: String(second.pid) });
  await sleep(250);
  control({ selectRow: 0 });
  capture("pending-new-process");
  await sleep(2400);
  report.stillUpdating = await inspect([first.pid, second.pid]);
  assert(report.stillUpdating.seq >= report.delaying.seq + 2, "whole snapshots stopped");
  assert(report.stillUpdating.cpuPoints >= report.delaying.cpuPoints + 2, "system curve stopped");
  assert(!report.stillUpdating.targets.some(p=>p.pid===first.pid), "exited process returned");
  report.recovered = await observe([first.pid, second.pid], e => !e.delayed && e.targets.some(p=>p.pid===second.pid && !p.pending && p.memory > 0));
  assert(!report.recovered.targets.some(p=>p.pid===first.pid), "late counters resurrected an exited process");
  assert.equal(report.recovered.targets.find(p=>p.pid===second.pid).key, identity, "process identity changed as counters arrived");
  assert.equal(report.recovered.selected, identity, "selection changed as counters arrived");
  capture("counters-recovered");
  child.stdin.write("mark recovered\n");
  await until(() => events.some(e=>e.event==="mark" && e.tag==="recovered"));
  second.kill();
  await new Promise(done => second.once("exit", done));
  report.removed = await observe([second.pid], e=>e.targets.length===0);
} catch (error) {
  report.failure = String(error.stack ?? error);
  throw error;
} finally {
  for (const helper of helpers) if (helper.exitCode === null && helper.signalCode === null) helper.kill();
  if (!exited) child.stdin.write("stop\n");
  for (let i=0; i<300 && !exited; i++) await sleep(50);
  if (!exited) child.kill();
  report.exitCode = exitCode; report.stderr = stderr;
  writeFileSync(join(output,"result.json"), JSON.stringify(report,null,2));
  if (exited) cleanupTestProfile(identifier);
}
assert.equal(exitCode,0,stderr);
assert(!events.some(e=>e.event==="focusViolation"));
console.log(JSON.stringify({ output, snapshotsDuringDelay: report.stillUpdating.seq-report.delaying.seq, identityStable:true, foregroundViolations:0 }));
