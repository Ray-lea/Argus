import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { GROUPS, cargoArgs, environmentOf, outcomes, placeholders } from "../lib/live-tests.mjs";

/** Every `#[ignore]` test function of a source tree, by name. */
function ignoredFunctions(directory, found = new Set()) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== "target") ignoredFunctions(path, found);
    } else if (entry.name.endsWith(".rs")) {
      for (const match of readFileSync(path, "utf8").matchAll(/#\[ignore[^\]]*\]\s*(?:#\[[^\]]*\]\s*)*(?:pub\s+)?fn\s+(\w+)/g)) found.add(match[1]);
    }
  }
  return found;
}

test("every listed live test is an ignored test of this tree, listed once", () => {
  const root = join(import.meta.dirname, "..", "..");
  const ignored = new Set([...ignoredFunctions(join(root, "crates")), ...ignoredFunctions(join(root, "src-gpui"))]);
  const all = GROUPS.flatMap((group) => group.tests);
  assert.equal(new Set(all).size, all.length, "a test is listed twice");
  for (const name of all) assert(ignored.has(name.split("::").at(-1)), `${name} is not an #[ignore] test (renamed or no longer ignored?)`);
  assert.equal(new Set(GROUPS.map((group) => group.name)).size, GROUPS.length);
});

test("a group runs only its ignored tests, exactly and one at a time", () => {
  const group = { name: "g", cargo: ["-p", "argus-core", "--lib"], tests: ["a::b", "c"], env: { X: "{argus}", Y: "plain" } };
  assert.deepEqual(cargoArgs(group), ["test", "--locked", "-p", "argus-core", "--lib", "--", "--ignored", "--exact", "--test-threads=1", "a::b", "c"]);
  assert.deepEqual(environmentOf(group, { argus: "D:\\a\\argus.exe" }), { X: "D:\\a\\argus.exe", Y: "plain" });
  assert.throws(() => environmentOf(group, {}), /no value for \{argus\}/);
  assert.deepEqual([...placeholders(GROUPS)].sort(), ["account", "argus", "secondVolume"]);
});

test("outcomes come from libtest's lines; a test that never ran is missing", () => {
  const group = { tests: ["a::b", "c", "d"] };
  const output = "running 2 tests\ntest a::b ... ok\ntest c ... FAILED\n\nfailures:\n";
  assert.deepEqual(outcomes(group, output), [
    { test: "a::b", result: "passed" },
    { test: "c", result: "failed" },
    { test: "d", result: "missing" },
  ]);
});
