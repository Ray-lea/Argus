// When each toast leaves: after its time, unless the pointer is on it or
// keyboard focus is in it, and then a moment after both have left. No Solid,
// so ui/tests can run it.

/** What keeps a toast: the pointer on it, focus in it (on one of its buttons). */
export type ToastHold = "pointer" | "focus";

/** How long a toast stays once nothing holds it any more. */
export const LINGER_MS = 2500;

export interface ToastClock {
  /** A new toast, to leave after `ms`. */
  start(id: number, ms: number): void;
  /** The pointer or focus arriving (`held`) or leaving; unknown toasts are ignored. */
  hold(id: number, why: ToastHold, held: boolean): void;
  /** The toast is gone: no timer, no holds. */
  forget(id: number): void;
}

/** Calls `expire` for each toast whose time is up. */
export function toastClock(expire: (id: number) => void): ToastClock {
  const timers = new Map<number, ReturnType<typeof setTimeout>>();
  const holds = new Map<number, Set<ToastHold>>();
  const stop = (id: number) => {
    clearTimeout(timers.get(id));
    timers.delete(id);
  };
  const arm = (id: number, ms: number) => {
    stop(id);
    timers.set(
      id,
      setTimeout(() => {
        timers.delete(id);
        expire(id);
      }, ms),
    );
  };
  return {
    start(id, ms) {
      holds.set(id, new Set());
      arm(id, ms);
    },
    hold(id, why, held) {
      const set = holds.get(id);
      if (!set || set.has(why) === held) return;
      if (held) set.add(why);
      else set.delete(why);
      if (set.size > 0) stop(id);
      else arm(id, LINGER_MS);
    },
    forget(id) {
      stop(id);
      holds.delete(id);
    },
  };
}
