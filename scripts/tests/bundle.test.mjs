import assert from "node:assert/strict";
import { test } from "node:test";
import { renderInstaller } from "../bundle.mjs";
test("installer escapes paths and carries native updater restart flags", () => {
  const text = renderInstaller(
    {
      productName: "Argus",
      publisher: "Test",
      version: "1.2.3-beta.1",
      homepage: "https://example.com",
      identifier: "dev.argus.test",
      copyright: "Test",
    },
    {
      binary: "C:\\a $b\\argus.exe",
      output: "C:\\out\\setup.exe",
      icon: "C:\\app.ico",
      hooks: "C:\\hooks.nsh",
      sizeKiB: 10,
    },
  );
  assert.ok(text.includes("C:\\a $$b\\argus.exe"));
  assert.ok(text.includes('!define VERSIONWITHBUILD "1.2.3.0"'));
  assert.ok(text.includes("--argus-launch-installed $R0"));
  assert.ok(!text.includes("WebView"));
  assert.ok(!text.includes("{{"));
});
