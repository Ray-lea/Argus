// Runs the ignored tests that need a real, disposable machine
// (scripts/lib/live-tests.mjs) group by group, carries on after a failing
// group, and writes a summary. A group whose conditions this machine does not
// meet (standard rights, UAC, a second volume) is skipped with the reason.
// The nightly workflow runs it on its runners; in a lab VM set
// ARGUS_DISPOSABLE_MACHINE=1.
//
//   node scripts/live-tests.mjs [GROUP…] [--second-volume <folder>] [--output <folder>] [--list]
import { spawnSync } from "node:child_process";
import { appendFileSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join, parse, resolve } from "node:path";
import { tmpdir } from "node:os";
import assert from "node:assert/strict";
import { GROUPS, cargoArgs, environmentOf, outcomes, placeholders, unmet } from "./lib/live-tests.mjs";
import { elevated, registryValue, requireDisposable } from "./lib/machine-smoke.mjs";

const root = resolve(import.meta.dirname, "..");
process.chdir(root);
const argv = process.argv.slice(2);
const option = (name, fallback) => (argv.includes(name) ? argv[argv.indexOf(name) + 1] : fallback);
const named = argv.filter((arg, i) => !arg.startsWith("--") && !argv[i - 1]?.startsWith("--"));
for (const name of named) assert(GROUPS.some((group) => group.name === name), `no group "${name}"; --list shows them`);
const chosen = named.length ? GROUPS.filter((group) => named.includes(group.name)) : GROUPS;

if (argv.includes("--list")) {
  for (const group of chosen)
    console.log(`${group.name}: ${group.about}${group.requires ? ` (needs ${group.requires.join(", ")})` : ""}\n    ${group.tests.join("\n    ")}`);
  process.exit(0);
}
requireDisposable();

const output = resolve(option("--output", "target/nightly/live-tests"));
mkdirSync(output, { recursive: true });

/** A writable folder on another volume than %TEMP%, or undefined. */
function secondVolume() {
  const temp = parse(resolve(tmpdir())).root.toLowerCase();
  const candidates = option("--second-volume") ? [option("--second-volume")] : "CDEFGHIJ".split("").map((letter) => `${letter}:\\argus-second-volume`);
  for (const candidate of candidates) {
    const folder = resolve(candidate);
    if (parse(folder).root.toLowerCase() === temp || !existsSync(parse(folder).root)) continue;
    try {
      mkdirSync(folder, { recursive: true });
      return folder;
    } catch {
      // Read-only or not a fixed volume; try the next.
    }
  }
  return undefined;
}

const facts = {
  elevated: elevated(),
  // EnableLUA 0 turns UAC off; a missing value means on.
  uac: registryValue(String.raw`HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System`, "EnableLUA") !== "0x0",
  values: { account: process.env.USERNAME, secondVolume: secondVolume() },
  missing: { secondVolume: "needs a second writable volume; this machine has only the one %TEMP% is on" },
};
const runnable = chosen.filter((group) => !unmet(group, { ...facts, values: { ...facts.values, argus: "later" } }));
if (placeholders(runnable).has("argus")) {
  // The development build, as `cargo build -p argus` makes it.
  const built = spawnSync("cargo", ["build", "--locked", "-p", "argus", "--bin", "argus"], { stdio: "inherit", windowsHide: true });
  assert.equal(built.status, 0, "cargo build -p argus failed");
  facts.values.argus = resolve("target/debug/argus.exe");
}
console.log(`elevated: ${facts.elevated}; UAC: ${facts.uac}; second volume: ${facts.values.secondVolume ?? "none"}`);

const results = [];
for (const group of chosen) {
  console.log(`\n=== ${group.name}: ${group.about}`);
  const reason = unmet(group, facts);
  if (reason) {
    console.log(`skipped: ${reason}`);
    results.push({ group: group.name, skipped: reason, tests: group.tests.map((test) => ({ test, result: "skipped" })) });
    continue;
  }
  const started = Date.now();
  const run = spawnSync("cargo", cargoArgs(group), {
    encoding: "utf8",
    windowsHide: true,
    maxBuffer: 256 * 1024 * 1024,
    env: { ...process.env, ...environmentOf(group, facts.values) },
  });
  const text = `${run.stdout ?? ""}\n${run.stderr ?? ""}`;
  writeFileSync(join(output, `${group.name.replace(/\W+/g, "-")}.log`), text);
  const tests = outcomes(group, run.stdout ?? "");
  const passed = run.status === 0 && tests.every((test) => test.result === "passed");
  results.push({ group: group.name, passed, exitCode: run.status, seconds: Math.round((Date.now() - started) / 1000), tests });
  for (const test of tests) console.log(`${test.result.padEnd(8)} ${test.test}`);
  if (!passed) console.log(text.split(/\r?\n/).filter((line) => /^error|error\[|panicked|exit code: 0x/.test(line)).slice(0, 20).join("\n"));
}

writeFileSync(join(output, "summary.json"), JSON.stringify({ facts: { elevated: facts.elevated, uac: facts.uac, secondVolume: Boolean(facts.values.secondVolume) }, results }, null, 2));
const failed = results.filter((result) => !result.skipped && !result.passed);
const lines = [
  `| group | result | tests | seconds |`,
  `|---|---|---|---|`,
  ...results.map((r) =>
    r.skipped
      ? `| ${r.group} | skipped: ${r.skipped} | 0/${r.tests.length} | |`
      : `| ${r.group} | ${r.passed ? "passed" : "**failed**"} | ${r.tests.filter((t) => t.result === "passed").length}/${r.tests.length} | ${r.seconds} |`,
  ),
];
if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, `### Live tests\n\n${lines.join("\n")}\n`);
console.log(`\n${lines.join("\n")}\nsummary: ${join(output, "summary.json")}`);
process.exit(failed.length ? 1 : 0);
