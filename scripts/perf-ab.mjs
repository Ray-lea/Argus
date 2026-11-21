// Compares two builds (or one build under two environments) by alternating
// runs of scripts/perf-native.mjs, and reports per phase how the new side
// differs from the base: the method of docs/development/performance.md in one
// command.
//
//   node scripts/perf-ab.mjs <label> <base.exe> <new.exe> [--pairs N]
//     [--base-env NAME=VALUE]… [--new-env NAME=VALUE]… [--default-scheduling]
//     [-- <options of perf-native.mjs>…]
//
// Each pair measures both sides back to back, and the side that goes first
// alternates. Runs are confined to the performance cores (see
// ARGUS_PROBE_PERFORMANCE_CORES) unless --default-scheduling is given. The
// same executable may stand on both sides with --base-env / --new-env, to
// compare a switch such as ARGUS_TABLE_ROWS=component. Everything after `--`
// goes to every run: --scenario, --fixture, --stream-fixture, --seconds,
// --width, --no-capture and so on.
//
// Runs are written to target/native-validation/<label>-b<N> and -n<N>, the
// comparison to target/native-validation/<label>/summary.json.
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import assert from "node:assert/strict";
import { assignments, compare, format, phaseFigures, sides } from "./lib/perf-ab.mjs";

const root = resolve(import.meta.dirname, "..");
process.chdir(root);
const argv = process.argv.slice(2);
const split = argv.indexOf("--");
const own = split < 0 ? argv : argv.slice(0, split);
const passed = split < 0 ? [] : argv.slice(split + 1);
const positional = [];
const environments = { base: [], new: [] };
let pairs = 3;
let performanceCores = true;
for (let i = 0; i < own.length; i++) {
  if (own[i] === "--pairs") pairs = Number(own[++i]);
  else if (own[i] === "--base-env") environments.base.push(own[++i] ?? "");
  else if (own[i] === "--new-env") environments.new.push(own[++i] ?? "");
  else if (own[i] === "--default-scheduling") performanceCores = false;
  else if (own[i].startsWith("--")) throw new Error(`unknown option ${own[i]}: options of perf-native.mjs go after --`);
  else positional.push(own[i]);
}
const [label, ...inputs] = positional;
assert(
  /^[a-z0-9-]+$/.test(label ?? "") && inputs.length === 2,
  "usage: node scripts/perf-ab.mjs <label> <base.exe> <new.exe> [--pairs N] [--base-env NAME=VALUE]… [--new-env NAME=VALUE]… [-- perf-native options…]",
);
assert(Number.isInteger(pairs) && pairs >= 1 && pairs <= 20, "--pairs is a whole number from 1 to 20");
const executables = { base: resolve(inputs[0]), new: resolve(inputs[1]) };
for (const path of Object.values(executables)) assert(existsSync(path), `${path} is missing`);
const env = { base: assignments(environments.base), new: assignments(environments.new) };
assert(
  executables.base !== executables.new || JSON.stringify(env.base) !== JSON.stringify(env.new),
  "both sides are the same executable in the same environment",
);
const output = resolve("target/native-validation", label);
mkdirSync(output, { recursive: true });

async function measure(side, pair) {
  const name = `${label}-${side[0]}${pair + 1}`;
  const child = spawn(process.execPath, ["scripts/perf-native.mjs", name, executables[side], ...passed], {
    windowsHide: true,
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, ...(performanceCores ? { ARGUS_PROBE_PERFORMANCE_CORES: "1" } : {}), ...env[side] },
  });
  let text = "";
  child.stdout.on("data", (chunk) => (text += chunk));
  child.stderr.on("data", (chunk) => (text += chunk));
  const code = await new Promise((done) => child.on("exit", (status) => done(status ?? -1)));
  writeFileSync(resolve(output, `${name}.log`), text);
  const run = resolve("target/native-validation", name);
  // Each run works on its own copy of the executable.
  for (const file of ["argus.exe", "argus.pdb"]) rmSync(resolve(run, file), { force: true, maxRetries: 5, retryDelay: 200 });
  if (code !== 0) throw new Error(`${name} failed (${code}):\n${text.trim().split(/\r?\n/).slice(-12).join("\n")}`);
  const figures = phaseFigures(JSON.parse(readFileSync(resolve(run, "result.json"), "utf8")));
  console.log(`${name}: ${figures.map((entry) => `${entry.phase} ${entry.figures.cpuMsPerSecond?.toFixed(0) ?? "-"} ms/s ${entry.figures.presentedFps?.toFixed(0) ?? "-"} fps`).join(", ")}`);
  return figures;
}

const runs = [];
for (let pair = 0; pair < pairs; pair++) {
  const figures = {};
  for (const side of sides(pair)) figures[side] = await measure(side, pair);
  runs.push(figures);
}
const report = compare(runs);
writeFileSync(
  resolve(output, "summary.json"),
  JSON.stringify({ label, executables, environments: env, performanceCores, options: passed, runs, report }, null, 2),
);
console.log(`base: ${inputs[0]}${environments.base.length ? ` (${environments.base.join(" ")})` : ""}`);
console.log(`new:  ${inputs[1]}${environments.new.length ? ` (${environments.new.join(" ")})` : ""}`);
console.log(`change is new/base - 1, as the median over the pairs; ${performanceCores ? "performance cores only" : "default scheduling"}`);
for (const line of format(report)) console.log(line);
console.log(`\nsummary: ${resolve(output, "summary.json")}`);
