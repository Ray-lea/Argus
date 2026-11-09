// An isolated, non-activated test window driven by a steps file: captures,
// recordings, contact sheets and checked inspections. Usage and step kinds:
// docs/development/gpui.md#截图与录制.
//
//   node scripts/ui-shots.mjs <label> <steps.json> [--live | --fixture PATH]
//     [--release | --executable PATH] [--probe PATH] [--width W --height H]
//     [--theme dark|light] [--language zh-CN|en-US|ru-RU]
import { spawn, spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { createInterface } from "node:readline";
import assert from "node:assert/strict";
import { cleanupTestProfile, testDirectories } from "./lib/owned-test.mjs";
import { INSPECTIONS, parseSteps, pickFrames, runSeconds, unmet } from "./lib/ui-shots.mjs";

const root = resolve(import.meta.dirname, "..");
process.chdir(root);
const argv = process.argv.slice(2);
const flag = (name) => argv.includes(name);
const option = (name, fallback) => {
  const at = argv.indexOf(name);
  return at >= 0 ? argv[at + 1] : fallback;
};
const [label, stepsFile] = argv;
assert(label && /^[a-z0-9-]+$/.test(label), "usage: node scripts/ui-shots.mjs <label> <steps.json> [options]");
assert(stepsFile && existsSync(stepsFile), "a steps file is required");
// A list of steps, or { "window": [width, height], "steps": [...] } to fix the window size.
const plan = JSON.parse(readFileSync(stepsFile, "utf8"));
const steps = parseSteps(Array.isArray(plan) ? plan : plan.steps);
const size = option("--width") ? [option("--width"), option("--height", option("--width"))] : (plan.window ?? null);

const live = flag("--live");
const output = resolve("target/native-validation", label);
mkdirSync(output, { recursive: true });
const metadata = JSON.parse(readFileSync("resources/app.json"));
const identifier = `${metadata.identifier}.test.${label}`;
const [config] = testDirectories(identifier);
mkdirSync(config, { recursive: true });
writeFileSync(
  resolve(config, "settings.json"),
  JSON.stringify({ language: option("--language", "zh-CN"), theme: option("--theme", "dark"), closeToTray: false, updateCheck: false }),
);
// The development build is enough to look at the interface; measure with --release.
const source = option("--executable", flag("--release") ? "target/release/argus.exe" : "target/debug/argus.exe");
assert(existsSync(source), `${source} is missing: build it first`);
const executable = resolve(output, "argus.exe");
copyFileSync(source, executable);
const probe = resolve(option("--probe", "target/release/argus-window-probe.exe"));
assert(existsSync(probe), "build the probe first: cargo build --release -p argus-window-probe --features capture");
const args = ["run", executable, "--hidden", "--test", "--identifier", identifier, "--stop-after", String(runSeconds(steps))];
if (!live) {
  // Keys and presses only act with a fixture, where nothing reaches real processes.
  const fixture = resolve(output, "fixture.json");
  copyFileSync(option("--fixture", "tools/window-probe/fixtures/desktop.json"), fixture);
  args.push("--fixture", fixture);
}
if (size) args.push("--width", String(size[0]), "--height", String(size[1]));

const child = spawn(probe, args, { windowsHide: true, stdio: ["pipe", "pipe", "pipe"] });
const events = [];
const waiters = [];
let shown;
let exited = false;
let exitCode;
let error = "";
child.stderr.on("data", (b) => (error += b));
child.on("error", (e) => {
  error += e.message;
  exited = true;
});
child.on("exit", (code) => {
  exited = true;
  exitCode = code;
});
createInterface({ input: child.stdout }).on("line", (line) => {
  let event;
  try {
    event = JSON.parse(line);
  } catch {
    error += `${line}\n`;
    return;
  }
  events.push(event);
  if (event.event === "shown") shown = event;
  for (const waiter of waiters.splice(0)) waiter(event);
});
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
const next = (predicate, ms = 20000) =>
  new Promise((done, fail) => {
    const timer = setTimeout(() => fail(Error("no answer from the test window")), ms);
    const check = (event) => (predicate(event) ? (clearTimeout(timer), done(event)) : waiters.push(check));
    waiters.push(check);
  });
const send = (control) => child.stdin.write(`app ${JSON.stringify(control)}\n`);
const tool = (...toolArgs) => {
  const result = spawnSync(probe, toolArgs, { windowsHide: true, encoding: "utf8", timeout: 60000 });
  if (result.status !== 0) throw Error(`${toolArgs[0]}: ${result.stderr || result.stdout}`);
  return result.stdout.trim();
};
const recorded = (name) => events.find((e) => e.event === "recorded" && String(e.dir).endsWith(name));

const results = [];
const failures = [];
try {
  for (let i = 0; i < 600 && !shown && !exited; i++) await sleep(50);
  if (!shown) throw Error(`window unavailable: ${error}`);
  await sleep(1500);
  for (const [index, step] of steps.entries()) {
    const result = { step: index + 1 };
    if (step.note) console.log(`— ${step.note}`);
    if (step.control) send(step.control);
    if (step.probe) child.stdin.write(`${step.probe}\n`);
    if (step.record) {
      const { name, ms, crop } = step.record;
      child.stdin.write(`record ${join(output, name)} ${ms}${crop ? ` ${crop.join(" ")}` : ""}\n`);
    }
    if (step.wait) await sleep(step.wait);
    if (step.inspect) {
      const [control, answer] = INSPECTIONS[step.inspect];
      const pending = next((e) => e.event === answer && e.label === step.label);
      send({ [control]: step.label });
      const event = await pending;
      result.inspect = event;
      const missed = step.expect ? unmet(event, step.expect) : [];
      for (const message of missed) failures.push(`step ${index + 1} (${step.label}): ${message}`);
      console.log(`inspect ${step.label}: ${missed.length ? `${missed.length} unmet` : "ok"}`);
    }
    if (step.capture) {
      tool("capture", String(shown.pid), join(output, `${step.capture}.png`));
      console.log(`captured ${step.capture}`);
    }
    if (step.crop) {
      const { from, name, region, scale } = step.crop;
      tool("crop", join(output, `${from}.png`), join(output, `${name}.png`), ...region.map(String), String(scale));
      console.log(`cropped ${name}`);
    }
    if (step.sheet) {
      const { name, from, region, columns, scale, every, limit } = step.sheet;
      let files;
      if (Array.isArray(from)) {
        files = from.map((capture) => join(output, `${capture}.png`));
      } else {
        // The probe writes a recording's frames after it ends.
        for (let i = 0; i < 1200 && !recorded(from) && !exited; i++) await sleep(50);
        const done = recorded(from);
        assert(done && !done.error, `recording ${from}: ${done?.error ?? "not written"}`);
        files = pickFrames(done.frames, every, limit).map((frame) => join(output, from, `${String(frame).padStart(4, "0")}.png`));
      }
      tool("sheet", join(output, `${name}.png`), ...region.map(String), String(columns), String(scale), ...files);
      console.log(`sheet ${name} (${files.length} frames)`);
    }
    results.push(result);
  }
} catch (cause) {
  failures.push(String(cause?.message ?? cause));
} finally {
  child.stdin.write("stop\n");
  for (let i = 0; i < 400 && !exited; i++) await sleep(50);
  if (!exited) child.kill();
  writeFileSync(join(output, "results.json"), JSON.stringify({ identifier, live, results, failures, events, stderr: error, exitCode }, null, 2));
  if (exited) cleanupTestProfile(identifier);
}
if (exitCode !== 0 && exitCode !== undefined) failures.push(`probe exited with ${exitCode}: ${error.slice(0, 500)}`);
console.log(`output: ${output}`);
if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}
console.log(`UI shots passed: ${identifier}; foreground violations: 0`);
