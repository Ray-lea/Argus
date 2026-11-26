import assert from "node:assert/strict";
import { test } from "node:test";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pruneRuns, staleRuns } from "../lib/dev-runs.mjs";

test("only older development copies are stale", () => {
  const names = ["run-5", "run-1", "run-40", "run-3", "hold.json", "run-x", "run-2"];
  assert.deepEqual(staleRuns(names, "run-40", 3), ["run-2", "run-1"]);
  // The running copy is never stale, even when it is not among the newest.
  assert.deepEqual(staleRuns(names, "run-1", 2), ["run-3", "run-2"]);
  assert.deepEqual(staleRuns(["run-1"], "run-1", 0), []);
});

test("pruning removes stale run folders and nothing else", () => {
  const root = mkdtempSync(join(tmpdir(), "argus-dev-runs-"));
  try {
    for (const name of ["run-1", "run-2", "run-3", "run-4", "run-5", "keep-me"]) {
      mkdirSync(join(root, name));
      writeFileSync(join(root, name, "argus.exe"), "");
    }
    assert.deepEqual(pruneRuns(root, "run-5", 2).sort(), ["run-1", "run-2", "run-3"]);
    assert.deepEqual(readdirSync(root).sort(), ["keep-me", "run-4", "run-5"]);
    assert.deepEqual(pruneRuns(join(root, "missing"), "run-5"), []);
    assert(existsSync(join(root, "keep-me", "argus.exe")));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
