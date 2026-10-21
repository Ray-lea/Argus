// When the process list takes keyboard focus back (ui/src/lib/list-focus.ts).

import assert from "node:assert/strict";
import { test } from "node:test";
import { type FocusState, reclaimsFocus } from "../src/lib/list-focus.ts";

const state = (change: Partial<FocusState>): FocusState => ({
  holder: "nothing",
  windowFocused: true,
  popupOpen: false,
  pointerDown: false,
  selecting: false,
  afterClick: false,
  ...change,
});

test("focus resting nowhere goes back to the list", () => {
  assert.equal(reclaimsFocus(state({})), true);
  assert.equal(reclaimsFocus(state({ afterClick: true })), true);
});

test("a button keeps keyboard focus, but not focus a click left on it", () => {
  assert.equal(reclaimsFocus(state({ holder: "control" })), false);
  assert.equal(reclaimsFocus(state({ holder: "control", afterClick: true })), true);
});

test("the list itself and text fields keep focus", () => {
  for (const afterClick of [false, true]) {
    assert.equal(reclaimsFocus(state({ holder: "list", afterClick })), false);
    assert.equal(reclaimsFocus(state({ holder: "text", afterClick })), false);
  }
});

test("nothing moves while another window, a popup, a pressed pointer or a text selection is in the way", () => {
  assert.equal(reclaimsFocus(state({ windowFocused: false })), false);
  assert.equal(reclaimsFocus(state({ popupOpen: true })), false);
  assert.equal(reclaimsFocus(state({ pointerDown: true })), false);
  assert.equal(reclaimsFocus(state({ selecting: true, afterClick: true })), false);
  assert.equal(reclaimsFocus(state({ holder: "control", afterClick: true, popupOpen: true })), false);
});
