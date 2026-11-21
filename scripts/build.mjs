// Build the native host after its embedded task manager helpers.
//
//   node scripts/build.mjs [--installer | --bundle-only] [--skip-helpers]
//   node scripts/build.mjs --quick       target/quick/argus.exe, the quick profile (Cargo.toml)
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
const root = resolve(import.meta.dirname, "..");
const args = new Set(process.argv.slice(2));
const run = (program, argv) => {
  const result = spawnSync(program, argv, { cwd: root, stdio: "inherit", windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
};
const quick = args.has("--quick");
if (quick && (args.has("--installer") || args.has("--bundle-only"))) throw new Error("a quick build is not packaged");
const profile = quick ? ["--profile", "quick"] : ["--release"];
if (!args.has("--bundle-only")) {
  if (!args.has("--skip-helpers")) run("cargo", ["build", "--locked", ...profile, "-p", "argus-service", "--bins"]);
  run("cargo", ["build", "--locked", ...profile, "-p", "argus", "--bin", "argus"]);
}
if (args.has("--installer") || args.has("--bundle-only")) run(process.execPath, ["scripts/bundle.mjs"]);
