import assert from "node:assert/strict";
import { test } from "node:test";
import { renderInstaller, validateBuildInfo } from "../bundle.mjs";
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

test("installer keeps artifact metadata visible and package precedence separate", () => {
  const build = { version: "1.2.3", build: "1.2.3+672.g1234567", buildKind: "test", fileVersion: "1.2.3.672" };
  assert.equal(validateBuildInfo(build, "1.2.3"), build);
  const text = renderInstaller({ productName: "Argus", publisher: "Test", version: "1.2.3", homepage: "https://example.com", identifier: "dev.argus.test", copyright: "Test" },
    { build, binary: "C:/test/argus.exe", output: "C:/test/setup.exe", icon: "C:/test/icon.ico", hooks: "C:/test/hooks.nsh", sizeKiB: 10 });
  assert.ok(text.includes('!define VERSION "1.2.3"'));
  assert.ok(text.includes('!define DISPLAYVERSION "1.2.3+672.g1234567"'));
  assert.ok(text.includes('!define VERSIONWITHBUILD "1.2.3.672"'));
  assert.ok(text.includes('"ProductVersion" "${DISPLAYVERSION}"'));
  assert.ok(text.includes('"DisplayVersion" "${VERSION}"'));
  for (const change of [{version:"1.2.4"}, {buildKind:"development"}, {buildKind:"stable"}, {build:"1.2.3+../escape"}, {build:"1.2.3+a..b"}, {fileVersion:"1.2.3.65536"}, {fileVersion:"1.2.4.672"}])
    assert.throws(() => validateBuildInfo({...build,...change}, "1.2.3"));
  validateBuildInfo({version:"1.2.3",build:"1.2.3",buildKind:"stable",fileVersion:"1.2.3.0"}, "1.2.3");
  validateBuildInfo({version:"1.2.4-beta.1",build:"1.2.4-beta.1",buildKind:"test",fileVersion:"1.2.4.0"}, "1.2.4-beta.1");
});
