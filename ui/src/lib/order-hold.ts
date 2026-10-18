// What keeps the process list from re-sorting, and when the status bar says
// so. Nothing here imports the UI; ui/tests runs it directly.

/** What could be holding the rows of the process list in place right now. */
export interface OrderHolds {
  /** The pointer is over the list. */
  pointer: boolean;
  /** A context menu is open. */
  menu: boolean;
  /** A confirmation dialog is open. */
  dialog: boolean;
}

/**
 * While the user is aiming at a row (pointer over the list, a menu or a
 * dialog open), new data updates values in place but never reorders rows.
 */
export const holdsOrder = (h: OrderHolds): boolean => h.pointer || h.menu || h.dialog;

/**
 * Whether a held order is the pointer's doing alone, the one case the status
 * bar names: its hint promises a re-sort once the pointer leaves the list.
 * While a menu or a dialog is open the rows wait for it instead, and the
 * pointer is usually on it, not on the list.
 */
export const pointerAloneHoldsOrder = (h: OrderHolds): boolean => h.pointer && !h.menu && !h.dialog;
