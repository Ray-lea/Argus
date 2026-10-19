import assert from "node:assert/strict";
import { test } from "node:test";
import {
  applicationKey,
  applicationMembers,
  broadRuleScope,
  canGroup,
  processRowId,
} from "../src/lib/process-identity.ts";
import type { ProcessRow, WebViewInfo } from "../src/lib/types.ts";

const process = (pid: number, path: string | null, name = "helper.exe", webView?: WebViewInfo): ProcessRow =>
  ({ pid, createTime: pid * 100, name, path, webView }) as ProcessRow;
const runtime = (pid: number, created = pid * 100): WebViewInfo => ({
  browserPid: pid,
  browserCreateTime: created,
  hostName: "app.exe",
  hostDescription: null,
  hostIcon: null,
  role: "renderer",
});

test("equal filenames in different installations never share a display or action group", () => {
  const a = process(1, "C:\\App A\\helper.exe");
  const child = process(2, "C:\\App A\\helper.exe");
  const b = process(3, "C:\\App B\\helper.exe");
  assert.ok(applicationKey(a) !== applicationKey(b));
  assert.deepEqual(
    applicationMembers([a, child, b], applicationKey(a)).map((p) => p.pid),
    [1, 2],
  );
});

test("path case, separators and extended DOS/UNC syntax do not split one executable", () => {
  for (const paths of [
    ["C:\\Apps\\helper.exe", "c:/apps/HELPER.EXE", "\\\\?\\C:\\Apps\\helper.exe"],
    ["\\\\server\\share\\helper.exe", "\\\\?\\UNC\\SERVER\\share\\helper.exe"],
  ])
    assert.equal(new Set(paths.map((p, i) => applicationKey(process(i, p)))).size, 1);
});

test("missing image paths remain separate rather than guessing from the filename", () => {
  assert.equal(canGroup(process(1, null)), false);
  assert.ok(applicationKey(process(1, null)) !== applicationKey(process(2, null)));
  assert.ok(applicationKey(process(1, null)) !== applicationKey(process(1, "C:\\Apps\\helper.exe")));
});

test("WebView2 runtime groups remain separate even for the same host application", () => {
  const a = process(11, "C:\\Runtime\\msedgewebview2.exe", "msedgewebview2.exe", runtime(10));
  const crash = process(12, a.path, a.name, { ...runtime(10), role: "crashpad" });
  const b = process(21, a.path, a.name, runtime(20));
  assert.ok(applicationKey(a) !== applicationKey(b));
  assert.deepEqual(
    applicationMembers([a, crash, b], applicationKey(a)).map((p) => p.pid),
    [11, 12],
  );
});

test("a reused browser PID cannot join surviving helpers of the old instance", () => {
  const old = process(11, "C:\\Runtime\\msedgewebview2.exe", "msedgewebview2.exe", runtime(10, 1000));
  const current = process(12, old.path, old.name, runtime(10, 2000));
  assert.ok(applicationKey(old) !== applicationKey(current));
  assert.deepEqual(applicationMembers([old, current], applicationKey(old)), [old]);
});

test("late host descriptions and icons preserve the group history identity", () => {
  const early = process(11, "C:\\Runtime\\msedgewebview2.exe", "msedgewebview2.exe", runtime(10));
  const ready = {
    ...early,
    webView: { ...runtime(10), hostDescription: "Example application", hostIcon: 9 },
  };
  assert.equal(applicationKey(early), applicationKey(ready));
});

test("shared hosts and interpreters stay per instance without reliable runtime identity", () => {
  for (const name of [
    "msedgewebview2.exe",
    "SVCHOST.EXE",
    "dllhost.exe",
    "rundll32.exe",
    "node.exe",
    "python.exe",
    "javaw.exe",
    "dotnet.exe",
    "pwsh.exe",
  ]) {
    const a = process(1, `C:\\Runtime\\${name}`, name),
      b = process(2, a.path, name);
    assert.ok(applicationKey(a) !== applicationKey(b), name);
    assert.equal(canGroup(a), false, name);
  }
});

test("only real image and runtime groups need aggregate history", () => {
  assert.equal(canGroup(process(1, "C:\\Apps\\helper.exe")), true);
  assert.equal(canGroup(process(11, null, "msedgewebview2.exe", runtime(10))), true);
});

test("a disappeared group cannot target an unrelated process with the same name", () => {
  const old = process(1, "C:\\App A\\helper.exe"),
    other = process(2, "C:\\App B\\helper.exe");
  assert.deepEqual(applicationMembers([other], applicationKey(old)), []);
});

test("rule scope remains explicitly broader than shared-host and cross-path groups", () => {
  const a = process(1, "C:\\App A\\helper.exe"),
    b = process(2, "C:\\App B\\helper.exe");
  assert.equal(broadRuleScope(a, [a, b]), true);
  assert.equal(broadRuleScope(a, [a, { ...a, pid: 3 }]), false);
  const webview = process(11, "C:\\Runtime\\msedgewebview2.exe", "msedgewebview2.exe", runtime(10));
  assert.equal(broadRuleScope(webview, [webview]), true);
});

test("row IDs do not collide when paths contain underscores and directory separators", () => {
  const keys = ["g:image:c:\\a\\b.exe", "g:image:c:\\a_b.exe", "g:image:c:\\a%5cb.exe"];
  assert.equal(new Set(keys.map(processRowId)).size, keys.length);
  for (const key of keys) assert.equal(decodeURIComponent(processRowId(key).slice(4)), key);
});
