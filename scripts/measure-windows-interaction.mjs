// The user starts this bounded benchmark explicitly. It never elevates itself.
// File commands can only open the two fixed tools and exercise their own UI.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { cleanupTestProfile, testDirectories } from "./lib/owned-test.mjs";

const root = resolve(import.meta.dirname, "..");
process.chdir(root);
const [label, permission] = process.argv.slice(2);
assert(/^[a-z0-9-]+$/.test(label) && permission === "--interactive-desktop",
  "Usage: node scripts/measure-windows-interaction.mjs <label> --interactive-desktop");
const output = resolve("target/interaction-validation", label);
mkdirSync(output, { recursive: true });
function writeJson(path, value) {
  writeFileSync(`${path}.tmp`, JSON.stringify(value, null, 2));
  renameSync(`${path}.tmp`, path);
}
const helper = resolve("target/interaction-tools/interaction.exe");
const idleWorker = resolve("target/interaction-tools/idle-worker.exe");
const system = () => {
  const result = spawnSync(helper, ["--system"], { encoding: "utf8", windowsHide: true });
  assert.equal(result.status, 0, result.stderr);
  return JSON.parse(result.stdout);
};
const machine = system();
assert(machine.benchmarkVm, "Run this benchmark only inside the dedicated PVE virtual machine.");
assert(machine.elevated, "Start this benchmark as administrator so both tools use the same token.");
const nativeRunning = () => {
  const result = spawnSync("tasklist.exe", ["/fi", "IMAGENAME eq Taskmgr.exe", "/fo", "csv", "/nh"], { encoding: "utf8", windowsHide: true });
  return /"Taskmgr\.exe"/i.test(result.stdout);
};
assert(!nativeRunning(), "Close the existing Task Manager before starting the benchmark.");
const nativeSettings = join(process.env.LOCALAPPDATA, "Microsoft/Windows/TaskManager/settings.json");
const backup = existsSync(nativeSettings) ? readFileSync(nativeSettings) : null;
if (backup) writeFileSync(join(output, "taskmgr-settings.backup"), backup);
function prepareNative(performanceFirst = false) {
  // Let Task Manager generate its complete settings schema on the first run.
  if (!existsSync(nativeSettings)) return;
  const settings = JSON.parse(readFileSync(nativeSettings, "utf8").replace(/^\uFEFF/, ""));
  // These settings were verified through Task Manager's own Settings page.
  settings.CustomizeMode = 2;
  settings.DefaultStartPage = performanceFirst ? 1 : 5;
  mkdirSync(resolve(nativeSettings, ".."), { recursive: true });
  writeFileSync(nativeSettings, JSON.stringify(settings));
}
const identifier = `dev.argus.taskmanager.test.${label}`;
const [config] = testDirectories(identifier);
mkdirSync(config, { recursive: true });
const app = JSON.parse(readFileSync("resources/app.json"));
writeFileSync(join(config, "settings.json"), JSON.stringify({
  language: "en-US", theme: "dark", onboarding: 1, seenVersion: app.version,
  closeToTray: false, hotkeyEnabled: true, history: true, kernelSensors: false, updateCheck: false,
}));
writeFileSync(join(config, "preferences.json"), JSON.stringify({ values: {
  view: "list", sort: { key: "pid", desc: false }, startPage: "processes", cpuMode: "overall",
  navPosition: "top", detailsPanel: false,
  "notice.trayRate": true, "notice.minimizedRate": true,
  processColumns: { selected: ["name", "pid", "cpu", "mem", "diskRead", "diskWrite", "io", "gpu", "threads", "handles"], order: [], autoWidth: true, widths: {} },
} }));
const exe = join(output, "argus.exe");
copyFileSync("target/release/argus.exe", exe);
const hash = path => createHash("sha256").update(readFileSync(path)).digest("hex");
const nativeExe = join(process.env.WINDIR, "System32/Taskmgr.exe");
const versionScript = `@(${[exe, nativeExe].map(path => `'${path.replaceAll("'", "''")}'`).join(",")}) | ForEach-Object { (Get-Item -LiteralPath $_).VersionInfo.FileVersion } | ConvertTo-Json -Compress`;
const versions = spawnSync("powershell.exe", ["-NoProfile", "-EncodedCommand", Buffer.from(versionScript, "utf16le").toString("base64")], { windowsHide: true, encoding: "utf8" });
assert.equal(versions.status, 0, versions.stderr);
const [argusFileVersion, nativeFileVersion] = JSON.parse(versions.stdout);
writeJson(join(output, "environment.json"), {
  started: new Date().toISOString(), argusVersion: app.version, argusSHA256: hash(exe),
  argusFileVersion, nativeFileVersion, nativeSHA256: hash(nativeExe), probeSHA256: hash(helper), system: system(),
  build: "release", clientPixels: [1240, 800], privileges: "Administrator",
  interactionMeasurementVersion: 1,
  clock: "CreateProcess, restore or input QPC to a complete WGC window frame; readiness additionally requires the search edit to display typed input",
  processColdStart: "All target processes exited; OS file cache is left intact",
  argusMode: "Normal startup with an isolated profile; control-stdin is used only for graceful exit",
  settings: { theme: "dark", history: true, processNetwork: true, hotkey: true, updateCheck: false, cpuView: "overall", processView: "list", networkColumn: false },
});
const sleep = ms => new Promise(done => setTimeout(done, ms));
const loads = [];
let active;
async function load(count) {
  assert(Number.isInteger(count) && count >= 0 && count <= 1536);
  for (const child of loads.splice(count)) child.kill();
  while (loads.length < count) {
    assert(system().availableBytes > 2 * 1024 ** 3, "Keep at least 2 GiB available during load creation");
    const remaining = Math.min(32, count - loads.length);
    for (let i = 0; i < remaining; i++) loads.push(spawn(idleWorker, [String(process.pid)], { windowsHide: true, stdio: "ignore" }));
    await sleep(100);
  }
  await sleep(2000);
  assert(loads.every(p => p.exitCode === null), "An owned load process exited early");
}
function validCommand(command) {
  return /^(?:wait [0-9]{1,5}|elements|stop)$/.test(command)
    || /^click [0-9]{1,4} [0-9]{1,4}$/.test(command)
    || /^(?:bench [a-z0-9-]+|lifecycle (?:cold|resident|restore))(?: [0-9]{1,4}){9}$/.test(command);
}
async function run(request) {
  assert(/^[a-z0-9-]+$/.test(request.id));
  assert(["argus", "native"].includes(request.tool));
  assert(request.commands.every(validCommand), "Only bounded benchmark commands are accepted");
  assert(loads.every(p => p.exitCode === null), "An owned load process exited early");
  if (request.tool === "native") {
    assert(!nativeRunning(), "Another Task Manager is already open");
    prepareNative(request.commands.some(command => command.startsWith("bench cpu ")));
  } else {
    const path = join(config, "settings.json");
    const settings = JSON.parse(readFileSync(path, "utf8"));
    settings.closeToTray = request.commands.some(command => command.startsWith("lifecycle resident "));
    writeFileSync(path, JSON.stringify(settings));
  }
  const report = { ...request, began: new Date().toISOString(), before: system(), addedProcesses: loads.length, events: [] };
  writeJson(join(output, "state.json"), { phase: "running", id: request.id, sampled: report.began, system: report.before });
  const artifacts = join(output, request.id);
  mkdirSync(artifacts, { recursive: true });
  const target = request.tool === "argus" ? exe : join(process.env.WINDIR, "System32/Taskmgr.exe");
  const arguments_ = request.tool === "argus" ? `--control-stdin --identifier ${identifier} --width 1240 --height 800` : "";
  active = spawn(helper, ["--interactive-desktop", target, arguments_, artifacts], { windowsHide: true, stdio: ["pipe", "pipe", "pipe"] });
  const child = active;
  report.helperPid = child.pid;
  let stderr = "", exited = false;
  child.stderr.on("data", data => { stderr += data; });
  const exit = new Promise(done => child.on("exit", code => { exited = true; done(code); }));
  createInterface({ input: child.stdout }).on("line", line => {
    try { report.events.push(JSON.parse(line)); } catch { stderr += `${line}\n`; }
  });
  const until = async predicate => {
    const end = performance.now() + 180000;
    while (!predicate() && !exited && performance.now() < end) await sleep(30);
    assert(predicate(), stderr || "Benchmark response timed out");
  };
  try {
    await until(() => report.events.some(e => e.event === "launched"));
    for (const command of [...request.commands, "stop"]) {
      const at = report.events.length;
      child.stdin.write(`${command}\n`);
      if (command === "stop") break;
      await until(() => report.events.slice(at).some(e => e.event === "complete" && e.command === command));
    }
    report.exitCode = await exit;
    assert.equal(report.exitCode, 0, stderr);
  } catch (error) {
    report.failure = String(error.stack ?? error);
    if (!exited) { child.kill(); await exit; }
  } finally {
    active = undefined;
    report.after = system(); report.stderr = stderr;
    writeJson(join(output, `${request.id}.json`), report);
    writeJson(join(output, "state.json"), { phase: "idle", id: request.id, sampled: new Date().toISOString(), system: report.after });
  }
  return report;
}
writeJson(join(output, "ready.json"), { pid: process.pid, ready: true });
console.log(`Benchmark ready: ${output}\nPress Ctrl+C to stop. Each run also stops if focus leaves its test window.`);
const requests = join(output, "requests.jsonl");
let handled = 0;
try {
  const deadline = Date.now() + 2 * 60 * 60 * 1000;
  while (Date.now() < deadline) {
    const queued = existsSync(requests) ? readFileSync(requests, "utf8") : "";
    const lines = queued.slice(0, queued.lastIndexOf("\n") + 1).split("\n").filter(Boolean);
    if (handled === lines.length) { await sleep(200); continue; }
    const request = JSON.parse(lines[handled++]);
    if (request.stop) break;
    if (request.load !== undefined) {
      await load(request.load); const sample = system();
      writeJson(join(output, `load-${request.load}.json`), sample);
      writeJson(join(output, "state.json"), { phase: "idle", sampled: new Date().toISOString(), system: sample });
    }
    else {
      if (request.tool === "native" && !existsSync(nativeSettings)) {
        const initialized = await run({ id: `${request.id}-initialize`, tool: "native", commands: ["wait 2000"] });
        assert(!initialized.failure && existsSync(nativeSettings), "Task Manager must initialize its settings before measurement");
      }
      const report = await run(request); console.log(`${request.id}: ${report.failure ? "failed" : "complete"}`);
    }
  }
} finally {
  active?.kill();
  for (const child of loads) if (child.exitCode === null) child.kill();
  await sleep(1500);
  const restoreNative = !nativeRunning();
  if (restoreNative) {
    if (backup) writeFileSync(nativeSettings, backup);
    else rmSync(nativeSettings, { force: true });
  }
  cleanupTestProfile(identifier);
  writeJson(join(output, "finished.json"), { finished: new Date().toISOString(), nativeSettingsRestored: restoreNative });
}
