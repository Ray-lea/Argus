import assert from "node:assert/strict";
import { test } from "node:test";
import { resolve, dirname } from "node:path";
import { testDirectories } from "../lib/owned-test.mjs";

test("owned profile cleanup rejects real identities and path traversal", () => {
  const environment = { APPDATA: resolve("roaming"), LOCALAPPDATA: resolve("local") };
  for (const id of ["dev.argus.taskmanager", "dev.argus.taskmanager.test", "..", "dev.argus.taskmanager.test.a/../b", "dev.argus.taskmanager.test.a\\b"])
    assert.throws(() => testDirectories(id, environment));
  assert.deepEqual(testDirectories("dev.argus.taskmanager.test.run-1", environment).map(dirname), Object.values(environment));
  assert.throws(() => testDirectories("dev.argus.taskmanager.test.run-1", {}));
});
