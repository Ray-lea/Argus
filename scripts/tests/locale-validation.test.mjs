import assert from "node:assert/strict";
import test from "node:test";
import { markupError, maySpellOne, textErrors } from "../lib/locale-validation.mjs";

test("rich text requires properly nested and closed tags", () => {
  assert.equal(markupError("<link>a <u>{count}</u></link>"), null);
  for (const text of ["<link>a", "</link>", "<link><u>a</link></u>", "<u>a</link>"]) assert.ok(markupError(text));
  assert.equal(markupError("value < 1, path > 2"), null);
});
test("catalogs reject blank text and hidden direction or control characters", () => {
  assert.deepEqual(textErrors("source", " "), ["empty translation"]);
  for (const control of ["\0", "\u200b", "\u202e", "\u2066", "\ufeff"]) assert.ok(textErrors("source", `text${control}`).includes("invisible control character"));
  assert.ok(textErrors("first\n\nlast", "first\nQ\nlast").includes("translation marker on its own line"));
  assert.deepEqual(textErrors("source", "Français\n日本語\tРусский\u00a0"), []);
  assert.deepEqual(textErrors("", ""), []);
});
test("singular words cannot stand in for every French or Russian one count", () => {
  for (const locale of ["fr-FR", "ru-RU", "ja-JP", "zh-CN"]) assert.equal(maySpellOne(locale), false);
  for (const locale of ["en-US", "de-DE", "es-ES"]) assert.equal(maySpellOne(locale), true);
});

test("recovery instructions preserve registry values, JSON literals, paths and switches", () => {
  const source = "Debugger: argus-taskmgr-launcher.exe --task-manager; before.debugger in taskmgr-recovery.json is null";
  assert.deepEqual(textErrors(source, `説明：${source}`), []);
  for (const token of ["Debugger", "--task-manager", "before.debugger", "taskmgr-recovery.json", "null"]) {
    assert.ok(textErrors(source, source.replace(token, "translated")).some(error => error.includes(token)));
  }
  const path = "Program Files\\Argus Sensor Setup";
  assert.ok(textErrors(path, "Programme\\Argus Sensor Setup").some(error => error.includes(path)));
});
