import assert from "node:assert/strict";
import { test } from "node:test";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve, dirname, join } from "node:path";
import { cleanupTestProfile, panics, testDirectories } from "../lib/owned-test.mjs";

test("owned profile cleanup rejects real identities and path traversal", () => {
  const environment = { APPDATA: resolve("roaming"), LOCALAPPDATA: resolve("local") };
  for (const id of ["dev.argus.taskmanager", "dev.argus.taskmanager.test", "..", "dev.argus.taskmanager.test.a/../b", "dev.argus.taskmanager.test.a\\b"])
    assert.throws(() => testDirectories(id, environment));
  assert.deepEqual(testDirectories("dev.argus.taskmanager.test.run-1", environment).map(dirname), Object.values(environment));
  assert.throws(() => testDirectories("dev.argus.taskmanager.test.run-1", {}));
});

test("cleanup keeps the diagnostics logs of the removed profile", () => {
  const root = mkdtempSync(join(tmpdir(), "argus-owned-test-"));
  try {
    const environment = { APPDATA: join(root, "roaming"), LOCALAPPDATA: join(root, "local") };
    const identifier = "dev.argus.taskmanager.test.logs-1";
    const [config, data] = testDirectories(identifier, environment);
    const diagnostics = join(data, "logs", "diagnostics");
    mkdirSync(join(diagnostics, "older"), { recursive: true });
    mkdirSync(config, { recursive: true });
    writeFileSync(join(config, "settings.json"), "{}");
    writeFileSync(join(diagnostics, "argus.log"), "start\r\n12:00:01 ERROR panic at src\\frontend\\tips.rs:42\r\nnot a panicky line\r\n");
    writeFileSync(join(diagnostics, "older", "nested.log"), "nested");

    const kept = cleanupTestProfile(identifier, { logs: join(root, "out", "logs"), environment });
    assert.deepEqual(kept, [join(root, "out", "logs", "argus.log")]);
    assert(readFileSync(kept[0], "utf8").startsWith("start"));
    assert(!existsSync(join(root, "out", "logs", "older")), "only the plain files of the folder are kept");
    assert(!existsSync(config) && !existsSync(data));
    assert.deepEqual(panics(kept), ["12:00:01 ERROR panic at src\\frontend\\tips.rs:42"]);

    // A profile that never wrote a log, and a cleanup that keeps nothing.
    mkdirSync(config, { recursive: true });
    assert.deepEqual(cleanupTestProfile(identifier, { logs: join(root, "out", "none"), environment }), []);
    assert(!existsSync(join(root, "out", "none")) && !existsSync(config));
    assert.deepEqual(cleanupTestProfile(identifier, { environment }), []);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
