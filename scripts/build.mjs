// Build the native host after its embedded task manager helpers.
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
const root = resolve(import.meta.dirname, "..");
const args = new Set(process.argv.slice(2));
const run = (program, argv) => {
  const result = spawnSync(program, argv, { cwd: root, stdio: "inherit", windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
};
if (!args.has("--bundle-only")) {
  if (!args.has("--skip-helpers"))
    run("cargo", ["build", "--locked", "--release", "-p", "argus-service", "--bins"]);
  run("cargo", ["build", "--locked", "--release", "-p", "argus", "--bin", "argus"]);
}
if (args.has("--installer") || args.has("--bundle-only")) run(process.execPath, ["scripts/bundle.mjs"]);
