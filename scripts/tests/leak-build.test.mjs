import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { runCompilerScript } from "../build-leak-tools.mjs";

test("compiler launcher treats shell metacharacters as directory names", { skip: process.platform !== "win32" }, () => {
  const root = resolve(mkdtempSync(join(tmpdir(), "argus-leak-build-test-")));
  try {
    for (const name of ["spaces and (parentheses)", "path&echo.ARGUS_INJECTED&rem", "bang!PATH!", "caret^path", "路径"]) {
      const directory = join(root, name);
      mkdirSync(directory);
      writeFileSync(join(directory, "build.cmd"), "@echo off\r\necho BUILD_OK\r\n");
      const result = runCompilerScript(directory, "pipe");
      assert.equal(result.stdout.toString().trim(), "BUILD_OK", name);
      assert.equal(result.stderr.toString().trim(), "", name);
    }
  } finally {
    assert.equal(dirname(root), resolve(tmpdir()));
    assert.ok(basename(root).startsWith("argus-leak-build-test-"));
    rmSync(root, { recursive: true, force: true });
  }
});
