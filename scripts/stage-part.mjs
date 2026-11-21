// Stages part of one file's unstaged changes, for a file that holds changes
// of several commits or of someone else. `git add -p` needs a terminal; this
// does not.
//
//   node scripts/stage-part.mjs <file>                               list the hunks
//   node scripts/stage-part.mjs <file> <hunks|all> [--leave TEXT]… [--dry-run]
//
// <hunks> are numbers from the list, separated by commas. `--leave` keeps a
// changed line that contains TEXT out of the index: an added line is not
// staged, a removed line stays. `--dry-run` prints the patch instead of
// applying it. The working tree is never changed.
import { spawnSync } from "node:child_process";
import assert from "node:assert/strict";
import { listHunks, selectPatch } from "./lib/patch-part.mjs";

const argv = process.argv.slice(2);
const dry = argv.includes("--dry-run");
const leave = [];
const rest = [];
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === "--leave") leave.push(argv[++i]);
  else if (argv[i] !== "--dry-run") rest.push(argv[i]);
}
const [file, choice] = rest;
assert(file && rest.length <= 2 && leave.every(Boolean), "usage: node scripts/stage-part.mjs <file> [<hunks|all> [--leave TEXT]… [--dry-run]]");

const git = (args, input) => {
  const result = spawnSync("git", args, { input, encoding: "utf8", maxBuffer: 256 * 1024 ** 2 });
  if (result.status !== 0) throw Error(`git ${args[0]}: ${result.stderr.trim()}`);
  return result.stdout;
};
const diff = git(["diff", "--no-color", "--no-ext-diff", "--", file]);
assert(diff, `${file}: no unstaged changes`);
if (!choice) {
  console.log(listHunks(diff).join("\n"));
  process.exit(0);
}
const keep = choice === "all" ? "all" : choice.split(",").map(Number);
const { patch, kept } = selectPatch(diff, keep, leave);
assert(kept, "nothing left to stage");
if (dry) process.stdout.write(patch);
else {
  git(["apply", "--cached", "--recount", "-"], patch);
  console.log(`${file}: staged ${kept} hunk(s)${leave.length ? `, left out lines containing ${leave.map((part) => JSON.stringify(part)).join(", ")}` : ""}`);
}
