// Run the real NSIS uninstall body synchronously from an external copy.
// Keeping EXEDIR different from INSTDIR also preserves Argus's uninstall hooks.
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { isAbsolute, join, relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

const script = String.raw`
$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [Console]::OutputEncoding
$directory = $env:ARGUS_SMOKE_DIRECTORY
$key = $env:ARGUS_SMOKE_UNINSTALL_KEY
$uninstaller = Join-Path $PSScriptRoot "uninstall.exe"
$timeout = [int]$env:ARGUS_SMOKE_TIMEOUT_MS
$files = @("argus.exe", "uninstall.exe") | ForEach-Object { Join-Path $directory $_ }
if (-not (Test-Path -LiteralPath $key)) { throw "Uninstall entry missing before uninstall: $key" }
foreach ($file in $files) {
  if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "Installed file missing before uninstall: $file" }
}
# _?= must be the final argument, without quotes even when the path has spaces.
# The external copy can delete the installed uninstaller and runs the full hooks.
$process = Start-Process -FilePath $uninstaller -ArgumentList "/S", "_?=$directory" -WorkingDirectory $PSScriptRoot -WindowStyle Hidden -PassThru
if (-not $process.WaitForExit($timeout)) {
  $process.Kill()
  $process.WaitForExit()
  throw "Uninstaller timed out after $timeout ms: $directory"
}
$code = $process.ExitCode
Write-Output "Uninstaller exit code: $code"
if ($code -ne 0) { throw "Uninstaller failed with exit code $code" }
$deadline = [DateTime]::UtcNow.AddMilliseconds($timeout)
do {
  $entryRemains = Test-Path -LiteralPath $key
  $remaining = @($files | Where-Object { Test-Path -LiteralPath $_ })
  if (-not $entryRemains -and $remaining.Count -eq 0) {
    Write-Output "Uninstall completed: entry and installed executables removed."
    exit 0
  }
  if ([DateTime]::UtcNow -ge $deadline) { break }
  Start-Sleep -Milliseconds 200
} while ($true)
Write-Output "Uninstall incomplete: registry entry remains=$entryRemains; key=$key"
$remaining | ForEach-Object { Write-Output "Remaining installed file: $_" }
throw "Uninstall incomplete after $timeout ms."
`;

export function uninstallSmoke(directory, {
  runnerTemp = process.env.RUNNER_TEMP,
  uninstallKey = "HKLM:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Argus",
  timeoutMs = 60_000,
} = {}) {
  if (process.platform !== "win32") throw new Error("Uninstall smoke checks require Windows.");
  if (!runnerTemp || !isAbsolute(runnerTemp)) throw new Error("An absolute RUNNER_TEMP is required.");
  const root = resolve(runnerTemp);
  const target = resolve(directory);
  const child = relative(root, target);
  if (!child || child === ".." || child.startsWith(`..${sep}`) || isAbsolute(child)) {
    throw new Error("The smoke installation must be inside RUNNER_TEMP.");
  }
  if (!Number.isInteger(timeoutMs) || timeoutMs < 200 || timeoutMs > 300_000) throw new Error("Invalid uninstall timeout.");
  if (!existsSync(join(target, "uninstall.exe"))) throw new Error("The installed uninstaller is missing.");
  const scratch = mkdtempSync(join(root, "argus-uninstall-smoke-"));
  try {
    copyFileSync(join(target, "uninstall.exe"), join(scratch, "uninstall.exe"));
    const file = join(scratch, "run.ps1");
    writeFileSync(file, script);
    const result = spawnSync("pwsh", ["-NoProfile", "-NonInteractive", "-File", file], {
      encoding: "utf8", windowsHide: true, timeout: timeoutMs * 2 + 15_000,
      env: {
        ...process.env,
        ARGUS_SMOKE_DIRECTORY: target,
        ARGUS_SMOKE_UNINSTALL_KEY: uninstallKey,
        ARGUS_SMOKE_TIMEOUT_MS: String(timeoutMs),
      },
    });
    if (result.error) throw result.error;
    if (result.status !== 0) throw new Error(`Uninstall smoke check failed (${result.status}):\n${result.stdout}${result.stderr}`);
    return result.stdout.trim();
  } finally {
    // mkdtemp created this exact child of the validated temporary root.
    rmSync(scratch, { recursive: true, force: true });
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (process.argv.length !== 3) throw new Error("Usage: node scripts/uninstall-smoke.mjs <installation-directory>");
  console.log(uninstallSmoke(process.argv[2]));
}
