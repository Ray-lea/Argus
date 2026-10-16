// How keyboard shortcuts read: accelerators (`Ctrl+Alt+Escape`, `Ctrl+KeyK`)
// as the keys printed on a keyboard, and why one cannot be used.

import { type HotkeyIssue, keyByCode, keyName } from "./hotkey";
import { t } from "./i18n";
import type { HotkeyError } from "./types";

const MODIFIER_LABELS: Record<string, string> = {
  ctrl: "Ctrl",
  control: "Ctrl",
  alt: "Alt",
  shift: "Shift",
  super: "Win",
  win: "Win",
  meta: "Win",
};

/** One part of an accelerator, e.g. `KeyK` → `K`, `Escape` → `Esc`. */
export function keyLabel(part: string): string {
  const modifier = MODIFIER_LABELS[part.toLowerCase()];
  if (modifier) return modifier;
  const key = keyByCode(part.toLowerCase() === "numpadenter" ? "Enter" : part);
  if (key?.code === "Space") return t("common.keys.space");
  return key ? keyName(key) : part;
}

/** The keys of an accelerator, in order; empty when there is none. */
export const shortcutKeys = (accelerator: string): string[] =>
  accelerator ? accelerator.split("+").map((part) => keyLabel(part.trim())) : [];

/** An accelerator as text, e.g. `Ctrl+Alt+Esc`. */
export const shortcutText = (accelerator: string): string => shortcutKeys(accelerator).join("+");

/** Why `shortcut` is refused. */
export function hotkeyIssueText(issue: HotkeyIssue, shortcut: string): string {
  const key = shortcut.split("+").pop() ?? "";
  return t(`settings.hotkey.issue.${issue}`, { shortcut: shortcutText(shortcut), key: keyLabel(key.trim()) });
}

/** Why the hotkey is not active or was not changed; null for none. */
export function hotkeyErrorText(error: HotkeyError | null | unknown): string | null {
  if (!error) return null;
  if (typeof error !== "object" || !("kind" in error)) return String(error);
  const e = error as HotkeyError;
  switch (e.kind) {
    case "invalid":
      return t("settings.hotkey.error.invalid", {
        shortcut: shortcutText(e.shortcut),
        reason: hotkeyIssueText(e.issue, e.shortcut),
      });
    case "taken":
      return t("settings.hotkey.error.taken", { shortcut: shortcutText(e.shortcut) });
    case "failed":
      return t("settings.hotkey.error.failed", { shortcut: shortcutText(e.shortcut), code: e.code });
    case "saveFailed":
      return t("settings.hotkey.error.saveFailed");
  }
}
