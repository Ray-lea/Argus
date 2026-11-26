// Runs the checks the changed files call for, as AGENTS.md's table of
// verification scope sets them out (rules in scripts/lib/verify.mjs): Biome
// and translations for locales, syntax and script tests for scripts, rustfmt,
// Clippy (with the packages depending on the changed ones) and tests for Rust,
// and the step files of the interface that changed. Carries on after a failing
// check and reports all of them.
//
//   node scripts/verify.mjs [FILE…] [--since REV] [--no-ui] [--plan]
//
// Without FILE, the changed files are those of the working tree (staged,
// unstaged and untracked); with --since, also everything committed since REV
// (`--since origin/main` checks what a push would bring). In a checkout that
// others share, name your own files: their unfinished changes are in the
// working tree too. --plan prints the checks without running them.
import { spawnSync } from "node:child_process";
import { readdirSync } from "node:fs";
import { resolve } from "node:path";
import assert from "node:assert/strict";
import { changedPaths, dependentsOf, plan } from "./lib/verify.mjs";

const root = resolve(import.meta.dirname, "..");
process.chdir(root);
const argv = process.argv.slice(2);
const option = (name) => (argv.includes(name) ? argv[argv.indexOf(name) + 1] : undefined);
const since = option("--since");
const named = argv.filter((arg, i) => !arg.startsWith("--") && argv[i - 1] !== "--since");

const git = (...args) => {
  const result = spawnSync("git", args, { encoding: "utf8", windowsHide: true, maxBuffer: 64 * 1024 * 1024 });
  assert.equal(result.status, 0, `git ${args.join(" ")}: ${result.stderr}`);
  return result.stdout;
};
let files = named.map((file) => file.replaceAll("\\", "/").replace(/^\.\//, ""));
if (!files.length) {
  files = changedPaths(git("status", "--porcelain=v1", "-z", "--untracked-files=all"));
  if (since) files.push(...git("diff", "--name-only", "-z", `${since}...HEAD`).split("\0").filter(Boolean));
}
files = [...new Set(files)].sort();
if (!files.length) {
  console.log("nothing changed");
  process.exit(0);
}

const metadata = spawnSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], { encoding: "utf8", windowsHide: true, maxBuffer: 64 * 1024 * 1024 });
assert.equal(metadata.status, 0, `cargo metadata: ${metadata.stderr}`);
const stepNames = readdirSync("tools/window-probe/steps")
  .filter((f) => f.endsWith(".json") && f !== "known-failures.json")
  .map((f) => f.replace(/\.json$/, ""));
const { checks, notes } = plan(files, { dependents: dependentsOf(JSON.parse(metadata.stdout)), ui: !argv.includes("--no-ui"), stepNames });

console.log(`${files.length} changed file(s)`);
for (const check of checks) console.log(`- ${check.name}: ${[check.command, ...check.args].join(" ")}\n    for ${check.why.slice(0, 3).join(", ")}${check.why.length > 3 ? ` and ${check.why.length - 3} more` : ""}`);
for (const note of notes) console.log(`- not checked here: ${note}`);
if (!checks.length) console.log("no command applies: check the diff by eye");
if (argv.includes("--plan")) process.exit(0);

const outcomes = [];
for (const check of checks) {
  console.log(`\n=== ${check.name}`);
  const started = Date.now();
  // pnpm is a .cmd on Windows: run it through the shell, arguments are ours.
  const result = spawnSync(check.command, check.args, {
    stdio: "inherit",
    windowsHide: true,
    shell: check.command === "pnpm",
    env: { ...process.env, ...check.env },
  });
  outcomes.push({ name: check.name, passed: result.status === 0, seconds: Math.round((Date.now() - started) / 1000) });
}
console.log("\n| check | result | seconds |\n|---|---|---|");
for (const o of outcomes) console.log(`| ${o.name} | ${o.passed ? "passed" : "**failed**"} | ${o.seconds} |`);
process.exit(outcomes.every((o) => o.passed) ? 0 : 1);
