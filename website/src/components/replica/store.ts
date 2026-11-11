"use client";
import { useSyncExternalStore } from "react";

// Shared between the parts of the home page: the accent picker recolours the
// window in the first screen, the comparison rows can open one of its pages,
// and the taskbar shows or minimizes it.
export interface Accent {
  light: string;
  dark: string;
}
export type Page =
  | "processes"
  | "performance"
  | "services"
  | "startup"
  | "connections"
  | "history"
  | "settings"
  | "about";
// A row of the comparison asks for a place in the window: a page; a view of
// the process list; a device of the performance page, with its details page
// (CPU, disk) or the CPU's per-core view; a range of the history.
export interface Request {
  page: Page;
  view?: "apps" | "tree" | "list";
  device?: "cpu" | "memory" | "disk" | "wifi" | "gpu" | "battery";
  details?: boolean;
  cores?: boolean;
  range?: "1h" | "6h" | "1d" | "7d";
  at: number;
}
interface State {
  accent: Accent | null;
  request: Request | null;
  // The taskbar shows or minimizes the window; the guided demo looks up a file
  // as if it had been dropped on the window.
  command: { action: "show" | "minimize"; at: number } | { action: "lock"; names: string[]; at: number } | null;
}
let state: State = { accent: null, request: null, command: null };
const listeners = new Set<() => void>();
export const store = {
  get: () => state,
  set(patch: Partial<State>) {
    state = { ...state, ...patch };
    for (const listener of listeners) listener();
  },
  subscribe(listener: () => void) {
    listeners.add(listener);
    return () => listeners.delete(listener);
  },
};
export const useShared = () =>
  useSyncExternalStore(store.subscribe, store.get, store.get);
