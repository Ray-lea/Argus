// Transient UI surfaces: context menu, confirm dialog and toasts.

import { createSignal, type JSX, untrack } from "solid-js";
import { uiMotion } from "./motion";
import { type ToastHold, toastClock } from "./toast-clock";

// ---- Context menu ------------------------------------------------------------

export type MenuEntry =
  | "separator"
  | {
      label: string;
      icon?: () => JSX.Element;
      hint?: string;
      danger?: boolean;
      disabled?: boolean;
      /** Shows a check mark (radio-style choices such as a start type). */
      checked?: boolean;
      /** Opens a submenu instead of running anything. */
      children?: MenuEntry[];
      run?: () => unknown;
    };

export interface MenuState {
  x: number;
  y: number;
  items: MenuEntry[];
}
export const [menu, setMenu] = createSignal<MenuState | null>(null);

export function openMenu(e: MouseEvent, items: MenuEntry[]) {
  e.preventDefault();
  setMenu({ x: e.clientX, y: e.clientY, items });
}

// ---- Confirm dialog ----------------------------------------------------------

export interface ConfirmRequest {
  title: string;
  body: string;
  confirmLabel: string;
  danger?: boolean;
  resolve: (ok: boolean) => void;
}
export const [confirmRequest, setConfirmRequest] = createSignal<ConfirmRequest | null>(null);

export function confirm(req: Omit<ConfirmRequest, "resolve">): Promise<boolean> {
  return new Promise((resolve) => {
    setConfirmRequest({
      ...req,
      resolve: (ok) => {
        setConfirmRequest(null);
        resolve(ok);
      },
    });
  });
}

// ---- Toasts ------------------------------------------------------------------

export interface ToastAction {
  label: string;
  run: () => unknown;
}

export interface Toast {
  id: number;
  /** `info` draws the accent and, unless given one, the info icon. */
  kind: "error" | "success" | "info";
  text: string;
  icon?: () => JSX.Element;
  /** Buttons under the text; using one also closes the toast. */
  actions?: ToastAction[];
}
export const [toasts, setToasts] = createSignal<Toast[]>([]);
let toastId = 0;
const clock = toastClock((id) => dismissToast(id));

/** Shows a toast for `ms`; at most four at a time, the oldest giving way. Returns its id. */
export function toast(
  kind: Toast["kind"],
  text: string,
  ms = 5000,
  extra: Pick<Toast, "icon" | "actions"> = {},
): number {
  const id = ++toastId;
  // Not tracked: a toast raised from an effect must not make it follow the list.
  const list = untrack(toasts);
  for (const old of list.slice(0, Math.max(0, list.length - 3))) clock.forget(old.id);
  setToasts([...list.slice(-3), { id, kind, text, ...extra }]);
  clock.start(id, ms);
  return id;
}

/**
 * Keeps a toast while the pointer is on it or keyboard focus is in it (its
 * buttons); it leaves a moment after both are gone.
 */
export const holdToast = (id: number, why: ToastHold, held: boolean) => clock.hold(id, why, held);

/** Toasts playing their exit animation. */
export const [leavingToasts, setLeavingToasts] = createSignal<ReadonlySet<number>>(new Set());

export function dismissToast(id: number) {
  clock.forget(id);
  const remove = () => {
    setToasts((list) => list.filter((t) => t.id !== id));
    setLeavingToasts((set) => {
      const next = new Set(set);
      next.delete(id);
      return next;
    });
  };
  if (leavingToasts().has(id) || !toasts().some((t) => t.id === id)) return;
  if (!uiMotion()) {
    remove();
    return;
  }
  setLeavingToasts((set) => new Set(set).add(id));
  setTimeout(remove, 170);
}
