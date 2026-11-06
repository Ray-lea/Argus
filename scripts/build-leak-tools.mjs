// Optional MSVC helpers for black-box, process-local resource soak tests.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve, join } from "node:path";

const root = resolve(import.meta.dirname, "..");
const output = resolve(root, "target/leak-tools");
mkdirSync(output, { recursive: true });
const find = spawnSync(
  join(process.env["ProgramFiles(x86)"], "Microsoft Visual Studio/Installer/vswhere.exe"),
  ["-latest", "-products", "*", "-property", "installationPath"],
  { encoding: "utf8", windowsHide: true },
);
assert.equal(find.status, 0, find.stderr);
const vcvars = join(find.stdout.trim(), "VC/Auxiliary/Build/vcvars64.bat");
const quote = value => {
  assert(!/["\r\n%]/.test(value), "unsupported build path");
  return `"${value}"`;
};
const compile = (name, flags, libraries) => [
  "cl /nologo /O2 /W4 /WX /EHsc /std:c++17", flags,
  quote(join(root, `tools/window-probe/${name}.cpp`)),
  `/Fo:${quote(join(output, `${name}.obj`))}`,
  `/link /OUT:${quote(join(output, `${name}.${flags ? "dll" : "exe"}`))}`,
  `/IMPLIB:${quote(join(output, `${name}.lib`))}`, libraries,
].join(" ");
const script = join(output, "build.cmd");
writeFileSync(script, [
  "@echo off", `call ${quote(vcvars)} >nul`, "if errorlevel 1 exit /b %errorlevel%",
  compile("clock-hook", "/LD", "synchronization.lib"), "if errorlevel 1 exit /b %errorlevel%",
  compile("load-worker", "", "ws2_32.lib"), "exit /b %errorlevel%", "",
].join("\r\n"));
const build = spawnSync(process.env.ComSpec ?? "cmd.exe", ["/d", "/c", script], {
  cwd: root, stdio: "inherit", windowsHide: true,
});
assert.equal(build.status, 0, "leak tool build failed");
console.log(output);
