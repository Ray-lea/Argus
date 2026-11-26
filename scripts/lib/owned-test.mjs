import assert from "node:assert/strict";
import { copyFileSync, lstatSync, mkdirSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

// Only direct children of the two application-data roots, with a test identity.
// With ARGUS_TEST_DATA_ROOT set, a test instance keeps them under that folder
// instead (`<root>\APPDATA\<identifier>`, `<root>\LOCALAPPDATA\<identifier>`):
// the instance reads the same variable, so it is passed on to the child.
export function testDirectories(identifier, environment = process.env) {
  assert(/^dev\.argus\.taskmanager\.test\.[a-z0-9-]+$/.test(identifier), "isolated test identifier required");
  const own = environment.ARGUS_TEST_DATA_ROOT;
  return ["APPDATA", "LOCALAPPDATA"].map((key) => {
    assert(own || environment[key], `${key} required`);
    const root = own ? resolve(own, key) : resolve(environment[key]);
    const path = resolve(root, identifier);
    assert.equal(dirname(path), root);
    return path;
  });
}

/**
 * Removes the folders of a test identity. With `logs`, the instance's
 * diagnostics logs are copied into that folder first and their paths
 * returned: a test instance that panics aborts, and the place of the panic is
 * written nowhere else.
 */
export function cleanupTestProfile(identifier, { logs, environment = process.env } = {}) {
  const directories = testDirectories(identifier, environment);
  for (const path of directories) {
    const stat = lstatSync(path, { throwIfNoEntry: false });
    assert(!stat?.isSymbolicLink(), "test profile must not be a link");
  }
  const kept = logs ? keepLogs(join(directories[1], "logs", "diagnostics"), logs) : [];
  for (const path of directories) rmSync(path, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  return kept;
}

// The plain files of the diagnostics folder; nothing below it and no links.
function keepLogs(from, to) {
  let entries;
  try {
    entries = readdirSync(from, { withFileTypes: true });
  } catch {
    return [];
  }
  const kept = [];
  for (const entry of entries) {
    if (!entry.isFile()) continue;
    mkdirSync(to, { recursive: true });
    const path = join(to, entry.name);
    copyFileSync(join(from, entry.name), path);
    kept.push(path);
  }
  return kept;
}

/** The lines of kept logs that record a panic, for the failure message of a run. */
export function panics(logs) {
  return logs.flatMap((path) =>
    readFileSync(path, "utf8")
      .split(/\r?\n/)
      .filter((line) => /\bpanic\b/.test(line))
      .map((line) => line.trim()),
  );
}
