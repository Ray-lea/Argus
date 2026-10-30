import assert from "node:assert/strict";
import { lstatSync, rmSync } from "node:fs";
import { dirname, resolve } from "node:path";

// Only direct children of the two application-data roots, with a test identity.
export function testDirectories(identifier, environment = process.env) {
  assert(/^dev\.argus\.taskmanager\.test\.[a-z0-9-]+$/.test(identifier), "isolated test identifier required");
  return ["APPDATA", "LOCALAPPDATA"].map((key) => {
    assert(environment[key], `${key} required`);
    const root = resolve(environment[key]);
    const path = resolve(root, identifier);
    assert.equal(dirname(path), root);
    return path;
  });
}

export function cleanupTestProfile(identifier) {
  for (const path of testDirectories(identifier)) {
    const stat = lstatSync(path, { throwIfNoEntry: false });
    assert(!stat?.isSymbolicLink(), "test profile must not be a link");
    rmSync(path, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  }
}
