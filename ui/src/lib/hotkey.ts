// The show/hide hotkey's rules, shared with the native side through
// hotkey-table.json (see src-tauri/src/hotkey.rs, which refuses the same
// `block` combinations). Windows' RegisterHotKey compares Ctrl, Alt, Shift
// and Win, either side of each, when the main key goes down: a combination is
// those modifiers plus one key from the table. Nothing here imports the UI,
// so node runs ui/tests on it directly.

import table from "./hotkey-table.json" with { type: "json" };

export type KeyGroup =
  | "letter"
  | "digit"
  | "symbol"
  | "function"
  | "extra"
  | "control"
  | "navigation"
  | "numpad"
  | "numpadOperator"
  | "system"
  | "lock"
  | "media";

export interface KeySpec {
  /** The W3C `code` name, as stored. */
  code: string;
  /** The Windows virtual-key code: what RegisterHotKey takes and `keyCode` reports. */
  vk: number;
  group: KeyGroup;
  /** How the key is printed, when that is not its name. */
  label?: string;
}

const KEYS = table.keys as KeySpec[];
/** Lower case for ASCII letters only, as the native side compares. */
const lowerAscii = (text: string) => text.replace(/[A-Z]/g, (c) => c.toLowerCase());
const BY_CODE = new Map(KEYS.map((k) => [lowerAscii(k.code), k]));
const BY_VK = new Map(KEYS.map((k) => [k.vk, k]));

export const keyByCode = (code: string): KeySpec | undefined => BY_CODE.get(lowerAscii(code));
export const keyByVk = (vk: number): KeySpec | undefined => BY_VK.get(vk);

export interface Mods {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  win: boolean;
}

export interface Accelerator extends Mods {
  key: KeySpec;
}

/** Why a combination is refused; the native side refuses the same ones. */
export type HotkeyIssue =
  | "invalid"
  | "noKey"
  | "unsupported"
  | "lockKey"
  | "needsModifier"
  | "reserved"
  | "taskManager"
  | "editing";

/** Allowed after a confirmation: it takes the combination from every program. */
export type HotkeyWarning = "common" | "inApp" | "imeSwitch" | "numpad" | "printScreen";

export type Verdict =
  | { level: "ok" }
  | { level: "block"; issue: HotkeyIssue }
  | { level: "warn"; warnings: HotkeyWarning[] };

const MODIFIER_NAMES = new Map<string, keyof Mods>([
  ["ctrl", "ctrl"],
  ["control", "ctrl"],
  ["commandorcontrol", "ctrl"],
  ["commandorctrl", "ctrl"],
  ["cmdorctrl", "ctrl"],
  ["cmdorcontrol", "ctrl"],
  ["alt", "alt"],
  ["option", "alt"],
  ["shift", "shift"],
  ["super", "win"],
  ["win", "win"],
  ["meta", "win"],
  ["cmd", "win"],
  ["command", "win"],
]);

function keyFromToken(token: string): KeySpec | undefined {
  const lower = lowerAscii(token);
  if (lower === "esc") return keyByCode("Escape");
  if (lower === "numpadenter") return keyByCode("Enter");
  if (["up", "down", "left", "right"].includes(lower)) return keyByCode(`Arrow${lower}`);
  if (/^[a-z]$/.test(lower)) return keyByCode(`Key${lower}`);
  if (/^\d$/.test(lower)) return keyByCode(`Digit${lower}`);
  return keyByCode(lower);
}

/** Reads a stored or hand-written combination, as the native side does. */
export function parseHotkey(text: string): Accelerator | HotkeyIssue {
  const tokens = text.split("+").map((t) => t.trim());
  if (tokens.some((t) => t === "")) return "invalid";
  const last = tokens.pop() as string;
  const mods: Mods = { ctrl: false, alt: false, shift: false, win: false };
  for (const token of tokens) {
    const mod = MODIFIER_NAMES.get(lowerAscii(token));
    if (!mod) return "invalid";
    mods[mod] = true;
  }
  if (MODIFIER_NAMES.has(lowerAscii(last))) return "noKey";
  const key = keyFromToken(last);
  return key ? { ...mods, key } : "unsupported";
}

/** The stored form: `Ctrl+Alt+Shift+Super+<code>`. */
export function canonical(a: Accelerator): string {
  const parts: string[] = [];
  if (a.ctrl) parts.push("Ctrl");
  if (a.alt) parts.push("Alt");
  if (a.shift) parts.push("Shift");
  if (a.win) parts.push("Super");
  parts.push(a.key.code);
  return parts.join("+");
}

const RESERVED = new Set(table.reserved);
const TASK_MANAGER = new Set(table.taskManager);
const EDITING = new Set(table.editing);
// Argus's own Ctrl+1–7 (pages) and Ctrl+F (search); see App.tsx.
const IN_APP = new Set([
  ...[1, 2, 3, 4, 5, 6, 7].flatMap((n) => [`Ctrl+Digit${n}`, `Ctrl+Numpad${n}`]),
  "Ctrl+KeyF",
  "Ctrl+Shift+KeyF",
]);

/** What the rules say about a parsed combination. */
export function verdict(a: Accelerator): Verdict {
  const { key } = a;
  if (key.group === "media") return { level: "block", issue: "unsupported" };
  if (key.group === "lock") return { level: "block", issue: "lockKey" };
  const name = canonical(a);
  if (RESERVED.has(name)) return { level: "block", issue: "reserved" };
  if (TASK_MANAGER.has(name)) return { level: "block", issue: "taskManager" };
  if (EDITING.has(name)) return { level: "block", issue: "editing" };
  // With Ctrl down the keyboard sends Break, which Windows reports as
  // VK_CANCEL: a hotkey on Ctrl+…+Pause would never fire.
  if (key.code === "Pause" && a.ctrl) return { level: "block", issue: "unsupported" };
  const shiftFunction = a.shift && !a.ctrl && !a.alt && !a.win && key.group === "function";
  if (!a.ctrl && !a.alt && !a.win && key.group !== "extra" && !shiftFunction) {
    return { level: "block", issue: "needsModifier" };
  }
  const warnings: HotkeyWarning[] = [];
  const fKey = key.group === "function" || key.group === "extra";
  if (IN_APP.has(name)) warnings.push("inApp");
  else if (name === "Ctrl+Space") warnings.push("imeSwitch");
  // Ctrl alone or Alt alone (Shift aside) with an ordinary key, or Shift with
  // F1–F11: in use in most programs.
  else if (shiftFunction || (!a.win && !(a.ctrl && a.alt) && !fKey)) warnings.push("common");
  if (key.group === "numpad") warnings.push("numpad");
  if (key.code === "PrintScreen") warnings.push("printScreen");
  return warnings.length > 0 ? { level: "warn", warnings } : { level: "ok" };
}

/** Parses and judges a combination in one go. */
export function judge(text: string): Verdict {
  const a = parseHotkey(text);
  return typeof a === "string" ? { level: "block", issue: a } : verdict(a);
}

/** How a key is printed: `KeyK` → `K`, `Digit7` → `7`, `Escape` → `Esc`. */
export function keyName(key: KeySpec): string {
  if (key.label) return key.label;
  if (key.group === "letter") return key.code.slice(3);
  if (key.group === "digit") return key.code.slice(5);
  return key.code;
}

// ---- Recording -----------------------------------------------------------------------
//
// The settings page records a combination as RegisterHotKey will match it:
// the modifiers held when the main key goes down, committed when that key
// comes up. Committing on the way down would register the hotkey while the
// key is still held, and its next auto-repeat would fire it (measured, even
// with MOD_NOREPEAT): the window would hide at once.

/** One key event, as the page saw it. */
export interface KeyInput {
  type: "down" | "up";
  /** Physical position, e.g. `ControlRight`. */
  code: string;
  key: string;
  /** Windows' virtual-key code for most keys; 229 while an IME handles the key. */
  keyCode: number;
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  meta: boolean;
  /** The key types a character with AltGr (or Ctrl+Alt, its equivalent). */
  altGraph: boolean;
  repeat: boolean;
}

/** A key of the chord being recorded; `key` is the table code of a main key. */
export interface ChordKey {
  code: string;
  kind: "modifier" | "altGraph" | "lock" | "key";
  key?: string;
}

/** Why nothing was committed, or what the keys did not do. */
export type RecordHint =
  | { kind: "noKey" | "order" | "altGraph" | "lockKey" }
  | { kind: "unsupported"; key: string };

export interface Recording {
  /** Keys down now. */
  held: string[];
  /** Every key pressed since the keys were last all up, in order. */
  chord: ChordKey[];
  /** The main key and the combination it makes, until it comes up. */
  pending: { code: string; hotkey: string; mods: Mods } | null;
  /** The main key went down before a modifier. */
  order: boolean;
  hint: RecordHint | null;
}

export type RecordEffect =
  | { type: "none" }
  | { type: "cancel" }
  | { type: "disable" }
  | { type: "commit"; hotkey: string };

export const startRecording = (): Recording => ({
  held: [],
  chord: [],
  pending: null,
  order: false,
  hint: null,
});

const MODIFIER_CODES: Record<string, keyof Mods> = {
  ControlLeft: "ctrl",
  ControlRight: "ctrl",
  AltLeft: "alt",
  AltRight: "alt",
  ShiftLeft: "shift",
  ShiftRight: "shift",
  MetaLeft: "win",
  MetaRight: "win",
  OSLeft: "win",
  OSRight: "win",
};
const MODIFIER_KEYS: Record<string, keyof Mods> = {
  Control: "ctrl",
  Alt: "alt",
  Shift: "shift",
  Meta: "win",
  OS: "win",
};
const LOCK_CODES = new Set(["CapsLock", "NumLock", "ScrollLock"]);

/** Which modifier a key is, if it is one; by position, or by name when there is none. */
export const modifierOf = (input: Pick<KeyInput, "code" | "key">): keyof Mods | undefined =>
  MODIFIER_CODES[input.code] ?? (input.code ? undefined : MODIFIER_KEYS[input.key]);

/** Which modifier a key listed in `Recording.held` (a code, or a name without one) is. */
export const heldModifier = (id: string): keyof Mods | undefined => MODIFIER_CODES[id] ?? MODIFIER_KEYS[id];

const modsOf = (input: KeyInput): Mods => ({
  ctrl: input.ctrl,
  alt: input.alt,
  shift: input.shift,
  win: input.meta,
});

/**
 * The table key for a main key. Windows reports the virtual-key code, which is
 * what RegisterHotKey takes, so the layout's own letters are recorded (Z on a
 * German keyboard is Z, wherever it sits). Only an IME's 229 and a missing
 * code fall back to the physical key: any other code the table lacks is a key
 * of its own (VK_OEM_8, or Pause with Ctrl, which arrives as VK_CANCEL), and
 * the physical key's name would register a different one.
 */
export function tokenFor(input: Pick<KeyInput, "code" | "keyCode">): KeySpec | undefined {
  if (input.keyCode !== 229 && input.keyCode !== 0) {
    const byVk = keyByVk(input.keyCode);
    return byVk?.group === "lock" ? undefined : byVk;
  }
  return keyByCode(input.code === "NumpadEnter" ? "Enter" : input.code);
}

const noMods = (input: KeyInput) => !input.ctrl && !input.alt && !input.shift && !input.meta;

/** Advances a recording by one key event. */
export function record(state: Recording, input: KeyInput): { state: Recording; effect: RecordEffect } {
  const none = { type: "none" } as const;
  if (input.repeat) return { state, effect: none };
  const id = input.code || input.key;
  const modifier = modifierOf(input);
  const mods = modsOf(input);
  let { chord, pending, order, hint } = state;
  // A keyup lost to a focus change leaves a modifier listed; the flags tell
  // (AltGr aside: Chromium clears Ctrl and Alt while it is down).
  const altGraph = (code: string) => chord.some((k) => k.code === code && k.kind === "altGraph");
  let held = state.held.filter((code) => {
    const m = heldModifier(code);
    return !m || mods[m] || code === id || altGraph(code);
  });

  if (input.type === "down") {
    if (held.length === 0) {
      chord = [];
      order = false;
      hint = null;
    }
    if (!held.includes(id)) held = [...held, id];
    const add = (key: ChordKey) => {
      if (!chord.some((k) => k.code === key.code)) chord = [...chord, key];
    };
    // Right Alt on layouts that type characters with it (AltGr): Chromium
    // clears Ctrl and Alt for the keys pressed with it.
    if (input.key === "AltGraph") {
      add({ code: id, kind: "altGraph" });
      return { state: { held, chord, pending: null, order, hint: { kind: "altGraph" } }, effect: none };
    }
    if (modifier) {
      add({ code: id, kind: "modifier" });
      // A modifier the main key went down without: pressed too late.
      if (pending && !pending.mods[modifier]) order = true;
      return { state: { held, chord, pending, order, hint }, effect: none };
    }
    if (LOCK_CODES.has(input.code) || LOCK_CODES.has(input.key)) {
      add({ code: id, kind: "lock" });
      return { state: { held, chord, pending, order, hint: { kind: "lockKey" } }, effect: none };
    }
    // Tab and Shift+Tab move the focus on, out of the recording.
    if (input.key === "Tab" && !input.ctrl && !input.alt && !input.meta) {
      return { state: startRecording(), effect: { type: "cancel" } };
    }
    if (held.length === 1 && noMods(input)) {
      if (input.key === "Escape") return { state: startRecording(), effect: { type: "cancel" } };
      if (input.key === "Backspace") return { state: startRecording(), effect: { type: "disable" } };
    }
    if (input.altGraph || chord.some((k) => k.kind === "altGraph")) {
      add({ code: id, kind: "key" });
      return { state: { held, chord, pending: null, order, hint: { kind: "altGraph" } }, effect: none };
    }
    const key = tokenFor(input);
    if (!key) {
      add({ code: id, kind: "key" });
      const physical = keyByCode(input.code);
      const name =
        input.key.length === 1
          ? input.key.toUpperCase()
          : physical
            ? keyName(physical)
            : input.key || input.code;
      return {
        state: { held, chord, pending: null, order, hint: { kind: "unsupported", key: name } },
        effect: none,
      };
    }
    add({ code: id, kind: "key", key: key.code });
    return {
      state: {
        held,
        chord,
        pending: { code: id, hotkey: canonical({ ...mods, key }), mods },
        order: false,
        hint: null,
      },
      effect: none,
    };
  }

  held = held.filter((code) => code !== id);
  if (pending && pending.code === id) {
    const next = { held, chord, pending: null, order, hint };
    if (order) return { state: { ...next, hint: { kind: "order" } }, effect: none };
    return { state: next, effect: { type: "commit", hotkey: pending.hotkey } };
  }
  // A main key whose keydown never arrived: PrintScreen has none on Windows,
  // and a combination another program registered loses it. Taken as the
  // main key while Ctrl, Alt or Win is still held, so that registering it
  // can say whether it is free.
  if (
    !modifier &&
    !pending &&
    !chord.some((k) => k.code === id) &&
    (input.ctrl || input.alt || input.meta) &&
    chord.some((k) => k.kind === "modifier")
  ) {
    const key = tokenFor(input);
    if (key && !LOCK_CODES.has(key.code)) {
      return {
        state: {
          held,
          chord: [...chord, { code: id, kind: "key", key: key.code }],
          pending: null,
          order,
          hint,
        },
        effect: { type: "commit", hotkey: canonical({ ...mods, key }) },
      };
    }
  }
  // Everything up, and no main key: modifiers alone (CapsLock aside).
  const onlyModifiers = chord.some((k) => k.kind === "modifier") && chord.every((k) => k.kind !== "key");
  if (held.length === 0 && onlyModifiers && (!hint || hint.kind === "lockKey")) hint = { kind: "noKey" };
  return { state: { held, chord, pending, order, hint }, effect: none };
}
