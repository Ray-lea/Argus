// Runs the step files of tools/window-probe/steps one after another, each in
// its own isolated test window (scripts/ui-shots.mjs), and reports which fail:
// the regression check for interface changes that the unit tests cannot see.
//
//   node scripts/ui-steps.mjs [NAME…] [--release | --executable PATH] [--probe PATH]
//     [--language TAG] [--theme dark|light] [--list]
//
// NAME is a step file without `.json`; none runs them all. Each run writes
// target/native-validation/steps-<name>, and the outcome of all of them goes
// to target/native-validation/steps/summary.json. A step file listed in
// tools/window-probe/known-failures.json is run and reported, but only an
// unlisted failure (or a listed file that passes) makes the exit code 1.
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import assert from "node:assert/strict";
import { parseSteps, runSeconds, verdict } from "./lib/ui-shots.mjs";

const root = resolve(import.meta.dirname, "..");
process.chdir(root);
const argv = process.argv.slice(2);
const PASSED = ["--release", "--executable", "--probe", "--language", "--theme"];
const passed = [];
const names = [];
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === "--release") passed.push(argv[i]);
  else if (PASSED.includes(argv[i])) passed.push(argv[i], argv[++i]);
  else if (argv[i] !== "--list") names.push(argv[i]);
}
const directory = "tools/window-probe/steps";
const all = readdirSync(directory).filter((name) => name.endsWith(".json")).map((name) => name.slice(0, -5)).sort();
for (const name of names) assert(all.includes(name), `${name}: no such step file in ${directory}`);
const chosen = names.length ? names : all;
const known = JSON.parse(readFileSync("tools/window-probe/known-failures.json", "utf8"));
for (const name of Object.keys(known)) assert(all.includes(name), `known-failures.json: ${name} is not a step file`);

if (argv.includes("--list")) {
  for (const name of chosen) console.log(known[name] ? `${name}  (known failure: ${known[name]})` : name);
  process.exit(0);
}

// One at a time: test windows share the screen, the pointer position and the
// machine's load, and a recording under load drops frames.
const outcomes = [];
for (const name of chosen) {
  const file = `${directory}/${name}.json`;
  const plan = JSON.parse(readFileSync(file, "utf8"));
  const limit = (runSeconds(parseSteps(Array.isArray(plan) ? plan : plan.steps)) + 120) * 1000;
  const started = Date.now();
  const child = spawn(process.execPath, ["scripts/ui-shots.mjs", `steps-${name}`, file, ...passed], { windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
  let stderr = "";
  child.stdout.resume();
  child.stderr.on("data", (chunk) => (stderr += chunk));
  const timer = setTimeout(() => child.kill(), limit);
  const code = await new Promise((done) => child.on("exit", (status) => done(status ?? -1)));
  clearTimeout(timer);
  const failures = code === 0 ? [] : stderr.trim().split(/\r?\n/).filter(Boolean).slice(-8);
  const outcome = { name, ...verdict(code === 0, known[name]), seconds: Math.round((Date.now() - started) / 1000), failures };
  outcomes.push(outcome);
  console.log(`${outcome.result.padEnd(14)} ${name} (${outcome.seconds} s)${failures.length ? `\n    ${failures.join("\n    ")}` : ""}`);
}

const output = resolve("target/native-validation/steps");
mkdirSync(output, { recursive: true });
writeFileSync(resolve(output, "summary.json"), JSON.stringify({ arguments: argv, outcomes }, null, 2));
const count = (result) => outcomes.filter((outcome) => outcome.result === result).length;
console.log(`${outcomes.length} step files: ${count("passed")} passed, ${count("failed")} failed, ${count("known failure")} known failures, ${count("fixed")} listed as failing but passed`);
if (existsSync(resolve(output, "summary.json"))) console.log(`summary: ${resolve(output, "summary.json")}`);
process.exit(outcomes.some((outcome) => outcome.unexpected) ? 1 : 0);
