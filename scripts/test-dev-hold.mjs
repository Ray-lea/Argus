// Exercises the real development script with a rebuild hold: a hidden,
// isolated instance, a watched file that changes while the hold is in force
// (no build), the release of the hold (one build) and a change after it (a
// build at once). The watched file is an empty marker beside the vendored
// packages, which no build reads.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { HOLD, readHold } from "./lib/dev-hold.mjs";

const root = resolve(import.meta.dirname, "..");
process.chdir(root);
if (process.platform !== "win32") throw new Error("The development script runs on Windows.");
assert(!readHold(resolve(HOLD)), "a hold is in force: release it before running this test");
const app = JSON.parse(readFileSync("resources/app.json", "utf8"));
const id = `${app.identifier}.test.hold-${randomUUID().slice(0, 8)}`;
const directories = [process.env.APPDATA, process.env.LOCALAPPDATA].map((base) => resolve(base, id));
mkdirSync(directories[0], { recursive: true });
writeFileSync(
  resolve(directories[0], "settings.json"),
  JSON.stringify({ language: "en-US", theme: "dark", closeToTray: false, hotkeyEnabled: false, history: false, kernelSensors: false, updateCheck: false, rules: [], onboarding: 1, seenVersion: app.version }),
);
const marker = resolve("vendor/.dev-hold-test");
const runs = resolve("target/native-dev/runs");
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
const hold = (...args) => {
  const result = spawnSync(process.execPath, ["scripts/dev-hold.mjs", ...args], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
};

// English messages, whatever the display language of the machine.
const child = spawn(process.execPath, ["scripts/desktop.mjs", "dev", "--hidden", "--identifier", id, "--stop-after", "120"], {
  windowsHide: true,
  stdio: ["ignore", "pipe", "pipe"],
  env: { ...process.env, ARGUS_LOCALE: "en-US" },
});
let output = "";
child.stdout.on("data", (chunk) => (output += chunk));
child.stderr.on("data", (chunk) => (output += chunk));
const count = (text) => output.split(text).length - 1;
const builds = () => count("Finished `dev` profile");
async function until(what, predicate, seconds) {
  const end = Date.now() + seconds * 1000;
  while (!predicate() && child.exitCode === null && Date.now() < end) await sleep(100);
  assert(predicate(), `${what}\n--- output of the development script ---\n${output}`);
}
let touches = 0;
const touch = () => writeFileSync(marker, `${++touches}\n`);

let pid;
let copy;
try {
  await until("the development instance did not start", () => /GPUI process started \(PID (\d+)\)/.test(output), 600);
  pid = Number(output.match(/GPUI process started \(PID (\d+)\)/)[1]);
  // The copy this run's instance was started from. Another development
  // script may be running in the same checkout: its copies are not ours.
  const path = spawnSync("powershell", ["-NoProfile", "-Command", `(Get-Process -Id ${pid}).Path`], { encoding: "utf8" }).stdout.trim();
  if (path && resolve(path, "../..") === runs) copy = resolve(path, "..");
  const started = builds();

  hold("on", "--minutes", "2", "test of the hold");
  touch();
  await until("the hold was not reported", () => count("Rebuilds are on hold") === 1, 10);
  await sleep(1500);
  touch();
  await sleep(3000);
  assert.equal(builds(), started, "a build ran while the hold was in force");
  assert.equal(count("Rebuilds are on hold"), 1, "the hold is reported once");

  hold("off");
  await until("the hold's end was not reported", () => count("The hold has ended") === 1, 10);
  await until("no build followed the hold", () => builds() === started + 1, 300);
  await sleep(2000);
  assert.equal(builds(), started + 1, "the changes made during the hold are built once");

  touch();
  await until("a change after the hold did not build at once", () => builds() === started + 2, 300);
  assert.equal(count("Rebuilds are on hold"), 1);
  console.log(`Development hold: passed (${touches} changes, ${builds() - started} builds, instance PID ${pid})`);
} finally {
  rmSync(marker, { force: true });
  rmSync(resolve(HOLD), { force: true });
  child.kill();
  if (pid) {
    try {
      process.kill(pid);
    } catch {}
  }
  await sleep(1500);
  for (const directory of directories) rmSync(directory, { recursive: true, force: true, maxRetries: 5, retryDelay: 300 });
  if (copy) rmSync(copy, { recursive: true, force: true, maxRetries: 5, retryDelay: 300 });
}
