import assert from "node:assert/strict";
import { test } from "node:test";
import type { KeyPress } from "../src/lib/escape.ts";
import { type PauseState, refreshPause } from "../src/lib/refresh-pause.ts";
import { controlPauseHeld, endProcessShortcut, startsSearch } from "../src/lib/shortcuts.ts";

const turn = () => new Promise<void>((resolve) => setTimeout(resolve, 0));
const press = (change: Partial<KeyPress> = {}): KeyPress => ({
  key: "e",
  altKey: true,
  ctrlKey: false,
  shiftKey: false,
  metaKey: false,
  repeat: false,
  defaultPrevented: false,
  isComposing: false,
  ...change,
});

function fixture(send?: (paused: boolean) => Promise<void>) {
  const commands: boolean[] = [];
  const errors: unknown[] = [];
  let state: PauseState = { manual: false, control: false, paused: false };
  const control = refreshPause(
    (next) => {
      state = next;
    },
    async (paused) => {
      commands.push(paused);
      await send?.(paused);
    },
    (error) => errors.push(error),
  );
  return { ...control, commands, errors, state: () => state };
}

test("Ctrl hold pauses once and release restores the live sampler", async () => {
  const p = fixture();
  p.setControl(true);
  p.setControl(true);
  assert.equal(p.state().paused, true);
  p.setControl(false);
  assert.equal(p.state().paused, false);
  await turn();
  assert.deepEqual(p.commands, [true, false]);
});

test("releasing Ctrl, hiding or losing focus preserves a manual pause", async () => {
  const p = fixture();
  p.toggleManual();
  p.setControl(true);
  p.setControl(false);
  assert.deepEqual(p.state(), { manual: true, control: false, paused: true });
  await turn();
  assert.deepEqual(p.commands, [true]);
});

test("the toolbar can keep a temporary pause after Ctrl is released", async () => {
  const p = fixture();
  p.setControl(true);
  p.toggleManual();
  p.setControl(false);
  assert.equal(p.state().paused, true);
  p.toggleManual();
  assert.equal(p.state().paused, false);
  await turn();
  assert.deepEqual(p.commands, [true, false]);
});

test("cancelling a manual pause while Ctrl is held waits for its release", async () => {
  const p = fixture();
  p.toggleManual();
  p.setControl(true);
  p.toggleManual();
  assert.deepEqual(p.state(), { manual: false, control: true, paused: true });
  p.setControl(false);
  await turn();
  assert.deepEqual(p.commands, [true, false]);
});

test("a fast press/release cannot deliver resume before pause", async () => {
  let finish!: () => void;
  const p = fixture((paused) =>
    paused
      ? new Promise((resolve) => {
          finish = resolve;
        })
      : Promise.resolve(),
  );
  p.setControl(true);
  p.setControl(false);
  await turn();
  assert.deepEqual(p.commands, [true]);
  finish();
  await turn();
  assert.deepEqual(p.commands, [true, false]);
});

test("a failed resume restores the last confirmed state and permits retry", async () => {
  let fail = true;
  const p = fixture(async (paused) => {
    if (!paused && fail) throw new Error("resume failed");
  });
  p.toggleManual();
  await turn();
  p.toggleManual();
  await turn();
  assert.equal(p.state().paused, true);
  assert.equal(p.errors.length, 1);
  fail = false;
  p.toggleManual();
  await turn();
  assert.equal(p.state().paused, false);
  assert.deepEqual(p.commands, [true, false, false]);
});

test("failure of an earlier command does not overwrite a newer pause request", async () => {
  let reject!: (error: Error) => void;
  let first = true;
  const p = fixture(() => {
    if (!first) return Promise.resolve();
    first = false;
    return new Promise((_, no) => {
      reject = no;
    });
  });
  p.setControl(true);
  await turn();
  p.setControl(false);
  p.toggleManual();
  reject(new Error("first command failed"));
  await turn();
  assert.deepEqual(p.state(), { manual: true, control: false, paused: true });
  assert.deepEqual(p.commands, [true, false, true]);
});

test("Alt+E recognizes letter case without accepting extra modifiers or repeats", () => {
  assert.equal(endProcessShortcut(press()), true);
  assert.equal(endProcessShortcut(press({ key: "E" })), true);
  for (const change of [
    { key: "r" },
    { altKey: false },
    { ctrlKey: true },
    { shiftKey: true },
    { metaKey: true },
    { repeat: true },
    { defaultPrevented: true },
    { isComposing: true },
  ])
    assert.equal(endProcessShortcut(press(change)), false, JSON.stringify(change));
});

test("Ctrl flags cover both sides and lost keyups without treating AltGr as a hold", () => {
  const ctrl = press({ key: "Control", ctrlKey: true, altKey: false });
  assert.equal(controlPauseHeld(ctrl), true);
  assert.equal(controlPauseHeld(press({ key: "Shift", ctrlKey: true, shiftKey: true, altKey: false })), true);
  for (const change of [{ ctrlKey: false }, { altKey: true }, { metaKey: true }, { isComposing: true }]) {
    assert.equal(controlPauseHeld({ ...ctrl, ...change }), false);
  }
});

test("typing a letter or digit outside a text field starts a search, and nothing else does", () => {
  const typed = (change: Partial<KeyPress>) => startsSearch(press({ altKey: false, ...change }));
  for (const key of ["c", "C", "7", "é", "ж"]) assert.equal(typed({ key }), true, key);
  assert.equal(typed({ key: "A", shiftKey: true }), true);
  // Keys with a meaning of their own, and punctuation pressed by accident.
  for (const key of [
    " ",
    "/",
    "+",
    "-",
    "*",
    ".",
    "Enter",
    "Escape",
    "Tab",
    "F5",
    "ArrowDown",
    "Dead",
    "Process",
    "Unidentified",
  ])
    assert.equal(typed({ key }), false, key);
  // Shortcuts, AltGr (Ctrl+Alt), held keys, compositions and keys already handled.
  assert.equal(typed({ key: "e", altKey: true }), false);
  assert.equal(typed({ key: "f", ctrlKey: true }), false);
  assert.equal(typed({ key: "q", ctrlKey: true, altKey: true }), false);
  assert.equal(typed({ key: "r", metaKey: true }), false);
  assert.equal(typed({ key: "a", repeat: true }), false);
  assert.equal(typed({ key: "n", isComposing: true }), false);
  assert.equal(typed({ key: "a", defaultPrevented: true }), false);
});
