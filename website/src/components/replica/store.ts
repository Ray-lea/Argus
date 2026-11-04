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
  | "settings";
interface State {
  accent: Accent | null;
  request: { page: Page; view?: "apps" | "tree" | "list"; at: number } | null;
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
