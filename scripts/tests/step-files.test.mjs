import assert from "node:assert/strict";
import { test } from "node:test";
import { readdirSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { aboutOf, parseSteps, runOptions } from "../lib/ui-shots.mjs";

const directory = resolve(import.meta.dirname, "../../tools/window-probe/steps");
const names = readdirSync(directory).filter((name) => name.endsWith(".json"));

// The step files are not run by `pnpm check`; what can be checked without a window is checked here.
test("every step file is well formed and says what it is for", () => {
  assert(names.length > 0);
  for (const name of names) {
    const plan = JSON.parse(readFileSync(resolve(directory, name), "utf8"));
    assert.doesNotThrow(() => parseSteps(Array.isArray(plan) ? plan : plan.steps), `${name}: steps`);
    assert.doesNotThrow(() => runOptions(plan), `${name}: run`);
    assert(aboutOf(plan).length >= 10, `${name}: add an "about" text saying what the steps check`);
    assert(/^[a-z0-9-]+\.json$/.test(name), `${name}: the name becomes a test label (lower-case letters, digits, hyphens)`);
  }
});

test("known failures name step files and give a reason", () => {
  const known = JSON.parse(readFileSync(resolve(directory, "../known-failures.json"), "utf8"));
  for (const [name, reason] of Object.entries(known)) {
    assert(names.includes(`${name}.json`), `${name} is not a step file`);
    assert(typeof reason === "string" && reason.trim().length >= 10, `${name}: say why it fails`);
  }
});
