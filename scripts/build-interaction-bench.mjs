import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { runCompilerScript } from "./build-leak-tools.mjs";

const root = resolve(import.meta.dirname, "..");
const output = join(root, "target/interaction-tools");
mkdirSync(output, { recursive: true });
const find = spawnSync(join(process.env["ProgramFiles(x86)"], "Microsoft Visual Studio/Installer/vswhere.exe"),
  ["-latest", "-products", "*", "-property", "installationPath"], { encoding: "utf8", windowsHide: true });
assert.equal(find.status, 0, find.stderr);
const quote = value => { assert(!/["\r\n%]/.test(value)); return `"${value}"`; };
writeFileSync(join(output, "build.cmd"), [
  "@echo off", "setlocal DisableDelayedExpansion",
  `call ${quote(join(find.stdout.trim(), "VC/Auxiliary/Build/vcvars64.bat"))} >nul`,
  "if errorlevel 1 exit /b %errorlevel%",
  `cl /nologo /O2 /W4 /EHsc /std:c++17 ${quote(join(root, "tools/window-probe/interaction.cpp"))} /Fo:${quote(join(output, "interaction.obj"))} /link /OUT:${quote(join(output, "interaction.exe"))} d3d11.lib dxgi.lib dwmapi.lib windowsapp.lib windowscodecs.lib user32.lib gdi32.lib ole32.lib oleaut32.lib advapi32.lib psapi.lib`,
  "if errorlevel 1 exit /b %errorlevel%",
  `cl /nologo /O1 /W4 /GS- /GR- ${quote(join(root, "tools/window-probe/idle-worker.cpp"))} /Fo:${quote(join(output, "idle-worker.obj"))} /link /OUT:${quote(join(output, "idle-worker.exe"))} /NODEFAULTLIB /SUBSYSTEM:WINDOWS /ENTRY:mainCRTStartup kernel32.lib`,
  "if errorlevel 1 exit /b %errorlevel%",
  `cl /nologo /O2 /W4 /EHsc /std:c++17 ${quote(join(root, "tools/window-probe/media-builder.cpp"))} /Fo:${quote(join(output, "media-builder.obj"))} /link /OUT:${quote(join(output, "media-builder.exe"))} ole32.lib oleaut32.lib shlwapi.lib uuid.lib`,
  "exit /b %errorlevel%", "",
].join("\r\n"));
runCompilerScript(output);
console.log(output);
