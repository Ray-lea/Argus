import assert from "node:assert/strict";
import { test } from "node:test";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";

const fixtures = resolve(import.meta.dirname, "../../tools/window-probe/fixtures");

// Generated test data is committed; an edit to one side alone must not pass.
for (const generator of ["history.mjs", "history-picker.mjs"])
  test(`${generator} writes what is committed`, () => {
    const result = spawnSync(process.execPath, [resolve(fixtures, generator), "--check"], { encoding: "utf8" });
    assert.equal(result.status, 0, result.stdout + result.stderr);
  });
