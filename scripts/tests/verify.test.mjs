import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { RUST_PACKAGE_PATHS, STEP_RULES, changedPaths, dependentsOf, plan, stepFilesFor } from "../lib/verify.mjs";

const root = join(import.meta.dirname, "..", "..");
const stepFiles = readdirSync(join(root, "tools/window-probe/steps"))
  .filter((f) => f.endsWith(".json") && f !== "known-failures.json")
  .map((f) => f.replace(/\.json$/, ""));
const names = (result) => result.checks.map((check) => check.name);
const find = (result, name) => result.checks.find((check) => check.name === name);

test("every step file is called for by some source, and every rule names real step files", () => {
  const named = new Set(STEP_RULES.flatMap(([, list]) => list));
  for (const name of named) assert(stepFiles.includes(name), `a rule names ${name}, which is not a step file`);
  for (const name of stepFiles) assert(named.has(name), `no rule in scripts/lib/verify.mjs runs ${name}.json: add it to the sources it checks`);
});

test("the package folders are the workspace's members", () => {
  const manifest = readFileSync(join(root, "Cargo.toml"), "utf8");
  const members = JSON.parse(/^members\s*=\s*(\[[^\]]*\])/m.exec(manifest)[1]);
  assert.deepEqual(Object.keys(RUST_PACKAGE_PATHS).sort(), [...members].sort());
  for (const [folder, name] of Object.entries(RUST_PACKAGE_PATHS)) {
    const own = readFileSync(join(root, folder, "Cargo.toml"), "utf8");
    assert.equal(/^name\s*=\s*"([^"]+)"/m.exec(own)[1], name, `${folder}/Cargo.toml`);
  }
});

test("locale texts are linted, checked for translations, and seen on every page", () => {
  const result = plan(["locales/zh-CN/app.json"]);
  assert.deepEqual(names(result), ["biome", "translations", "development build", "step files"]);
  assert.deepEqual(find(result, "step files").args, ["scripts/ui-steps.mjs", "localization"]);
  assert.deepEqual(names(plan(["locales/zh-CN/app.json"], { ui: false })), ["biome", "translations"]);
});

test("a script gets a syntax check and the script tests", () => {
  const result = plan(["scripts/lib/ui-shots.mjs"]);
  assert.deepEqual(names(result), ["syntax scripts/lib/ui-shots.mjs", "script tests"]);
  assert.deepEqual(find(result, "syntax scripts/lib/ui-shots.mjs").args, ["--check", "scripts/lib/ui-shots.mjs"]);
});

test("a Rust change formats and tests its package and lints what depends on it", () => {
  const dependents = { "argus-core": ["argus", "argus-app", "argus-service"] };
  const result = plan(["crates/argus-core/src/util.rs"], { dependents });
  assert.deepEqual(names(result), ["rustfmt", "clippy", "rust tests"]);
  assert.deepEqual(find(result, "rustfmt").args, ["fmt", "--check", "-p", "argus-core"]);
  const clippy = find(result, "clippy").args;
  for (const p of ["argus-core", "argus", "argus-app", "argus-service"]) assert(clippy.includes(p), p);
  const tests = find(result, "rust tests");
  assert.deepEqual(tests.args.slice(0, 4), ["test", "--locked", "-p", "argus-core"]);
  assert.equal(tests.env.ARGUS_LOCALE, "en-US", "as on CI's English Windows");
});

test("vendored GPUI is checked through Argus and the step files that cover it", () => {
  const result = plan(["vendor/gpui-pre/src/window.rs", "vendor/gpui-pre-windows/src/window.rs"]);
  assert(!names(result).includes("rustfmt"), "vendored code keeps upstream's formatting");
  assert(find(result, "clippy").args.includes("argus"));
  assert(find(result, "rust tests").args.includes("argus"));
  assert(find(result, "gpui-pre-windows tests"));
  assert(find(result, "step files").args.includes("table-rows"));
});

test("a frontend file runs its step files; one no rule names runs localization", () => {
  assert.deepEqual(stepFilesFor("src-gpui/src/frontend/star.rs"), ["star-reminder"]);
  assert.deepEqual(stepFilesFor("src-gpui/src/frontend/something_new.rs"), ["localization"]);
  // Every file of a page split into a folder runs that page's step files.
  assert.deepEqual(stepFilesFor("src-gpui/src/frontend/pages/settings/hotkey.rs"), ["section-rail", "service-damaged", "hardware-acceleration", "history-settings"]);
  // The byte unit setting is a card of the data section.
  assert.deepEqual(stepFilesFor("src-gpui/src/frontend/pages/settings/data.rs"), ["section-rail", "service-damaged", "hardware-acceleration", "history-settings", "byte-units"]);
  assert.deepEqual(stepFilesFor("tools/window-probe/steps/header-menu.json"), ["header-menu"]);
  assert.deepEqual(stepFilesFor("tools/window-probe/steps/known-failures.json"), []);
  assert.deepEqual(stepFilesFor("crates/argus-core/src/util.rs"), []);
});

test("the interface's logic outside GPUI runs the step files of the module that re-exports it", () => {
  for (const [moved, old] of [
    ["crates/argus-view-logic/src/columns.rs", "src-gpui/src/frontend/columns.rs"],
    ["crates/argus-view-logic/src/collation.rs", "src-gpui/src/frontend/collation.rs"],
    ["crates/argus-view-logic/src/gpu_usage.rs", "src-gpui/src/frontend/gpu_usage.rs"],
    ["crates/argus-view-logic/src/visibility.rs", "src-gpui/src/frontend/visibility.rs"],
    ["crates/argus-view-logic/src/history_series.rs", "src-gpui/src/frontend/pages/history/series.rs"],
    ["crates/argus-view-logic/src/taskbar_layout.rs", "src-gpui/src/shell/taskbar_draw.rs"],
    ["crates/argus-view-logic/src/model.rs", "src-gpui/src/frontend/model.rs"],
    ["crates/argus-view-logic/src/model/sensor_layout.rs", "src-gpui/src/frontend/model.rs"],
    ["crates/argus-view-logic/src/prefs.rs", "src-gpui/src/frontend/prefs.rs"],
  ])
    assert.deepEqual(stepFilesFor(moved), stepFilesFor(old), moved);
});

test("the lock file checks the whole workspace and the notices", () => {
  const result = plan(["Cargo.lock"]);
  assert.deepEqual(names(result), ["rustfmt", "clippy", "rust tests", "third-party notices"]);
  assert(find(result, "clippy").args.includes("--workspace"));
});

test("documents and workflows need no command, only a look", () => {
  const result = plan(["docs/development/gpui.md", "AGENTS.md"]);
  assert.deepEqual(result.checks, []);
  assert.match(result.notes.join("\n"), /documents/);
  const workflow = plan([".github/workflows/nightly.yml"]);
  assert.deepEqual(names(workflow), ["script tests"]);
  assert.match(workflow.notes.join("\n"), /workflows/);
});

test("changed paths come from git status, renamed files by their new name", () => {
  const porcelain = [" M scripts/a.mjs", "?? scripts/new.mjs", "R  docs/new.md", "docs/old.md", "A  src-gpui/src/x.rs", ""].join("\0");
  assert.deepEqual(changedPaths(porcelain), ["scripts/a.mjs", "scripts/new.mjs", "docs/new.md", "src-gpui/src/x.rs"]);
});

test("dependents reach through more than one package", () => {
  const metadata = {
    packages: [
      { name: "core", dependencies: [{ name: "serde" }] },
      { name: "app", dependencies: [{ name: "core" }] },
      { name: "ui", dependencies: [{ name: "app" }, { name: "core" }] },
    ],
  };
  assert.deepEqual(dependentsOf(metadata), { core: ["app", "ui"], app: ["ui"] });
});
