import type { KeyPress } from "./escape.ts";

/** AltGr, composition and auto-repeat must never end another process. */
export const endProcessShortcut = (e: KeyPress) =>
  e.key.toLowerCase() === "e" &&
  e.altKey &&
  !e.ctrlKey &&
  !e.shiftKey &&
  !e.metaKey &&
  !e.repeat &&
  !e.defaultPrevented &&
  !e.isComposing;

/** Modifier flags also recover from a keyup lost before the window regained focus. */
export const controlPauseHeld = (e: Pick<KeyPress, "ctrlKey" | "altKey" | "metaKey" | "isComposing">) =>
  e.ctrlKey && !e.altKey && !e.metaKey && !e.isComposing;
