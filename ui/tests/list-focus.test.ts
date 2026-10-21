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
  enteringPage: false,
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

test("entering the page hands navigation focus to the list even when pointerup preceded mounting", () => {
  assert.equal(reclaimsFocus(state({ holder: "control", enteringPage: true })), true);
  // Once on the page, Tab can still move to and stay on its buttons.
  assert.equal(reclaimsFocus(state({ holder: "control", enteringPage: false })), false);
});

test("the list itself and text fields keep focus", () => {
  for (const handoff of [{}, { afterClick: true }, { enteringPage: true }]) {
    assert.equal(reclaimsFocus(state({ holder: "list", ...handoff })), false);
    assert.equal(reclaimsFocus(state({ holder: "text", ...handoff })), false);
  }
});

test("nothing moves while another window, a popup, a pressed pointer or a text selection is in the way", () => {
  assert.equal(reclaimsFocus(state({ windowFocused: false })), false);
  assert.equal(reclaimsFocus(state({ popupOpen: true })), false);
  assert.equal(reclaimsFocus(state({ pointerDown: true })), false);
  assert.equal(reclaimsFocus(state({ selecting: true, afterClick: true })), false);
  assert.equal(reclaimsFocus(state({ holder: "control", afterClick: true, popupOpen: true })), false);
  for (const guard of [
    { windowFocused: false },
    { popupOpen: true },
    { pointerDown: true },
    { selecting: true },
  ]) {
    assert.equal(reclaimsFocus(state({ holder: "control", enteringPage: true, ...guard })), false);
  }
});
