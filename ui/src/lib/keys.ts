// How keyboard shortcuts read: accelerators (`Ctrl+Alt+Escape`, `Ctrl+KeyK`)
// as the keys printed on a keyboard.

import { t } from "./i18n";

const KEY_LABELS: Record<string, string> = {
  Super: "Win",
  Escape: "Esc",
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
  Backquote: "`",
  Minus: "-",
  Equal: "=",
};

/** One part of an accelerator, e.g. `KeyK` → `K`, `Escape` → `Esc`. */
export function keyLabel(part: string): string {
  if (part === "Space") return t("common.keys.space");
  if (KEY_LABELS[part]) return KEY_LABELS[part];
  if (/^Key[A-Z]$/.test(part)) return part.slice(3);
  if (/^Digit\d$/.test(part)) return part.slice(5);
  if (/^Numpad\d$/.test(part)) return `Num ${part.slice(6)}`;
  return part;
}

/** The keys of an accelerator, in order; empty when there is none. */
export const shortcutKeys = (accelerator: string): string[] =>
  accelerator ? accelerator.split("+").map(keyLabel) : [];
