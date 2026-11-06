// Owned-window fault injection. Every deadline and measurement uses the supervisor's clock.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve, join } from "node:path";
import { createInterface } from "node:readline";
import { cleanupTestProfile, testDirectories } from "./lib/owned-test.mjs";

const root = resolve(import.meta.dirname, "..");
process.chdir(root);
const [label, input] = process.argv.slice(2);
assert(
  /^[a-z0-9-]+$/.test(label ?? "") && input,
  "usage: node scripts/resilience-native.mjs LABEL EXE [--tray-delay MS --expect-responsive --capture]",
);
const option = (name, fallback) => {
  const at = process.argv.indexOf(name);
  return at < 0 ? fallback : process.argv[at + 1];
};
const output = resolve("target/native-validation", label);
mkdirSync(output, { recursive: true });
const executable = join(output, "argus.exe");
copyFileSync(resolve(input), executable);
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
const probe = resolve(option("--probe", "target/release/argus-window-probe.exe"));
const delay = Number(option("--tray-delay", "2000"));
assert(Number.isInteger(delay) && delay >= 1 && delay <= 10000);
const events = [];
const child = spawn(
  probe,
  ["run", executable, "--hidden", "--test", "--identifier", identifier, "--stop-after", "90"],
  {
    windowsHide: true,
    stdio: ["pipe", "pipe", "pipe"],
    env: { ...process.env, ARGUS_TEST_TRAY_DELAY_MS: String(delay) },
  },
);
let exited = false,
  exitCode,
  stderr = "";
child.on("exit", (code) => {
  exited = true;
  exitCode = code;
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
    events.push(JSON.parse(line));
  } catch {
    stderr += `${line}\n`;
  }
});
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
async function until(predicate, seconds = 30) {
  const end = performance.now() + seconds * 1000;
  while (!predicate() && !exited && performance.now() < end) await sleep(10);
  assert(predicate(), `missing response: ${stderr}`);
}
const control = (value) => child.stdin.write(`app ${JSON.stringify(value)}\n`);
async function inspect(label, extra = {}) {
  const start = performance.now();
  control({ ...extra, inspectLayout: label });
  await until(() => events.some((event) => event.event === "ui-layout" && event.label === label));
  return {
    elapsedMs: performance.now() - start,
    ...events.find((event) => event.event === "ui-layout" && event.label === label),
  };
}
const report = { identifier, delay, events };
try {
  const start = performance.now();
  await until(() => events.some((event) => event.event === "shown"));
  report.shownMs = performance.now() - start;
  await sleep(3500);
  report.ui = await inspect("tray-update", { testTrayUpdate: true });
  report.before = await inspect("sampling-before");
  await sleep(6500);
  report.after = await inspect("sampling-after");
  report.sampleAdvance = report.after.sequence - report.before.sequence;
  if (process.argv.includes("--capture")) {
    const pid = events.find((event) => event.event === "shown").pid;
    const shot = spawnSync(probe, ["capture", String(pid), join(output, "window.png")], {
      windowsHide: true,
      encoding: "utf8",
      timeout: 20000,
    });
    assert.equal(shot.status, 0, shot.stderr);
  }
} finally {
  if (!exited) child.stdin.write("stop\n");
  for (let i = 0; i < 300 && !exited; i++) await sleep(50);
  if (!exited) child.kill();
  report.exitCode = exitCode;
  report.stderr = stderr;
  writeFileSync(join(output, "result.json"), JSON.stringify(report, null, 2));
  if (exited) cleanupTestProfile(identifier);
}
assert.equal(exitCode, 0, stderr);
assert(!events.some((event) => event.event === "focusViolation"));
if (process.argv.includes("--expect-responsive")) {
  assert(report.ui.elapsedMs < 400, `UI blocked ${report.ui.elapsedMs.toFixed(1)} ms`);
  assert(report.sampleAdvance >= 5, `sampling advanced ${report.sampleAdvance} times`);
}
console.log(
  JSON.stringify({
    shownMs: report.shownMs,
    uiMs: report.ui.elapsedMs,
    sampleAdvance: report.sampleAdvance,
    output,
  }),
);
