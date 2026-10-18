// What Argus may do beyond a standard user's rights, for the status bar and
// the hints that depend on it: it runs as administrator, or it reaches the
// privileged service, which does what needs those rights on its behalf.

import { createSignal } from "solid-js";
import { t } from "./i18n";
import { api } from "./ipc";
import { hasDiskReading, missingDiskReason } from "./process-io";
import { appInfo, snapshot } from "./store";
import type { PrivilegedServiceStatus } from "./types";

export type Privilege = "admin" | "service" | "standard";

/** The privileged service's card on the settings page, `setting-<id>` (store.ts, `goToSetting`). */
export const SERVICE_SETTING = "privilegedService";

const [service, setService] = createSignal<PrivilegedServiceStatus | null>(null);

/** The service runs for this account and answers. */
export const serviceConnected = (s: PrivilegedServiceStatus | null | undefined) =>
  s?.state === "running" && s.authorized && !s.error;

export const privilege = (): Privilege => {
  if (appInfo()?.elevated) return "admin";
  return serviceConnected(service()) ? "service" : "standard";
};

/** Takes a status read elsewhere (the settings page, the onboarding). */
export const noteServiceStatus = (status: PrivilegedServiceStatus | null) => setService(status);

const refresh = async () => setService(await api.privilegedServiceStatus().catch(() => null));

/**
 * Follows the service while the window shows: it may start after Argus (both
 * start with Windows) or be stopped elsewhere. A check is a few service
 * manager calls and a ping, so every half minute costs nothing to speak of.
 */
export function watchPrivilege(): () => void {
  if (appInfo()?.elevated) return () => {};
  void refresh();
  const check = () => {
    if (document.visibilityState === "visible") void refresh();
  };
  const timer = setInterval(check, 30_000);
  document.addEventListener("visibilitychange", check);
  return () => {
    clearInterval(timer);
    document.removeEventListener("visibilitychange", check);
  };
}

/** Why processes show no disk reading, from the status followed above. */
export const missingDiskHint = () => {
  const reason = missingDiskReason({
    measured: hasDiskReading(snapshot()?.processes ?? []),
    elevated: appInfo()?.elevated ?? false,
    service: service(),
    appVersion: appInfo()?.version ?? null,
  });
  // The key stays a plain template, which `i18n.mjs --unused` reads as a prefix.
  return t(`processes.diskUnavailable.${reason}`);
};
