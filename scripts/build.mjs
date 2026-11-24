// Build the native host after its embedded task manager helpers.
//
//   node scripts/build.mjs [--arch x64|arm64] [--installer | --bundle-only] [--skip-helpers]
//   node scripts/build.mjs [--arch x64|arm64] --quick   target/[<triple>/]quick/argus.exe
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { parseArchFlag, resolveWindowsTarget } from "./lib/windows-target.mjs";
const root = resolve(import.meta.dirname, "..");
const { arch: requestedArch, rest } = parseArchFlag(process.argv.slice(2));
const target = resolveWindowsTarget(requestedArch);
const args = new Set(rest);
const run = (program, argv) => {
  const result = spawnSync(program, argv, { cwd: root, stdio: "inherit", windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
};
const quick = args.has("--quick");
if (quick && (args.has("--installer") || args.has("--bundle-only"))) throw new Error("a quick build is not packaged");
const profile = quick ? ["--profile", "quick"] : ["--release"];
const targetArgs = target.cross ? ["--target", target.rustTarget] : [];
if (!args.has("--bundle-only")) {
  if (!args.has("--skip-helpers")) run("cargo", ["build", "--locked", ...targetArgs, ...profile, "-p", "argus-service", "--bins"]);
  run("cargo", ["build", "--locked", ...targetArgs, ...profile, "-p", "argus", "--bin", "argus"]);
}
if (args.has("--installer") || args.has("--bundle-only"))
  run(process.execPath, ["scripts/bundle.mjs", "--arch", target.arch]);
