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
