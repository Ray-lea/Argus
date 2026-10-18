// Which Escape presses close what is open; no Solid, run by ui/tests directly.

/** What `plainEscape` reads of a key press; a KeyboardEvent has it all. */
export type KeyPress = Pick<
  KeyboardEvent,
  "key" | "repeat" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey" | "defaultPrevented" | "isComposing"
>;

/**
 * Esc pressed on its own: not held (Ctrl+Shift+Esc opens Argus through the
 * task manager replacement, and its repeats reach the window it opens), no
 * modifier, not taken by something else first and not ending a composition.
 */
export const plainEscape = (e: KeyPress) =>
  e.key === "Escape" &&
  !e.repeat &&
  !e.ctrlKey &&
  !e.shiftKey &&
  !e.altKey &&
  !e.metaKey &&
  !e.defaultPrevented &&
  !e.isComposing;
