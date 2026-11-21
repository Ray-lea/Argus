import assert from "node:assert/strict";
import { test } from "node:test";
import { seedEntries } from "../lib/worktree.mjs";

test("a worktree is seeded with compiled dependencies only", () => {
  const current = ["argus.exe", "argus.pdb", "build", "examples", "incremental", ".cargo-lock"];
  assert.deepEqual(seedEntries("target/quick", () => current), ["build"]);
  const older = ["deps", ".fingerprint", "build", "incremental", "argus.exe", "argus.d"];
  assert.deepEqual(seedEntries("target/release", () => older), [".fingerprint", "build", "deps"]);
  assert.deepEqual(seedEntries("target/empty", () => []), []);
});
