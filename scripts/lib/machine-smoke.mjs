// Shared steps of the smoke tests that change the machine they run on
// (scripts/upgrade-smoke.mjs, scripts/privileged-smoke.mjs): they install
// Argus, its service and the task manager replacement for real, so they run
// only on a disposable machine — a CI runner or a lab VM, never the
// development machine.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

export const IDENTIFIER = "dev.argus.taskmanager";
export const UNINSTALL_KEY = String.raw`HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall\Argus`;
export const IFEO_KEY = String.raw`HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\taskmgr.exe`;

/** Whether this machine may be changed: GitHub Actions, or a lab VM that says so. */
export function disposable(environment = process.env) {
  return environment.GITHUB_ACTIONS === "true" || environment.ARGUS_DISPOSABLE_MACHINE === "1";
}

export function requireDisposable(environment = process.env) {
  assert(
    disposable(environment),
    "this test changes the machine it runs on: run it on a CI runner or a lab VM (ARGUS_DISPOSABLE_MACHINE=1), never on the development machine",
  );
}

/** Runs a program without a shell; returns { status, stdout, stderr }. */
export function run(program, args = [], { timeout = 120000, env } = {}) {
  const result = spawnSync(program, args, { encoding: "utf8", windowsHide: true, timeout, env: env ?? process.env });
  if (result.error) return { status: -1, stdout: result.stdout ?? "", stderr: String(result.error.message) };
  return { status: result.status ?? -1, stdout: result.stdout ?? "", stderr: result.stderr ?? "" };
}

/** A registry value as text, or null (`reg query` prints `name  REG_SZ  value`). */
export function registryValue(key, name, query = run) {
  const result = query("reg.exe", ["query", key, "/v", name]);
  if (result.status !== 0) return null;
  return parseRegistryValue(result.stdout, name);
}

export function parseRegistryValue(text, name) {
  for (const line of text.split(/\r?\n/)) {
    const match = /^\s+(.+?)\s{4}(REG_\w+)\s{4}(.*)$/.exec(line);
    if (match && match[1].toLowerCase() === name.toLowerCase()) return match[3].trim();
    const empty = /^\s+(.+?)\s{4}(REG_\w+)\s*$/.exec(line);
    if (empty && empty[1].toLowerCase() === name.toLowerCase()) return "";
  }
  return null;
}

/** The SCM state of a service (`RUNNING`, `STOPPED`, …) or null when there is no such service. */
export function serviceState(name, query = run) {
  const result = query("sc.exe", ["query", name]);
  if (result.status !== 0) return null;
  return /STATE\s+:\s+\d+\s+(\w+)/.exec(result.stdout)?.[1] ?? null;
}

/** The current account's SID (`whoami /user` in CSV). */
export function currentSid(query = run) {
  const result = query("whoami.exe", ["/user", "/fo", "csv", "/nh"]);
  const sid = /"(S-1-[\d-]+)"/.exec(result.stdout)?.[1];
  assert(sid, `no SID in whoami output: ${result.stdout}${result.stderr}`);
  return sid;
}

/**
 * The elevation type of the desktop shell's token (explorer.exe):
 * 1 default (no split token), 2 full, 3 limited (the standard half of a split
 * token), or null when there is no shell or its token cannot be read.
 */
export function shellElevationType(query = run) {
  const script = `
$ErrorActionPreference = 'Stop'
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class ArgusToken {
  [DllImport("advapi32.dll", SetLastError = true)] public static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
  [DllImport("advapi32.dll", SetLastError = true)] public static extern bool GetTokenInformation(IntPtr token, int kind, out int value, int length, out int returned);
  [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr handle);
}
'@
$session = (Get-Process -Id $PID).SessionId
$shell = Get-Process explorer -ErrorAction SilentlyContinue | Where-Object SessionId -eq $session | Select-Object -First 1
if (-not $shell) { 'none'; exit }
$token = [IntPtr]::Zero
if (-not [ArgusToken]::OpenProcessToken($shell.Handle, 8, [ref]$token)) { 'none'; exit }
$value = 0; $returned = 0
[void][ArgusToken]::GetTokenInformation($token, 18, [ref]$value, 4, [ref]$returned)
[void][ArgusToken]::CloseHandle($token)
$value`;
  const encoded = Buffer.from(script, "utf16le").toString("base64");
  const result = query("powershell.exe", ["-NoProfile", "-NonInteractive", "-EncodedCommand", encoded], { timeout: 60000 });
  const value = Number(result.stdout.trim().split(/\r?\n/).at(-1));
  return [1, 2, 3].includes(value) ? value : null;
}

export function elevated(query = run) {
  // `net session` needs administrator rights and changes nothing.
  return query("net.exe", ["session"]).status === 0;
}

/** Image names of running processes, lower case. */
export function runningImages(query = run) {
  const result = query("tasklist.exe", ["/fo", "csv", "/nh"]);
  return new Set(
    result.stdout
      .split(/\r?\n/)
      .map((line) => /^"([^"]+)"/.exec(line)?.[1]?.toLowerCase())
      .filter(Boolean),
  );
}

export const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

/** Polls `check` every 500 ms until it returns a truthy value or `ms` passes; returns the last value. */
export async function waitFor(check, ms) {
  const end = Date.now() + ms;
  let value = await check();
  while (!value && Date.now() < end) {
    await sleep(500);
    value = await check();
  }
  return value;
}

/** The data folders of the installed identity under the current account. */
export function dataFolders(environment = process.env) {
  return {
    config: join(environment.APPDATA, IDENTIFIER),
    local: join(environment.LOCALAPPDATA, IDENTIFIER),
  };
}

/** Lines of the diagnostics log from the last start of `version` on, or [] without a log. */
export function logSince(path, version) {
  if (!existsSync(path)) return [];
  const lines = readFileSync(path, "utf8").split(/\r?\n/);
  const start = lines.findLastIndex((line) => line.includes(`Argus ${version} started`));
  return start < 0 ? [] : lines.slice(start);
}

/** Log lines that fail a smoke run: panics, errors, and failures to read settings or history. */
export function troubles(lines) {
  return lines.filter(
    (line) =>
      /\bpanic/i.test(line) ||
      /\sERROR\s/.test(line) ||
      /\bhistory [a-z ]*(read|open|write)\b|settings: (not read|\d+ bytes, )/.test(line),
  );
}

/** `{ version, build }` of an Argus program (`--argus-build-info`), or null when it cannot tell. */
export function buildInfo(exe, query = run) {
  const result = query(exe, ["--argus-build-info"], { timeout: 30000 });
  try {
    const info = JSON.parse(result.stdout.trim());
    return typeof info.build === "string" ? info : null;
  } catch {
    return null;
  }
}
