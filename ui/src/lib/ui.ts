// Transient UI surfaces: context menu, confirm dialog and toasts.

import { createSignal, type JSX } from "solid-js";
import { uiMotion } from "./motion";

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

export interface Toast {
  id: number;
  /** `info` draws the accent and, unless given one, the info icon. */
  kind: "error" | "success" | "info";
  text: string;
  icon?: () => JSX.Element;
  /** One button beside the text; using it also closes the toast. */
  action?: { label: string; run: () => unknown };
}
export const [toasts, setToasts] = createSignal<Toast[]>([]);
let toastId = 0;
const toastTimers = new Map<number, ReturnType<typeof setTimeout>>();

export function toast(
  kind: Toast["kind"],
  text: string,
  ms = 5000,
  extra: Pick<Toast, "icon" | "action"> = {},
) {
  const id = ++toastId;
  setToasts((list) => [...list.slice(-3), { id, kind, text, ...extra }]);
  toastTimers.set(
    id,
    setTimeout(() => dismissToast(id), ms),
  );
}

/** Keeps a toast while the pointer is on it; it leaves a moment after. */
export function holdToast(id: number, held: boolean) {
  clearTimeout(toastTimers.get(id));
  if (!held)
    toastTimers.set(
      id,
      setTimeout(() => dismissToast(id), 2500),
    );
}

/** Toasts playing their exit animation. */
export const [leavingToasts, setLeavingToasts] = createSignal<ReadonlySet<number>>(new Set());

export function dismissToast(id: number) {
  clearTimeout(toastTimers.get(id));
  toastTimers.delete(id);
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
