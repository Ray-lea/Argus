import { copyFileSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import assert from "node:assert/strict";
const root = resolve(import.meta.dirname, "..");
const payload = join(root, "target/pve-benchmark/payload");
for (const name of ["resources/app.json", "scripts/measure-windows-interaction.mjs", "scripts/vm-desktop-agent.mjs", "scripts/register-vm-desktop-agent.ps1", "scripts/configure-vm-ssh.ps1", "scripts/lib/owned-test.mjs", "target/release/argus.exe", "target/interaction-tools/interaction.exe", "target/interaction-tools/idle-worker.exe"]) {
  const destination = join(payload, name);
  mkdirSync(resolve(destination, ".."), { recursive: true });
  copyFileSync(join(root, name), destination);
}
copyFileSync(process.execPath, join(payload, "node.exe"));
const license = join(resolve(process.execPath, ".."), "LICENSE");
if (existsSync(license)) copyFileSync(license, join(payload, "NODE-LICENSE.txt"));
writeFileSync(join(payload, "run-benchmark.cmd"), '@echo off\r\ncd /d "%~dp0"\r\n"%~dp0node.exe" scripts\\vm-desktop-agent.mjs\r\npause\r\n');
const iso = join(root, "target/pve-benchmark/argus-benchmark-tools.iso");
const built = spawnSync(join(root, "target/interaction-tools/media-builder.exe"), [payload, iso], { windowsHide: true, stdio: "inherit" });
assert.equal(built.status, 0, "guest media build failed");
console.log(iso);
