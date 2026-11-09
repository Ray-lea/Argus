// Publish only a complete paired cohort; keep local paths and UI contents private.
import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";

const label = process.argv[2];
assert(typeof label === "string" && /^[a-z0-9-]{1,80}$/.test(label), "Provide the completed measurement label");
const folder = resolve("target/interaction-validation", label);
const read = name => JSON.parse(readFileSync(join(folder, name)));
const environment = read("environment.json");
assert(read("finished.json").nativeSettingsRestored, "The cohort did not finish cleanup");
assert(environment.system.benchmarkVm && environment.argusMode.startsWith("Normal startup"));
const median = values => { const a = values.toSorted((x, y) => x - y), n = a.length; return n % 2 ? a[n >> 1] : (a[n / 2 - 1] + a[n / 2]) / 2; };
const runs = [];
function collect(id, tool, operation, load) {
  const report = read(`${id}.json`);
  assert.equal(report.tool, tool);
  assert.equal(report.addedProcesses, load);
  assert.equal(report.exitCode, 0, report.stderr);
  assert(!report.failure && report.events.some(event => event.event === "complete" && event.command.startsWith(operation === "sort" || operation === "cpu" ? "bench " : "lifecycle ")));
  const samples = report.events.filter(event => event.event === "measurement");
  assert.equal(samples.length, 6, `${id}: expected three pairs`);
  if (!["sort", "cpu"].includes(operation)) assert.equal(report.events.filter(event => event.event === "input-ack").length, 3);
  for (const event of samples) {
    assert(Number.isFinite(event.ms) && event.ms > 0 && event.ms < 15000);
    assert(Number.isInteger(event.processCount) && event.processCount > load);
    assert(existsSync(join(folder, id, basename(event.frame.replaceAll("\\", "/")))));
  }
  assert(existsSync(join(folder, id, `${report.helperPid}-final.png`)));
  const names = operation === "sort" ? ["sort"] : operation === "cpu" ? ["cpu-back", "cpu-out"] : [`${operation}-visible`, `${operation}-ready`];
  for (const name of names) {
    const group = samples.filter(event => (event.operation.startsWith("sort-") ? "sort" : event.operation === "cpu-a" ? "cpu-back" : event.operation === "cpu-b" ? "cpu-out" : event.operation) === name);
    assert.equal(group.length, operation === "sort" ? 6 : 3);
    const resources = report.events.find(event => event.event === "resources");
    runs.push({ id, sourceCohort: report.sourceCohort || label, probeSHA256: report.probeSHA256 || environment.probeSHA256, tool, operation: name, addedProcesses: load, processCount: median(group.map(event => event.processCount)), processCounts: group.map(event => event.processCount), ms: group.map(event => event.ms), ...(resources ? { gdiObjects: resources.gdiObjects, userObjects: resources.userObjects } : {}) });
  }
}
for (let round = 1; round <= 2; round++) for (const operation of ["cold", "resident", "restore"]) for (const tool of ["argus", "native"])
  collect(`r${round}-${tool}-${operation}`, tool, operation, 0);
for (const load of [0, 512, 1024]) for (let round = 1; round <= 2; round++) for (const operation of ["sort", "cpu"]) for (const tool of ["argus", "native"])
  collect(`p${load}-r${round}-${tool}-${operation}`, tool, operation, load);
const { system } = environment;
const data = {
  schemaVersion: 1,
  sessions: [{
    id: label, date: environment.started.slice(0, 10), startedUtc: environment.started,
    os: `Windows 11 ${system.windowsRelease} (${system.windowsBuild})`, cpu: system.cpu.trim(),
    logicalProcessors: system.logicalProcessors, memoryGiB: Math.round(system.totalBytes / 1024 ** 3), usableMemoryBytes: system.totalBytes,
    disk: "96 GiB SATA virtual disk", graphics: "QXL virtual display, no GPU passthrough",
    desktopPixels: system.desktopPixels, clientPixels: environment.clientPixels, dpi: system.dpi, refreshHz: system.refreshHz,
    argusVersion: environment.argusFileVersion, argusSHA256: environment.argusSHA256,
    nativeVersion: environment.nativeFileVersion, nativeSHA256: environment.nativeSHA256, probeSHA256: environment.probeSHA256,
    build: environment.build, privileges: environment.privileges, settings: environment.settings,
    ...(environment.resumedFrom ? { resumedFrom: environment.resumedFrom } : {}),
    clock: environment.clock, processColdStart: environment.processColdStart, argusMode: environment.argusMode,
    residentStart: "Reopen the executable: Argus is hidden to tray, Task Manager is minimized; both remain running for two seconds before the request",
    restore: "Restore the same window after two seconds minimized; verify complete content and a typed-space response in search, then clear input",
    interaction: "PID sort and process-list / overall-CPU page changes; two rounds in reversed application order, three repetitions per direction in each round",
    readiness: "Visible rows contain process names, PIDs, CPU and memory counters; the CPU page contains labels, graph grid and a data curve; WM_NULL responds and the result persists for at least 120 ms. The final window remains complete for one second before cleanup.",
  }],
  runs,
};
const destination = resolve(process.argv[3] || "website/src/data/windows-expanded.json");
writeFileSync(destination, JSON.stringify(data, null, 2) + "\n");
console.log(`${runs.length} paired run groups exported to ${destination}`);
