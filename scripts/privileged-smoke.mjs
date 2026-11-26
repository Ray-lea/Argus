// The privileged service and the task manager replacement end to end, on a
// disposable machine as an administrator: install the service for this
// account, update it, repair it after its authorization record is lost,
// replace Task Manager and open taskmgr.exe, restore Task Manager, and remove
// the service through the uninstaller's command. Runs in the nightly workflow
// and in lab VMs only (scripts/lib/machine-smoke.mjs).
//
//   node scripts/privileged-smoke.mjs <installed argus.exe> [--output <folder>]
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import {
  IFEO_KEY,
  currentSid,
  dataFolders,
  elevated,
  registryValue,
  requireDisposable,
  run,
  runningImages,
  serviceState,
  sleep,
  waitFor,
} from "./lib/machine-smoke.mjs";

const argv = process.argv.slice(2);
const option = (name, fallback) => (argv.includes(name) ? argv[argv.indexOf(name) + 1] : fallback);
const exe = argv[0] && resolve(argv[0]);
assert(exe && existsSync(exe), "usage: node scripts/privileged-smoke.mjs <installed argus.exe> [--output O]");
requireDisposable();
assert(elevated(), "run as an administrator");

const root = resolve(import.meta.dirname, "..");
const output = resolve(option("--output", join(root, "target", "nightly", "privileged")));
mkdirSync(output, { recursive: true });
const SERVICE = "ArgusPrivileged";
const folder = join(process.env.ProgramFiles, "Argus Privileged");
const sid = currentSid();
const report = { sid: "current account", steps: [] };
const step = (name, details = {}) => {
  report.steps.push({ name, ...details });
  console.log(`${name}${Object.keys(details).length ? ` ${JSON.stringify(details)}` : ""}`);
};
const listing = () => (existsSync(folder) ? readdirSync(folder).sort() : null);

function helper(args, label) {
  const result = run(exe, args, { timeout: 180000 });
  step(label, { exitCode: result.status });
  assert.equal(result.status, 0, `${label}: exit code ${result.status} ${result.stdout}${result.stderr}`);
}

const running = () => serviceState(SERVICE) === "RUNNING";
let caller;

try {
  assert.equal(serviceState(SERVICE), null, "the service is already installed: start from a clean machine");

  helper(["--argus-service-manage", "install", sid], "service installed");
  assert(await waitFor(running, 30000), `service state ${serviceState(SERVICE)} after install`);
  step("service running", { files: listing() });
  assert(listing()?.includes("owner.sid"), "no authorization record in the service folder");

  helper(["--argus-service-manage", "update", sid], "service updated");
  assert(await waitFor(running, 30000), `service state ${serviceState(SERVICE)} after update`);

  // A lost authorization record makes the service "broken"; repair writes it again.
  rmSync(join(folder, "owner.sid"));
  helper(["--argus-service-manage", "repair", sid], "service repaired after losing its authorization record");
  assert(existsSync(join(folder, "owner.sid")), "repair did not write the authorization record");
  assert(await waitFor(running, 30000), `service state ${serviceState(SERVICE)} after repair`);

  // The replacement helper checks that a running Argus of this account asked for it.
  caller = spawn(exe, ["--hidden", "--stop-after", "240"], { windowsHide: true, stdio: "ignore" });
  await sleep(8000);
  const created = run("powershell.exe", ["-NoProfile", "-Command", `(Get-Process -Id ${caller.pid}).StartTime.ToFileTimeUtc()`]).stdout.trim();
  assert(/^\d+$/.test(created), `no start time for the calling instance (${created})`);
  helper(["--argus-taskmgr-manage", "enable", sid, String(caller.pid), created], "task manager replaced");
  const debuggerValue = registryValue(IFEO_KEY, "Debugger");
  step("taskmgr.exe redirected", { debugger: debuggerValue });
  assert(debuggerValue?.toLowerCase().includes("argus-taskmgr-launcher.exe"), `IFEO Debugger is ${debuggerValue}`);

  // With the caller still open, the replacement would hand the request over to it.
  caller.kill();
  await waitFor(() => caller.exitCode !== null || caller.signalCode !== null, 15000);
  await waitFor(() => !runningImages().has("argus.exe"), 15000);
  spawn("taskmgr.exe", [], { windowsHide: true, stdio: "ignore", detached: true }).unref();
  const opened = await waitFor(() => runningImages().has("argus-taskmgr-ui.exe"), 45000);
  step("taskmgr.exe opened Argus", { opened: Boolean(opened) });
  assert(opened, "taskmgr.exe did not start argus-taskmgr-ui.exe");
  run("taskkill.exe", ["/IM", "argus-taskmgr-ui.exe", "/F"]);

  helper(["--argus-taskmgr-manage", "restore", sid], "task manager restored");
  const restored = registryValue(IFEO_KEY, "Debugger");
  step("taskmgr.exe no longer redirected", { debugger: restored });
  assert(restored === null || !restored.toLowerCase().includes("argus"), `IFEO Debugger is still ${restored}`);

  // What the NSIS uninstaller runs once the user agrees to remove the service.
  helper(["--argus-service-remove"], "service removed");
  const gone = await waitFor(() => serviceState(SERVICE) === null, 30000);
  step("service registration gone", { state: serviceState(SERVICE), files: listing() });
  assert(gone, `service still registered (${serviceState(SERVICE)})`);
  const left = (listing() ?? []).filter((name) => /^argus-service|^owner\.sid$/i.test(name));
  assert.deepEqual(left, [], `service files left: ${left.join(", ")}`);
  report.passed = true;
} catch (error) {
  report.passed = false;
  report.error = String(error?.message ?? error);
  console.error(report.error);
  process.exitCode = 1;
} finally {
  if (caller && caller.exitCode === null) caller.kill();
  // Logs of the interface and of each helper, for the artifact.
  const { local } = dataFolders();
  const logs = join(local, "logs", "diagnostics");
  if (existsSync(logs))
    for (const name of readdirSync(logs)) if (name.endsWith(".log")) copyFileSync(join(logs, name), join(output, name));
  for (const name of ["taskmgr-error.json", "service-error.json"])
    if (existsSync(join(folder, name))) copyFileSync(join(folder, name), join(output, name));
  report.folder = listing();
  writeFileSync(join(output, "report.json"), JSON.stringify(report, null, 2));
  console.log(`report: ${join(output, "report.json")}`);
}
