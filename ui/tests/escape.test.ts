// Which Escape presses close what is open.

import assert from "node:assert/strict";
import { test } from "node:test";
import { type KeyPress, plainEscape } from "../src/lib/escape.ts";

const press = (change: Partial<KeyPress> = {}): KeyPress => ({
  key: "Escape",
  repeat: false,
  ctrlKey: false,
  shiftKey: false,
  altKey: false,
  metaKey: false,
  defaultPrevented: false,
  isComposing: false,
  ...change,
});

test("Esc on its own closes", () => {
  assert.equal(plainEscape(press()), true);
});

test("Ctrl+Shift+Esc held from opening Argus does not", () => {
  // As the task manager replacement's window gets it: repeats, modifiers down.
  assert.equal(plainEscape(press({ ctrlKey: true, shiftKey: true, repeat: true })), false);
  // Modifiers let go first, Esc still held.
  assert.equal(plainEscape(press({ repeat: true })), false);
  // Esc let go first, a new press with the modifiers still down.
  assert.equal(plainEscape(press({ ctrlKey: true, shiftKey: true })), false);
});

test("other modifiers, handled presses, compositions and other keys do not", () => {
  for (const change of [
    { ctrlKey: true },
    { shiftKey: true },
    { altKey: true },
    { metaKey: true },
    { defaultPrevented: true },
    { isComposing: true },
    { key: "Enter" },
    { key: "Esc" },
  ]) {
    assert.equal(plainEscape(press(change)), false, JSON.stringify(change));
  }
});
