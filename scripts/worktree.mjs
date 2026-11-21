// Worktrees for builds and changes that must not disturb the main checkout,
// where the development script rebuilds on every save and other people's
// uncommitted work may not compile.
//
//   node scripts/worktree.mjs add <name> [<ref>] [--branch <branch>] [--seed <profile>]…
//   node scripts/worktree.mjs remove <name>
//   node scripts/worktree.mjs list
//
// A worktree is created at target/worktrees/<name>, inside the repository
// folder, so the folder's rustup override (the MSVC toolchain) applies to it.
// Without --branch it is detached at <ref> (HEAD by default). --seed copies
// the compiled dependencies of a Cargo profile (debug, quick, release) from
// the main checkout, so that the first build there does not start from
// nothing; see docs/development/build.md for what that saves.
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import assert from "node:assert/strict";
import { seedEntries } from "./lib/worktree.mjs";

const root = resolve(import.meta.dirname, "..");
const home = resolve(root, "target/worktrees");
const [command, ...rest] = process.argv.slice(2);
const git = (args, options = {}) => {
  const result = spawnSync("git", args, { cwd: root, encoding: "utf8", ...options });
  if (result.status !== 0) throw new Error(`git ${args.slice(0, 2).join(" ")}: ${(result.stderr || result.stdout).trim()}`);
  return result.stdout;
};
const named = (name) => {
  assert(/^[a-z0-9][a-z0-9-]*$/.test(name ?? ""), "a worktree name is lower-case letters, digits and hyphens");
  return resolve(home, name);
};

if (command === "add") {
  const seeds = [];
  const positional = [];
  let branch;
  for (let i = 0; i < rest.length; i++) {
    if (rest[i] === "--seed") seeds.push(rest[++i]);
    else if (rest[i] === "--branch") branch = rest[++i];
    else if (rest[i].startsWith("--")) throw new Error(`unknown option ${rest[i]}`);
    else positional.push(rest[i]);
  }
  const [name, ref = "HEAD"] = positional;
  const path = named(name);
  assert(!existsSync(path), `${path} already exists`);
  for (const profile of seeds) assert(existsSync(resolve(root, "target", profile ?? "")) && profile, `--seed ${profile}: the main checkout has no target/${profile}`);
  mkdirSync(home, { recursive: true });
  git(["worktree", "add", ...(branch ? ["-b", branch] : ["--detach"]), path, ref], { stdio: ["ignore", "ignore", "pipe"] });
  for (const profile of seeds) {
    const from = resolve(root, "target", profile);
    const to = resolve(path, "target", profile);
    let copied = 0;
    for (const entry of seedEntries(from)) {
      // Cargo compares file times: a copy with new times would look newer than what was built from it.
      cpSync(join(from, entry), join(to, entry), { recursive: true, preserveTimestamps: true });
      copied++;
    }
    console.log(`seeded target/${profile}: ${copied} entries`);
  }
  console.log(`${path}\n  ${branch ? `branch ${branch}` : "detached"} at ${git(["-C", path, "log", "-1", "--format=%h %s"]).trim()}`);
  console.log("  build there with `node scripts/build.mjs [--quick]` or `cargo build -p argus --bin argus`; the development script does not watch it.");
} else if (command === "remove") {
  const path = named(rest[0]);
  assert(existsSync(path), `${path} does not exist`);
  try {
    git(["worktree", "remove", "--force", path], { stdio: ["ignore", "ignore", "pipe"] });
  } catch (error) {
    // Git gives up on a folder that a process still has open; what it could not delete is build output.
    console.error(error.message);
    rmSync(path, { recursive: true, force: true, maxRetries: 3, retryDelay: 300 });
  }
  git(["worktree", "prune"]);
  console.log(existsSync(path) ? `${path} is still there: a process has it open. Its files are gone; remove the folder later.` : `removed ${path}`);
} else if (command === "list") {
  const names = existsSync(home) ? readdirSync(home) : [];
  const known = git(["worktree", "list"]);
  for (const name of names) {
    const line = known.split(/\r?\n/).find((entry) => entry.replaceAll("\\", "/").toLowerCase().startsWith(resolve(home, name).replaceAll("\\", "/").toLowerCase()));
    console.log(line ? line.trim() : `${resolve(home, name)}  (not a registered worktree)`);
  }
  if (!names.length) console.log("no worktrees under target/worktrees");
} else {
  console.error("usage: node scripts/worktree.mjs add <name> [<ref>] [--branch <branch>] [--seed <profile>]… | remove <name> | list");
  process.exit(2);
}
