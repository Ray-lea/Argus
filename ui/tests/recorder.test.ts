// Recording a hotkey from key events (record() in ui/src/lib/hotkey.ts). The
// sequences follow what Chromium reports on Windows: a modifier's own flag is
// set on its keydown and cleared on its keyup, `keyCode` is the virtual-key
// code.

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  type KeyInput,
  keyByCode,
  modifierOf,
  type RecordEffect,
  type Recording,
  record,
  startRecording,
} from "../src/lib/hotkey.ts";

const MODIFIER_VK: Record<string, number> = { ctrl: 17, alt: 18, shift: 16, win: 91 };
const MODIFIER_KEY: Record<string, string> = { ctrl: "Control", alt: "Alt", shift: "Shift", win: "Meta" };

/** A keyboard that tracks the modifiers held, as Chromium's flags do. */
function keyboard() {
  const held = new Set<string>();
  let state: Recording = startRecording();
  const effects: RecordEffect[] = [];
  const flag = (m: string) => [...held].some((code) => modifierOf({ code, key: "" }) === m);
  const send = (type: "down" | "up", code: string, extra: Partial<KeyInput> = {}) => {
    const modifier = modifierOf({ code, key: "" });
    if (type === "down") held.add(code);
    else held.delete(code);
    const spec = keyByCode(code);
    const input: KeyInput = {
      type,
      code,
      key: modifier ? MODIFIER_KEY[modifier] : (spec?.code.replace(/^Key|^Digit/, "") ?? code),
      keyCode: modifier ? MODIFIER_VK[modifier] : (spec?.vk ?? 0),
      ctrl: flag("ctrl"),
      alt: flag("alt"),
      shift: flag("shift"),
      meta: flag("win"),
      altGraph: false,
      repeat: false,
      ...extra,
    };
    const result = record(state, input);
    state = result.state;
    if (result.effect.type !== "none") effects.push(result.effect);
    return result.effect;
  };
  return {
    down: (code: string, extra?: Partial<KeyInput>) => send("down", code, extra),
    up: (code: string, extra?: Partial<KeyInput>) => send("up", code, extra),
    /** Presses the keys in order, then lets go in reverse. */
    chord(...codes: string[]) {
      for (const code of codes) send("down", code);
      for (const code of [...codes].reverse()) send("up", code);
    },
    get state() {
      return state;
    },
    effects,
    commits: () => effects.flatMap((e) => (e.type === "commit" ? [e.hotkey] : [])),
  };
}

test("commits when the main key comes up, not when it goes down", () => {
  const k = keyboard();
  k.down("ControlLeft");
  k.down("AltLeft");
  assert.deepEqual(k.down("KeyK"), { type: "none" });
  k.down("KeyK", { repeat: true });
  k.down("KeyK", { repeat: true });
  assert.deepEqual(k.up("KeyK"), { type: "commit", hotkey: "Ctrl+Alt+KeyK" });
  assert.deepEqual(k.commits(), ["Ctrl+Alt+KeyK"]);
});

test("left and right modifiers alone are shown and explained", () => {
  const k = keyboard();
  k.chord("ControlLeft", "ControlRight", "AltLeft");
  assert.deepEqual(k.commits(), []);
  assert.deepEqual(k.state.hint, { kind: "noKey" });
  assert.deepEqual(
    k.state.chord.map((c) => c.code),
    ["ControlLeft", "ControlRight", "AltLeft"],
  );
});

test("CapsLock held first is shown but left out of the combination", () => {
  const k = keyboard();
  k.chord("CapsLock", "ControlLeft", "KeyK");
  assert.deepEqual(k.commits(), ["Ctrl+KeyK"]);
  assert.equal(k.state.chord[0].kind, "lock");
});

test("modifiers and CapsLock without a main key", () => {
  const k = keyboard();
  k.chord("ControlLeft", "CapsLock");
  assert.deepEqual(k.commits(), []);
  assert.deepEqual(k.state.hint, { kind: "noKey" });
});

test("the main key pressed before the modifiers is not taken", () => {
  const k = keyboard();
  k.down("KeyK");
  k.down("ControlLeft");
  k.up("KeyK");
  k.up("ControlLeft");
  assert.deepEqual(k.commits(), []);
  assert.deepEqual(k.state.hint, { kind: "order" });
  // Pressed again the right way round.
  k.chord("ControlLeft", "AltLeft", "KeyK");
  assert.deepEqual(k.commits(), ["Ctrl+Alt+KeyK"]);
});

test("Esc cancels, Backspace turns off, Tab leaves; with modifiers they are keys", () => {
  assert.deepEqual(keyboard().down("Escape"), { type: "cancel" });
  assert.deepEqual(keyboard().down("Backspace"), { type: "disable" });
  assert.deepEqual(keyboard().down("Tab"), { type: "cancel" });
  const shiftTab = keyboard();
  shiftTab.down("ShiftLeft");
  assert.deepEqual(shiftTab.down("Tab"), { type: "cancel" });

  for (const [mods, want] of [
    [["ControlLeft", "AltLeft"], "Ctrl+Alt+Escape"],
    [["ShiftLeft"], "Shift+Backspace"],
    [["MetaLeft"], "Super+Backspace"],
    [["ControlLeft"], "Ctrl+Backspace"],
  ] as const) {
    const k = keyboard();
    k.chord(...mods, want.endsWith("Escape") ? "Escape" : "Backspace");
    assert.deepEqual(k.commits(), [want]);
  }
});

test("AltGr is refused with a reason", () => {
  const k = keyboard();
  // Windows reports AltGr as Left Ctrl plus Right Alt; Chromium names the
  // latter AltGraph and clears Ctrl and Alt for the keys typed with it.
  k.down("ControlLeft");
  k.down("AltRight", { key: "AltGraph", ctrl: false, alt: false });
  k.down("KeyE", { ctrl: false, alt: false, altGraph: true });
  k.up("KeyE", { ctrl: false, alt: false, altGraph: true });
  k.up("AltRight", { key: "AltGraph", ctrl: false, alt: false });
  k.up("ControlLeft");
  assert.deepEqual(k.commits(), []);
  assert.deepEqual(k.state.hint, { kind: "altGraph" });
});

test("PrintScreen, which only comes up, is taken while a modifier is held", () => {
  const k = keyboard();
  k.down("ControlLeft");
  k.up("PrintScreen");
  assert.deepEqual(k.commits(), ["Ctrl+PrintScreen"]);
});

test("a keyup left over from clicking Change is ignored", () => {
  const k = keyboard();
  k.up("Enter");
  assert.deepEqual(k.effects, []);
});

test("keys are recorded as Windows sees them", () => {
  const cases: [string, Partial<KeyInput>, string][] = [
    // Z on a German keyboard sits where Y is on a US one.
    ["KeyY", { keyCode: 0x5a, key: "z" }, "Ctrl+KeyZ"],
    // An IME reports 229; the position stands in.
    ["KeyK", { keyCode: 229, key: "Process" }, "Ctrl+KeyK"],
    // With NumLock off, the keypad's 1 is End.
    ["Numpad1", { keyCode: 0x23, key: "End" }, "Ctrl+End"],
    ["NumpadEnter", { keyCode: 0x0d, key: "Enter" }, "Ctrl+Enter"],
  ];
  for (const [code, extra, want] of cases) {
    const k = keyboard();
    k.down("ControlLeft");
    k.down(code, extra);
    k.up(code, extra);
    assert.deepEqual(k.commits(), [want], code);
  }
});

test("Ctrl+Alt typing a character on an AltGr layout is refused", () => {
  const k = keyboard();
  k.down("ControlLeft");
  k.down("AltLeft");
  k.down("KeyQ", { key: "@", altGraph: true });
  k.up("KeyQ", { key: "@", altGraph: true });
  assert.deepEqual(k.commits(), []);
  assert.deepEqual(k.state.hint, { kind: "altGraph" });
});

test("virtual-key codes the table lacks are refused, not taken by position", () => {
  const cases: [string, Partial<KeyInput>, string][] = [
    // Ctrl+Pause arrives as VK_CANCEL, which no hotkey on Pause matches.
    ["Pause", { keyCode: 3, key: "Cancel" }, "Pause"],
    // AZERTY's !, VK_OEM_8, where a US keyboard has /.
    ["Slash", { keyCode: 0xdf, key: "!" }, "!"],
    // The keypad's 5 with NumLock off is VK_CLEAR.
    ["Numpad5", { keyCode: 0x0c, key: "Clear" }, "Num 5"],
  ];
  for (const [code, extra, name] of cases) {
    const k = keyboard();
    k.down("ControlLeft");
    k.down("AltLeft");
    k.down(code, extra);
    k.up(code, extra);
    assert.deepEqual(k.commits(), [], code);
    assert.deepEqual(k.state.hint, { kind: "unsupported", key: name }, code);
  }
});

test("a modifier added after the main key is reported, not dropped", () => {
  for (const [first, key, late] of [
    ["ShiftLeft", "KeyA", "ControlLeft"],
    ["ControlLeft", "KeyK", "AltLeft"],
  ]) {
    const k = keyboard();
    k.down(first);
    k.down(key);
    k.down(late);
    k.up(key);
    assert.deepEqual(k.commits(), [], key);
    assert.deepEqual(k.state.hint, { kind: "order" }, key);
  }
});

test("two ordinary keys together are refused with the reason", () => {
  const numpad = keyboard();
  numpad.down("NumpadAdd");
  numpad.down("NumpadSubtract");
  numpad.up("NumpadSubtract");
  numpad.up("NumpadAdd");
  assert.deepEqual(numpad.commits(), []);
  assert.deepEqual(numpad.state.hint, { kind: "multipleKeys" });
  assert.deepEqual(
    numpad.state.chord.map((k) => k.key),
    ["NumpadAdd", "NumpadSubtract"],
  );

  const withCtrl = keyboard();
  withCtrl.down("ControlLeft");
  withCtrl.down("KeyK");
  withCtrl.down("KeyJ");
  withCtrl.up("KeyK");
  withCtrl.up("KeyJ");
  withCtrl.up("ControlLeft");
  assert.deepEqual(withCtrl.commits(), []);
  assert.deepEqual(withCtrl.state.hint, { kind: "multipleKeys" });
});

test("keys the table lacks are named", () => {
  const k = keyboard();
  k.down("ControlLeft");
  k.down("Lang1", { key: "HangulMode", keyCode: 0 });
  assert.deepEqual(k.state.hint, { kind: "unsupported", key: "HangulMode" });
});

test("a modifier whose keyup was lost is dropped by the next event's flags", () => {
  const k = keyboard();
  k.down("ControlLeft");
  // Focus went elsewhere and the keyup never came.
  k.down("KeyK", { ctrl: false });
  assert.deepEqual(k.state.held, ["KeyK"]);
});

test("another main key can follow while the modifiers stay down", () => {
  const k = keyboard();
  k.down("ControlLeft");
  k.down("KeyK");
  k.up("KeyK");
  k.down("KeyJ");
  k.up("KeyJ");
  assert.deepEqual(k.commits(), ["Ctrl+KeyK", "Ctrl+KeyJ"]);
});
