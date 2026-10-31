// Capture both READMEs from the current release build and one frozen workload.
import { spawnSync } from "node:child_process";
import { copyFileSync } from "node:fs";
import { resolve } from "node:path";

process.chdir(resolve(import.meta.dirname, ".."));
function run(program, args) {
  const result = spawnSync(program, args, { stdio: "inherit", windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}
run(process.execPath, ["scripts/build.mjs"]);
run("cargo", ["build", "--locked", "--release", "-p", "argus-window-probe", "--features", "capture"]);
const batch = Date.now();
const captures = [];
for (const language of ["zh-CN", "en-US"]) {
  const label = `readme-${language.toLowerCase()}-${batch}`;
  const output = resolve("target/native-validation", label);
  run(process.execPath, ["scripts/ui-smoke.mjs", label, "--readme", language]);
  for (const theme of ["light", "dark"]) {
    const suffix = language === "en-US" ? "en-" : "";
    captures.push([resolve(output, `${theme}.png`), `docs/images/screenshot-${suffix}${theme}.png`]);
  }
}
for (const [capture, destination] of captures) copyFileSync(capture, destination);
