// Upgrade on a disposable machine: install an older Argus, let it save
// settings and write history, install the new one over it, and check that
// the new one starts, keeps the settings, reads the history and logs no
// failure; then uninstall. Runs in the nightly workflow and in lab VMs only
// (scripts/lib/machine-smoke.mjs), never on the development machine.
//
//   node scripts/upgrade-smoke.mjs <old-setup.exe> <new-setup.exe> [--dir <install folder>] [--output <folder>] [--seconds N]
import assert from "node:assert/strict";
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import {
  UNINSTALL_KEY,
  buildInfo,
  dataFolders,
  logSince,
  registryValue,
  requireDisposable,
  run,
  runningImages,
  troubles,
  waitFor,
} from "./lib/machine-smoke.mjs";

const argv = process.argv.slice(2);
const option = (name, fallback) => (argv.includes(name) ? argv[argv.indexOf(name) + 1] : fallback);
const [oldSetup, newSetup] = argv.filter((arg, i) => !arg.startsWith("--") && !argv[i - 1]?.startsWith("--"));
assert(oldSetup && newSetup && existsSync(oldSetup) && existsSync(newSetup), "usage: node scripts/upgrade-smoke.mjs <old-setup.exe> <new-setup.exe> [--dir D] [--output O]");
requireDisposable();

const root = resolve(import.meta.dirname, "..");
const dir = resolve(option("--dir", join(process.env.RUNNER_TEMP ?? process.env.TEMP, "argus-upgrade")));
assert(!/\s/.test(dir), "NSIS takes /D= unquoted: the install folder must have no spaces");
const output = resolve(option("--output", join(root, "target", "nightly", "upgrade")));
const seconds = Number(option("--seconds", 25));
mkdirSync(output, { recursive: true });
const exe = join(dir, "argus.exe");
const { config, local } = dataFolders();
const settingsPath = join(config, "settings.json");
const historyDir = join(local, "history");
const logPath = join(local, "logs", "diagnostics", "argus.log");
const report = { steps: [] };
const step = (name, details = {}) => {
  report.steps.push({ name, ...details });
  console.log(`${name}${Object.keys(details).length ? ` ${JSON.stringify(details)}` : ""}`);
};

function install(setup) {
  const result = run(resolve(setup), ["/S", `/D=${dir}`], { timeout: 300000 });
  assert.equal(result.status, 0, `${setup}: installer exit code ${result.status} ${result.stderr}`);
  assert(existsSync(exe), `${setup}: no argus.exe in ${dir}`);
}

async function runOnce(label) {
  // An instance left running (one an installer started) would take the start
  // over through the single-instance lock, and this run would test nothing.
  if (runningImages().has("argus.exe")) run("taskkill.exe", ["/IM", "argus.exe", "/F"]);
  await waitFor(() => !runningImages().has("argus.exe"), 15000);
  const started = Date.now();
  const result = run(exe, ["--hidden", "--stop-after", String(seconds)], { timeout: (seconds + 90) * 1000 });
  step(`${label}: ran`, { exitCode: result.status, seconds: Math.round((Date.now() - started) / 1000) });
  assert.equal(result.status, 0, `${label}: exit code ${result.status} ${result.stderr}`);
  return started;
}

const history = () =>
  existsSync(historyDir)
    ? readdirSync(historyDir, { withFileTypes: true })
        .filter((entry) => entry.isFile())
        .map((entry) => ({ name: entry.name, size: statSync(join(historyDir, entry.name)).size, modified: statSync(join(historyDir, entry.name)).mtimeMs }))
    : [];

// Values that differ from the defaults, so that a reset to defaults shows.
const SEEDED = { theme: "light", language: "en-US", closeToTray: false, updateCheck: false };

try {
  // An installed newer version makes the old installer refuse (exit code 2).
  const present = registryValue(UNINSTALL_KEY, "DisplayVersion");
  assert.equal(present, null, `Argus ${present} is already installed: start from a machine without Argus`);
  install(oldSetup);
  const old = buildInfo(exe);
  assert(old, "the old program does not answer --argus-build-info");
  step("old installed", { build: old.build, displayVersion: registryValue(UNINSTALL_KEY, "DisplayVersion") });
  mkdirSync(config, { recursive: true });
  writeFileSync(settingsPath, JSON.stringify({ ...SEEDED, onboarding: 1, seenVersion: old.version }));
  await runOnce("old");
  const before = JSON.parse(readFileSync(settingsPath, "utf8"));
  const written = history();
  copyFileSync(settingsPath, join(output, "settings-old.json"));
  step("old wrote", { historyFiles: written.length, historyBytes: written.reduce((sum, file) => sum + file.size, 0) });
  assert(written.length > 0, "the old version wrote no history");

  install(newSetup);
  const next = buildInfo(exe);
  assert(next, "the new program does not answer --argus-build-info");
  step("new installed over it", { build: next.build, displayVersion: registryValue(UNINSTALL_KEY, "DisplayVersion") });
  assert.notEqual(next.build, old.build, "the new installer installed the same build");
  const started = await runOnce("new");

  const after = JSON.parse(readFileSync(settingsPath, "utf8"));
  copyFileSync(settingsPath, join(output, "settings-new.json"));
  const lost = Object.keys(SEEDED).filter((key) => JSON.stringify(after[key]) !== JSON.stringify(before[key]));
  step("settings kept", { lost });
  assert.deepEqual(lost, [], `settings changed by the upgrade: ${lost.join(", ")}`);

  const lines = logSince(logPath, next.build);
  writeFileSync(join(output, "argus-new.log"), lines.join("\n"));
  assert(lines.length, `no "Argus ${next.build} started" in ${logPath}`);
  const bad = troubles(lines);
  step("new log", { lines: lines.length, troubles: bad.length });
  assert.deepEqual(bad, [], `the new version logged failures:\n${bad.join("\n")}`);

  const now = history();
  const appended = now.filter((file) => file.modified >= started - 2000);
  step("history after upgrade", { files: now.length, writtenByNew: appended.length });
  assert(now.length >= written.length, "history files disappeared in the upgrade");
  assert(appended.length > 0, "the new version wrote no history");

  // uninstall-smoke keeps to RUNNER_TEMP, which only CI sets; a lab VM gets the install folder's parent.
  const removed = run(process.execPath, [join(root, "scripts", "uninstall-smoke.mjs"), dir], {
    timeout: 300000,
    env: { ...process.env, RUNNER_TEMP: process.env.RUNNER_TEMP ?? dirname(dir) },
  });
  step("uninstalled", { exitCode: removed.status, dataKept: { settings: existsSync(settingsPath), history: existsSync(historyDir) } });
  assert.equal(removed.status, 0, `uninstall: ${removed.stdout}${removed.stderr}`);
  report.passed = true;
} catch (error) {
  report.passed = false;
  report.error = String(error?.message ?? error);
  console.error(report.error);
  process.exitCode = 1;
} finally {
  if (existsSync(logPath)) copyFileSync(logPath, join(output, "argus.log"));
  writeFileSync(join(output, "report.json"), JSON.stringify(report, null, 2));
  console.log(`report: ${join(output, "report.json")}`);
}
