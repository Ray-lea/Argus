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

/**
 * A key that, pressed outside any text field, starts a process search with
 * its character: a letter or digit, without Ctrl, Alt (AltGr is both) or Win,
 * not an auto-repeat (a key held down since before the window had focus) and
 * not part of an input method's composition. Space, punctuation and the keys
 * that name themselves (Enter, F5, …) keep their own meaning.
 */
export const startsSearch = (e: KeyPress) =>
  /^[\p{L}\p{N}]$/u.test(e.key) &&
  !e.ctrlKey &&
  !e.altKey &&
  !e.metaKey &&
  !e.repeat &&
  !e.defaultPrevented &&
  !e.isComposing;

/** Modifier flags also recover from a keyup lost before the window regained focus. */
export const controlPauseHeld = (e: Pick<KeyPress, "ctrlKey" | "altKey" | "metaKey" | "isComposing">) =>
  e.ctrlKey && !e.altKey && !e.metaKey && !e.isComposing;
