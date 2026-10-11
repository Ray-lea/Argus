// Preferences kept in localStorage (per webview profile, so dev and installed builds differ).

import { createSignal } from "solid-js";

export function load<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(`argus.${key}`);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

/** A signal that survives restarts. */
export function persisted<T>(key: string, fallback: T) {
  const [get, set] = createSignal<T>(load(key, fallback));
  const update = (value: T) => {
    set(() => value);
    try {
      localStorage.setItem(`argus.${key}`, JSON.stringify(value));
    } catch {
      // Storage unavailable: the preference just won't survive a restart.
    }
  };
  return [get, update] as const;
}
