import assert from "node:assert/strict";
import { test } from "node:test";
import { quotedTaskPath, validTaskCommand } from "../src/lib/task-command.ts";

test("task input accepts paths and arguments without treating shell characters as extra commands", () => {
  for (const command of [
    "explorer.exe",
    '"C:\\Example App\\app.exe" "a b"',
    "cmd /c echo a & b",
    "%SystemRoot%\\explorer.exe",
  ])
    assert.equal(validTaskCommand(command), true);
});
test("empty input, embedded nulls, line breaks and oversized commands are rejected", () => {
  for (const command of ["", " \t ", "one\0two", "one\ntwo", "one\rtwo", "a".repeat(8193)])
    assert.equal(validTaskCommand(command), false);
  assert.equal(validTaskCommand("a".repeat(8192)), true);
});
test("a browsed path with spaces is quoted as one target", () => {
  assert.equal(quotedTaskPath("C:\\Example App\\app.exe"), '"C:\\Example App\\app.exe"');
});
