import assert from "node:assert/strict";
import { test } from "node:test";
import { ErrorGate, errorReport } from "../src/lib/error-report.ts";

const origin = "http://tauri.localhost";

test("reports own script locations without messages, external URLs or query strings", () => {
  const report = errorReport(
    {
      name: "TypeError",
      message: "C:/Users/Private --token secret",
      stack:
        "TypeError: private\n at fail (http://tauri.localhost/assets/index-abcd.js?token=secret:12:34)\n at http://evil.invalid/private.js:5:6\n at file:///C:/Users/Private/app.js:7:8\n at http://tauri.localhost/src/lib/ipc.ts:90:10",
    },
    origin,
  );
  assert.equal(report.category, "TypeError");
  assert.deepEqual(report.frames, [
    { file: "assets/index-abcd.js", line: 12, column: 34 },
    { file: "src/lib/ipc.ts", line: 90, column: 10 },
  ]);
  assert.ok(!/secret|private|Private|evil|token|Users/.test(JSON.stringify(report)));
});

test("structured and string IPC rejections retain only their numeric failure", () => {
  assert.equal(errorReport("secret (os error 1813)", origin).nativeCode, 1813);
  const report = errorReport({ kind: "os", message: "C:/private (0x80004005)" }, origin);
  assert.equal(report.actionKind, "os");
  assert.equal(report.nativeCode, 0x80004005);
  assert.ok(!/private/.test(JSON.stringify(report)));
  for (const code of [NaN, -1, 2 ** 32, 1.2, "secret"]) {
    assert.equal(errorReport({ code }, origin).nativeCode, null);
  }
});

test("hostile error objects and malformed source locations cannot break reporting", () => {
  const hostile = new Proxy(
    {},
    {
      get() {
        throw new Error("secret");
      },
    },
  );
  assert.equal(errorReport(hostile, origin).category, "Unknown");
  for (const url of [
    "http://user:secret@tauri.localhost/assets/index.js",
    "http://tauri.localhost/private/document.txt",
    "http://tauri.localhost/src/../../private.ts",
    "http://tauri.localhost/assets/%0aforged.js",
  ])
    assert.deepEqual(errorReport(null, origin, { url, line: 1, column: 1 }).frames, []);
  assert.deepEqual(
    errorReport(null, origin, { url: `${origin}/assets/index.js`, line: 0, column: 1 }).frames,
    [],
  );
});

test("different failures still report after a retry storm and a long session", () => {
  const gate = new ErrorGate();
  assert.equal(gate.admit("one", 0), true);
  for (let i = 1; i < 1000; i++) assert.equal(gate.admit("one", i), false);
  assert.equal(gate.admit("two", 1000), true);
  assert.equal(gate.admit("one", 60_000), true);
  for (let i = 0; i < 100; i++) assert.equal(gate.admit(`other-${i}`, 70_000), true);
  assert.equal(gate.admit("later failure", 80_000), true);
});
