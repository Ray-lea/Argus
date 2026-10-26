// Native preferences survive WebView profile cleanup. localStorage remains a
// synchronous cache and the standalone browser preview's storage.
import { invoke } from "@tauri-apps/api/core";
import { createSignal, getOwner, onCleanup } from "solid-js";

const native = "__TAURI_INTERNALS__" in window;
const cache = new Map<string, unknown>();
const readers = new Map<string, Set<(value: unknown) => void>>();
export const [preferenceSaveFailed, setPreferenceSaveFailed] = createSignal(false);

export function load<T>(key: string, fallback: T): T {
  if (cache.has(key)) return cache.get(key) as T;
  try {
    const raw = localStorage.getItem(`argus.${key}`);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

function remember(key: string, value: unknown) {
  cache.set(key, value);
  try {
    localStorage.setItem(`argus.${key}`, JSON.stringify(value));
  } catch {
    /* Native storage remains available. */
  }
}

/** Called before rendering, including signals already created by module imports. */
export async function initializePreferences() {
  if (!native) return;
  const legacy: Record<string, unknown> = {};
  try {
    for (const key of Object.keys(localStorage)) {
      if (!key.startsWith("argus.")) continue;
      try {
        legacy[key.slice(6)] = JSON.parse(localStorage.getItem(key) ?? "null");
      } catch {
        /* Skip just this key. */
      }
    }
  } catch {
    /* A disabled WebView cache must not block native restoration. */
  }
  try {
    const saved = await invoke<Record<string, unknown>>("load_ui_preferences", { legacy });
    for (const [key, value] of Object.entries(saved)) {
      remember(key, value);
      for (const read of readers.get(key) ?? []) read(value);
    }
  } catch {
    setPreferenceSaveFailed(true);
  }
}

/** A signal that survives restarts; validators also protect legacy cache values. */
export function persisted<T>(key: string, fallback: T, valid?: (value: unknown) => value is T) {
  const normalize = (value: unknown): T => (!valid || valid(value) ? (value as T) : fallback);
  const [get, set] = createSignal<T>(normalize(load(key, fallback)));
  const read = (value: unknown) => set(() => normalize(value));
  const listeners = readers.get(key) ?? new Set();
  readers.set(key, listeners);
  listeners.add(read);
  if (getOwner())
    onCleanup(() => {
      listeners.delete(read);
      if (!listeners.size) readers.delete(key);
    });
  const update = (next: T) => {
    const value = normalize(next);
    remember(key, value);
    for (const listener of readers.get(key) ?? []) listener(value);
    if (native)
      void invoke("save_ui_preference", { key, value }).then(
        () => setPreferenceSaveFailed(false),
        () => setPreferenceSaveFailed(true),
      );
  };
  return [get, update] as const;
}
