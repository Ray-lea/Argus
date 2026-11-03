import { useSyncExternalStore } from "react";

// Enable island controls after hydration; the server already renders their content.
const subscribe = () => () => {};
export const useInteractive = () =>
  useSyncExternalStore(
    subscribe,
    () => true,
    () => false,
  );
