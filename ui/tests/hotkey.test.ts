// The hotkey rules (ui/src/lib/hotkey.ts). The cases in hotkey-cases.json are
// shared with the native side's tests, which must refuse the same ones.

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  canonical,
  judge,
  keyByCode,
  keyByVk,
  keyName,
  parseHotkey,
  type Verdict,
} from "../src/lib/hotkey.ts";
import table from "../src/lib/hotkey-table.json" with { type: "json" };
import cases from "./hotkey-cases.json" with { type: "json" };

const stored = (text: string) => {
  const a = parseHotkey(text);
  return typeof a === "string" ? a : canonical(a);
};

test("reads stored and hand-written forms", () => {
  assert.equal(stored("Ctrl+Alt+Escape"), "Ctrl+Alt+Escape");
  assert.equal(stored("super+shift+alt+ctrl+k"), "Ctrl+Alt+Shift+Super+KeyK");
  assert.equal(stored(" Control + Esc "), "Ctrl+Escape");
  assert.equal(stored("Ctrl+Alt+NumpadEnter"), "Ctrl+Alt+Enter");
  assert.equal(stored("Win+Up"), "Super+ArrowUp");
  assert.equal(stored("Ctrl+7"), "Ctrl+Digit7");
  assert.equal(stored("Ctrl+"), "invalid");
  assert.equal(stored("Ctrl+KeyK+Alt"), "invalid");
  assert.equal(stored("Ctrl+Alt"), "noKey");
  assert.equal(stored("Ctrl+Alt+AltRight"), "unsupported");
});

test("judges every shared case", () => {
  assert.ok(cases.length > 20);
  for (const c of cases) {
    const want: Verdict =
      c.level === "block"
        ? { level: "block", issue: c.issue as Extract<Verdict, { level: "block" }>["issue"] }
        : c.level === "warn"
          ? { level: "warn", warnings: c.warnings as Extract<Verdict, { level: "warn" }>["warnings"] }
          : { level: "ok" };
    assert.deepEqual(judge(c.hotkey), want, c.hotkey);
  }
});

test("the default hotkey passes", () => {
  assert.deepEqual(judge("Ctrl+Alt+Escape"), { level: "ok" });
});

test("each key has one code and one virtual-key code", () => {
  assert.equal(new Set(table.keys.map((k) => k.code)).size, table.keys.length);
  assert.equal(new Set(table.keys.map((k) => k.vk)).size, table.keys.length);
  assert.equal(keyByVk(0x4b)?.code, "KeyK");
  assert.equal(keyByCode("numpad1")?.vk, 0x61);
});

test("keys read as printed", () => {
  const name = (code: string) => {
    const key = keyByCode(code);
    return key ? keyName(key) : undefined;
  };
  assert.equal(name("KeyK"), "K");
  assert.equal(name("Digit7"), "7");
  assert.equal(name("Escape"), "Esc");
  assert.equal(name("Slash"), "/");
  assert.equal(name("Numpad1"), "Num 1");
  assert.equal(name("F13"), "F13");
});
