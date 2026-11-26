import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { buildInfo, currentSid, disposable, logSince, parseRegistryValue, requireDisposable, serviceState, troubles } from "../lib/machine-smoke.mjs";

test("machine-changing smoke tests run only where the machine is disposable", () => {
  assert(disposable({ GITHUB_ACTIONS: "true" }));
  assert(disposable({ ARGUS_DISPOSABLE_MACHINE: "1" }));
  assert(!disposable({}));
  assert(!disposable({ ARGUS_DISPOSABLE_MACHINE: "true", CI: "true" }));
  assert.throws(() => requireDisposable({}), /never on the development machine/);
});

test("registry values, service states and the SID are read from the tools' output", () => {
  const reg = "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Image File Execution Options\\taskmgr.exe\r\n    Debugger    REG_SZ    \"C:\\Program Files\\Argus Privileged\\argus-taskmgr-launcher.exe\"\r\n    Empty    REG_SZ\r\n";
  assert.equal(parseRegistryValue(reg, "debugger"), '"C:\\Program Files\\Argus Privileged\\argus-taskmgr-launcher.exe"');
  assert.equal(parseRegistryValue(reg, "Empty"), "");
  assert.equal(parseRegistryValue(reg, "Other"), null);

  const sc = "\r\nSERVICE_NAME: ArgusPrivileged\r\n        TYPE               : 10  WIN32_OWN_PROCESS\r\n        STATE              : 4  RUNNING\r\n";
  assert.equal(serviceState("ArgusPrivileged", () => ({ status: 0, stdout: sc })), "RUNNING");
  assert.equal(serviceState("ArgusPrivileged", () => ({ status: 1060, stdout: "" })), null);

  assert.equal(currentSid(() => ({ status: 0, stdout: '"runner\\runneradmin","S-1-5-21-1-2-3-500"\r\n' })), "S-1-5-21-1-2-3-500");
  assert.throws(() => currentSid(() => ({ status: 1, stdout: "", stderr: "denied" })));

  assert.deepEqual(buildInfo("argus.exe", () => ({ status: 0, stdout: '{"version":"0.6.0","build":"0.6.0+12.gabc"}\n' })), { version: "0.6.0", build: "0.6.0+12.gabc" });
  assert.equal(buildInfo("argus.exe", () => ({ status: 87, stdout: "" })), null);
});

test("the log of the last start is checked for panics, errors and unreadable data", () => {
  const root = mkdtempSync(join(tmpdir(), "argus-machine-smoke-"));
  try {
    const log = join(root, "argus.log");
    writeFileSync(
      log,
      [
        "t INFO Argus 0.6.0 started; renderer=GPUI",
        "t ERROR old trouble",
        "t INFO Argus 0.7.0+3.gabc started; renderer=GPUI",
        "t INFO settings: loaded (onboarding 1)",
        "t WARN history read: kind=InvalidData; path=...",
        "t WARN gpu: no adapter",
      ].join("\n"),
    );
    const lines = logSince(log, "0.7.0+3.gabc");
    assert.equal(lines.length, 4);
    assert.deepEqual(troubles(lines), ["t WARN history read: kind=InvalidData; path=..."]);
    assert.deepEqual(logSince(log, "0.8.0"), []);
    assert.deepEqual(logSince(join(root, "none.log"), "0.7.0"), []);
    assert.deepEqual(troubles(["x WARN settings: 0 bytes, empty (Eof error at line 1, column 0), path=…", "x INFO panicked at a.rs", "x INFO fine"]).length, 2);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
