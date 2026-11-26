// Which checks a set of changed files calls for (scripts/verify.mjs): the
// "验证范围与频率" table of AGENTS.md as rules from paths to commands. Pure,
// so that the plan for given files can be tested.

/** Locale-tagged environment of Rust tests, as CI runs them (English Windows). */
const TEST_ENV = { ARGUS_LOCALE: "en-US" };
const RUST_TARGET = ["--target-dir", "target/rust-analyzer"];

/**
 * Step files (tools/window-probe/steps) to run when a source file changes,
 * by path. A frontend file no rule names falls back to `localization`, which
 * passes every page. Every step file is named by at least one rule
 * (scripts/tests/verify.test.mjs).
 */
export const STEP_RULES = [
  [/^src-gpui\/src\/frontend\/pages\/about\.rs$/, ["about-links", "about-scroll", "diagnostic-summary"]],
  [/^src-gpui\/src\/frontend\/diagnostic_summary\.rs$/, ["diagnostic-summary"]],
  [/^src-gpui\/src\/frontend\/pages\/(history|watch)\/|^crates\/argus-view-logic\/src\/history_series\.rs$/, ["history-page", "history-menus", "history-picker"]],
  [/^src-gpui\/src\/frontend\/pages\/history\/settings\.rs$/, ["history-settings"]],
  [/^src-gpui\/src\/frontend\/pages\/performance\/disk_details/, ["disk-details-layout", "disk-details-held", "disk-counters-unavailable"]],
  [/^src-gpui\/src\/frontend\/pages\/performance/, ["performance-menus", "cpu-cores", "device-order", "gpu-low-power"]],
  [/^src-gpui\/src\/frontend\/(charts|gpu_usage|hundred)|^crates\/argus-view-logic\/src\/gpu_usage\.rs$/, ["performance-menus", "cpu-cores", "gpu-low-power", "desk-live"]],
  [/^src-gpui\/src\/frontend\/pages\/(settings\/|section_rail\.rs$)/, ["section-rail", "service-damaged", "hardware-acceleration", "history-settings"]],
  [/^src-gpui\/src\/frontend\/pages\/settings\/data\.rs$/, ["byte-units"]],
  [/^src-gpui\/src\/frontend\/pages\/components\.rs$/, ["tray-icon", "monitor-metrics", "section-rail", "tray-flyouts"]],
  [/^src-gpui\/src\/frontend\/pages\/vitals\.rs$/, ["vitals-visibility"]],
  [/^src-gpui\/src\/frontend\/pages\/details/, ["process-copy", "process-facts", "process-counters", "power-columns"]],
  [/^src-gpui\/src\/frontend\/(tables|plain)/, ["table-rows", "column-widths", "column-widths-stable", "process-groups"]],
  [/^src-gpui\/src\/frontend\/tables\/touch\.rs$/, ["touch-input"]],
  [/^src-gpui\/src\/frontend\/tables\/export\.rs$|^src-gpui\/src\/frontend\/list_copy\.rs$/, ["table-csv", "process-copy"]],
  [/^src-gpui\/src\/frontend\/columns|^crates\/argus-view-logic\/src\/columns\.rs$/, ["list-columns", "column-drag", "process-facts", "process-counters", "power-columns", "header-menu", "process-no-gpu"]],
  [/^src-gpui\/src\/frontend\/menus\.rs$/, ["header-menu", "performance-menus", "history-menus", "process-tree"]],
  [/^src-gpui\/src\/frontend\/(lists|collation|type_search)\.rs$|^crates\/argus-view-logic\/src\/collation\.rs$/, ["process-tree", "process-groups", "process-order-held", "process-visibility"]],
  [/^src-gpui\/src\/frontend\/(users|visibility)\.rs$|^crates\/argus-view-logic\/src\/visibility\.rs$/, ["process-users", "process-visibility"]],
  [/^src-gpui\/src\/frontend\/(monitors|tray\/monitor)|^src-gpui\/src\/shell\/(desk|taskbar|taskbar_draw|displays)\.rs$|^crates\/argus-view-logic\/src\/taskbar_layout\.rs$/,["monitors", "monitor-display", "monitor-order", "monitor-metrics", "desk-looks", "desk-start", "desk-live"]],
  [/^src-gpui\/src\/frontend\/tray|^src-gpui\/src\/shell\/tray/, ["tray-flyouts", "tray-reopen", "tray-page", "tray-icon"]],
  [/^src-gpui\/src\/frontend\/caption\.rs$/, ["caption-buttons"]],
  [/^src-gpui\/src\/frontend\/dialogs\/file_lock\.rs$/, ["file-lock-layout"]],
  [/^src-gpui\/src\/frontend\/dialogs\/process_actions\.rs$/, ["process-suspend", "criticality-unknown"]],
  [/^src-gpui\/src\/frontend\/dialogs\/service_actions\.rs$/, ["delete-entries"]],
  [/^src-gpui\/src\/frontend\/leaks\.rs$/, ["leak-alerts"]],
  [/^src-gpui\/src\/frontend\/star\.rs$/, ["star-reminder"]],
  [/^src-gpui\/src\/frontend\/(portable|welcome)\.rs$/, ["portable-notice"]],
  [/^src-gpui\/src\/frontend\/release_notes\.rs$/, ["update-reader"]],
  [/^src-gpui\/src\/frontend\/(scrollbar|shift)\.rs$/, ["scale-scroll", "about-scroll"]],
  [/^src-gpui\/src\/frontend\/reorder\.rs$/, ["device-order", "monitor-order", "column-drag"]],
  [/^src-gpui\/src\/frontend\/(tips|icon_motion)\.rs$/, ["column-widths", "caption-buttons", "about-links"]],
  [/^src-gpui\/src\/frontend\/motion\.rs$|^src-gpui\/src\/window_motion/, ["window-motion", "vitals-visibility", "tray-reopen"]],
  [/^src-gpui\/src\/frontend\/(theme|appearance|assets)\.rs$/, ["localization", "backdrop-fallback", "caption-buttons"]],
  [/^src-gpui\/src\/frontend\/units\.rs$/, ["byte-units", "column-widths", "monitor-metrics", "vitals-visibility"]],
  [/^src-gpui\/src\/frontend\.rs$|^src-gpui\/src\/frontend\/(layout|pages|widgets|inline)\.rs$/, ["localization", "status-rate"]],
  [/^src-gpui\/src\/(window_state|platform\/)/, ["window-maximized-restore", "window-restore"]],
  [/^src-gpui\/src\/frontend\/backend\.rs$/, ["localization", "process-order-held"]],
  // The rest of the interface's logic outside GPUI, as for a frontend file
  // no rule names.
  [/^crates\/argus-view-logic\/src\/(model|prefs)[/.]/, ["localization"]],
  [/^locales\//, ["localization"]],
  [/^vendor\/gpui-pre[\w-]*\/src\//, ["table-rows", "localization", "scale-scroll", "touch-input"]],
];

/** Workspace packages by folder (the tests hold it to Cargo.toml's members). */
export const RUST_PACKAGE_PATHS = {
  "crates/argus-core": "argus-core",
  "crates/argus-service": "argus-service",
  "crates/argus-app": "argus-app",
  "crates/argus-view-logic": "argus-view-logic",
  "src-gpui": "argus",
  "tools/window-probe": "argus-window-probe",
  "tools/update-signer": "argus-update-signer",
};

/** The workspace package a path belongs to, or null. */
export function packageOf(path, packages = RUST_PACKAGE_PATHS) {
  for (const [folder, name] of Object.entries(packages)) if (path === folder || path.startsWith(`${folder}/`)) return name;
  return null;
}

/** The step files a changed path calls for: its own name for a step file, the rules' otherwise. */
export function stepFilesFor(path) {
  const own = /^tools\/window-probe\/steps\/([\w-]+)\.json$/.exec(path);
  if (own) return own[1] === "known-failures" ? [] : [own[1]];
  const named = STEP_RULES.filter(([pattern]) => pattern.test(path)).flatMap(([, names]) => names);
  if (named.length) return named;
  return /^src-gpui\/src\/frontend[/.]/.test(path) ? ["localization"] : [];
}

const isRust = (path) => /\.rs$|(^|\/)Cargo\.toml$|^Cargo\.lock$|(^|\/)build\.rs$/.test(path);

/**
 * The checks for `files` (paths relative to the repository, forward slashes),
 * in the order to run them: cheap and broad first. `dependents` maps a
 * package to the workspace packages that depend on it, so Clippy compiles
 * what a changed interface could break; tests run for the changed packages.
 * Each check is { name, command, args, env?, why }; `notes` are what no
 * command checks (docs, workflows, the installer).
 */
export function plan(files, { dependents = {}, ui = true, stepNames = null } = {}) {
  const checks = [];
  const notes = [];
  const why = (test) => files.filter(test);
  const add = (name, command, args, reasons, extra = {}) => {
    if (reasons.length) checks.push({ name, command, args, why: reasons, ...extra });
  };

  const biome = why((f) => /^(locales|resources)\//.test(f));
  add("biome", "pnpm", ["lint"], biome);
  add("translations", "pnpm", ["i18n"], why((f) => /^locales\//.test(f) || f === "scripts/i18n.mjs" || f === "resources/locales.json"));
  add(
    "generated adapters",
    "pnpm",
    ["check:generated"],
    why((f) => ["src-gpui/src/commands.rs", "src-gpui/src/dispatch.rs", "scripts/native-dispatch.mjs"].includes(f)),
  );
  const scripts = why((f) => /^scripts\/.*\.(mjs|ts)$/.test(f) || /^\.github\/workflows\//.test(f) || /^tools\/window-probe\/(steps|fixtures)\//.test(f));
  for (const file of scripts.filter((f) => /^scripts\/.*\.mjs$/.test(f))) add(`syntax ${file}`, "node", ["--check", file], [file]);
  add("script tests", "pnpm", ["test"], scripts);
  add("website", "pnpm", ["site:check"], why((f) => /^website\//.test(f)));

  // Rust: format the changed packages, Clippy them with what depends on them,
  // test the changed ones. Vendored GPUI is checked through Argus.
  const changed = new Set();
  const reasons = new Map();
  for (const file of files.filter(isRust)) {
    const own = packageOf(file);
    const name = own ?? (/^vendor\/gpui-pre/.test(file) ? "argus" : /^Cargo\.(toml|lock)$/.test(file) ? "*" : null);
    if (!name) continue;
    changed.add(name);
    reasons.set(name, [...(reasons.get(name) ?? []), file]);
  }
  const everything = changed.has("*");
  const own = [...changed].filter((name) => name !== "*").sort();
  const allRust = [...reasons.values()].flat();
  const ownFiles = files.filter((f) => isRust(f) && packageOf(f));
  if (everything) {
    add("rustfmt", "cargo", ["fmt", "--all", "--check"], allRust);
    add("clippy", "cargo", ["clippy", "--locked", "--workspace", "--all-targets", ...RUST_TARGET, "--", "-D", "warnings"], allRust);
    add("rust tests", "cargo", ["test", "--locked", "--workspace", ...RUST_TARGET], allRust, { env: TEST_ENV });
  } else if (own.length) {
    const formatted = [...new Set(ownFiles.map((f) => packageOf(f)))].sort();
    add("rustfmt", "cargo", ["fmt", "--check", ...formatted.flatMap((p) => ["-p", p])], ownFiles);
    const linted = [...new Set([...own, ...own.flatMap((p) => dependents[p] ?? [])])].sort();
    add("clippy", "cargo", ["clippy", "--locked", "--all-targets", ...linted.flatMap((p) => ["-p", p]), ...RUST_TARGET, "--", "-D", "warnings"], allRust);
    add("rust tests", "cargo", ["test", "--locked", ...own.flatMap((p) => ["-p", p]), ...RUST_TARGET], allRust, { env: TEST_ENV });
  }
  const windowsVendor = why((f) => /^vendor\/gpui-pre-windows\/.*\.rs$/.test(f));
  add("gpui-pre-windows tests", "cargo", ["test", "--locked", "-p", "gpui-pre-windows", "--lib", ...RUST_TARGET], windowsVendor);
  // The notices list the crates linked into argus.exe and the vendored code.
  add("third-party notices", "node", ["scripts/notices.mjs", "--check"], why((f) => /(^|\/)Cargo\.toml$|^Cargo\.lock$|^vendor\/[^/]+\/LICENSE/.test(f)));

  // Interface: a development build, the probe if its source changed, then
  // the step files the changes call for.
  const steps = new Map();
  for (const file of files) for (const name of stepFilesFor(file)) steps.set(name, [...(steps.get(name) ?? []), file]);
  if (stepNames) for (const name of steps.keys()) if (!stepNames.includes(name)) steps.delete(name);
  if (steps.size) {
    const names = [...steps.keys()].sort();
    const stepWhy = [...new Set([...steps.values()].flat())];
    if (!ui) notes.push(`step files left out (--no-ui): ${names.join(" ")}`);
    else {
      add("development build", "cargo", ["build", "-p", "argus", "--bin", "argus"], stepWhy);
      add("window probe", "cargo", ["build", "--release", "-p", "argus-window-probe", "--features", "capture"], why((f) => /^tools\/window-probe\/(src|Cargo\.toml)/.test(f)));
      add("step files", "node", ["scripts/ui-steps.mjs", ...names], stepWhy);
    }
  }
  if (why((f) => f === "tools/window-probe/fixtures/desktop.json").length)
    notes.push("the shared fixture changed: every step file reads it (node scripts/ui-steps.mjs runs them all)");
  if (why((f) => /^scripts\/(lib\/)?(ui-shots|ui-steps|owned-test|window-probe|capture-pixels)\.mjs$/.test(f) || /^tools\/window-probe\/src\//.test(f)).length)
    notes.push("every step file runs through the changed script or probe: run a few of them, or node scripts/ui-steps.mjs for all");

  if (why((f) => /\.md$/.test(f)).length) notes.push("documents: check the diff, the facts and the links that changed (docs/AGENTS.md)");
  if (why((f) => /^\.github\//.test(f)).length) notes.push("workflows: actions pinned to a commit, nightly and release rules in .github/AGENTS.md");
  if (why((f) => /^packaging\//.test(f)).length) notes.push("installer: never run it on this machine; test builds of its hooks as packaging/AGENTS.md says");
  if (why((f) => /^changelog\//.test(f)).length) notes.push("changelog: entries and translations as changelog/AGENTS.md says");
  if (why((f) => f === "scripts/release.mjs").length) notes.push("release script: pnpm release --dry-run, then a full release in a throwaway clone (scripts/AGENTS.md)");
  return { checks, notes };
}

/** Workspace packages that depend on each package, from `cargo metadata --no-deps`. */
export function dependentsOf(metadata) {
  const names = new Set(metadata.packages.map((p) => p.name));
  const dependents = {};
  for (const pkg of metadata.packages)
    for (const dependency of pkg.dependencies)
      if (names.has(dependency.name) && dependency.name !== pkg.name) (dependents[dependency.name] ??= []).push(pkg.name);
  // Through more than one step (argus-core → argus-app → argus).
  const closure = {};
  for (const name of names) {
    const seen = new Set();
    const queue = [...(dependents[name] ?? [])];
    while (queue.length) {
      const next = queue.shift();
      if (seen.has(next)) continue;
      seen.add(next);
      queue.push(...(dependents[next] ?? []));
    }
    if (seen.size) closure[name] = [...seen].sort();
  }
  return closure;
}

/** Paths in `git status --porcelain=v1 -z` output: changed, added, renamed (the new name), untracked. */
export function changedPaths(porcelain) {
  const entries = porcelain.split("\0").filter(Boolean);
  const paths = [];
  for (let i = 0; i < entries.length; i++) {
    const entry = entries[i];
    const status = entry.slice(0, 2);
    paths.push(entry.slice(3));
    if (status.includes("R") || status.includes("C")) i++; // the old name follows
  }
  return paths;
}
