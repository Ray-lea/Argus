// Runs the step files of tools/window-probe/steps one after another, each in
// its own isolated test window (scripts/ui-shots.mjs), and reports which fail:
// the regression check for interface changes that the unit tests cannot see.
//
//   node scripts/ui-steps.mjs [NAME…] [--release | --executable PATH] [--probe PATH]
//     [--language TAG] [--theme dark|light] [--skip-recordings] [--parity-tolerance N] [--list]
//
// --skip-recordings reports step files that record frames as skipped: where
// Windows.Graphics.Capture refuses windows (hosted CI runners), stills still
// come from PrintWindow but nothing can record. --parity-tolerance N lets the
// parity captures differ by up to N per channel: the software rasterizer of
// CI runners draws scrolled tables a little differently on every run.
//
// NAME is a step file without `.json`; none runs them all. Each run writes
// target/native-validation/steps-<name>, and the outcome of all of them goes
// to target/native-validation/steps/summary.json. A step file listed in
// tools/window-probe/known-failures.json is run and reported, but only an
// unlisted failure (or a listed file that passes) makes the exit code 1.
//
// A step file with `run.parity` is run a second time with those switches set
// (output in steps-<name>-parity), and every capture of the two runs must be
// the same pixel for pixel. Captures that differ are taken once more on both
// sides before the file fails: the real pointer may have rested on a window.
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import assert from "node:assert/strict";
import { parityDifferences } from "./lib/capture-pixels.mjs";
import { aboutOf, parseSteps, runOptions, runSeconds, verdict } from "./lib/ui-shots.mjs";

const root = resolve(import.meta.dirname, "..");
process.chdir(root);
const argv = process.argv.slice(2);
const PASSED = ["--release", "--executable", "--probe", "--language", "--theme"];
const passed = [];
const names = [];
let parityTolerance = 0;
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === "--release") passed.push(argv[i]);
  else if (PASSED.includes(argv[i])) passed.push(argv[i], argv[++i]);
  else if (argv[i] === "--parity-tolerance") parityTolerance = Number(argv[++i]);
  else if (argv[i] !== "--list" && argv[i] !== "--skip-recordings") names.push(argv[i]);
}
assert(Number.isInteger(parityTolerance) && parityTolerance >= 0 && parityTolerance < 255, "--parity-tolerance is a whole channel difference");
const skipRecordings = argv.includes("--skip-recordings");
const directory = "tools/window-probe/steps";
const all = readdirSync(directory).filter((name) => name.endsWith(".json")).map((name) => name.slice(0, -5)).sort();
for (const name of names) assert(all.includes(name), `${name}: no such step file in ${directory}`);
const chosen = names.length ? names : all;
const known = JSON.parse(readFileSync("tools/window-probe/known-failures.json", "utf8"));
for (const name of Object.keys(known)) assert(all.includes(name), `known-failures.json: ${name} is not a step file`);

if (argv.includes("--list")) {
  for (const name of chosen) {
    const about = aboutOf(JSON.parse(readFileSync(`${directory}/${name}.json`, "utf8")));
    console.log(`${name}${known[name] ? `  (known failure: ${known[name]})` : ""}\n    ${about || "(no description)"}`);
  }
  process.exit(0);
}

const folder = (label) => resolve("target/native-validation", label);
const copy = (label) => resolve(folder(label), "argus.exe");

/** One run of a step file under a label; resolves to the lines of its failure, none when it passed. */
async function run(label, file, limit, { executable, env = {} } = {}) {
  // An executable given here stands before the caller's own, so that it is the one taken.
  // The copies are kept for the parity runs, which reuse the first, and removed below.
  const args = ["scripts/ui-shots.mjs", label, file, ...(executable ? ["--executable", executable] : []), "--keep-executable", ...passed];
  const child = spawn(process.execPath, args, { windowsHide: true, stdio: ["ignore", "pipe", "pipe"], env: { ...process.env, ...env } });
  let stderr = "";
  child.stdout.resume();
  child.stderr.on("data", (chunk) => (stderr += chunk));
  const timer = setTimeout(() => child.kill(), limit);
  const code = await new Promise((done) => child.on("exit", (status) => done(status ?? -1)));
  clearTimeout(timer);
  return code === 0 ? [] : stderr.trim().split(/\r?\n/).filter(Boolean).slice(-8);
}

/** The second run under the file's parity switches, and the comparison of the two; resolves to failure lines. */
async function parity(name, file, limit, switches) {
  const spelled = Object.entries(switches).map(([key, value]) => `${key}=${value}`).join(" ");
  // Both sides run the copy the first run took: the development script may rebuild the program meanwhile.
  const executable = copy(`steps-${name}`);
  const labels = { ours: [`steps-${name}`], theirs: [] };
  const used = [];
  for (let attempt = 1; attempt <= 2; attempt++) {
    const suffix = attempt === 1 ? "" : `-${attempt}`;
    if (attempt > 1) {
      const again = `steps-${name}${suffix}`;
      used.push(again);
      const failed = await run(again, file, limit, { executable });
      if (failed.length) return failed;
      labels.ours.push(again);
    }
    const other = `steps-${name}-parity${suffix}`;
    used.push(other);
    const failed = await run(other, file, limit, { executable, env: switches });
    if (failed.length) return [`with ${spelled}:`, ...failed];
    labels.theirs.push(other);
    const { compared, differ, missing } = parityDifferences(labels.ours.map(folder), labels.theirs.map(folder), parityTolerance);
    if (!differ.length && !missing.length) {
      for (const label of used) rmSync(copy(label), { force: true, maxRetries: 5, retryDelay: 200 });
      const same = parityTolerance ? `within ${parityTolerance}` : "identical";
      console.log(`    ${compared} captures ${same} with ${spelled}${attempt > 1 ? " (some only on the second attempt)" : ""}`);
      return [];
    }
    if (attempt === 2) {
      for (const label of used) rmSync(copy(label), { force: true, maxRetries: 5, retryDelay: 200 });
      return [
        ...differ.map((entry) => `${entry.name} differs with ${spelled}: ${entry.error ?? `${entry.changedPixels} pixels, largest difference ${entry.maxChannelDelta}, box ${JSON.stringify(entry.box)}`}`),
        ...missing.map((entry) => `${entry} was not captured with ${spelled}`),
      ];
    }
  }
  return [];
}

// One at a time: test windows share the screen, the pointer position and the
// machine's load, and a recording under load drops frames.
const outcomes = [];
for (const name of chosen) {
  const file = `${directory}/${name}.json`;
  const plan = JSON.parse(readFileSync(file, "utf8"));
  const steps = parseSteps(Array.isArray(plan) ? plan : plan.steps);
  if (skipRecordings && steps.some((step) => step.record)) {
    outcomes.push({ name, result: "skipped", unexpected: false, reason: "records frames" });
    console.log(`${"skipped".padEnd(14)} ${name} (records frames)`);
    continue;
  }
  const limit = (runSeconds(steps) + 120) * 1000;
  const switches = runOptions(plan).parity;
  const started = Date.now();
  let failures = await run(`steps-${name}`, file, limit);
  if (!failures.length && Object.keys(switches).length) failures = await parity(name, file, limit, switches);
  // Each run works on its own copy of the executable; fifty of them are gigabytes.
  rmSync(copy(`steps-${name}`), { force: true, maxRetries: 5, retryDelay: 200 });
  const outcome = { name, ...verdict(failures.length === 0, known[name]), seconds: Math.round((Date.now() - started) / 1000), failures };
  outcomes.push(outcome);
  console.log(`${outcome.result.padEnd(14)} ${name} (${outcome.seconds} s)${failures.length ? `\n    ${failures.join("\n    ")}` : ""}`);
}

const output = resolve("target/native-validation/steps");
mkdirSync(output, { recursive: true });
writeFileSync(resolve(output, "summary.json"), JSON.stringify({ arguments: argv, outcomes }, null, 2));
const count = (result) => outcomes.filter((outcome) => outcome.result === result).length;
console.log(`${outcomes.length} step files: ${count("passed")} passed, ${count("failed")} failed, ${count("known failure")} known failures, ${count("fixed")} listed as failing but passed${count("skipped") ? `, ${count("skipped")} skipped` : ""}`);
if (existsSync(resolve(output, "summary.json"))) console.log(`summary: ${resolve(output, "summary.json")}`);
process.exit(outcomes.some((outcome) => outcome.unexpected) ? 1 : 0);
